//There must be dsd player, with ability to play dsd without pcm conversion
//But asio is hard af, also needed UAC2 for linux/android
use crate::dsd_readers;
#[cfg(target_os = "linux")]
use crate::dsd_readers::{DSDFormat, DSDReader};
use crate::semaphore::Semaphore;
#[cfg(target_os = "linux")]
use alsa_sys::{SND_PCM_NONBLOCK, SND_PCM_STREAM_PLAYBACK};
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::sleep;
use std::time::Duration;
use std::{io, ptr};
use crate::players::DSDPlayer;

#[cfg(target_os = "linux")]
extern crate alsa_sys as alsa;
#[cfg(target_os = "linux")]
static BIT_REVERSE_TABLE: [u8; 256] = [
    0x00, 0x80, 0x40, 0xc0, 0x20, 0xa0, 0x60, 0xe0, 0x10, 0x90, 0x50, 0xd0, 0x30, 0xb0, 0x70, 0xf0,
    0x08, 0x88, 0x48, 0xc8, 0x28, 0xa8, 0x68, 0xe8, 0x18, 0x98, 0x58, 0xd8, 0x38, 0xb8, 0x78, 0xf8,
    0x04, 0x84, 0x44, 0xc4, 0x24, 0xa4, 0x64, 0xe4, 0x14, 0x94, 0x54, 0xd4, 0x34, 0xb4, 0x74, 0xf4,
    0x0c, 0x8c, 0x4c, 0xcc, 0x2c, 0xac, 0x6c, 0xec, 0x1c, 0x9c, 0x5c, 0xdc, 0x3c, 0xbc, 0x7c, 0xfc,
    0x02, 0x82, 0x42, 0xc2, 0x22, 0xa2, 0x62, 0xe2, 0x12, 0x92, 0x52, 0xd2, 0x32, 0xb2, 0x72, 0xf2,
    0x0a, 0x8a, 0x4a, 0xca, 0x2a, 0xaa, 0x6a, 0xea, 0x1a, 0x9a, 0x5a, 0xda, 0x3a, 0xba, 0x7a, 0xfa,
    0x06, 0x86, 0x46, 0xc6, 0x26, 0xa6, 0x66, 0xe6, 0x16, 0x96, 0x56, 0xd6, 0x36, 0xb6, 0x76, 0xf6,
    0x0e, 0x8e, 0x4e, 0xce, 0x2e, 0xae, 0x6e, 0xee, 0x1e, 0x9e, 0x5e, 0xde, 0x3e, 0xbe, 0x7e, 0xfe,
    0x01, 0x81, 0x41, 0xc1, 0x21, 0xa1, 0x61, 0xe1, 0x11, 0x91, 0x51, 0xd1, 0x31, 0xb1, 0x71, 0xf1,
    0x09, 0x89, 0x49, 0xc9, 0x29, 0xa9, 0x69, 0xe9, 0x19, 0x99, 0x59, 0xd9, 0x39, 0xb9, 0x79, 0xf9,
    0x05, 0x85, 0x45, 0xc5, 0x25, 0xa5, 0x65, 0xe5, 0x15, 0x95, 0x55, 0xd5, 0x35, 0xb5, 0x75, 0xf5,
    0x0d, 0x8d, 0x4d, 0xcd, 0x2d, 0xad, 0x6d, 0xed, 0x1d, 0x9d, 0x5d, 0xdd, 0x3d, 0xbd, 0x7d, 0xfd,
    0x03, 0x83, 0x43, 0xc3, 0x23, 0xa3, 0x63, 0xe3, 0x13, 0x93, 0x53, 0xd3, 0x33, 0xb3, 0x73, 0xf3,
    0x0b, 0x8b, 0x4b, 0xcb, 0x2b, 0xab, 0x6b, 0xeb, 0x1b, 0x9b, 0x5b, 0xdb, 0x3b, 0xbb, 0x7b, 0xfb,
    0x07, 0x87, 0x47, 0xc7, 0x27, 0xa7, 0x67, 0xe7, 0x17, 0x97, 0x57, 0xd7, 0x37, 0xb7, 0x77, 0xf7,
    0x0f, 0x8f, 0x4f, 0xcf, 0x2f, 0xaf, 0x6f, 0xef, 0x1f, 0x9f, 0x5f, 0xdf, 0x3f, 0xbf, 0x7f, 0xff,
];
#[cfg(target_os = "linux")]
struct Buffers {
    work0: Vec<u8>,
    work1: Vec<u8>,
    alsa_buffer_size: usize,
}

