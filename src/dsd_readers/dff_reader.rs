use crate::dsd_readers::{DSDFormat, DSDMeta, DSDReader, MetaPicture};
use byteorder::{BigEndian, ReadBytesExt};
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};

use crate::dsd_readers::dst_dec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioKind {
    Dsd,
    Dst,
}



pub fn decode_dsdiff_text(raw: &[u8]) -> String {
    if raw.len() < 2 {
        return decode_bytes(raw);
    }
    let text_len = u16::from_be_bytes([raw[0], raw[1]]) as usize;
    let end = (2 + text_len).min(raw.len());
    decode_bytes(&raw[2..end])
}

fn decode_bytes(bytes: &[u8]) -> String {
    use chardetng::EncodingDetector;

    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, true);

    // Decode the bytes
    let (decoded, _, had_errors) = encoding.decode(bytes);

    if had_errors {
        eprintln!("Warning: decoding had errors");
    }

    decoded.into_owned()
}

pub struct DFFReader {
    file: File,
    buf: Vec<u8>,
    ch: usize,
    block_frames: usize,
    filled_frames: usize,
    pos_frames: usize,
    total_frames: u64,
    read_frames: u64,
    data_start: u64,

    // DST support
    audio_kind: Option<AudioKind>,
    data_end: u64,
    dst_framerate: u16,
    dst_frame_count: u32,
    dst_channel_frame_size: usize,
    dst_decoder: Option<dst_dec::Decoder>,
    dsti_index: Vec<u64>,
    dst_frame_buf: Vec<u8>,

    // Metadata – populated during open()
    metadata: DSDMeta,
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

            metadata: DSDMeta::default(),
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

            metadata: DSDMeta::default(),
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

    /// Read `len` bytes then skip the odd-byte padding if needed.
    fn read_payload(&mut self, len: u64) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len as usize];
        self.file.read_exact(&mut buf)?;
        if len & 1 != 0 {
            self.file.seek(SeekFrom::Current(1))?;
        }
        Ok(buf)
    }

    /// Decode a native DSDIFF text payload and store it in `self.metadata.tags`
    /// using `entry().or_insert()` so an earlier ID3 value is never overwritten.
    fn store_text_tag(&mut self, chunk_id: &[u8; 4], raw: &[u8]) {
        let text = decode_dsdiff_text(raw);
        if !text.is_empty() {
            match chunk_id {
                b"DITI" => self.metadata.title = Some(text),
                b"DIAR" => self.metadata.artist = Some(text),
                b"DIAL" => self.metadata.album = Some(text),
                b"DIGN" => self.metadata.genre = Some(text),
                b"DIFC" => self.metadata.comment = Some(text),
                _=>{}
            }
        }
    }

    #[cfg(feature = "dstdec")]
    fn decode_dst_frame(&mut self, compressed_len: usize) -> io::Result<()> {
        let compressed_bits = compressed_len * 8;
        let decoder = self.dst_decoder.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "DST decoder not initialized")
        })?;
        decoder
            .decode_frame(
                &self.dst_frame_buf[..compressed_len],
                compressed_bits,
                &mut self.buf,
            )
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("DST decode error: {:?}", e),
                )
            })
    }

    #[cfg(not(feature = "dstdec"))]
    fn decode_dst_frame(&mut self, _compressed_len: usize) -> io::Result<()> {
        panic!("DST decoding is disabled; enable the `dstdec` feature")
    }
}

