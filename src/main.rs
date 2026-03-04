
use crate::players::{create_player, enumerate_supported_devices};
use std::time::Duration;
use tokio::time::sleep;

pub mod dsd_readers;
pub mod players;
pub mod semaphore;
pub mod utils;
#[tokio::main]
async fn main() {
    let devices = enumerate_supported_devices();

    let mut player = create_player(devices[1].0.clone()).unwrap();

    player
        .load_new_track(
            "/mnt/ssd/The Axidentals - Axidentally on Purpose (1960) [4-track 3.75 ips, Pure DSD64 flat transfer]/A1 Tangerine.dsf",
        )
        .await;
    player.start().await;
    sleep(Duration::from_millis(3000)).await;
    player.seek(0.9f64).await.unwrap();
    sleep(Duration::from_millis(3000)).await;
    player.pause().await;
    sleep(Duration::from_millis(3000)).await;
    player.play().await;
    sleep(Duration::from_millis(3000)).await;

    player.load_new_track(
        "/mnt/ssd/Alphaville – Forever Young 1984/A3 Big In Japan.dsf".into(),
        )
        .await;
    sleep(Duration::from_millis(3000)).await;

    player.start().await;
    player.seek(0.9f64).await.unwrap();
    sleep(Duration::from_millis(3000)).await;
}
