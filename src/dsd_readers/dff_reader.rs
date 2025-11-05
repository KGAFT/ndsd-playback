use byteorder::{BigEndian, ReadBytesExt};
use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use crate::dsd_readers::{DSDFormat, DSDReader};

pub struct DFFReader {
    file: File,
    buf: Vec<u8>,          // internal interleaved read buffer (bytes: frames * channels)
    ch: usize,             // channels
    block_frames: usize,   // frames per internal read block (1 frame == 1 byte per channel)
    filled_frames: usize,  // frames currently in buf
    pos_frames: usize,     // current read position in frames inside buf
    total_frames: u64,     // total frames (samples per channel)
    read_frames: u64,      // frames read so far (position)
    data_start: u64,       // start offset of DSD chunk payload
}

impl DFFReader {
    pub fn new(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            file,
            buf: Vec::new(),
            ch: 0,
            block_frames: 4096, // default frames per block
            filled_frames: 0,
            pos_frames: 0,
            total_frames: 0,
            read_frames: 0,
            data_start: 0,
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
        }
    }

    // helper: read 4-byte id
    fn read_id(&mut self) -> io::Result<[u8; 4]> {
        let mut id = [0u8; 4];
        self.file.read_exact(&mut id)?;
        Ok(id)
    }

    // helper: read big-endian u64 (DFF/DSDIFF uses big-endian for chunk sizes)
    fn read_be_u64(&mut self) -> io::Result<u64> {
        self.file.read_u64::<BigEndian>()
    }
}

impl DSDReader for DFFReader {
    fn open(&mut self, format: &mut DSDFormat) -> io::Result<()> {
        // --- FRM8 header (big-endian) ---
        let id = self.read_id()?;
        if &id != b"FRM8" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not FRM8 / DFF"));
        }

        // FRM8 size (big-endian u64) - skip/validate
        let _frm8_size = self.read_be_u64()?; // we don't strictly need it here

        // Next 4 bytes: format id - should be "DSD "
        let fmt_id = self.read_id()?;
        if &fmt_id != b"DSD " {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not DSD container"));
        }

        // We'll parse chunks until we find the "DSD " audio chunk.
        // PROP chunk contains SND subchunks (FS, CHNL, CMPR).
        let found_dsd;
        let dsd_chunk_size: u64;

        // metadata we will fill from PROP/SND
        let mut sample_rate_hz: Option<u32> = None;
        let mut channels: Option<u16> = None;
        // `lsbitfirst` in original C++ was configurable; default false here
        format.is_lsb_first = false;

        loop {
            // read chunk header: 4-byte id + 8-byte BE size
            let mut chunk_id = [0u8; 4];
            // If we reach EOF unexpectedly, error
            if let Err(e) = self.file.read_exact(&mut chunk_id) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unexpected EOF reading chunk id: {}", e)));
            }
            let chunk_size = self.read_be_u64()?;
            let chunk_payload_start = self.file.seek(SeekFrom::Current(0))?;

