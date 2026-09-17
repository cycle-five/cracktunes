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

    /// 🪤 The toggle flips the setting and changes nothing downstream: the join
    /// path defers publicly before any command body replies, and Discord fixes
    /// a response's visibility when it is created, so `ephemeral(true)` on the
    /// reply that edits it is ignored (#535). Registering the command would
    /// promise a privacy the bot cannot deliver, so it stays out of the set
    /// Discord is given until #535 lands -- built and tested, unreachable.
    ///
    /// 🪤 It first lived under `/settings`, whose module has been compiled out
    /// since v0.4.0 (#534): that copy was never even type-checked. This one is.
    #[test]
    fn ephemeral_is_built_but_deliberately_not_registered() {
        let command = super::ephemeral();

        assert!(command.slash_action.is_some(), "a slash command");
        assert!(command.guild_only, "guild only");
        assert!(command
            .required_permissions
            .contains(Permissions::ADMINISTRATOR));
        assert!(command
            .default_member_permissions
            .contains(Permissions::ADMINISTRATOR));

        assert!(
            !crate::commands::commands_to_register()
                .into_iter()
                .any(|command| command.name == "ephemeral"),
            "not registered while #535 is open: replies cannot be private yet"
        );
    }
}
