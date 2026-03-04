//! Windows ASIO native DSD player.
//!
//! This module is intentionally written to preserve the **control flow** from the provided C++
//! reference as closely as Rust/FFI allows.

#![cfg(target_os = "windows")]

use crate::dsd_readers::{self, DSDFormat, DSDReader};
use crate::players::DSDPlayer;
use crate::semaphore::Semaphore;

use ndsd_asio_sys::bindings::asio_import as ai;
use ndsd_asio_sys::bindings::errors::AsioErrorWrapper;

use ndsd_asio_sys::AsioMessageSelectors::{
    kAsioEngineVersion, kAsioLatenciesChanged, kAsioResetRequest, kAsioResyncRequest,
    kAsioSelectorSupported, kAsioSupportsInputMonitor, kAsioSupportsTimeCode,
    kAsioSupportsTimeInfo,
};
use ndsd_asio_sys::AsioSampleType::{ASIOSTDSDInt8LSB1, ASIOSTDSDInt8MSB1, ASIOSTDSDInt8NER8};
use std::ffi::{CStr, CString, c_char, c_double, c_long, c_void};
use std::io;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::thread::sleep;
use std::time::Duration;
// ---------------------------------------------------------------------------
// Win32 import (avoid new deps, keep it minimal).
// ---------------------------------------------------------------------------