            match &chunk_id {
                b"PROP" => {
                    // PROP payload starts with a 4-byte prop id (e.g. "SND ")
                    let mut prop_id = [0u8; 4];
                    self.file.read_exact(&mut prop_id)?;
                    if &prop_id == b"SND " {
                        // Parse subchunks inside SND until end of PROP
                        let prop_end = chunk_payload_start + chunk_size;
                        while self.file.seek(SeekFrom::Current(0))? < prop_end {
                            // subchunk header: 4-byte id + 8-byte BE size
                            let mut sub_id = [0u8; 4];
                            if let Err(e) = self.file.read_exact(&mut sub_id) {
                                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unexpected EOF in SND subchunk id: {}", e)));
                            }
                            let sub_size = self.read_be_u64()?;
                            let sub_payload_start = self.file.seek(SeekFrom::Current(0))?;

                            match &sub_id {
                                b"FS  " => {
                                    // sample rate (big-endian u32)
                                    if sub_size >= 4 {
                                        let sr = self.file.read_u32::<BigEndian>()?;
                                        sample_rate_hz = Some(sr);
                                    } else {
                                        // invalid FS subchunk: skip
                                        self.file.seek(SeekFrom::Start(sub_payload_start + sub_size))?;
                                    }
                                }
                                b"CHNL" => {
                                    // channels (big-endian u16)
                                    if sub_size >= 2 {
                                        let ch = self.file.read_u16::<BigEndian>()?;
                                        channels = Some(ch);
                                    } else {
                                        self.file.seek(SeekFrom::Start(sub_payload_start + sub_size))?;
                                    }
                                }
                                b"CMPR" => {
                                    // compression id (4 bytes), we accept only "DSD "
                                    if sub_size >= 4 {
                                        let mut cmp = [0u8; 4];
                                        self.file.read_exact(&mut cmp)?;
                                        if &cmp != b"DSD " {
                                            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported CMPR (not DSD)"));
                                        }
                                    } else {
                                        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid CMPR chunk"));
                                    }
                                }
                                _ => {
                                    // ignore unknown subchunk
                                }
                            }

                            // subchunk payloads are padded to even length per spec:
                            // padded = (sub_size + 1) & ~1
                            let padded = (sub_size + 1) & !1u64;
                            self.file.seek(SeekFrom::Start(sub_payload_start + padded))?;
                        }
                    } else {
                        // not a SND PROP variant - skip PROP payload (padded)
                        let padded = (chunk_size + 1) & !1u64;
                        self.file.seek(SeekFrom::Start(chunk_payload_start + padded))?;
                    }
                }

                b"DSD " => {
                    // Found audio chunk
                    found_dsd = true;
                    dsd_chunk_size = chunk_size;
                    // data_start points at the beginning of the audio payload
                    self.data_start = self.file.seek(SeekFrom::Current(0))?;
                    // break: we have what we need to prepare decoding
                    break;
                }

                _ => {
                    // skip unknown chunk payload (padded to even)
                    let padded = (chunk_size + 1) & !1u64;
                    self.file.seek(SeekFrom::Start(chunk_payload_start + padded))?;
                }
            }
        }

        if !found_dsd {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "DSD chunk not found"));
        }

        // require CHNL and FS
        let channels = channels.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CHNL missing"))?;
        let fs = sample_rate_hz.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "FS missing"))?;

        // Fill format and internal fields
        format.num_channels = channels as u32;
        self.ch = channels as usize;
        format.sampling_rate = fs;

        // total_frames = chunk_size / channels (1 byte per channel per frame)
        let total_frames = dsd_chunk_size / (self.ch as u64);
        format.total_samples = total_frames; // samples == frames per channel
        self.total_frames = total_frames;

        // allocate buffer: block_frames * channels bytes
        self.buf.resize(self.block_frames * self.ch, 0);

        Ok(())
    }

    fn read(&mut self, data: &mut [&mut [u8]], bytes_per_channel: usize) -> io::Result<usize> {
        // Mirror C++ decode loop semantics: produce bytes_per_channel bytes per channel
        if self.ch == 0 {
            return Ok(0);
        }
        if data.len() < self.ch {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not enough channel buffers"));
        }

        // `written` is how many bytes per channel we've written so far
        let mut written = 0usize;

        while written < bytes_per_channel {
            // refill internal buffer if exhausted
            if self.pos_frames == self.filled_frames {
                // frames to read in this pass (try to fill block_frames but at least enough to fulfill request)
                let frames_to_read = (bytes_per_channel - written).min(self.block_frames);
                let bytes_to_read = frames_to_read * self.ch;
                self.buf.resize(bytes_to_read, 0);
                let n = self.file.read(&mut self.buf)?;
                if n == 0 {
                    // EOF
                    return Ok(written);
                }
                // n should be multiple of channels; compute frames
                self.filled_frames = n / self.ch;
                self.pos_frames = 0;
            }

            let available_frames = self.filled_frames - self.pos_frames;
            let need_frames = bytes_per_channel - written;
            let take_frames = available_frames.min(need_frames);

            // deinterleave `take_frames` frames from buf into channel slices
            // frames are interleaved: for each frame f: byte0..byte(ch-1)
            for ch_idx in 0..self.ch {
                let dst = &mut data[ch_idx][written..written + take_frames];
                // copy bytes for this channel from each frame with stride
                let mut dst_i = 0usize;
                let mut src_offset = self.pos_frames * self.ch + ch_idx;
                for _ in 0..take_frames {
                    dst[dst_i] = self.buf[src_offset];
                    dst_i += 1;
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
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "percent out of range"));
        }
        let target_frame = (self.total_frames as f64 * percent) as u64;
        self.seek_samples(target_frame)
    }

    fn seek_samples(&mut self, sample_index: u64) -> io::Result<()> {
        // In DFF: 1 frame => 1 byte per channel; byte offset = sample_index * channels
        let byte_offset = sample_index.checked_mul(self.ch as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek overflow"))?;
        let offset = self.data_start + byte_offset;
        self.file.seek(SeekFrom::Start(offset))?;
        self.read_frames = sample_index;
        self.pos_frames = 0;
        self.filled_frames = 0;
        Ok(())
    }

    fn get_position_frames(&self) -> u64 {
        self.read_frames
    }

    fn get_position_percent(&self) -> f64 {
        if self.total_frames == 0 {
            0.0
        } else {
            (self.get_position_frames() as f64 / self.total_frames as f64).min(1.0)
        }
    }
}
