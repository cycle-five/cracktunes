use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_response_poise, Context,
    Error,
};

/// Leave the voice channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only, aliases("dc", "fuckoff"))]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NotConnected)?;
    manager.remove(guild_id).await?;

    let msg = send_response_poise(ctx, CrackedMessage::Leaving, true).await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}
