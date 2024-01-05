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
    vec![
        "get_settings",
        "set_premium",
        "prefix",
        "add_prefix",
        "remove_prefix",
    ]
}

pub fn get_commands() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("music", get_music_commands()),
        ("admin", get_admin_commands()),
        ("settings", get_settings_commands()),
    ]
}

#[cfg(test)]
pub mod test;
