pub mod config;

pub use config::*;

/// Music commands list
pub fn get_music_commands() -> Vec<&'static str> {
    vec![
        "play",
        "pause",
        "resume",
        "stop",
        "skip",
        "seek",
        "summon",
        "leave",
        "lyrics",
        "volume",
        "now_playing",
        "queue",
        "repeat",
        "shuffle",
        "clear",
        "clean",
        "remove",
        "grab",
        "voteskip",
        "version",
        "help",
        "autopause",
        "autoplay",
    ]
}

/// Playlist related commands
pub fn get_playlist_commands() -> Vec<&'static str> {
    vec![
        "playlist",
        "playlist_create",
        "playlist_delete",
        "playlist_add",
        "playlist_remove",
        "playlist_list",
        "playlist_play",
        "playlist_queue",
        "playlist_clear",
        "playlist_rename",
        "playlist_import",
        "playlist_export",
    ]
}

/// Mod commands list
pub fn get_mod_commands() -> Vec<(&'static str, Vec<&'static str>)> {
    // get_admin_commands()
    //     .into_iter()
    //     .chain(get_settings_commands())
    //     .collect()
    vec![
        ("admin", get_admin_commands()),
        ("settings", get_settings_commands()),
    ]
}

/// Admin commands list
pub fn get_admin_commands() -> Vec<&'static str> {
    vec![
        "audit_logs",
        "authorize",
        "ban",
        "unban",
        "create_text_channel",
        "create_voice_chaneel",
        "defean",
        "deauthorize",
        "delete_channel",
        "get_active",
        "kick",
        "move_users",
        "set_vc_size",
        "role",
        "timeout",
        "mute",
        "unmute",
        "role",
        "create_role",
        "assign_role",
        "delete_role",
    ]
}

/// Settings commands list
pub fn get_settings_commands() -> Vec<&'static str> {
    vec![
        "get",
        "set",
        "get_settings",
        "prefix",
        "add_prefix",
        "clear_prefixes",
    ]
}

/// Commands only available to the bot owner
pub fn get_owner_commands() -> Vec<&'static str> {
    vec![
        "set_premium",
        "get_active_vcs",
        "broadcast_voice",
        "debug",
        "defend",
        "invite_tracker",
        "random_mute_lol",
        "message_cache",
    ]
    //vec!["shutdown", "eval", "reload", "update"]
}

/// All commands list
pub fn get_commands() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("music", get_music_commands()),
        ("admin", get_admin_commands()),
        ("settings", get_settings_commands()),
        ("owner", get_owner_commands()),
    ]
}

#[cfg(test)]
pub mod test;
