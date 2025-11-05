pub mod semaphore;
pub mod dsd_readers;
pub mod players;

#[cfg(test)]
mod tests{
    use crate::players;

    #[test]
    fn it_works(){
        let player_names = players::enumerate_supported_devices();
        player_names.iter().for_each(|name|{
            eprintln!("Found device: {}, {}",name.0.to_str().unwrap(), name.1.to_str().unwrap());
        });
        let mut player = players::create_player_and_open(player_names[1].0.clone(), "/mnt/hdd/Music/Pink Floyd - The Wall 1979 [SACD-R]/04. The Happiest Days of Our Lives.dff");

        //player.load_new_track();
        println!("{:?}", player.get_format_info());
        player.play();
        player.seek(0.5).unwrap();
        player.play_on_current_thread();
        player.load_new_track("/mnt/hdd/Music/Rainbow – Ritchie Blackmore's Rainbow - (1975)/02. Self Portrait.dsf");
        println!("{:?}", player.get_format_info());
        player.play();
        player.play_on_current_thread();
    }
}