use crate::{
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    messaging::message::CrackedMessage,
    utils::{get_guild_name, send_response_poise},
    Context, Error,
};

/// Toggle autopause.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    autopause_internal(ctx).await
}

/// Toggle autopause internal.
#[cfg(not(tarpaulin_include))]
pub async fn autopause_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let prefix = ctx.data().bot_settings.get_prefix();

    let autopause = {
        let name = get_guild_name(ctx.serenity_context(), guild_id);
        let prefix = Some(prefix.as_str());
        let mut guild_settings = ctx
            .data()
            .get_or_create_guild_settings(guild_id, name, prefix);
        guild_settings.toggle_autopause();
        // guild_settings.save(&pool).await?;
        guild_settings.autopause
    };
    let msg = if autopause {
        send_response_poise(ctx, CrackedMessage::AutopauseOn, true)
    } else {
        send_response_poise(ctx, CrackedMessage::AutopauseOff, true)
    }
    .await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}
