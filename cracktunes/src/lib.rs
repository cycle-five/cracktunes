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
        "remove",
        "grab",
        "playlist",
        "voteskip",
        "version",
        "help",
        "autopause",
        "autoplay",
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
    vec!["set_vc_size", "role", "timeout", "mute", "unmute"]
}

/// Settings commands list
pub fn get_settings_commands() -> Vec<&'static str> {
    vec!["get_settings", "prefix", "add_prefix", "clear_prefixes"]
}

/// Commands only available to the bot owner
pub fn get_owner_commands() -> Vec<&'static str> {
    vec!["set_premium", "get_active_vcs"]
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