impl DSDReader for DFFReader {
    fn open(&mut self, format: &mut DSDFormat) -> io::Result<()> {
        // -----------------------------------------------------------------------
        // FRM8 / DSD  header
        // -----------------------------------------------------------------------
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

        // Top-level chunk walk  (audio + metadata in one pass)
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
                // -----------------------------------------------------------
                // PROP / SND  – sample-rate, channel-count, compression type
                // -----------------------------------------------------------
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
                                        self.file
                                            .seek(SeekFrom::Start(sub_payload_start + sub_size))?;
                                    }
                                }
                                b"CHNL" => {
                                    if sub_size >= 2 {
                                        let ch = self.file.read_u16::<BigEndian>()?;
                                        channels = Some(ch);
                                    } else {
                                        self.file
                                            .seek(SeekFrom::Start(sub_payload_start + sub_size))?;
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

                // DSTI – DST frame index
                b"DSTI" => {
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

                // DSD  – uncompressed audio payload
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

                // DST  – compressed audio payload
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
                        self.data_start = self.file.seek(SeekFrom::Current(0))?;
                    }
                    let padded = (chunk_size + 1) & !1u64;
                    self.file
                        .seek(SeekFrom::Start(chunk_payload_start + padded))?;
                }

                // -----------------------------------------------------------
                // DIIN – disc information block
                //   Sub-chunks: DITI (title), DIAR (artist), ALCH (cover art)
                // -----------------------------------------------------------
                b"DIIN" => {
                    let diin_end = chunk_payload_start + chunk_size;
                    while self.file.seek(SeekFrom::Current(0))? < diin_end {
                        let mut sub_id = [0u8; 4];
                        if self.file.read_exact(&mut sub_id).is_err() {
                            break;
                        }
                        let sub_size = match self.read_be_u64() {
                            Ok(s) => s,
                            Err(_) => break,
                        };
                        let sub_start = self.file.seek(SeekFrom::Current(0))?;

                        match &sub_id {
                            b"DITI" | b"DIAR" => {
                                if let Ok(raw) = self.read_payload(sub_size) {
                                    self.store_text_tag(&sub_id, &raw);
                                    // read_payload already consumed + padded
                                    continue;
                                }
                            }
                            // Cover art extension (AudioGate / some mastering tools)
                            b"ALCH" => {
                                if let Ok(raw) = self.read_payload(sub_size) {
                                    self.metadata.cover_art.push(MetaPicture {
                                        data: raw,
                                        ..Default::default()
                                    });
                                    continue;
                                }
                            }
                            _ => {}
                        }

                        let padded = (sub_size + 1) & !1u64;
                        self.file.seek(SeekFrom::Start(sub_start + padded))?;
                    }
                    // Ensure we land exactly at diin_end even if a sub-chunk was malformed
                    self.file.seek(SeekFrom::Start(diin_end))?;
                    if diin_end & 1 != 0 {
                        self.file.seek(SeekFrom::Current(1))?;
                    }
                }

                // Top-level extended text chunks
                //   DIAL  album
                //   DIGN  genre
                //   DICR  copyright
                //   DIFC  comment / notes
                b"DIAL" | b"DIGN" | b"DICR" | b"DIFC" => {
                    if let Ok(raw) = self.read_payload(chunk_size) {
                        self.store_text_tag(&chunk_id, &raw);
                        // read_payload consumed + padded already
                        continue;
                    }
                    let padded = (chunk_size + 1) & !1u64;
                    self.file
                        .seek(SeekFrom::Start(chunk_payload_start + padded))?;
                }

                // ID3  – raw ID3v2 block; takes priority, store once.
                // Because ID3 may appear before native text chunks in some
                // files, we store the raw bytes here and overwrite any
                // already-parsed native tags below after the loop.
                b"ID3 " => {
                    if self.metadata.id3_raw.is_none() {
                        if let Ok(raw) = self.read_payload(chunk_size) {
                            if !raw.is_empty() {
                                let cursor = std::io::Cursor::new(&raw);
                                if let Ok(tag) = id3::Tag::read_from(cursor) {
                                    self.metadata.update_from_id3(tag);
                                }
                                self.metadata.id3_raw = Some(raw);
                                continue;
                            }
                        }
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
                let channel_frame_size = (fs as usize / 8)
                    .checked_div(self.dst_framerate as usize)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid DST frame size")
                    })?;
                self.dst_channel_frame_size = channel_frame_size;
                self.dst_decoder = if cfg!(feature = "dstdec") {
                    Some(dst_dec::Decoder::new(self.ch, self.dst_channel_frame_size))
                } else {
                    None
                };

                let total_frames = (self.dst_frame_count as u64)
                    .saturating_mul(self.dst_channel_frame_size as u64);
                format.total_samples = total_frames;
                self.total_frames = total_frames;

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
            if self.dst_frame_count != 0 && (self.dsti_index.len() as u32) != self.dst_frame_count {
                eprintln!(
                    "warning: DSTI entries ({}) != FRTE frame_count ({})",
                    self.dsti_index.len(),
                    self.dst_frame_count
                );
            }
        }

        Ok(())
    }

    fn get_metadata(&self) -> Option<&DSDMeta> {
        Some(&self.metadata)
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
                        let frames_to_read = (bytes_per_channel - written).min(self.block_frames);
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

                        let current_frame_nr =
                            (self.read_frames / (self.dst_channel_frame_size as u64)) as usize;

                        if !self.dsti_index.is_empty() && current_frame_nr < self.dsti_index.len() {
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
                            if (frame_len & 1) != 0 {
                                self.file.seek(SeekFrom::Current(1))?;
                            }

                            self.decode_dst_frame(frame_len)?;
                            self.filled_frames = self.dst_channel_frame_size;
                            self.pos_frames = 0;
                        } else {
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

                                    self.decode_dst_frame(frame_len)?;
                                    self.filled_frames = self.dst_channel_frame_size;
                                    self.pos_frames = 0;
                                    got_frame = true;
                                    break;
                                } else {
                                    let padded = (chunk_size + 1) & !1u64;
                                    self.file.seek(SeekFrom::Start(payload_start + padded))?;
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
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek overflow"))?;
                self.file
                    .seek(SeekFrom::Start(self.data_start + byte_offset))?;
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

                let target_frame = (sample_index / (self.dst_channel_frame_size as u64)) as usize;
                let target_frame = target_frame.min(self.dsti_index.len().saturating_sub(1));

                self.file
                    .seek(SeekFrom::Start(self.dsti_index[target_frame]))?;
                self.read_frames = (target_frame as u64) * (self.dst_channel_frame_size as u64);
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
