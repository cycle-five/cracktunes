pub mod add_to_playlist;
pub mod create_playlist;
pub mod delete_playlist;
pub mod get_playlist;
pub mod list_playlists;
pub mod loadspotify;
pub mod play_playlist;

pub use add_to_playlist::add_to_playlist as addto;
pub use create_playlist::create_playlist as create;
pub use delete_playlist::delete_playlist as delete;
pub use get_playlist::get_playlist as get;
pub use list_playlists::list_playlists as list;
pub use loadspotify::loadspotify;
pub use play_playlist::play_playlist as play;

use crate::{
    commands::{cmd_check_music, sub_help as help},
    messaging::message::CrackedMessage,
    utils::send_reply,
    Context, Error,
};

/// Playlist commands.
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    subcommands(
        "addto",
        "create",
        "delete",
        "get",
        "list",
        "play",
        "loadspotify",
        "help"
    ),
    aliases("pl"),
    check = "cmd_check_music"
)]
#[cfg(not(tarpaulin_include))]
pub async fn playlist(ctx: Context<'_>) -> Result<(), Error> {
    send_reply(
        &ctx,
        CrackedMessage::Other("You found the playlist command! Try /playlist help.".to_string()),
        true,
    )
    .await?;

    Ok(())
}

pub fn playlist_commands() -> [crate::Command; 1] {
    [playlist()]
    // [
    //     addto(),
    //     create(),
    //     delete(),
    //     get(),
    //     list(),
    //     play(),
    //     loadspotify(),
    // ]
}
