use crate::dsd_readers::{DSDFormat, DSDReader};
use byteorder::{BigEndian, ReadBytesExt};
use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};

use crate::dsd_readers::dst_dec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioKind {
    Dsd,
    Dst,
}

pub struct DFFReader {
    file: File,
    buf: Vec<u8>,         // internal interleaved read buffer (bytes: frames * channels)
    ch: usize,            // channels
    block_frames: usize,  // frames per internal read block (1 frame == 1 byte per channel)
    filled_frames: usize, // frames currently in buf
    pos_frames: usize,    // current read position in frames inside buf
    total_frames: u64,    // total frames (bytes per channel)
    read_frames: u64,     // frames read so far (bytes per channel consumed)
    data_start: u64,      // start offset of audio chunk payload

    // DST support
    audio_kind: Option<AudioKind>,
    data_end: u64,              // end offset (exclusive) of the audio payload region
    dst_framerate: u16,
    dst_frame_count: u32,
    dst_channel_frame_size: usize, // decoded bytes per channel per DST frame
    dst_decoder: Option<dst_dec::Decoder>,
    dsti_index: Vec<u64>,          // file offsets (to DSTF chunk header) per DST frame number
    dst_frame_buf: Vec<u8>,        // scratch buffer for encoded DSTF payload
}

impl DFFReader {
    pub fn new(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            file,
            buf: Vec::new(),
            ch: 0,
            block_frames: 4096,
            filled_frames: 0,
            pos_frames: 0,
            total_frames: 0,
            read_frames: 0,
            data_start: 0,

            audio_kind: None,
            data_end: 0,
            dst_framerate: 0,
            dst_frame_count: 0,
            dst_channel_frame_size: 0,
            dst_decoder: None,
            dsti_index: Vec::new(),
            dst_frame_buf: Vec::new(),
        })
    }

    pub fn empty() -> Self {
        Self {
            file: File::create("super_empty").unwrap(),
            buf: Vec::new(),
            ch: 0,
            block_frames: 4096,
            filled_frames: 0,
            pos_frames: 0,
            total_frames: 0,
            read_frames: 0,
            data_start: 0,

            audio_kind: None,
            data_end: 0,
            dst_framerate: 0,
            dst_frame_count: 0,
            dst_channel_frame_size: 0,
            dst_decoder: None,
            dsti_index: Vec::new(),
            dst_frame_buf: Vec::new(),
        }
    }

    fn read_id(&mut self) -> io::Result<[u8; 4]> {
        let mut id = [0u8; 4];
        self.file.read_exact(&mut id)?;
        Ok(id)
    }

    fn read_be_u64(&mut self) -> io::Result<u64> {
        self.file.read_u64::<BigEndian>()
    }

    // Decode one DST frame already loaded into dst_frame_buf into self.buf.
    //
    // decode_frame's second argument is the number of *compressed input* bits
    // (i.e. how many valid bits are in dst_data).  The decoder already knows
    // the decoded output size from Decoder::new(channels, channel_frame_size).
    // Passing decoded_size * 8 here made calc_nr_of_bytes = decoded_size inside
    // the decoder, which is always larger than the actual compressed payload →
    // the dst_data.len() < bytes check fired immediately → ReadPastEnd.
    fn decode_dst_frame(&mut self, compressed_len: usize) -> io::Result<()> {
        let compressed_bits = compressed_len * 8;

        let decoder = self.dst_decoder.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "DST decoder not initialized")
        })?;

        decoder
            .decode_frame(&self.dst_frame_buf[..compressed_len], compressed_bits, &mut self.buf)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("DST decode error: {:?}", e),
                )
            })
    }
}

impl DSDReader for DFFReader {
    fn open(&mut self, format: &mut DSDFormat) -> io::Result<()> {
        // --- FRM8 header ---
        let id = self.read_id()?;
        if &id != b"FRM8" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not FRM8 / DFF"));
        }

