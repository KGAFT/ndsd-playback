use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use crate::dsd_readers::dff_reader::DFFReader;
use crate::dsd_readers::dsf_reader::DSFReader;

pub mod dsf_reader;
pub mod dff_reader;

#[derive(Copy, Clone, Eq, PartialEq, Default, Debug)]
pub struct DSDFormat {
    pub sampling_rate: u32,
    pub num_channels: u32,
    pub total_samples: u64,
    pub is_lsb_first: bool,
}

impl DSDFormat {
    pub fn is_alsa_update_need(&self, other: &Self) -> bool {
        return self.sampling_rate != other.sampling_rate
            || self.num_channels != other.num_channels;
    }
}

pub fn open_dsd_auto(path: &str, format: &mut DSDFormat) -> io::Result<Box<dyn DSDReader>> {
    let mut file = File::open(path)?;

    let mut ident = [0u8; 4];
    file.read_exact(&mut ident)?;
    file.seek(SeekFrom::Start(0))?; // rewind for the reader itself

    match &ident {
        b"DSD " => {
            // DSF file
            let mut reader = DSFReader::new(path)?;
            reader.open(format)?;
            Ok(Box::new(reader))
        }
        b"FRM8" => {
            // DFF file
            let mut reader = DFFReader::new(path)?;
            reader.open(format)?;
            Ok(Box::new(reader))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown DSD format",
        )),
    }
}

pub trait DSDReader {
    fn open(&mut self, format: &mut DSDFormat) -> io::Result<()>;
    fn read(&mut self, data: &mut [&mut [u8]], bytes_per_channel: usize) -> io::Result<usize>;
    fn seek_percent(&mut self, percent: f64) -> io::Result<()>;
    fn seek_samples(&mut self, sample_index: u64) -> io::Result<()>;
    fn get_position_frames(&self) -> u64;
    fn get_position_percent(&self) -> f64;
}