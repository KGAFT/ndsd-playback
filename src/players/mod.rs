use std::ffi::CString;
use std::io;
use async_trait::async_trait;
use crate::dsd_readers::DSDFormat;

#[cfg(target_os = "windows")]
pub mod asio;
#[cfg(target_os = "linux")]
pub mod alsa;

/*
#[cfg(target_os = "linux")]
pub fn enumerate_supported_devices() -> Vec<(CString, CString)> {
    alsa::DsdPlayer::enumerate_supported_devices()
}
#[cfg(target_os = "linux")]
pub fn create_player(device_id: CString) -> Option<Box<dyn DSDPlayer>>{
    Some(Box::new(alsa::DsdPlayer::new(device_id.to_str().unwrap())?))
}
#[cfg(target_os = "linux")]
pub fn create_player_and_open(device_id: CString, path: &str) -> Option<Box<dyn DSDPlayer>>{
    Some(Box::new(alsa::DsdPlayer::open(path, device_id.to_str().unwrap())?))
}


 */
#[cfg(target_os = "windows")]

pub fn enumerate_supported_devices() -> Vec<(CString, CString)> {
    asio::AsioDsdPlayer::enumerate_supported_devices()
}

#[cfg(target_os = "windows")]
pub fn create_player(device_id: CString) -> Option<Box<dyn DSDPlayer>>{
    Some(Box::new(asio::AsioDsdPlayer::new(device_id)))
}

#[cfg(target_os = "windows")]
pub fn create_player_and_open(device_id: CString, path: &str) -> Option<Box<dyn DSDPlayer>>{
    Some(Box::new(asio::AsioDsdPlayer::open(device_id, path)))
}
#[async_trait]
pub trait DSDPlayer: Send + Sync{
    async fn start(&mut self);
    async fn pause(&self);
    async fn play(&self);
    async fn get_pos(&self) -> f64;
    async fn stop(&self);
    async fn is_playing(&self) -> bool;
    async fn load_new_track(&mut self, filename: &str);
    async fn seek(&mut self, percent: f64) -> Result<(), io::Error>;
    async fn get_format_info(&self) -> DSDFormat;
}