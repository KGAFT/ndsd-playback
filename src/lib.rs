pub mod semaphore;
pub mod dsd_readers;
pub mod players;
pub mod utils;
/*
#[cfg(test)]
mod tests{
    use crate::players;

    #[test]
    fn it_works(){
        let player_names = players::enumerate_supported_devices();
        player_names.iter().for_each(|name|{
            eprintln!("Found device: {}, {}",name.0.to_str().unwrap(), name.1.to_str().unwrap());
        });
        let mut player = players::create_player_and_open(player_names[0].0.clone(), "/mnt/hdd/Music/Alice In Chains - Greatest Hits (2001) [SACD] (ISO)/01 - Man In The Box.dsf").unwrap();

        //player.load_new_track();
        println!("{:?}", player.get_format_info());
        player.play();
        player.play_on_current_thread();
        println!("next track");
        player.load_new_track("/mnt/hdd/Music/Alice In Chains - Greatest Hits (2001) [SACD] (ISO)/02 - Them Bones.dsf");
        player.seek(0.9).unwrap();
        println!("{:?}", player.get_format_info());
        player.play();
        player.play_on_current_thread();
    }
}

 */