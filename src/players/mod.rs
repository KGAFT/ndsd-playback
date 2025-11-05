use std::ffi::CString;
use std::io;
use crate::dsd_readers::DSDFormat;

#[cfg(target_os = "windows")]
pub mod asio;
#[cfg(target_os = "linux")]
mod alsa;

#[cfg(target_os = "linux")]
pub fn enumerate_supported_devices() -> Vec<(CString, CString)> {
    alsa::DsdPlayer::enumerate_supported_devices()
}
#[cfg(target_os = "linux")]
pub fn create_player(device_id: CString) -> Box<dyn DSDPlayer>{
    Box::new(alsa::DsdPlayer::new(device_id.to_str().unwrap()))
}
#[cfg(target_os = "linux")]
pub fn create_player_and_open(device_id: CString, path: &str) -> Box<dyn DSDPlayer>{
    Box::new(alsa::DsdPlayer::open(path, device_id.to_str().unwrap()))
}

#[cfg(target_os = "windows")]

pub fn enumerate_supported_devices() -> Vec<(CString, CString)> {
    todo!()
}

#[cfg(target_os = "windows")]
pub fn create_player(device_id: CString) -> Box<dyn DSDPlayer>{
    todo!()
}

#[cfg(target_os = "windows")]
pub fn create_player_and_open(device_id: CString, path: &str) -> Box<dyn DSDPlayer>{
    todo!()
}

pub trait DSDPlayer{
    fn get_current_position_percents(&self) -> f64;
    fn pause(&self);
    fn play(&self);
    fn get_pos(&self) -> f64;
    fn stop(&self);
    fn is_playing(&self) -> bool;
    fn load_new_track(&mut self, filename: &str);
    fn seek(&mut self, percent: f64) -> Result<(), io::Error>;
    fn play_on_current_thread(&mut self);

    fn get_format_info(&self) -> DSDFormat;
}