#[cfg(target_os = "linux")]
use ndsd_read;
#[cfg(target_os = "linux")]
use ndsd_read::{DSDFormat, DSDReader};
#[cfg(target_os = "linux")]
use crate::players::DSDPlayer;
#[cfg(target_os = "linux")]
use crate::utils::bit_reverse_table::BIT_REVERSE_TABLE;
#[cfg(target_os = "linux")]
use alsa_sys::{SND_PCM_NONBLOCK, SND_PCM_STREAM_PLAYBACK};
#[cfg(target_os = "linux")]
use atomic_float::AtomicF64;
#[cfg(target_os = "linux")]
use std::ffi::{CStr, CString, c_char, c_void};
#[cfg(target_os = "linux")]
use std::io::{Error, ErrorKind};
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::ptr;
#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "linux")]
use std::sync::atomic::Ordering::Relaxed;
use ndsd_read::DSDMeta;
#[cfg(target_os = "linux")]
use tokio::sync::Mutex;
#[cfg(target_os = "linux")]
use tokio::sync::mpsc::Sender;
#[cfg(target_os = "linux")]
use tokio::sync::{mpsc, mpsc::Receiver};

#[cfg(target_os = "linux")]
pub enum ControlRequest {
    LoadTrack(PathBuf),
    Start,
    Stop,
    Seek(f64),
    Pause,
    Play,
    Terminate,
}
#[cfg(target_os = "linux")]

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
#[cfg(target_os = "linux")]
#[allow(unused)]
pub struct AlsaPlayer {
    device_name: CString,
    player_thread: std::thread::JoinHandle<()>,
    message_channel: Sender<ControlRequest>,
    current_pos: Arc<AtomicF64>,
    is_playing: Arc<AtomicBool>,
    cur_format: Arc<Mutex<DSDFormat>>,
    cur_meta: Arc<Mutex<Option<DSDMeta>>>,
}
#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl DSDPlayer for AlsaPlayer {
    async fn start(&mut self) {
        self.message_channel
            .send(ControlRequest::Start)
            .await
            .unwrap();
    }

    async fn pause(&self) {
        let _ = self.message_channel.send(ControlRequest::Pause).await;
    }

    async fn play(&self) {
        let _ = self.message_channel.send(ControlRequest::Play).await;
    }

    async fn get_pos(&self) -> f64 {
        self.current_pos.load(Relaxed)
    }

    async fn stop(&self) {
        let _ = self.message_channel.send(ControlRequest::Stop).await;
    }

    async fn is_playing(&self) -> bool {
        self.is_playing.load(Relaxed)
    }

    async fn load_new_track(&mut self, filename: &str) {
        let _ = self
            .message_channel
            .send(ControlRequest::LoadTrack(PathBuf::from(filename)))
            .await;
    }

    async fn seek(&mut self, percent: f64) -> Result<(), Error> {
        let res = self
            .message_channel
            .send(ControlRequest::Seek(percent))
            .await;
        if let Err(_) = res {
            return Err(Error::new(ErrorKind::Other, "Alsa player seek error"));
        }
        Ok(())
    }

    async fn get_format_info(&self) -> DSDFormat {
        self.cur_format.lock().await.clone()
    }

    async fn get_current_file_meta(&self) -> Option<DSDMeta> {
        self.cur_meta.lock().await.clone()
    }
}

#[cfg(target_os = "linux")]
impl AlsaPlayer {
    pub fn new(device_name: &str) -> Self {
        let device = std::ffi::CString::new(device_name).unwrap();
        let mpsc = mpsc::channel::<ControlRequest>(16);
        let cur_pos = Arc::new(AtomicF64::new(0.));
        let is_playing = Arc::new(AtomicBool::new(false));
        let cur_format = Arc::new(Mutex::new(DSDFormat::default()));
        let cur_meta = Arc::new(Mutex::new(None));
        Self {
            device_name: device.clone(),
            player_thread: Self::player_main(
                device,
                mpsc.1,
                cur_pos.clone(),
                is_playing.clone(),
                cur_format.clone(),
                cur_meta.clone(),
            ),
            message_channel: mpsc.0,
            current_pos: cur_pos,
            is_playing,
            cur_format,
            cur_meta,
        }
    }

