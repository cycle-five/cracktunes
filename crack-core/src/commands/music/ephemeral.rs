use crate::{
    commands::help, errors::CrackedError, guild::operations::GuildSettingsOperations,
    http_utils::SendMessageParams, messaging::message::CrackedMessage, poise_ext::PoiseContextExt,
    Context, Error,
};

/// Toggle whether /play, /skip and /nowplaying reply privately in this server.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn ephemeral(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    flag: bool,
) -> Result<(), Error> {
    if flag {
        return help::wrapper(ctx).await;
    }
    ephemeral_internal(ctx).await
}

/// Toggle ephemeral replies internal: flip and save the guild setting, then
/// say which way it went.
#[cfg(not(tarpaulin_include))]
pub async fn ephemeral_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let private = ctx.data().toggle_ephemeral_replies(guild_id).await?;
    let params = SendMessageParams {
        msg: if private {
            CrackedMessage::EphemeralRepliesOn
        } else {
            CrackedMessage::EphemeralRepliesOff
        },
        ..Default::default()
    };
    ctx.send_message(params).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use poise::serenity_prelude::all::Permissions;

    /// 🪤 v0.13.0 first put this toggle under `/settings`, whose module has been
    /// compiled out since v0.4.0 (#534): it was never type-checked and never
    /// registered. Pin that `/ephemeral` is in the set Discord is actually given,
    /// and that only admins can see and run it.
    #[test]
    fn ephemeral_is_registered_as_an_admin_only_guild_slash_command() {
        let command = crate::commands::commands_to_register()
            .into_iter()
            .find(|command| command.name == "ephemeral")
            .expect("/ephemeral is registered");

        assert!(command.slash_action.is_some(), "a slash command");
        assert!(command.guild_only, "guild only");
        assert!(command
            .required_permissions
            .contains(Permissions::ADMINISTRATOR));
        assert!(command
            .default_member_permissions
            .contains(Permissions::ADMINISTRATOR));
    }
}
