//! `/buttons`: turn the now-playing message's buttons off or on for a server.
use crate::{
    commands::{help, music_utils::connected_call},
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    messaging::{
        courier,
        message::CrackedMessage,
        status::{buttons_switched, DiscordTransport},
    },
    Context, Error,
};

/// Turn the buttons on the now-playing message off or on for this server.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn buttons(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    flag: bool,
) -> Result<(), Error> {
    if flag {
        return help::wrapper(ctx).await;
    }
    buttons_internal(ctx).await
}

/// Flip and save the server's `now_playing_buttons`, say which way it went,
/// then bring the status message in line.
#[cfg(not(tarpaulin_include))]
pub async fn buttons_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let on = data.toggle_now_playing_buttons(guild_id).await?;
    let msg = if on {
        CrackedMessage::NowPlayingButtonsOn
    } else {
        CrackedMessage::NowPlayingButtonsOff
    };
    courier::reply(ctx, msg).await?;
    let call = connected_call(&data.songbird, guild_id, None).await;
    let transport = DiscordTransport::of(ctx.serenity_context());
    buttons_switched(&data, &transport, guild_id, on, call.as_ref()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use poise::serenity_prelude::all::Permissions;

    #[test]
    fn buttons_is_an_admin_only_registered_guild_command() {
        let command = super::buttons();
        assert_eq!(command.name, "buttons");
        assert!(command.slash_action.is_some(), "a slash command");
        assert!(command.guild_only, "guild only");
        assert!(command
            .required_permissions
            .contains(Permissions::ADMINISTRATOR));
        assert!(command
            .default_member_permissions
            .contains(Permissions::ADMINISTRATOR));
        assert!(
            crate::commands::commands_to_register()
                .into_iter()
                .any(|command| command.name == "buttons"),
            "registered"
        );
    }
}
