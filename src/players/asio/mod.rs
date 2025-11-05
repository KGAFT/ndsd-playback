use crate::dsd_readers::dsf_reader::DSFReader;
use crate::dsd_readers::{DSDFormat, DSDReader};
use crate::players::DSDPlayer;
use asio_sys::asio_import::{ASIOBool, ASIOBufferInfo, ASIOCallbacks, ASIOCanSampleRate, ASIOCreateBuffers, ASIODriverInfo, ASIOExit, ASIOFuture, ASIOGetBufferSize, ASIOGetChannels, ASIOGetSampleRate, ASIOInit, ASIOIoFormat, ASIOIoFormat_s, ASIOIoFormatType, ASIOIoFormatType_e_kASIODSDFormat, ASIOSampleRate, ASIOSetSampleRate, ASIOTime, kAsioCanDoIoFormat, kAsioGetIoFormat, kAsioSetIoFormat, load_asio_driver, remove_current_driver, ASIOGetChannelInfo, ASIOChannelInfo, ASIOSampleType};
use asio_sys::errors::AsioErrorWrapper::{ASE_OK, ASE_SUCCESS};
use std::ffi::{CString, c_long};
use std::io::Error;
use std::os::raw::{c_char, c_void};
use std::ptr::null_mut;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;

static initialized: AtomicBool = AtomicBool::new(false);

pub struct AsioDSDPlayer {
    current_format: DSDFormat,
    current_reader: Box<dyn DSDReader>,
}

impl AsioDSDPlayer {
    fn check_driver_dsd_support(driver_name: &str) -> bool {
        let c_driver_name = CString::new(driver_name).unwrap();
        unsafe {
            if !load_asio_driver(c_driver_name.into_raw()) {
                return false;
            }
        }
        let mut driver_info = ASIODriverInfo {
            asioVersion: 0,
            driverVersion: 0,
            name: [0; 32],
            errorMessage: [0; 124],
            sysRef: null_mut(),
        };
        unsafe {
            if ASIOInit(&mut driver_info) != ASE_OK as i32 {
                remove_current_driver();
                return false;
            }
        }
        let mut format = ASIOIoFormat {
            FormatType: ASIOIoFormatType_e_kASIODSDFormat,
            future: [0; 508],
        };
        unsafe {
            let mut format_ptr = (&mut format as &mut _) as *mut _ as *mut c_void;
            let mut res = ASIOFuture(kAsioCanDoIoFormat, format_ptr);
            if res != ASE_SUCCESS as i32 {
                eprintln!("Driver does not support dsd format: {}", driver_name);
                ASIOExit();
                remove_current_driver();
                return false;
            }
            res = ASIOFuture(kAsioSetIoFormat, format_ptr);
            if res != ASE_SUCCESS as i32 {
                eprintln!("Failed to set driver to dsd format: {}", driver_name);
                ASIOExit();
                remove_current_driver();
                return false;
            }
            let mut current_format = ASIOIoFormat {
                FormatType: 0,
                future: [0; 508],
            };
            if (ASIOFuture(
                kAsioGetIoFormat,
                (&mut current_format as &mut _) as *mut _ as *mut c_void,
            ) != ASE_SUCCESS as i32
                || current_format.FormatType != ASIOIoFormatType_e_kASIODSDFormat)
            {
                eprintln!("Failed to confirm driver dsd format {}", driver_name);
                ASIOExit();
                remove_current_driver();
                return false;
            }
        }
        unsafe {
            ASIOExit();
            remove_current_driver();
        }
        return true;
    }
    pub fn enumerate_supported_devices() -> Vec<String> {
        let asio = asio_sys::Asio::new();
        let mut result = Vec::new();
        asio.driver_names().iter().for_each(|driver_name| {
            if Self::check_driver_dsd_support(driver_name.as_str()) {
                result.push(driver_name.clone())
            }
        });
        result
    }

    pub fn initialize(driver_name: &str) -> Option<Self> {
        if initialized.load(Relaxed) {
            return None;
        }
        let c_driver_name = CString::new(driver_name).unwrap();
        unsafe {
            if !load_asio_driver(c_driver_name.into_raw()) {
                return None;
            }
        }
        let mut driver_info = ASIODriverInfo {
            asioVersion: 0,
            driverVersion: 0,
            name: [0; 32],
            errorMessage: [0; 124],
            sysRef: null_mut(),
        };
        unsafe {
            if ASIOInit(&mut driver_info) != ASE_OK as i32 {
                remove_current_driver();
                return None;
            }
        }
        let mut format = ASIOIoFormat {
            FormatType: ASIOIoFormatType_e_kASIODSDFormat,
            future: [0; 508],
        };
        unsafe {
            let mut format_ptr = (&mut format as &mut _) as *mut _ as *mut c_void;
            let mut res = ASIOFuture(kAsioCanDoIoFormat, format_ptr);
            if res != ASE_SUCCESS as i32 {
                eprintln!("Driver does not support dsd format: {}", driver_name);
                ASIOExit();
                remove_current_driver();
                return None;
            }
            res = ASIOFuture(kAsioSetIoFormat, format_ptr);
            if res != ASE_SUCCESS as i32 {
                eprintln!("Failed to set driver to dsd format: {}", driver_name);
                ASIOExit();
                remove_current_driver();
                return None;
            }
            let mut current_format = ASIOIoFormat {
                FormatType: 0,
                future: [0; 508],
            };
            if (ASIOFuture(
                kAsioGetIoFormat,
                (&mut current_format as &mut _) as *mut _ as *mut c_void,
            ) != ASE_SUCCESS as i32
                || current_format.FormatType != ASIOIoFormatType_e_kASIODSDFormat)
            {
                eprintln!("Failed to confirm driver dsd format {}", driver_name);
                ASIOExit();
                remove_current_driver();
                return None;
            }
        }
        Some(Self {
            current_format: DSDFormat::default(),
            current_reader: Box::new(DSFReader::empty()),
        })
    }

