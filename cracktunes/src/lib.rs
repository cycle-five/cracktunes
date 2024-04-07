pub mod config;

pub use config::*;

use std::collections::HashSet;

/// Osint commands list
pub fn get_osint_commands() -> Vec<&'static str> {
    vec!["ip", "scan"]
}

/// Music commands list
pub fn get_music_commands() -> Vec<&'static str> {
    vec![
        "play",
        "playnext",
        "playlog",
        "pause",
        "resume",
        "stop",
        "skip",
        "seek",
        "summon",
        "search",
        "altplay",
        "leave",
        "lyrics",
        "volume",
        "nowplaying",
        "queue",
        "repeat",
        "shuffle",
        "clear",
        "clean",
        "remove",
        "grab",
        "voteskip",
        "downvote",
        "version",
        "help",
        "autopause",
        "autoplay",
        // we're putting other random commands under music for now to get them treated correctly by the perms system
        "ping",
        // "uptime",
    ]
}

/// Playlist related commands
pub fn get_playlist_commands() -> Vec<&'static str> {
    vec!["create", "delete", "addto", "get", "list", "play"]
}

/// Mod commands list
pub fn get_mod_commands() -> Vec<(&'static str, Vec<&'static str>)> {
    // get_admin_commands()
    //     .into_iter()
    //     .chain(get_settings_commands())
    //     .collect()
    vec![
        ("admin", get_admin_commands().to_vec()),
        ("settings", get_settings_commands()),
    ]
}

pub fn get_admin_commands_hashset() -> HashSet<&'static str> {
    get_admin_commands().into_iter().collect()
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
        "defend",
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
        ("playlist", get_playlist_commands()),
        ("admin", get_admin_commands().to_vec()),
        ("settings", get_settings_commands()),
        ("owner", get_owner_commands()),
        ("osint", get_owner_commands()),
    ]
}

#[cfg(test)]
pub mod test;