    fn player_main(
        device_name: CString,
        mut channel: Receiver<ControlRequest>,
        pos: Arc<AtomicF64>,
        is_playing: Arc<AtomicBool>,
        cur_format: Arc<Mutex<DSDFormat>>,
        cur_meta: Arc<Mutex<Option<DSDMeta>>>,
    ) -> std::thread::JoinHandle<()> {
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
                if !state.playing {
                    is_playing.store(false, Relaxed);
                    *cur_format.blocking_lock() = DSDFormat::default();
                    if let Some(cmd) = channel.blocking_recv() {
                        if !Self::process_command(cmd, &mut state, cur_format.clone(), cur_meta.clone()) {
                            break;
                        }
                    }
                } else {
                    if let Ok(cmd) = channel.try_recv() {
                        if !Self::process_command(cmd, &mut state, cur_format.clone(), cur_meta.clone()) {
                            break;
                        }
                    }
                    Self::playback_poll(&mut state);
                    pos.store(
                        state.reader.as_mut().unwrap().get_position_percent(),
                        Relaxed,
                    );
                    is_playing.store(true, Relaxed);
                }
            }
        })
    }

    fn process_command(
        command: ControlRequest,
        state: &mut PlayerState,
        cur_format: Arc<Mutex<DSDFormat>>,
        cur_meta: Arc<Mutex<Option<DSDMeta>>>,
    ) -> bool {
        let mut setup_reload_required = false;
        match command {
            ControlRequest::LoadTrack(path) => {
                let mut format = DSDFormat::default();
                if let Ok(reader) = ndsd_read::open_dsd_auto(path.to_str().unwrap(), &mut format)
                {
                    state.reader = Some(reader);
                    setup_reload_required = format.is_different(&state.format);
                    state.format = format.clone();
                    *cur_format.blocking_lock() = format;
                    *cur_meta.blocking_lock() = state.reader.as_ref().unwrap().get_metadata().map(|meta| meta.clone());
                }
            }
            ControlRequest::Start => {
                if let Some(reader) = state.reader.as_mut() {
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
                if let Some(reader) = state.reader.as_mut() {
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
            ControlRequest::Terminate => {
                return false;
            }
        }
        if setup_reload_required {
            if state.setup.is_none() {
                let res = AlsaSetup::new(state.device_name.clone());
                if res.is_none() {
                    eprintln!("Alsa failed ");
                    return false;
                }
                state.setup = Some(res.unwrap());
            } else {
                state.setup.as_mut().unwrap().reprepare_alsa_sync();
            }
            let alsa_buffer_size = 8192 * (state.format.sampling_rate / 2822400) as usize;
            let buffers = Buffers::new(alsa_buffer_size, state.format.num_channels as usize);
            state.setup.as_mut().unwrap().buffers = buffers;
            state
                .setup
                .as_mut()
                .unwrap()
                .update_hw_params(&state.format, alsa_buffer_size);
            state.alsa_buffer = Some(vec![0u8; alsa_buffer_size]);
        }
        return true;
    }

    fn playback_poll(state: &mut PlayerState) -> bool {
        if state.playing {
            let alsa_buffer = state.alsa_buffer.as_mut().unwrap();
            let setup = state.setup.as_mut().unwrap();
            let reader = state.reader.as_mut().unwrap();
            let format = &state.format;

            if state.paused {
                if state.first_paused {
                    unsafe {
                        alsa::snd_pcm_pause(setup.playback_handle, 1);
                    }
                    state.first_paused = false;
                }
                return false;
            } else if state.released_pause {
                unsafe {
                    alsa::snd_pcm_pause(setup.playback_handle, 0);
                }
                state.released_pause = false;
            }

            let alsa_buffer_size = alsa_buffer.len();
            let num_channels = format.num_channels;

            let mut work_slices = setup.buffers.get_slice_for_reader();
            let bytes =
                match reader.read(&mut work_slices, alsa_buffer_size / num_channels as usize) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("read error {:?}", e);
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
                setup.bytes_per_word,
                setup.word_is_le
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
                unsafe { alsa::snd_pcm_prepare(setup.playback_handle); }
                return true;
            }
            if written == -86 {
                eprintln!("cannot write audio frame ESTRPIPE");
                state.playing = false;
                return false;
            }
            if reader.eof() {
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
            return false;
        }
        unsafe {
            alsa::snd_pcm_hw_params_malloc(&mut params);
        }
        unsafe {
            alsa::snd_pcm_hw_params_any(handle, params);
        }
        let supported = AlsaSetup::detect_dsd_format(handle, params).is_some();
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
                if !name.is_null() {
                    if Self::support_dsd(name) {
                        res.push(( CStr::from_ptr(name).to_owned(),  CStr::from_ptr(name).to_owned()));
                    }
                }
                n = n.offset(1);
                iter = *n;
            }
            res
        }
    }
}
#[cfg(target_os = "linux")]
extern crate alsa_sys as alsa;

#[cfg(target_os = "linux")]
#[allow(unused)]
struct Buffers {
    work: Vec<Vec<u8>>,
    alsa_buffer_size: usize,
    num_channels: usize,
}
#[cfg(target_os = "linux")]
impl Buffers {
    pub fn new(alsa_buffer_size: usize, num_channels: usize) -> Self {
        Self {
            work: (0..num_channels)
                .map(|_| vec![0u8; alsa_buffer_size / num_channels])
                .collect(),
            alsa_buffer_size,
            num_channels,
        }
    }

