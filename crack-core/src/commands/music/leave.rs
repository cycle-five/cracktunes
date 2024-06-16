use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_reply, Context, Error,
};
use songbird::error::JoinError;

/// Leave  a voice channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    prefix_command,
    slash_command,
    guild_only,
    aliases("dc", "fuckoff", "fuck off")
)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    leave_internal(ctx).await
}

/// Leave a voice channel. Actually impl.
pub async fn leave_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NotConnected)?;
    // check if we're actually in a call
    let crack_msg = match manager.remove(guild_id).await {
        Ok(()) => {
            tracing::info!("Driver successfully removed.");
            CrackedMessage::Leaving
        },
        Err(err) => {
            tracing::error!("Driver could not be removed: {}", err);
            match err {
                JoinError::NoCall => CrackedMessage::CrackedError(CrackedError::NotConnected),
                _ => return Err(err.into()),
            }
        },
    };

    let _ = send_reply(&ctx, crack_msg, true).await?;
    Ok(())
}
