use crate::{
    commands::help,
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    messaging::{courier, message::CrackedMessage},
    Context, Error,
};

/// Toggle whether button and dashboard controls post a line in the channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn echoes(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    flag: bool,
) -> Result<(), Error> {
    if flag {
        return help::wrapper(ctx).await;
    }
    echoes_internal(ctx).await
}

/// Flip and save the guild's `control_echoes`, then say which way it went.
#[cfg(not(tarpaulin_include))]
pub async fn echoes_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let on = ctx.data().toggle_control_echoes(guild_id).await?;
    let msg = if on {
        CrackedMessage::ControlEchoesOn
    } else {
        CrackedMessage::ControlEchoesOff
    };
    courier::reply(ctx, msg).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use poise::serenity_prelude::all::Permissions;

    /// Unlike `/ephemeral`, this one is registered: an echo is a channel
    /// message, so nothing like #535 stands between the setting and its effect.
    #[test]
    fn echoes_is_an_admin_only_registered_guild_command() {
        let command = super::echoes();
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
                .any(|command| command.name == "echoes"),
            "registered"
        );
    }
}
