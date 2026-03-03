use crate::dsd_readers;
use crate::dsd_readers::{DSDFormat, DSDReader};
use crate::utils::bit_reverse_table::BIT_REVERSE_TABLE;
use alsa_sys::{SND_PCM_NONBLOCK, SND_PCM_STREAM_PLAYBACK};
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;

use std::{io, ptr};
use std::time::Duration;
use tokio::spawn;
use tokio::sync::{Mutex, mpsc::Receiver};
use tokio::task::JoinHandle;
use tokio::time::sleep;

pub enum ControlRequest {
    LoadTrack(PathBuf),
    Start,
    Stop,
    Seek(f64),
    Pause,
    Play,
}

struct PlayerState {
    reader: Option<Box<dyn DSDReader>>,
    format: DSDFormat,
    setup: Option<AlsaSetup>,
    playing: bool,
    paused: bool,
    first_paused: bool,
    released_pause: bool,
    device_name: CString,
    alsa_buffer: Option<Vec<u8>>,
}

pub struct AlsaPlayer {
    device_name: CString,
    control_flow: Option<JoinHandle<()>>,
}

impl AlsaPlayer {
    pub fn new(device_name: &str) -> Self {
        let device = std::ffi::CString::new(device_name).unwrap();
        Self {
            device_name: device,
            control_flow: None,
        }
    }