unsafe extern "system" {
    fn GetDesktopWindow() -> *mut c_void;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DsdFormat {
    Int8Lsb1,
    Int8Msb1,
    Int8Ner8,
}

#[derive(Clone, Copy, Debug)]
struct DsdBufferContext {
    __buffer_size: usize,       // ASIO buffer size in samples (DSD bits)
    channel_buffer_size: usize, // bytes per channel (buffer_size / 8)
    __buffer_bytes: usize,      // same as channel_buffer_size
    sample_format: DsdFormat,
    channels: usize,
    post_output: bool,
}

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

fn asio_ok(code: i32) -> bool {
    code == AsioErrorWrapper::ASE_OK as i32 || code == AsioErrorWrapper::ASE_SUCCESS as i32
}

fn detect_dsd_format(sample_type: i32) -> Option<DsdFormat> {
    if sample_type == ASIOSTDSDInt8LSB1 as i32 {
        Some(DsdFormat::Int8Lsb1)
    } else if sample_type == ASIOSTDSDInt8MSB1 as i32 {
        Some(DsdFormat::Int8Msb1)
    } else if sample_type == ASIOSTDSDInt8NER8 as i32 {
        Some(DsdFormat::Int8Ner8)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Global callback wiring (ASIO requires plain function pointers).
// ---------------------------------------------------------------------------

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut CURRENT_PLAYER: *mut AsioDsdPlayer = null_mut();

extern "C" fn on_buffer_switch(double_buffer_index: c_long, _direct_process: c_long) {
    unsafe {
        let p = CURRENT_PLAYER;
        if p.is_null() {
            return;
        }
        (*p).fill_buffer(double_buffer_index as i32);

        if (*p).dsd_context.post_output {
            let _ = ai::ASIOOutputReady();
        }
    }
}

unsafe extern "C" fn on_sample_rate_changed(_s_rate: c_double) {}

unsafe extern "C" fn on_asio_message(
    selector: c_long,
    value: c_long,
    _message: *mut c_void,
    _opt: *mut f64,
) -> c_long {
    match selector {
        x if x == kAsioSelectorSupported as c_long => match value {
            v if v == kAsioResetRequest as c_long
                || v == kAsioResyncRequest as c_long
                || v == kAsioLatenciesChanged as c_long
                || v == kAsioEngineVersion as c_long
                || v == kAsioSupportsTimeInfo as c_long
                || v == kAsioSupportsTimeCode as c_long
                || v == kAsioSupportsInputMonitor as c_long =>
            {
                1
            }
            _ => 0,
        },
        x if x == kAsioEngineVersion as c_long => 2,
        x if x == kAsioResetRequest as c_long => 1,
        x if x == kAsioResyncRequest as c_long => 1,
        x if x == kAsioLatenciesChanged as c_long => 1,
        _ => 0,
    }
}

extern "C" fn on_buffer_switch_time_info(
    time: *mut ai::ASIOTime,
    double_buffer_index: c_long,
    direct_process: c_long,
) -> *mut ai::ASIOTime {
    // Minimal: do work in bufferSwitch and return original time pointer.
    on_buffer_switch(double_buffer_index, direct_process);
    time
}

struct AsioDsdSetup {
    driver_info: ai::ASIODriverInfo,
    buffer_infos: [ai::ASIOBufferInfo; 32],
    channel_infos: [ai::ASIOChannelInfo; 32],
    callbacks: ai::ASIOCallbacks,
    dsd_supported: bool,
}

impl AsioDsdSetup {
    fn new() -> Self {
        unsafe {
            Self {
                driver_info: std::mem::zeroed(),
                buffer_infos: [ai::ASIOBufferInfo {
                    isInput: 0,
                    channelNum: 0,
                    buffers: [null_mut(); 2],
                }; 32],
                channel_infos: [ai::ASIOChannelInfo {
                    channel: 0,
                    isInput: 0,
                    isActive: 0,
                    channelGroup: 0,
                    type_: 0,
                    name: [0 as c_char; 32],
                }; 32],
                callbacks: ai::ASIOCallbacks {
                    bufferSwitch: Some(on_buffer_switch),
                    sampleRateDidChange: Some(on_sample_rate_changed),
                    asioMessage: Some(on_asio_message),
                    bufferSwitchTimeInfo: Some(on_buffer_switch_time_info),
                },
                dsd_supported: false,
            }
        }
    }

    unsafe fn initialize_driver(&mut self, driver_name: &CStr) -> Result<(), String> {
        unsafe {
            // IMPORTANT: The driver DLL MUST be loaded *before* calling ASIOInit().
            if ai::load_asio_driver(driver_name.as_ptr() as *mut i8) == false {
                return Err("Failed to load ASIO driver".into());
            }

            self.driver_info.asioVersion = 2;
            self.driver_info.sysRef = GetDesktopWindow();

            // copy driver name
            let bytes = driver_name.to_bytes();
            let name_len = bytes
                .len()
                .min(self.driver_info.name.len().saturating_sub(1));
            for i in 0..name_len {
                self.driver_info.name[i] = bytes[i] as c_char;
            }
            self.driver_info.name[name_len] = 0;

            let init_res = ai::ASIOInit(&mut self.driver_info as *mut _);
            if init_res == AsioErrorWrapper::ASE_NotPresent as i32 {
                return Err("ASIO driver not present (did you load it?)".into());
            }
            if init_res != AsioErrorWrapper::ASE_OK as i32 {
                return Err(format!("Failed to initialize ASIO driver: {init_res}"));
            }

            // Check DSD support.
            let mut io_format = ai::ASIOIoFormat {
                FormatType: ai::ASIOIoFormatType_e_kASIODSDFormat,
                future: [0; 508],
            };
            let can_do = ai::ASIOFuture(
                ai::kAsioCanDoIoFormat as i32,
                (&mut io_format as *mut _) as *mut c_void,
            );
            self.dsd_supported = can_do == AsioErrorWrapper::ASE_SUCCESS as i32;

            Ok(())
        }
    }

    unsafe fn get_device_buffer_size(&self) -> Result<(c_long, c_long), String> {
        let mut min_size: c_long = 0;
        let mut max_size: c_long = 0;
        let mut prefer_size: c_long = 0;
        let mut granularity: c_long = 0;
        let err = unsafe {
            ai::ASIOGetBufferSize(
                &mut min_size,
                &mut max_size,
                &mut prefer_size,
                &mut granularity,
            )
        };
        if !asio_ok(err) {
            return Err("Failed to get ASIO buffer size".into());
        }

        let mut buffer_size = prefer_size;

        if buffer_size == 0 {
            buffer_size = prefer_size;
        } else if buffer_size < min_size {
            buffer_size = min_size;
        } else if buffer_size > max_size {
            buffer_size = max_size;
        } else if granularity == -1 {
            let mut log2_of_min_size = 0;
            let mut log2_of_max_size = 0;
            for i in 0..(std::mem::size_of::<c_long>() * 8) {
                let bit = 1i64 << i;
                if (min_size as i64) & bit != 0 {
                    log2_of_min_size = i as i32;
                }
                if (max_size as i64) & bit != 0 {
                    log2_of_max_size = i as i32;
                }
            }

            let mut min_delta = ((buffer_size - (1 << log2_of_min_size)) as i64).abs();
            let mut min_delta_num = log2_of_min_size;

            for i in (log2_of_min_size + 1)..=(log2_of_max_size) {
                let current_delta = ((buffer_size - (1 << i)) as i64).abs();
                if current_delta < min_delta {
                    min_delta = current_delta;
                    min_delta_num = i;
                }
            }

            buffer_size = 1 << min_delta_num;
            if buffer_size < min_size {
                buffer_size = min_size;
            } else if buffer_size > max_size {
                buffer_size = max_size;
            }
        } else if granularity != 0 {
            // Set to an even multiple of granularity, rounding up.
            buffer_size = (buffer_size + granularity - 1) / granularity * granularity;
        }

        Ok((prefer_size, buffer_size))
    }

    unsafe fn set_output_sample_rate(&self, sample_rate: c_double) -> Result<(), String> {
        // Set device sample rate.
        let err = unsafe { ai::ASIOSetSampleRate(sample_rate) };
        if err == AsioErrorWrapper::ASE_NotPresent as i32 {
            return Err("Sample rate not supported".into());
        }
        if !asio_ok(err) {
            return Err(format!("Failed to set sample rate: {err}"));
        }

        const CLOCK_SOURCE_SIZE: usize = 32;
        let mut clock_sources: [ai::ASIOClockSource; CLOCK_SOURCE_SIZE] =
            unsafe { std::mem::zeroed() };
        let mut num_sources: c_long = CLOCK_SOURCE_SIZE as c_long;
        let err = unsafe { ai::ASIOGetClockSources(clock_sources.as_mut_ptr(), &mut num_sources) };
        if !asio_ok(err) {
            return Err("Failed to get clock sources".into());
        }

        let mut current_set = false;
        if num_sources > 0 {
            for i in 0..(num_sources as usize) {
                if clock_sources[i].isCurrentSource != 0 {
                    current_set = true;
                    break;
                }
            }
        }

        if !current_set && num_sources > 1 {
            let err = unsafe { ai::ASIOSetClockSource(clock_sources[0].index) };
            if !asio_ok(err) {
                return Err("Failed to set clock source".into());
            }
        }

        Ok(())
    }
    #[allow(unused_assignments)]
    unsafe fn setup_native_dsd(
        &mut self,
        num_channels: usize,
        sample_rate: c_double,
    ) -> Result<DsdBufferContext, String> {
        if !self.dsd_supported {
            return Err("ASIO driver does not support native DSD".into());
        }

        let mut io_format = ai::ASIOIoFormat {
            FormatType: ai::ASIOIoFormatType_e_kASIODSDFormat,
            future: [0; 508],
        };
        let err = unsafe {
            ai::ASIOFuture(
                ai::kAsioSetIoFormat as i32,
                (&mut io_format as *mut _) as *mut c_void,
            )
        };
        if err != AsioErrorWrapper::ASE_SUCCESS as i32 {
            return Err("Failed to set ASIO IO format to DSD".into());
        }

        // Sample rate + clock source setup.
        unsafe { self.set_output_sample_rate(sample_rate)? };

        // Buffer size calculation with granularity handling.
        let (prefer_size, buffer_size) = unsafe { self.get_device_buffer_size()? };

        for i in 0..32 {
            self.buffer_infos[i].isInput = 0;
            self.buffer_infos[i].channelNum = i as c_long;
            self.buffer_infos[i].buffers[0] = null_mut();
            self.buffer_infos[i].buffers[1] = null_mut();
        }
        unsafe {
            //Reading unaligned fields
            let field_ptr = std::ptr::addr_of!(self.callbacks.bufferSwitch);
            let bwswitch =  field_ptr.read_unaligned() ;
            let field_ptr = std::ptr::addr_of!(self.callbacks.asioMessage);
            let asiomsg = field_ptr.read_unaligned();

            // Safety check: valid callbacks (ASIO requirement).
            if bwswitch.is_none() || asiomsg.is_none() {
                return Err("ASIO callbacks not properly initialized".into());
            }
        }
        // Create buffers with fallback to prefer_size.
        let mut actual_buffer_size: c_long = 0;
        unsafe {
            let res = ai::ASIOCreateBuffers(
                self.buffer_infos.as_mut_ptr(),
                num_channels as i32,
                buffer_size,
                &mut self.callbacks as *mut _,
            );
            if !asio_ok(res) {
                let res2 = ai::ASIOCreateBuffers(
                    self.buffer_infos.as_mut_ptr(),
                    num_channels as i32,
                    prefer_size,
                    &mut self.callbacks as *mut _,
                );
                if !asio_ok(res2) {
                    return Err("Failed to create ASIO buffers".into());
                }
                actual_buffer_size = prefer_size;
            } else {
                actual_buffer_size = buffer_size;
            }
        }

        // Channel infos for all channels (exact loop).
        for i in 0..num_channels {
            self.channel_infos[i].channel = self.buffer_infos[i].channelNum;
            self.channel_infos[i].isInput = self.buffer_infos[i].isInput;
            let err = unsafe { ai::ASIOGetChannelInfo(&mut self.channel_infos[i]) };
            if !asio_ok(err) {
                return Err("Failed to get channel info".into());
            }
        }

        let mut ch0: ai::ASIOChannelInfo = unsafe { std::mem::zeroed() };
        ch0.isInput = 0;
        ch0.channel = 0;
        let err = unsafe { ai::ASIOGetChannelInfo(&mut ch0) };
        if !asio_ok(err) {
            return Err("Failed to get channel info".into());
        }
        let detected_format = detect_dsd_format(ch0.type_)
            .ok_or_else(|| "Unsupported DSD format reported by driver".to_string())?;

        // DSD buffer context calculation.
        let channel_buffer_size = (actual_buffer_size as usize) / 8;
        let mut ctx = DsdBufferContext {
            __buffer_size: actual_buffer_size as usize,
            channel_buffer_size,
            __buffer_bytes: channel_buffer_size,
            sample_format: detected_format,
            channels: num_channels,
            post_output: false,
        };

        // Latencies (exactly after buffer setup).
        let mut in_lat: c_long = 0;
        let mut out_lat: c_long = 0;
        let err = unsafe { ai::ASIOGetLatencies(&mut in_lat, &mut out_lat) };
        if !asio_ok(err) {
            return Err("Failed to get latencies".into());
        }

        // OutputReady support check.
        ctx.post_output = unsafe { asio_ok(ai::ASIOOutputReady()) };

        Ok(ctx)
    }

    unsafe fn cleanup(&mut self) {
        unsafe {
            let _ = ai::ASIOStop();
            let _ = ai::ASIODisposeBuffers();
            let _ = ai::ASIOExit();
            ai::remove_current_driver()
        };
    }
}

pub struct AsioDsdPlayer {
    driver_name: CString,
    setup: Option<AsioDsdSetup>,
    reader: Option<Box<dyn DSDReader>>,
    reader_semaphore: Semaphore,
    format: DSDFormat,
    dsd_context: DsdBufferContext,
    paused: AtomicBool,
    stopped: AtomicBool,
    is_playing: AtomicBool,
    need_bit_reverse: bool,
}

unsafe impl Send for AsioDsdPlayer {}
unsafe impl Sync for AsioDsdPlayer {}


impl AsioDsdPlayer {
    pub fn enumerate_supported_devices() -> Vec<(CString, CString)> {
        let asio = ndsd_asio_sys::bindings::Asio::new();
        asio.driver_names()
            .into_iter()
            .map(|n| {
                let c = CString::new(n).unwrap();
                (c.clone(), c)
            })
            .collect()
    }

    pub fn new(driver_name: CString) -> Self {
        Self {
            driver_name,
            setup: None,
            reader: None,
            reader_semaphore: Semaphore::new(1),
            format: DSDFormat::default(),
            dsd_context: DsdBufferContext {
                __buffer_size: 0,
                channel_buffer_size: 0,
                __buffer_bytes: 0,
                sample_format: DsdFormat::Int8Msb1,
                channels: 0,
                post_output: false,
            },
            paused: AtomicBool::new(false),
            stopped: AtomicBool::new(true),
            is_playing: AtomicBool::new(false),
            need_bit_reverse: false,
        }
    }

    pub fn open(driver_name: CString, path: &str) -> Self {
        let mut p = Self::new(driver_name);
        p.load_new_track(path);
        p
    }

    unsafe fn ensure_driver_initialized(&mut self) -> Result<(), String> {
        if INITIALIZED.swap(true, Relaxed) {
            // Only one ASIO driver instance at a time in this crate.
            // We keep this strict to avoid undefined ASIO global state.
            return Ok(());
        }

        let mut setup = AsioDsdSetup::new();
        unsafe { setup.initialize_driver(CStr::from_ptr(self.driver_name.as_ptr()))? };
        if !setup.dsd_supported {
            unsafe { setup.cleanup() };
            return Err("Driver does not support native DSD".into());
        }

        // Setup native DSD based on file format.
        let channels = self.format.num_channels as usize;
        let sample_rate = self.format.sampling_rate as c_double;
        let ctx = unsafe { setup.setup_native_dsd(channels, sample_rate)? };
        self.dsd_context = ctx;

        // Decide whether we need to bit-reverse file data to match driver format.
        // DSFReader exposes is_lsb_first, DFF reader likely sets it accordingly.
        let file_is_lsb = self.format.is_lsb_first;
        self.need_bit_reverse = match self.dsd_context.sample_format {
            DsdFormat::Int8Lsb1 => !file_is_lsb,
            DsdFormat::Int8Msb1 => file_is_lsb,
            DsdFormat::Int8Ner8 => file_is_lsb,
        };

        self.setup = Some(setup);
        Ok(())
    }

    unsafe fn start(&mut self) -> Result<(), String> {
        unsafe {
            if self.reader.is_none() {
                return Err("No file loaded".into());
            }
            if self.setup.is_none() {
                self.ensure_driver_initialized()?;
            }

            CURRENT_PLAYER = self as *mut _;
            let res = ai::ASIOStart();
            if !asio_ok(res) {
                return Err("Failed to start ASIO".into());
            }

            self.stopped.store(false, Relaxed);
            self.paused.store(false, Relaxed);
            self.is_playing.store(true, Relaxed);
            Ok(())
        }
    }

    unsafe fn stop_internal(&mut self) {
        if !self.stopped.swap(true, Relaxed) {
            unsafe {
                let _ = ai::ASIOStop();
            }
        }
        self.is_playing.store(false, Relaxed);
    }

    unsafe fn cleanup_internal(&mut self) {
        unsafe {
            self.stop_internal();
            if let Some(mut setup) = self.setup.take() {
                setup.cleanup();
            }
            CURRENT_PLAYER = null_mut();
            INITIALIZED.store(false, Relaxed);
        }
    }

    unsafe fn fill_buffer(&mut self, buffer_index: i32) {
        if self.stopped.load(Relaxed) || self.paused.load(Relaxed) {
            // While paused: output DSD silence.
            unsafe { self.fill_silence(buffer_index) };
            return;
        }

        let Some(setup) = self.setup.as_mut() else {
            unsafe { self.fill_silence(buffer_index) };
            return;
        };
        let Some(reader) = self.reader.as_mut() else {
            unsafe { self.fill_silence(buffer_index) };
            return;
        };

        let bytes_per_channel = self.dsd_context.channel_buffer_size;
        let channels = self.dsd_context.channels;

        // Build slices directly over the ASIO planar buffers.
        let mut out_slices: Vec<&mut [u8]> = Vec::with_capacity(channels);
        for ch in 0..channels {
            unsafe {
                let ptr = setup.buffer_infos[ch].buffers[buffer_index as usize] as *mut u8;
                if ptr.is_null() {
                    self.fill_silence(buffer_index);
                    return;
                }
                let slice = std::slice::from_raw_parts_mut(ptr, bytes_per_channel);
                out_slices.push(slice);
            }
        }

        self.reader_semaphore.acquire();
        let read_res = reader.read(out_slices.as_mut_slice(), bytes_per_channel);
        self.reader_semaphore.release();

        let bytes = match read_res {
            Ok(b) => b,
            Err(_) => 0,
        };

        if bytes == 0 {
            unsafe {
                self.fill_silence(buffer_index);
                self.stop_internal();
            }
            return;
        }

        // Convert MSB<->LSB if needed (bit reversal per byte).
        if self.need_bit_reverse {
            for s in out_slices.iter_mut() {
                for b in &mut s[..bytes] {
                    *b = BIT_REVERSE_TABLE[*b as usize];
                }
            }
        }
    }

    unsafe fn fill_silence(&mut self, buffer_index: i32) {
        let Some(setup) = self.setup.as_mut() else {
            return;
        };
        let bytes_per_channel = self.dsd_context.channel_buffer_size;
        for ch in 0..self.dsd_context.channels {
            unsafe {
                let ptr = setup.buffer_infos[ch].buffers[buffer_index as usize] as *mut u8;
                if ptr.is_null() {
                    continue;
                }
                let slice = std::slice::from_raw_parts_mut(ptr, bytes_per_channel);
                slice.fill(0x69); // DSD silence.
            }
        }
    }
}
#[async_trait::async_trait]
impl DSDPlayer for AsioDsdPlayer {


    async fn pause(&self) {
        self.paused.store(true, Relaxed);
        self.is_playing.store(false, Relaxed);
    }

    async fn play(&self) {
        self.paused.store(false, Relaxed);
        self.is_playing.store(true, Relaxed);
    }

    async fn get_pos(&self) -> f64 {
        if let Some(reader) = self.reader.as_ref() {
            reader.get_position_percent()
        } else {
            0.0
        }
    }

    async fn stop(&self) {
        self.stopped.store(true, Relaxed);
        self.is_playing.store(false, Relaxed);
        unsafe {
            let p = CURRENT_PLAYER;
            if !p.is_null() {
                (*p).stop_internal();
            }
        }
    }

    async fn is_playing(&self) -> bool {
        self.is_playing.load(Relaxed) && !self.paused.load(Relaxed) && !self.stopped.load(Relaxed)
    }

    async fn load_new_track(&mut self, filename: &str) {
        let mut format = DSDFormat::default();
        let reader = dsd_readers::open_dsd_auto(filename, &mut format).expect("Failed to open DSD");

        let need_full_reset = self.format.is_different(&format);

        if need_full_reset {
            unsafe {
                self.cleanup_internal(); // Full driver teardown
            }
            self.reader = Some(reader);
            self.format = format.clone();
            self.stopped.store(false, Relaxed);
            unsafe {
                self.ensure_driver_initialized().expect("Failed to initialize ASIO");
            }
        } else {
            self.reader_semaphore.acquire();
            self.reader = Some(reader);
            self.format = format.clone();
            self.stopped.store(false, Relaxed);

            unsafe {
                let _ = ai::ASIOStop();
                let _ = ai::ASIOStart();
            }
            self.reader_semaphore.release();
        }

        // Update bit reversal logic
        let file_is_lsb = format.is_lsb_first;
        self.need_bit_reverse = match self.dsd_context.sample_format {
            DsdFormat::Int8Lsb1 => !file_is_lsb,
            DsdFormat::Int8Msb1 => file_is_lsb,
            DsdFormat::Int8Ner8 => file_is_lsb,
        };
    }
    async fn seek(&mut self, percent: f64) -> Result<(), io::Error> {
        self.reader_semaphore.acquire();
        let res = if let Some(reader) = self.reader.as_mut() {
            reader.seek_percent(percent)
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "no reader"))
        };
        self.reader_semaphore.release();
        res
    }

    async fn get_format_info(&self) -> DSDFormat {
        self.format.clone()
    }

    async fn start(&mut self) {
        unsafe {
            // Ensure ASIO is started.
            if self.setup.is_none() {
                if self.reader.is_some() {
                    let _ = self.ensure_driver_initialized();
                }
            }
            let _ = self.start();
        }
    }
}

impl Drop for AsioDsdPlayer {
    fn drop(&mut self) {
        unsafe {
            self.cleanup_internal();
        }
    }
}