        let frm8_size = self.read_be_u64()?;
        let frm8_end = 12u64
            .checked_add(frm8_size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "FRM8 size overflow"))?;

        let fmt_id = self.read_id()?;
        if &fmt_id != b"DSD " {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not DSD container",
            ));
        }

        let mut audio_kind: Option<AudioKind> = None;
        let mut audio_chunk_size: u64 = 0;
        let mut sample_rate_hz: Option<u32> = None;
        let mut channels: Option<u16> = None;
        format.is_lsb_first = false;

        while self.file.seek(SeekFrom::Current(0))? < frm8_end {
            let mut chunk_id = [0u8; 4];
            if let Err(e) = self.file.read_exact(&mut chunk_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected EOF reading chunk id: {}", e),
                ));
            }
            let chunk_size = self.read_be_u64()?;
            let chunk_payload_start = self.file.seek(SeekFrom::Current(0))?;

            match &chunk_id {
                b"PROP" => {
                    let mut prop_id = [0u8; 4];
                    self.file.read_exact(&mut prop_id)?;
                    if &prop_id == b"SND " {
                        let prop_end = chunk_payload_start + chunk_size;
                        while self.file.seek(SeekFrom::Current(0))? < prop_end {
                            let mut sub_id = [0u8; 4];
                            if let Err(e) = self.file.read_exact(&mut sub_id) {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!("unexpected EOF in SND subchunk id: {}", e),
                                ));
                            }
                            let sub_size = self.read_be_u64()?;
                            let sub_payload_start = self.file.seek(SeekFrom::Current(0))?;

                            match &sub_id {
                                b"FS  " => {
                                    if sub_size >= 4 {
                                        let sr = self.file.read_u32::<BigEndian>()?;
                                        sample_rate_hz = Some(sr);
                                    } else {
                                        self.file.seek(SeekFrom::Start(
                                            sub_payload_start + sub_size,
                                        ))?;
                                    }
                                }
                                b"CHNL" => {
                                    if sub_size >= 2 {
                                        let ch = self.file.read_u16::<BigEndian>()?;
                                        channels = Some(ch);
                                    } else {
                                        self.file.seek(SeekFrom::Start(
                                            sub_payload_start + sub_size,
                                        ))?;
                                    }
                                }
                                b"CMPR" => {
                                    if sub_size >= 4 {
                                        let mut cmp = [0u8; 4];
                                        self.file.read_exact(&mut cmp)?;
                                        if &cmp == b"DSD " {
                                            self.audio_kind = Some(AudioKind::Dsd);
                                        } else if &cmp == b"DST " {
                                            self.audio_kind = Some(AudioKind::Dst);
                                        } else {
                                            return Err(io::Error::new(
                                                io::ErrorKind::InvalidData,
                                                "unsupported CMPR (not DSD/DST)",
                                            ));
                                        }
                                    } else {
                                        return Err(io::Error::new(
                                            io::ErrorKind::InvalidData,
                                            "invalid CMPR chunk",
                                        ));
                                    }
                                }
                                _ => {}
                            }

                            let padded = (sub_size + 1) & !1u64;
                            self.file
                                .seek(SeekFrom::Start(sub_payload_start + padded))?;
                        }
                    } else {
                        let padded = (chunk_size + 1) & !1u64;
                        self.file
                            .seek(SeekFrom::Start(chunk_payload_start + padded))?;
                    }
                }

                b"DSTI" => {
                    // Each entry: { offset: u64 BE, length: u32 BE } = 12 bytes.
                    // offset points to the start of the DSTF payload (past the 12-byte chunk header),
                    // so we subtract sizeof(Chunk)=12 to get the DSTF chunk header position,
                    // matching get_dsti_from_frame() in the C++ reference.
                    let mut remaining = chunk_size;
                    self.dsti_index.clear();
                    while remaining >= 12 {
                        let off = self.read_be_u64()?;
                        let _len = self.file.read_u32::<BigEndian>()?;
                        remaining -= 12;
                        self.dsti_index.push(off.saturating_sub(12));
                    }
                    let padded = (chunk_size + 1) & !1u64;
                    self.file
                        .seek(SeekFrom::Start(chunk_payload_start + padded))?;
                }

                b"DSD " => {
                    if audio_kind.is_none() {
                        audio_kind = Some(AudioKind::Dsd);
                        audio_chunk_size = chunk_size;
                        self.data_start = self.file.seek(SeekFrom::Current(0))?;
                        self.data_end = self.data_start + audio_chunk_size;
                    }
                    let padded = (chunk_size + 1) & !1u64;
                    self.file
                        .seek(SeekFrom::Start(chunk_payload_start + padded))?;
                }

                b"DST " => {
                    if audio_kind.is_none() {
                        audio_kind = Some(AudioKind::Dst);
                        audio_chunk_size = chunk_size;
                        let dst_payload_start = self.file.seek(SeekFrom::Current(0))?;
                        self.data_end = dst_payload_start + audio_chunk_size;

                        let frte_id = self.read_id()?;
                        let frte_size = self.read_be_u64()?;
                        if &frte_id != b"FRTE" || frte_size != 6 {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "DST chunk missing FRTE header",
                            ));
                        }
                        self.dst_frame_count = self.file.read_u32::<BigEndian>()?;
                        self.dst_framerate = self.file.read_u16::<BigEndian>()?;

                        // data_start points to the first DSTF/DSTC chunk inside the DST payload
                        self.data_start = self.file.seek(SeekFrom::Current(0))?;
                    }
                    let padded = (chunk_size + 1) & !1u64;
                    self.file
                        .seek(SeekFrom::Start(chunk_payload_start + padded))?;
                }

                _ => {
                    let padded = (chunk_size + 1) & !1u64;
                    self.file
                        .seek(SeekFrom::Start(chunk_payload_start + padded))?;
                }
            }
        }

        if audio_kind.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "audio chunk not found (DSD/DST)",
            ));
        }

        let channels =
            channels.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CHNL missing"))?;
        let fs = sample_rate_hz
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "FS missing"))?;

        format.num_channels = channels as u32;
        self.ch = channels as usize;
        format.sampling_rate = fs;
        self.audio_kind = audio_kind;

        match self.audio_kind {
            Some(AudioKind::Dsd) => {
                let total_frames = audio_chunk_size / (self.ch as u64);
                format.total_samples = total_frames;
                self.total_frames = total_frames;
                self.buf.resize(self.block_frames * self.ch, 0);
            }
            Some(AudioKind::Dst) => {
                if self.dst_framerate == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid DST framerate",
                    ));
                }

                // dst_channel_frame_size: decoded bytes per channel per DST frame.
                // Matches C++ `m_frame_size / m_channel_count`:
                //   m_frame_size = samplerate/8 * channels / framerate  (total interleaved bytes)
                //   per_channel  = samplerate/8 / framerate
                // e.g. DSD64 stereo 75fps: 2822400/8/75 = 4704 bytes per channel per frame
                let channel_frame_size = (fs as usize / 8)
                    .checked_div(self.dst_framerate as usize)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid DST frame size")
                    })?;
                self.dst_channel_frame_size = channel_frame_size;

                self.dst_decoder =
                    Some(dst_dec::Decoder::new(self.ch, self.dst_channel_frame_size));

                // total_frames is in bytes-per-channel units (same unit as read_frames increments).
                // = frame_count * decoded_bytes_per_channel_per_DST_frame
                let total_frames = (self.dst_frame_count as u64)
                    .saturating_mul(self.dst_channel_frame_size as u64);
                format.total_samples = total_frames;
                self.total_frames = total_frames;

                // buf holds one fully decoded DST frame, interleaved across all channels.
                // Size = dst_channel_frame_size * ch  (matches C++ m_frame_size).
                self.buf.resize(self.dst_channel_frame_size * self.ch, 0);
                self.filled_frames = 0;
                self.pos_frames = 0;

                eprintln!(
                    "DST open: samplerate={} ch={} framerate={} frame_count={} \
                     channel_frame_size={} buf_size={} total_frames={}",
                    fs,
                    self.ch,
                    self.dst_framerate,
                    self.dst_frame_count,
                    self.dst_channel_frame_size,
                    self.buf.len(),
                    self.total_frames,
                );
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "missing audio kind",
                ));
            }
        }

        self.seek_samples(0)?;

        if self.audio_kind == Some(AudioKind::Dst) && !self.dsti_index.is_empty() {
            if self.dst_frame_count != 0
                && (self.dsti_index.len() as u32) != self.dst_frame_count
            {
                eprintln!(
                    "warning: DSTI entries ({}) != FRTE frame_count ({})",
                    self.dsti_index.len(),
                    self.dst_frame_count
                );
            }
        }

        Ok(())
    }

    fn read(&mut self, data: &mut [&mut [u8]], bytes_per_channel: usize) -> io::Result<usize> {
        if self.ch == 0 {
            return Ok(0);
        }
        if data.len() < self.ch {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not enough channel buffers",
            ));
        }

        let mut written = 0usize;

        while written < bytes_per_channel {
            if self.pos_frames == self.filled_frames {
                match self.audio_kind {
                    Some(AudioKind::Dsd) => {
                        let frames_to_read =
                            (bytes_per_channel - written).min(self.block_frames);
                        let bytes_to_read = frames_to_read * self.ch;
                        self.buf.resize(bytes_to_read, 0);
                        let n = self.file.read(&mut self.buf)?;
                        if n == 0 {
                            return Ok(written);
                        }
                        self.filled_frames = n / self.ch;
                        self.pos_frames = 0;
                    }
                    Some(AudioKind::Dst) => {
                        if self.file.seek(SeekFrom::Current(0))? >= self.data_end {
                            return Ok(written);
                        }

                        // current DST frame number derived from read_frames.
                        // read_frames is in bytes-per-channel units; dividing by
                        // dst_channel_frame_size gives the DST frame index.
                        let current_frame_nr = (self.read_frames
                            / (self.dst_channel_frame_size as u64))
                            as usize;

                        if !self.dsti_index.is_empty()
                            && current_frame_nr < self.dsti_index.len()
                        {
                            // --- DSTI fast path ---
                            let frame_offset = self.dsti_index[current_frame_nr];
                            self.file.seek(SeekFrom::Start(frame_offset))?;

                            let chunk_id = self.read_id()?;
                            let chunk_size = self.read_be_u64()?;
                            if &chunk_id != b"DSTF" {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!(
                                        "DSTI[{}] offset {:#x} did not point to DSTF (got {:?})",
                                        current_frame_nr,
                                        frame_offset,
                                        std::str::from_utf8(&chunk_id)
                                    ),
                                ));
                            }
                            let payload_start = self.file.seek(SeekFrom::Current(0))?;
                            if payload_start + chunk_size > self.data_end {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "DSTF payload exceeds DST chunk bounds",
                                ));
                            }

                            let frame_len = chunk_size as usize;
                            self.dst_frame_buf.resize(frame_len, 0);
                            self.file.read_exact(&mut self.dst_frame_buf)?;
                            // skip odd-byte padding
                            if (frame_len & 1) != 0 {
                                self.file.seek(SeekFrom::Current(1))?;
                            }

                            // FIX: pass decoded output bits, not compressed input bits
                            self.decode_dst_frame(frame_len)?;

                            self.filled_frames = self.dst_channel_frame_size;
                            self.pos_frames = 0;
                        } else {
                            // --- Sequential fallback (no DSTI or past end of index) ---
                            let mut got_frame = false;
                            while self.file.seek(SeekFrom::Current(0))? < self.data_end {
                                let chunk_id = self.read_id()?;
                                let chunk_size = self.read_be_u64()?;
                                let payload_start = self.file.seek(SeekFrom::Current(0))?;

                                if &chunk_id == b"DSTF" {
                                    if payload_start + chunk_size > self.data_end {
                                        return Ok(written);
                                    }
                                    let frame_len = chunk_size as usize;
                                    self.dst_frame_buf.resize(frame_len, 0);
                                    self.file.read_exact(&mut self.dst_frame_buf)?;
                                    if (frame_len & 1) != 0 {
                                        self.file.seek(SeekFrom::Current(1))?;
                                    }

                                    // FIX: pass decoded output bits, not compressed input bits
                                    self.decode_dst_frame(frame_len)?;

                                    self.filled_frames = self.dst_channel_frame_size;
                                    self.pos_frames = 0;
                                    got_frame = true;
                                    break;
                                } else {
                                    // skip DSTC, unknown chunks, padding
                                    let padded = (chunk_size + 1) & !1u64;
                                    self.file
                                        .seek(SeekFrom::Start(payload_start + padded))?;
                                }
                            }

                            if !got_frame {
                                return Ok(written);
                            }
                        }
                    }
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "reader not opened",
                        ));
                    }
                }
            }

            let available_frames = self.filled_frames - self.pos_frames;
            let need_frames = bytes_per_channel - written;
            let take_frames = available_frames.min(need_frames);

            // deinterleave take_frames from buf into per-channel output slices
            for ch_idx in 0..self.ch {
                let dst = &mut data[ch_idx][written..written + take_frames];
                let mut src_offset = self.pos_frames * self.ch + ch_idx;
                for out_byte in dst.iter_mut() {
                    *out_byte = self.buf[src_offset];
                    src_offset += self.ch;
                }
            }

            self.pos_frames += take_frames;
            written += take_frames;
            // read_frames is in bytes-per-channel units, consistent with total_frames
            self.read_frames = self.read_frames.saturating_add(take_frames as u64);
        }

        Ok(written)
    }

    fn seek_percent(&mut self, percent: f64) -> io::Result<()> {
        if !(0.0..=1.0).contains(&percent) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "percent out of range",
            ));
        }
        let target_frame = (self.total_frames as f64 * percent) as u64;
        self.seek_samples(target_frame)
    }

    fn seek_samples(&mut self, sample_index: u64) -> io::Result<()> {
        match self.audio_kind {
            Some(AudioKind::Dsd) => {
                let byte_offset = sample_index
                    .checked_mul(self.ch as u64)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "seek overflow")
                    })?;
                self.file.seek(SeekFrom::Start(self.data_start + byte_offset))?;
                self.read_frames = sample_index;
                self.pos_frames = 0;
                self.filled_frames = 0;
                Ok(())
            }
            Some(AudioKind::Dst) => {
                if sample_index == 0 {
                    self.file.seek(SeekFrom::Start(self.data_start))?;
                    self.read_frames = 0;
                    self.pos_frames = 0;
                    self.filled_frames = 0;
                    return Ok(());
                }

                if self.dsti_index.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "seeking in DST requires DSTI (frame index) support",
                    ));
                }

                // sample_index is in bytes-per-channel units; map to DST frame number
                let target_frame =
                    (sample_index / (self.dst_channel_frame_size as u64)) as usize;
                let target_frame =
                    target_frame.min(self.dsti_index.len().saturating_sub(1));

                self.file.seek(SeekFrom::Start(self.dsti_index[target_frame]))?;
                // snap read_frames to the exact frame boundary
                self.read_frames =
                    (target_frame as u64) * (self.dst_channel_frame_size as u64);
                self.pos_frames = 0;
                self.filled_frames = 0;
                Ok(())
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "reader not opened",
            )),
        }
    }

    fn get_position_frames(&self) -> u64 {
        self.read_frames
    }

    fn get_position_percent(&self) -> f64 {
        if self.total_frames == 0 {
            0.0
        } else {
            (self.read_frames as f64 / self.total_frames as f64).min(1.0)
        }
    }

    fn eof(&self) -> bool {
        self.read_frames >= self.total_frames
    }
}