impl Buffers {
    pub fn new(alsa_buffer_size: usize) -> Self {
        Self {
            work0: vec![0u8; alsa_buffer_size >> 1],
            work1: vec![0u8; alsa_buffer_size >> 1],
            alsa_buffer_size,
        }
    }

    pub fn get_slice_for_reader(&mut self) -> [&mut [u8]; 2] {
        [self.work0.as_mut_slice(), self.work1.as_mut_slice()]
    }

    pub fn populate_alsa_buffer(
        &self,
        alsa_buffer: &mut [u8],
        bytes: usize,
        lsb_first: bool,
    ) -> i64 {
        let mut i = 0usize;

        if lsb_first {
            // bit reverse per byte
            let mut j = 0usize;
            while j + 3 < bytes {
                alsa_buffer[i + 0] = BIT_REVERSE_TABLE[self.work0[j + 0] as usize];
                alsa_buffer[i + 1] = BIT_REVERSE_TABLE[self.work0[j + 1] as usize];
                alsa_buffer[i + 2] = BIT_REVERSE_TABLE[self.work0[j + 2] as usize];
                alsa_buffer[i + 3] = BIT_REVERSE_TABLE[self.work0[j + 3] as usize];

                alsa_buffer[i + 4] = BIT_REVERSE_TABLE[self.work1[j + 0] as usize];
                alsa_buffer[i + 5] = BIT_REVERSE_TABLE[self.work1[j + 1] as usize];
                alsa_buffer[i + 6] = BIT_REVERSE_TABLE[self.work1[j + 2] as usize];
                alsa_buffer[i + 7] = BIT_REVERSE_TABLE[self.work1[j + 3] as usize];

                i += 8;
                j += 4;
            }
        } else {
            let mut j = 0usize;
            while j + 3 < bytes {
                alsa_buffer[i + 0] = self.work0[j + 0];
                alsa_buffer[i + 1] = self.work0[j + 1];
                alsa_buffer[i + 2] = self.work0[j + 2];
                alsa_buffer[i + 3] = self.work0[j + 3];

                alsa_buffer[i + 4] = self.work1[j + 0];
                alsa_buffer[i + 5] = self.work1[j + 1];
                alsa_buffer[i + 6] = self.work1[j + 2];
                alsa_buffer[i + 7] = self.work1[j + 3];

                i += 8;
                j += 4;
            }
        }
        (bytes / 4) as i64
    }

    pub fn alsa_buffer_size(&self) -> usize {
        self.alsa_buffer_size
    }
}

pub struct DsdPlayer {
    playback_handle: *mut alsa::snd_pcm_t,
    hw_params: *mut alsa::snd_pcm_hw_params_t,
    buffers: Buffers,
    reader: Option<Box<dyn DSDReader>>,
    reader_semaphore: Semaphore,
    format: DSDFormat,
    current_device: CString,
    paused: AtomicBool,
    stoped: AtomicBool,
    is_playing: AtomicBool,
}

impl DsdPlayer {
    pub fn support_dsd(device_name: *const c_char) -> bool {
        let mut handle: *mut alsa::snd_pcm_t = std::ptr::null_mut();
        let mut params: *mut alsa::snd_pcm_hw_params_t = std::ptr::null_mut();
        let err = unsafe {
            alsa::snd_pcm_open(
                &mut handle,
                device_name,
                SND_PCM_STREAM_PLAYBACK,
                SND_PCM_NONBLOCK,
            )
        };
        if err < 0 {
            unsafe {
                eprintln!(
                    "Failed to open device: {}",
                    CString::from(CStr::from_ptr(alsa::snd_strerror(err)))
                        .to_str()
                        .unwrap()
                );
            }
            return false;
        }
        unsafe { alsa::snd_pcm_hw_params_malloc(&mut params); }
        unsafe { alsa::snd_pcm_hw_params_any(handle, params); }
        let mut supported = false;
        unsafe {
            if alsa::snd_pcm_hw_params_test_format(handle, params, alsa::SND_PCM_FORMAT_DSD_U32_BE) == 0
            {
                supported = true;
            }
        }
        unsafe { alsa::snd_pcm_hw_params_free(params); }
        unsafe { alsa::snd_pcm_close(handle); }
        supported
    }

