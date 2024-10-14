use crate::messaging::message::CrackedMessage;
use crate::poise_ext::PoiseContextExt;
use crate::{Context, Error};

// pub mod welcome;
pub mod set_all_log_channel;
pub mod set_auto_role;
pub mod set_idle_timeout;
pub mod set_join_leave_log_channel;
pub mod set_music_channel;
pub mod set_premium;
pub mod set_volume;
pub mod set_welcome_settings;

pub use set_all_log_channel::*;
pub use set_auto_role::*;
pub use set_idle_timeout::*;
pub use set_join_leave_log_channel::*;
pub use set_music_channel::*;
pub use set_premium::*;
pub use set_volume::*;
pub use set_welcome_settings::*;

/// Settings-set commands
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    subcommands(
        "log_channel_for_guild",
        "join_leave_log_channel",
        "all_log_channel",
        "premium",
        "volume",
        "auto_role",
        "idle_timeout",
        "welcome_settings",
        "music_channel",
        // "log_all",
        // "log_guild"
    ),
    required_permissions = "ADMINISTRATOR",
)]
/// Set settings
#[cfg(not(tarpaulin_include))]
pub async fn set(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send_reply(
        CrackedMessage::CommandFound(String::from("settings-set")),
        true,
    )
    .await?;

    Ok(())
}

/// Get all settings-set commands
pub fn commands() -> Vec<crate::Command> {
    vec![
        set(),
        // auto_role(),
        // all_log_channel(),
        // join_leave_log_channel(),
        // music_channel(),
        // premium(),
        // volume(),
        // idle_timeout(),
        // welcome_settings(),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_commands() {
        let cmds = super::commands();
        let names = cmds.iter().map(|c| c.name.clone()).collect::<Vec<String>>();
        assert!(!names.contains(&String::from("premium")));
        assert!(!names.contains(&String::from("volume")));
        assert!(!names.contains(&String::from("idle_timeout")));
        assert!(names.contains(&String::from("set")));
    }
}
