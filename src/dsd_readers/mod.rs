use crate::dsd_readers::dff_reader::{DFFReader};
use crate::dsd_readers::dsf_reader::DSFReader;
use id3::{Tag, TagLike};
use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};

pub mod dff_reader;
pub mod dsf_reader;
pub mod dst_dec;
#[derive(Clone, Eq, PartialEq, Default, Debug)]

struct MetaPicture {
    pub description: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Eq, PartialEq, Default, Debug)]
pub struct DSDMeta {
    pub artist: Option<String>,
    pub album: Option<String>,
    pub title: Option<String>,
    pub comment: Option<String>,
    pub genre: Option<String>,
    pub lyrics: Vec<String>,
    pub year: Option<u32>,
    pub cover_art: Vec<MetaPicture>,
    pub id3_raw: Option<Vec<u8>>,
}

#[derive(Copy, Clone, Eq, PartialEq, Default, Debug)]
pub struct DSDFormat {
    pub sampling_rate: u32,
    pub num_channels: u32,
    pub total_samples: u64,
    pub is_lsb_first: bool,
}

impl DSDFormat {
    pub fn is_different(&self, other: &Self) -> bool {
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
            if let Some(meta) = reader.get_metadata() {
                meta.pretty_print()
            }

            Ok(Box::new(reader))
        }
        b"FRM8" => {
            // DFF file
            let mut reader = DFFReader::new(path)?;
            let res = reader.open(format);
            let _ = res?;

            if let Some(meta) = reader.get_metadata() {
                meta.pretty_print()
            }
            Ok(Box::new(reader))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown DSD format",
        )),
    }
}

pub trait DSDReader: Send + Sync {
    fn open(&mut self, format: &mut DSDFormat) -> io::Result<()>;
    fn read(&mut self, data: &mut [&mut [u8]], bytes_per_channel: usize) -> io::Result<usize>;
    fn seek_percent(&mut self, percent: f64) -> io::Result<()>;
    fn seek_samples(&mut self, sample_index: u64) -> io::Result<()>;
    fn get_position_frames(&self) -> u64;
    fn get_position_percent(&self) -> f64;

    fn get_metadata(&self) -> Option<&DSDMeta>;
    fn eof(&self) -> bool;

    fn reset(&mut self) -> io::Result<()> {
        self.seek_samples(0)
    }
}

impl DSDMeta {

    pub fn update_from_id3(&mut self, tag: Tag) {
        if let Some(artist) = tag
            .artist()
            .or_else(|| tag.album_artist())
            .or_else(|| tag.artists().and_then(|a| a.first().copied())){
            self.artist = Some(artist.to_string());
        }

        if let Some(album) = tag.album().map(|a| a.to_string()){
            self.album = Some(album);
        }

        if let Some(title) = tag.title().map(|t| t.to_string()){
            self.title = Some(title);
        }

        if let Some(year) = tag.year().map(|y| y as u32){
            self.year = Some(year);
        }
        tag.pictures().for_each(|p| {
            self.cover_art.push(MetaPicture {
                description: p.description.clone(),
                mime_type: p.mime_type.clone(),
                data: p.data.clone()});
        });

        let comment = tag
            .comments()
            .map(|x| format!("|{}| {}: {}", x.lang, x.description, x.text))
            .collect::<Vec<_>>()
            .join("");

        if !comment.is_empty(){
            self.comment = Some(comment);
        }

        tag.lyrics()
            .for_each(|l| self.lyrics.push(format!("|{}| {}: {}", l.lang, l.description, l.text)));
    }

    pub fn from_id3(id3_raw: Vec<u8>) -> Self {
        let mut res = Self::default();
        res.id3_raw = Some(id3_raw);

        let cursor = std::io::Cursor::new(res.id3_raw.as_mut().unwrap());
        if let Ok(tag) = id3::Tag::read_from(cursor) {
            res.update_from_id3(tag);
        }

        res
    }

    pub fn pretty_print(&self) {
        println!("────────── DSD Metadata ──────────");

        if let Some(title) = &self.title {
            println!("Title   : {}", title);
        }
        if let Some(artist) = &self.artist {
            println!("Artist  : {}", artist);
        }
        if let Some(album) = &self.album {
            println!("Album   : {}", album);
        }
        if let Some(year) = self.year {
            println!("Year    : {}", year);
        }
        if let Some(genre) = &self.genre {
            println!("Genre   : {}", genre);
        }

        if let Some(comment) = &self.comment {
            println!("Comment : {}", comment);
        }

        if !self.lyrics.is_empty() {
            println!("Lyrics  :");
            for line in &self.lyrics {
                println!("  {}", line);
            }
        }

        if !self.cover_art.is_empty() {
            println!("Cover   : {} image(s)", self.cover_art.len());
            for (i, pic) in self.cover_art.iter().enumerate() {
                println!(
                    "  [{}] {} ({}, {} bytes)",
                    i,
                    if pic.description.is_empty() {
                        "No description"
                    } else {
                        &pic.description
                    },
                    pic.mime_type,
                    pic.data.len()
                );
            }
        }

        if self.artist.is_none()
            && self.album.is_none()
            && self.title.is_none()
            && self.comment.is_none()
            && self.cover_art.is_empty()
        {
            println!("(no useful metadata found)");
        }

        println!("──────────────────────────────────");
    }
}