    pub fn enumerate_supported_devices() -> Vec<(CString, CString)> {
        unsafe {
            let pcm_const = CString::new("pcm").unwrap();
            let name_const = CString::new("NAME").unwrap();
            let desc_const = CString::new("DESC").unwrap();

            let mut devices_raw: *mut *mut c_void = std::ptr::null_mut();
            let err = alsa::snd_device_name_hint(-1, pcm_const.as_ptr(), &mut devices_raw);
            if err != 0 {
                eprintln!(
                    "Error getting device hints: {}\n",
                    CString::from(CStr::from_ptr(alsa::snd_strerror(err)))
                        .to_str()
                        .unwrap()
                );
                return vec![];
            }
            let mut res = Vec::new();
            let mut n = devices_raw;
            let mut iter = *n;
            while !iter.is_null() {
                let name = alsa::snd_device_name_get_hint(iter, name_const.as_ptr());
                let desc = alsa::snd_device_name_get_hint(iter, desc_const.as_ptr());
                let name_cstr = CStr::from_ptr(name);
                let desc_cstr = CStr::from_ptr(desc);
                if !name.is_null() {
                    if Self::support_dsd(name) {
                        eprintln!(
                            "cur support: {},{}",
                            name_cstr.to_str().unwrap(),
                            desc_cstr.to_str().unwrap()
                        );
                        res.push((CString::from_raw(name), CString::from_raw(desc)));
                    }
                }
                n = n.offset(1);
                iter = *n;
            }
            res
        }
    }

    pub fn new(device_name: &str) -> Self {
        unsafe {
            let buffers = Buffers::new(1);
            let err: i32;
            let mut playback_handle: *mut alsa::snd_pcm_t = ptr::null_mut();
            let hw_params: *mut alsa::snd_pcm_hw_params_t = ptr::null_mut();

            let device = std::ffi::CString::new(device_name).unwrap();
            err = alsa::snd_pcm_open(
                &mut playback_handle,
                device.as_ptr(),
                alsa::SND_PCM_STREAM_PLAYBACK,
                0,
            );
            if err < 0 {
                panic!("cannot open audio device: {}", err);
            }
            let mut res = Self {
                playback_handle,
                hw_params,
                buffers,
                reader: None,
                format: DSDFormat::default(),
                current_device: device,
                paused: AtomicBool::new(false),
                stoped: AtomicBool::new(false),
                is_playing: AtomicBool::new(false),
                reader_semaphore: Semaphore::new(1),
            };
            res.setup_params();
            res
        }
    }

    pub fn open(filename: &str, device_name: &str) -> Self {
        let mut res = Self::new(device_name);
        res.load_new_track(filename);
        res
    }



    fn reprepare_alsa_sync(&mut self) {
        unsafe {
            let err: i32;
            alsa::snd_pcm_close(self.playback_handle);
            err = alsa::snd_pcm_open(
                &mut self.playback_handle,
                self.current_device.as_ptr(),
                alsa::SND_PCM_STREAM_PLAYBACK,
                0,
            );
            if err < 0 {
                panic!("cannot open audio device: {}", err);
            }
            self.setup_params();
        }
    }



    fn setup_params(&mut self) {
        unsafe {
            if !self.hw_params.is_null() {
                alsa::snd_pcm_hw_params_free(self.hw_params);
            }
            if alsa::snd_pcm_hw_params_malloc(&mut self.hw_params) < 0 {
                panic!("cannot allocate hardware parameter structure");
            }
            if alsa::snd_pcm_hw_params_any(self.playback_handle.clone(), self.hw_params.clone()) < 0
            {
                panic!("cannot initialize hardware parameter structure");
            }
            if alsa::snd_pcm_hw_params_set_access(
                self.playback_handle.clone(),
                self.hw_params.clone(),
                alsa::SND_PCM_ACCESS_RW_INTERLEAVED,
            ) < 0
            {
                panic!("cannot set access type");
            }
        }
    }

    fn update_hw_params(&mut self, format: &DSDFormat, alsa_buffer_size: usize) {
        unsafe {
            let rate = format.sampling_rate / 8 / 4;
            if alsa::snd_pcm_hw_params_set_rate(
                self.playback_handle.clone(),
                self.hw_params.clone(),
                rate,
                0,
            ) < 0
            {
                panic!("cannot set sample rate");
            }
            if alsa::snd_pcm_hw_params_set_channels(
                self.playback_handle.clone(),
                self.hw_params.clone(),
                format.num_channels as u32,
            ) < 0
            {
                panic!("cannot set channel count");
            }
            // set DSD format constant
            if alsa::snd_pcm_hw_params_set_format(
                self.playback_handle.clone(),
                self.hw_params.clone(),
                alsa::SND_PCM_FORMAT_DSD_U32_BE,
            ) < 0
            {
                panic!("cannot set sample format");
            }

            let mut frames: alsa::snd_pcm_uframes_t =
                (alsa_buffer_size / format.num_channels as usize / 4) as alsa::snd_pcm_uframes_t;
            let mut dir: i32 = 0;
            alsa::snd_pcm_hw_params_set_period_size_near(
                self.playback_handle.clone(),
                self.hw_params.clone(),
                &mut frames,
                &mut dir,
            );
            let err = alsa::snd_pcm_hw_params(self.playback_handle.clone(), self.hw_params);
            if err < 0 {
                panic!("cannot set parameters {}", err);
            }
            if alsa::snd_pcm_prepare(self.playback_handle.clone()) < 0 {
                panic!("cannot prepare audio interface for use");
            }
        }
    }
}

