pub mod semaphore;
pub mod dsd_readers;
pub mod players;

fn main() {
    let player_names = players::enumerate_supported_devices();
    player_names.iter().for_each(|name|{
        eprintln!("Found device: {}, {}",name.0.to_str().unwrap(), name.1.to_str().unwrap());
    });
    let mut player = players::create_player_and_open(player_names[1].0.clone(), "/mnt/hdd/Music/Alice In Chains - Greatest Hits (2001) [SACD] (ISO)/01 - Man In The Box.dsf");

    //player.load_new_track();
    println!("{:?}", player.get_format_info());
    player.play();
    player.play_on_current_thread();
    println!("next track");
    player.load_new_track("/mnt/hdd/Music/Alice In Chains - Greatest Hits (2001) [SACD] (ISO)/02 - Them Bones.dsf");
    player.seek(0.8).unwrap();
    println!("{:?}", player.get_format_info());
    player.play();
    player.play_on_current_thread();
}