pub mod semaphore;
pub mod players;
pub mod utils;


#[cfg(test)]
mod tests{
    use std::time::Duration;
    use tokio::time::sleep;
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
                "/mnt/hdd/Music/Alphaville - Forever Young (Remastered) (1984_2019) [LP] DSD128/compressed.dff".into()
            )
            .await;
        player.start().await;


        sleep(Duration::from_millis(5600)).await;

        if let Some(meta) = player.get_current_file_meta().await{
            meta.pretty_print()
        }
        println!("{:?}", player.get_format_info().await);

        player.seek(0.9f64).await.unwrap();
        sleep(Duration::from_millis(1500)).await;
        player.pause().await;
        sleep(Duration::from_millis(1500)).await;
        player.play().await;
        sleep(Duration::from_millis(1500)).await;

        player.load_new_track(
            "/home/larry/Desktop/sacd/RUMOURS/Stereo/07 - THE CHAIN.dff".into(),
        )
            .await;
        sleep(Duration::from_millis(1500)).await;

        player.start().await;

        sleep(Duration::from_millis(5000)).await;

        if let Some(meta) = player.get_current_file_meta().await{
            meta.pretty_print()
        }
        player.seek(0.99f64).await.unwrap();
        loop {
            let pos =  player.get_pos().await;

            println!("Progress {}", pos);
            if pos == 1f64{
                player.stop().await;

                break;
            }
            sleep(Duration::from_millis(500)).await;

        }
    }
}