impl DSDPlayer for DsdPlayer{
    fn get_current_position_percents(&self) -> f64 {
        if let Some(reader) = self.reader.as_ref() {
            reader.get_position_percent()
        } else {
            0f64
        }
    }

    fn pause(&self) {
        self.paused.store(true, Relaxed);
        self.is_playing.store(false, Relaxed);
    }

    fn play(&self) {
        self.paused.store(false, Relaxed);
    }

    fn get_pos(&self) -> f64 {
        let res = if let Some(reader) = self.reader.as_ref() {
            reader.get_position_percent()
        } else {
            0f64
        };
        res
    }

    fn stop(&self) {
        if !self.stoped.load(Relaxed) {
            self.stoped.store(true, Relaxed);
            unsafe {
                alsa::snd_pcm_drain(self.playback_handle);
            }
        }
    }

    fn is_playing(&self) -> bool {
        self.is_playing.load(Relaxed)
    }

    fn load_new_track(&mut self, filename: &str) {
        self.stop();
        let mut format = DSDFormat {
            sampling_rate: 0,
            num_channels: 0,
            total_samples: 0,
            is_lsb_first: false,
        };
        let reader =
            dsd_readers::open_dsd_auto(filename, &mut format).expect("Failed to open DSD");
        let alsa_buffer_size = 8192 * (format.sampling_rate / 2822400) as usize;

        let buffers = Buffers::new(alsa_buffer_size);
        self.buffers = buffers;

        if format.num_channels != 2 {
            eprintln!("not stereo");
            std::process::exit(1);
        }
        self.reader = Some(reader);
        self.reprepare_alsa_sync();
        self.update_hw_params(&format, self.buffers.alsa_buffer_size);
        self.format = format;
        self.stoped.store(false, Relaxed);
    }

    fn seek(&mut self, percent: f64) -> Result<(), io::Error> {
        self.reader_semaphore.acquire();
        let res = if let Some(reader) = self.reader.as_mut() {
            let res = reader.seek_percent(percent);
            println!("Seeked: {}", percent);
            res
        } else {
            eprintln!("Failed");
            Err(io::Error::last_os_error())
        };
        self.reader_semaphore.release();
        res
    }

    fn play_on_current_thread(&mut self) {
        if self.reader.is_none() {
            return;
        }
        let mut alsa_buffer = vec![0u8; self.buffers.alsa_buffer_size()];
        loop {
            if self.stoped.load(Relaxed) {
                break;
            }
            if self.paused.load(Relaxed) {
                sleep(Duration::from_millis(100));
                continue;
            }
            self.is_playing.store(true, Relaxed);
            let alsa_buffer_size = self.buffers.alsa_buffer_size();
            let num_channels = self.format.num_channels;
            let mut work_slices = self.buffers.get_slice_for_reader();
            self.reader_semaphore.acquire();
            let bytes = match self
                .reader
                .as_mut()
                .unwrap()
                .read(&mut work_slices, alsa_buffer_size / num_channels as usize)
            {
                Ok(b) => b,
                Err(_) => {
                    self.reader_semaphore.release();
                    eprintln!("read error");
                    break;
                }
            };
            self.reader_semaphore.release();
            if bytes == 0 {
                break;
            }
            let write_frames = self.buffers.populate_alsa_buffer(
                alsa_buffer.as_mut_slice(),
                bytes,
                self.format.is_lsb_first,
            );
            let alsa_ptr = alsa_buffer.as_ptr() as *const std::ffi::c_void;

            let written = unsafe {
                alsa::snd_pcm_writei(
                    self.playback_handle,
                    alsa_ptr,
                    write_frames as alsa::snd_pcm_uframes_t,
                )
            };
            if written == -77 {
                eprintln!("cannot write audio frame EBADF");
                break;
            }
            if written == -32 {
                eprintln!("cannot write audio frame EPIPE");
                break;
            }
            if written == -86 {
                eprintln!("cannot write audio frame ESTRPIPE");
                break;
            }
        }
        self.is_playing.store(false, Relaxed);
        self.stoped.store(true, Relaxed);
    }

    fn get_format_info(&self) -> DSDFormat {
        self.format.clone()
    }
}

impl Drop for DsdPlayer {
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_drain(self.playback_handle);
            alsa::snd_pcm_close(self.playback_handle);
        }
    }
}