    pub async fn player_main(
        device_name: CString,
        mut channel: Receiver<ControlRequest>,
    ) {
        
        std::thread::spawn(move || {
            let mut state: PlayerState = PlayerState {
                reader: None,
                format: Default::default(),
                setup: None,
                playing: false,
                paused: false,
                first_paused: false,
                released_pause: false,
                device_name,
                alsa_buffer: None,
            };
            loop {
                if let Ok(command) = channel.try_recv() {
                    Self::process_command(command, &mut state);
                }
                if !Self::playback_poll(&mut state) {
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        });
        
    }

    fn process_command(command: ControlRequest, state: &mut PlayerState) {
        let mut setup_reload_required = false;
        match command {
            ControlRequest::LoadTrack(path) => {
                let mut format = DSDFormat::default();
                if let Ok(reader) = dsd_readers::open_dsd_auto(path.to_str().unwrap(), &mut format)
                {
                    state.reader = Some(reader);
                    setup_reload_required = format.is_different(&state.format);
                    state.format = format;
                }
            }
            ControlRequest::Start => {
                if let Some(mut reader) = state.reader.as_mut() {
                    if state.setup.is_none() {
                        setup_reload_required = true;
                    }
                    if reader.eof() {
                        let _ = reader.reset();
                    }
                    state.playing = true;
                }
            }
            ControlRequest::Stop => {
                state.playing = false;
            }
            ControlRequest::Seek(f64) => {
                if let Some(mut reader) = state.reader.as_mut() {
                    let _ = reader.seek_percent(f64);
                }
            }
            ControlRequest::Pause => {
                state.paused = true;
                state.first_paused = true;
            }
            ControlRequest::Play => {
                state.paused = false;
                state.released_pause = true;
            }
        }
        if setup_reload_required {
            if state.setup.is_none() {
                state.setup = Some(AlsaSetup::new(state.device_name.clone()).expect("Alsa failed"));
            } else {
                state.setup.as_mut().unwrap().reprepare_alsa_sync();
            }
            let alsa_buffer_size = 8192 * (state.format.sampling_rate / 2822400) as usize;
            let buffers = Buffers::new(alsa_buffer_size);
            state.setup.as_mut().unwrap().buffers = buffers;
            state
                .setup
                .as_mut()
                .unwrap()
                .update_hw_params(&state.format, alsa_buffer_size);
            state.alsa_buffer = Some(vec![0u8; alsa_buffer_size]);
        }
    }

    fn playback_poll(state: &mut PlayerState) -> bool {
        if state.playing {
          
            let alsa_buffer =state.alsa_buffer.as_mut().unwrap();
            let setup = state.setup.as_mut().unwrap();
            let reader = state.reader.as_mut().unwrap();
            let format = &state.format;
            
            if state.paused {
                if state.first_paused {
                    unsafe { alsa::snd_pcm_pause(setup.playback_handle, 1); }
                    state.first_paused = false;
                } 
                return false;
            } else if state.released_pause{
                unsafe { alsa::snd_pcm_pause(setup.playback_handle, 0); }
                state.released_pause = false;
            }
            
            let alsa_buffer_size = alsa_buffer.len();
            let num_channels = format.num_channels;
            
            
            let mut work_slices = setup.buffers.get_slice_for_reader();
            let bytes = match reader.read(&mut work_slices, alsa_buffer_size / num_channels as usize) {
                Ok(b) => b,
                Err(_) => {
                    eprintln!("read error");
                    return false;
                }
            };
            if bytes == 0 {
                return false;
            }
            let write_frames = setup.buffers.populate_alsa_buffer(
                alsa_buffer.as_mut_slice(),
                bytes,
                format.is_lsb_first,
            );
            let alsa_ptr = alsa_buffer.as_ptr() as *const std::ffi::c_void;

            let written = unsafe {
                alsa::snd_pcm_writei(
                    setup.playback_handle,
                    alsa_ptr,
                    write_frames as alsa::snd_pcm_uframes_t,
                )
            };
            if written == -77 {
                eprintln!("cannot write audio frame EBADF");
                state.playing = false;
                return false;
            }
            if written == -32 {
                eprintln!("cannot write audio frame EPIPE");
                state.playing = false;
                return false;
            }
            if written == -86 {
                eprintln!("cannot write audio frame ESTRPIPE");
                state.playing = false;
                return false;
            }
            if reader.eof(){
                state.playing = false;
            }
            return true;
        } else {
            return false;
        }
    }

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
        unsafe {
            alsa::snd_pcm_hw_params_malloc(&mut params);
        }
        unsafe {
            alsa::snd_pcm_hw_params_any(handle, params);
        }
        let mut supported = false;
        unsafe {
            if alsa::snd_pcm_hw_params_test_format(handle, params, alsa::SND_PCM_FORMAT_DSD_U32_BE)
                == 0
            {
                supported = true;
            }
        }
        unsafe {
            alsa::snd_pcm_hw_params_free(params);
        }
        unsafe {
            alsa::snd_pcm_close(handle);
        }
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
}

extern crate alsa_sys as alsa;

#[cfg(target_os = "linux")]
struct Buffers {
    work0: Vec<u8>,
    work1: Vec<u8>,
    alsa_buffer_size: usize,
}
#[cfg(target_os = "linux")]
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

pub struct AlsaSetup {
    playback_handle: *mut alsa::snd_pcm_t,
    hw_params: *mut alsa::snd_pcm_hw_params_t,
    buffers: Buffers,
    current_device: CString,
}

unsafe impl Send for AlsaSetup {}
unsafe impl Sync for AlsaSetup {}

impl AlsaSetup {
    pub fn new(device: CString) -> Option<Self> {
        unsafe {
            let buffers = Buffers::new(1);
            let err: i32;
            let mut playback_handle: *mut alsa::snd_pcm_t = ptr::null_mut();
            let hw_params: *mut alsa::snd_pcm_hw_params_t = ptr::null_mut();

            err = alsa::snd_pcm_open(
                &mut playback_handle,
                device.as_ptr(),
                alsa::SND_PCM_STREAM_PLAYBACK,
                0,
            );
            if err < 0 {
                eprintln!("cannot open audio device: {}", err);
                return None;
            }
            let mut res = Self {
                playback_handle,
                hw_params,
                buffers,
                current_device: device,
            };
            res.setup_params();
            Some(res)
        }
    }
    fn reprepare_alsa_sync(&mut self) {
        unsafe {
            let err: i32;
            alsa::snd_pcm_drain(self.playback_handle);
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

impl Drop for AlsaSetup {
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_drain(self.playback_handle);
            alsa::snd_pcm_close(self.playback_handle);
        }
    }
}