    pub fn get_slice_for_reader(&mut self) -> Vec<&mut [u8]> {
        self.work.iter_mut().map(|v| v.as_mut_slice()).collect()
    }
    pub fn populate_alsa_buffer(
        &self,
        alsa_buffer: &mut [u8],
        bytes: usize,
        lsb_first: bool,
        bytes_per_word: usize,
        word_is_le: bool,
    ) -> i64 {
        let mut out = 0usize;
        let mut j = 0usize;
        while j + bytes_per_word - 1 < bytes {
            for ch in 0..self.num_channels {
                let mut word = [0u8; 4];
                for k in 0..bytes_per_word {
                    let byte = self.work[ch][j + k];
                    word[k] = if lsb_first { BIT_REVERSE_TABLE[byte as usize] } else { byte };
                }
                // Byte-swap the word for LE formats
                if word_is_le {
                    word[..bytes_per_word].reverse();
                }
                for k in 0..bytes_per_word {
                    alsa_buffer[out] = word[k];
                    out += 1;
                }
            }
            j += bytes_per_word;
        }
        (bytes / bytes_per_word) as i64
    }
    #[allow(unused)]
    pub fn alsa_buffer_size(&self) -> usize {
        self.alsa_buffer_size
    }
}
#[cfg(target_os = "linux")]

pub struct AlsaSetup {
    playback_handle: *mut alsa::snd_pcm_t,
    hw_params: *mut alsa::snd_pcm_hw_params_t,
    buffers: Buffers,
    current_device: CString,
    dsd_format: alsa::snd_pcm_format_t,
    bytes_per_word: usize,
    word_is_le: bool,
}
#[cfg(target_os = "linux")]

unsafe impl Send for AlsaSetup {}
#[cfg(target_os = "linux")]

unsafe impl Sync for AlsaSetup {}
#[cfg(target_os = "linux")]

impl AlsaSetup {
    pub fn new(device: CString) -> Option<Self> {
        unsafe {
            let buffers = Buffers::new(1, 2);
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
                dsd_format: alsa::SND_PCM_FORMAT_DSD_U32_LE,
                bytes_per_word: 4,
                word_is_le: true,
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
            // Detect the best supported DSD format for this device
            let dsd_fmt = Self::detect_dsd_format(self.playback_handle, self.hw_params)
                .expect("no supported DSD format found");
            self.dsd_format = dsd_fmt;
            self.bytes_per_word = match dsd_fmt {
                alsa::SND_PCM_FORMAT_DSD_U8 => 1,
                alsa::SND_PCM_FORMAT_DSD_U16_BE | alsa::SND_PCM_FORMAT_DSD_U16_LE => 2,
                alsa::SND_PCM_FORMAT_DSD_U32_LE | alsa::SND_PCM_FORMAT_DSD_U32_BE => 4,
                _ => panic!("unsupported DSD format"),
            };
            self.word_is_le = matches!(
                dsd_fmt,
                alsa::SND_PCM_FORMAT_DSD_U32_LE | alsa::SND_PCM_FORMAT_DSD_U16_LE
            );
            // Rate is DSD bit-rate divided by bits-per-word (8 for U8, 16 for U16, 32 for U32)
            let rate = format.sampling_rate / 8 / self.bytes_per_word as u32;
            if alsa::snd_pcm_hw_params_set_rate(self.playback_handle, self.hw_params, rate, 0) < 0 {
                panic!("cannot set sample rate");
            }
            if alsa::snd_pcm_hw_params_set_channels(
                self.playback_handle,
                self.hw_params,
                format.num_channels,
            ) < 0
            {
                panic!("cannot set channel count");
            }
            if alsa::snd_pcm_hw_params_set_format(self.playback_handle, self.hw_params, dsd_fmt) < 0
            {
                panic!("cannot set sample format");
            }

            let mut frames: alsa::snd_pcm_uframes_t =
                (alsa_buffer_size / format.num_channels as usize / self.bytes_per_word)
                    as alsa::snd_pcm_uframes_t;
            let mut dir: i32 = 0;
            alsa::snd_pcm_hw_params_set_period_size_near(
                self.playback_handle,
                self.hw_params,
                &mut frames,
                &mut dir,
            );
            let err = alsa::snd_pcm_hw_params(self.playback_handle, self.hw_params);
            if err < 0 {
                panic!("cannot set parameters {}", err);
            }
            if alsa::snd_pcm_prepare(self.playback_handle) < 0 {
                panic!("cannot prepare audio interface for use");
            }
        }
    }

    fn detect_dsd_format(
        handle: *mut alsa::snd_pcm_t,
        params: *mut alsa::snd_pcm_hw_params_t,
    ) -> Option<alsa::snd_pcm_format_t> {

        let candidates = [
            alsa::SND_PCM_FORMAT_DSD_U32_BE,
            alsa::SND_PCM_FORMAT_DSD_U32_LE,
            alsa::SND_PCM_FORMAT_DSD_U16_BE,
            alsa::SND_PCM_FORMAT_DSD_U16_LE,
            alsa::SND_PCM_FORMAT_DSD_U8,
        ];

        for &fmt in &candidates {
            let supported =
                unsafe { alsa::snd_pcm_hw_params_test_format(handle, params, fmt) == 0 };
            if supported {
                return Some(fmt);
            }
        }
        None
    }
}
#[cfg(target_os = "linux")]

impl Drop for AlsaSetup {
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_drain(self.playback_handle);
            alsa::snd_pcm_close(self.playback_handle);
        }
    }
}
