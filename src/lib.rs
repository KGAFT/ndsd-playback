pub mod semaphore;
pub mod dsd_readers;
pub mod players;
pub mod utils;


#[cfg(test)]
mod tests{
    use std::time::Duration;
    use tokio::time::sleep;
    use crate::players;
    use crate::players::{create_player, enumerate_supported_devices};

    #[tokio::test]
    async fn it_works(){

        let devices = enumerate_supported_devices();

        devices.iter().for_each(|device| {
            eprintln!("{:?}{:?}", device.0, device.1);
        });
        let mut player = create_player(devices[1].0.clone()).unwrap();

        player
            .load_new_track(
                "/mnt/hdd/Music/Alphaville - Forever Young (Remastered) (1984_2019) [LP] DSD128/Alphaville - Forever Young (Remastered) (1984_2019) [LP] DSD128.dff".into()
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
            "/home/larry/Desktop/sacd/RUMOURS/Stereo/07 - THE CHAIN.dff".into(),
        )
            .await;
        sleep(Duration::from_millis(15000)).await;

        player.start().await;
        sleep(Duration::from_millis(1000)).await;
        player.seek(0.98f64).await.unwrap();
        loop {
            println!("Progress {}", player.get_pos().await);
            sleep(Duration::from_millis(500)).await;
        }
    }
}

