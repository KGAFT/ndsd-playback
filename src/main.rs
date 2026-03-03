use crate::players::alsa::{AlsaPlayer, ControlRequest};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::spawn;
use tokio::time::sleep;

pub mod dsd_readers;
pub mod players;
pub mod semaphore;
pub mod utils;
#[tokio::main]
async fn main() {
    let devices = AlsaPlayer::enumerate_supported_devices();
    //let player = AlsaPlayer::new(devices[1].0.to_str().unwrap());

    let mut mpsc = tokio::sync::mpsc::channel(512);
    AlsaPlayer::player_main(devices[1].0.clone(), mpsc.1).await;

    mpsc.0
        .send(ControlRequest::LoadTrack(
            "/mnt/hdd/Music/1983 - Let's Dance (2003 Re-Issue SACD-R)/01 - Modern Love.dff".into(),
        ))
        .await
        .unwrap();
    mpsc.0.send(ControlRequest::Start).await.unwrap();
    sleep(Duration::from_millis(3000)).await;
    mpsc.0.send(ControlRequest::Seek(0.5f64)).await.unwrap();
    sleep(Duration::from_millis(3000)).await;
    mpsc.0.send(ControlRequest::Pause).await.unwrap();
    sleep(Duration::from_millis(3000)).await;
    mpsc.0.send(ControlRequest::Play).await.unwrap();
    sleep(Duration::from_millis(3000)).await;

    mpsc.0
        .send(ControlRequest::LoadTrack(
            "/mnt/hdd/Music/Pixies - Bossanova (1990) [SACD] (2008 MFSL Remaster ISO)/All Over The World.dsf".into(),
        ))
        .await
        .unwrap();
    sleep(Duration::from_millis(3000)).await;

     mpsc.0.send(ControlRequest::Start).await.unwrap();

    loop{}
}