    fn sample_rate_to_asio(sample_rate: u64) -> ASIOSampleRate {
        let bytes = sample_rate.to_ne_bytes();
        let mut sample_rate = ASIOSampleRate { ieee: [0; 8] };
        for i in 0..8 {
            sample_rate.ieee[i] = bytes[i] as c_char;
        }
        sample_rate
    }

    fn set_sample_rate(sample_rate: &ASIOSampleRate) {
        unsafe {
            if ASIOCanSampleRate(sample_rate.clone()) != ASE_OK as i32 {
                panic!("Unsupported sample rate");
            }
            if ASIOSetSampleRate(sample_rate.clone()) != ASE_OK as i32 {
                panic!("Can't set set the sample rate");
            }
            let mut test_sample_rate = ASIOSampleRate { ieee: [0; 8] };
            if ASIOGetSampleRate(&mut test_sample_rate) != ASE_OK as i32 {
                panic!("Can't check sample rate");
            }
            if !test_sample_rate.ieee.eq(&sample_rate.ieee) {
                panic!("Sample rate check failed!");
            }
        }
    }

    fn init_virtual_buffers(&self) -> bool {
        let mut input_channels: c_long = 0;
        let mut output_channels: c_long = 0;
        unsafe {
            if ASIOGetChannels(&mut input_channels, &mut output_channels) != ASE_OK as i32 {
                panic!("Failed to query channels info!");
            }
            if output_channels < 2 {
                panic!("Unsupported stereo");
            }
            let (mut min_size, mut max_size, mut preferred_size, mut granularity): (
                c_long,
                c_long,
                c_long,
                c_long,
            ) = (0, 0, 0, 0);
            if ASIOGetBufferSize(
                &mut min_size,
                &mut max_size,
                &mut preferred_size,
                &mut granularity,
            ) != ASE_OK as i32
            {
                panic!("Failed to query buffer infos!");
            }
            let preferred_size = max_size;
            let mut output_buffers = [ASIOBufferInfo {
                isInput: 0,
                channelNum: 0,
                buffers: [null_mut(); 2],
            }; 2];
            output_buffers[0].channelNum = 0;
            output_buffers[1].channelNum = 1;
            let mut callbacks = ASIOCallbacks {
                bufferSwitch: Some(buffer_switch),
                sampleRateDidChange: Some(sample_rate_changed),
                asioMessage: Some(asio_message),
                bufferSwitchTimeInfo: Some(buffer_switch_time_info),
            };
            ASIOCreateBuffers(&mut output_buffers[0], 2, preferred_size, &mut callbacks);
            let mut channel_info = ASIOChannelInfo{
                channel: 0,
                isInput: 0,
                isActive: 0,
                channelGroup: 0,
                type_: 0,
                name: [0;32],
            };
            if ASIOGetChannelInfo(&mut channel_info) != ASE_OK as i32 {
                panic!("Failed to query channel info")
            }
            let need_reverse = match channel_info.type_{
                ASIOSTDSDInt8LSB1 => {
                    !self.current_format.is_lsb_first
                }
                ASIOSTDSDInt8MSB1 => {
                    self.current_format.is_lsb_first
                }
                ASIOSTDSDInt8NER8  => {
                    self.current_format.is_lsb_first
                }
                _ => {
                    panic!("unknown channel type");
                }
            };
            need_reverse
        }
    }

    fn setup_params(&self) {
        let sample_rate = Self::sample_rate_to_asio(self.current_format.sampling_rate as u64);
        Self::set_sample_rate(&sample_rate);
        self.init_virtual_buffers();
    }
}

unsafe extern "C" fn asio_message(
    selector: ::std::os::raw::c_long,
    value: ::std::os::raw::c_long,
    message: *mut ::std::os::raw::c_void,
    opt: *mut f64,
) -> ::std::os::raw::c_long {
    0
}

unsafe extern "C" fn sample_rate_changed(sample_rate: ASIOSampleRate) {}

unsafe extern "C" fn buffer_switch(double_buffer_index: c_long, direct_process: ASIOBool) {}

pub unsafe extern "C" fn buffer_switch_time_info(
    params: *mut ASIOTime,
    double_buffer_index: ::std::os::raw::c_long,
    direct_process: ASIOBool,
) -> *mut ASIOTime {
    null_mut()
}

impl DSDPlayer for AsioDSDPlayer {
    fn get_current_position_percents(&self) -> f64 {
        todo!()
    }

    fn pause(&self) {
        todo!()
    }

    fn play(&self) {
        todo!()
    }

    fn get_pos(&self) -> f64 {
        todo!()
    }

    fn stop(&self) {
        todo!()
    }

    fn is_playing(&self) -> bool {
        todo!()
    }

    fn load_new_track(&mut self, filename: &str) {
        self.current_reader = crate::dsd_readers::open_dsd_auto(filename, &mut self.current_format)
            .expect("failed to load file");
    }

    fn seek(&mut self, percent: f64) -> Result<(), Error> {
        todo!()
    }

    fn play_on_current_thread(&mut self) {
        todo!()
    }
}
