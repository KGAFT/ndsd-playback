use byteorder::{LittleEndian, ReadBytesExt};
use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use crate::dsd_readers::{DSDFormat, DSDReader};

pub struct DSFReader {
    file: File,
    buf: Vec<u8>,
    ch: usize,
    blocksize: usize,
    filled: usize,
    pos: usize,
    total_samples: u64,
    read_samples: u64,
    data_start: u64, // <---- new: start offset of "data" chunk content
}

impl DSFReader {
    pub(crate) fn new(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            file,
            buf: Vec::new(),
            ch: 0,
            blocksize: 0,
            filled: 0,
            pos: 0,
            total_samples: 0,
            read_samples: 0,
            data_start: 0,
        })
    }

    pub fn empty() -> Self{
        Self {
            file: File::create("super_empty").unwrap(),
            buf: Vec::new(),
            ch: 0,
            blocksize: 0,
            filled: 0,
            pos: 0,
            total_samples: 0,
            read_samples: 0,
            data_start: 0,
        }
    }
}

impl DSDReader for DSFReader {
    fn open(&mut self, format: &mut DSDFormat) -> io::Result<()> {
        let mut ident = [0u8; 4];

        // --- DSD chunk ---
        self.file.read_exact(&mut ident)?;
        if &ident != b"DSD " {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not DSF"));
        }
        let dsd_size = self.file.read_u64::<LittleEndian>()?;
        self.file.seek(SeekFrom::Current(dsd_size as i64 - 12))?;

        // --- fmt chunk ---
        self.file.read_exact(&mut ident)?;
        if &ident != b"fmt " {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fmt chunk missing",
            ));
        }
        let fmt_size = self.file.read_u64::<LittleEndian>()?;
        let format_version = self.file.read_u32::<LittleEndian>()?;
        if format_version != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported format version",
            ));
        }
        let format_id = self.file.read_u32::<LittleEndian>()?;
        if format_id != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported format id",
            ));
        }
        let _channel_type = self.file.read_u32::<LittleEndian>()?;
        let channels = self.file.read_u32::<LittleEndian>()?;
        format.num_channels = channels;
        self.ch = channels as usize;

        let sampling_freq = self.file.read_u32::<LittleEndian>()?;
        format.sampling_rate = sampling_freq;
        let bits_per_sample = self.file.read_u32::<LittleEndian>()?;
        format.is_lsb_first = bits_per_sample == 1;

        let sample_count = self.file.read_u64::<LittleEndian>()?;
        format.total_samples = sample_count;
        self.total_samples = sample_count;

        let block_size = self.file.read_u32::<LittleEndian>()? as usize;
        self.blocksize = block_size;

        self.file.seek(SeekFrom::Current(fmt_size as i64 - 48))?;

        // --- data chunk ---
        self.file.read_exact(&mut ident)?;
        if &ident != b"data" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "data chunk missing",
            ));
        }
        let _data_size = self.file.read_u64::<LittleEndian>()?;

        // mark data start
        self.data_start = self.file.seek(SeekFrom::Current(0))?;

        // allocate buffer
        self.buf.resize(self.blocksize * self.ch, 0);

        Ok(())
    }

    fn read(&mut self, data: &mut [&mut [u8]], bytes_per_channel: usize) -> io::Result<usize> {
        let mut read_bytes = 0usize;
        let mut want = bytes_per_channel;

        while want > 0 {
            if self.pos == self.filled {
                // read next interleaved block
                let to_read = self.blocksize * self.ch;
                self.buf.resize(to_read, 0);
                let n = self.file.read(&mut self.buf)?;
                if n == 0 {
                    return Ok(read_bytes);
                }
                self.filled = n / self.ch;
                self.pos = 0;
            }

            let available = self.filled - self.pos;
            let size = available.min(want);
            for i in 0..self.ch {
                let src_offset = self.blocksize * i + self.pos;
                let src = &self.buf[src_offset..src_offset + size];
                let dst = &mut data[i][read_bytes..read_bytes + size];
                dst.copy_from_slice(src);
            }

            self.pos += size;
            want -= size;
            read_bytes += size;
        }

        self.read_samples = self.read_samples.saturating_add((read_bytes as u64) * 8);
        Ok(read_bytes)
    }

    fn seek_percent(&mut self, percent: f64) -> io::Result<()> {
        if percent < 0.0 || percent > 1.0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "percent out of range",
            ));
        }
        let target_sample = (self.total_samples as f64 * percent) as u64;
        self.seek_samples(target_sample)
    }

    fn seek_samples(&mut self, sample_index: u64) -> io::Result<()> {
        // DSD = 1 bit per sample per channel
        let total_bits = sample_index * self.ch as u64;
        let total_bytes = total_bits / 8;

        // align to nearest block boundary
        let aligned_bytes =
            (total_bytes / (self.blocksize * self.ch) as u64) * (self.blocksize * self.ch) as u64;

        let offset = self.data_start + aligned_bytes;
        self.file.seek(SeekFrom::Start(offset))?;

        self.read_samples = aligned_bytes * 8;
        self.pos = 0;
        self.filled = 0;

        Ok(())
    }

    fn get_position_frames(&self) -> u64 {
        // self.read_samples counts *bits*, so divide by channels and by 1 bit/sample
        // That gives total DSD frames read so far
        if self.ch == 0 {
            return 0;
        }
        self.read_samples / (self.ch as u64)
    }

    fn get_position_percent(&self) -> f64 {
        if self.total_samples == 0 {
            return 0.0;
        }
        let frames = self.get_position_frames();
        (frames as f64 / self.total_samples as f64).min(1.0)
    }
}
