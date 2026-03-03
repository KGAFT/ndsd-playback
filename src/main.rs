use crate::players::alsa::{AlsaPlayer, ControlRequest};

use crate::players::DSDPlayer;
use std::time::Duration;
use tokio::time::sleep;

pub mod dsd_readers;
pub mod players;
pub mod semaphore;
pub mod utils;
#[tokio::main]
async fn main() {
    let devices = AlsaPlayer::enumerate_supported_devices();
    let player = AlsaPlayer::new(devices[1].0.to_str().unwrap());

    player
        .load_new_track(
            "/mnt/hdd/Music/1983 - Let's Dance (2003 Re-Issue SACD-R)/01 - Modern Love.dff",
        )
        .await;
    player.start().await;
    sleep(Duration::from_millis(3000)).await;
    player.seek(0.5f64).await.unwrap();
    sleep(Duration::from_millis(3000)).await;
    player.pause().await;
    sleep(Duration::from_millis(3000)).await;
    player.play().await;
    sleep(Duration::from_millis(3000)).await;

    player.load_new_track(
            "/mnt/hdd/Music/Pixies - Bossanova (1990) [SACD] (2008 MFSL Remaster ISO)/All Over The World.dsf".into(),
        )
        .await;
    sleep(Duration::from_millis(3000)).await;

    player.start().await;

    loop {
        println!("Pos {}", player.get_pos().await);
        sleep(Duration::from_millis(1000)).await;
    }
}
