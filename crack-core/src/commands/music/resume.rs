use crate::{
    commands::cmd_check_music,
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    music::{queue::resume_queue, PlaybackOwner},
    utils::send_reply,
    {Context, Error},
};

/// Resume the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    resume_internal(ctx).await
}

/// Internal function to resume the current track.
pub async fn resume_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let songbird = ctx.data().songbird.clone();
    let call = songbird.get(guild_id).ok_or(CrackedError::NotConnected)?;

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    //
    // 🔴 `/resume` reached `queue.resume()` on a running `/gp` round until this
    // line existed: it was on neither the blocklist nor the funnel. The
    // blocklist entry added alongside is the friendlier of the two refusals;
    // this is the one that cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    {
        let handler = call.lock().await;

        verify(!handler.queue().is_empty(), CrackedError::NothingPlaying)?;
        verify(resume_queue(&guard, &handler), CrackedError::FailedResume)?;
    }
    // Resuming does not fire `End`, but the guard still goes before the Discord
    // round trip -- exclusion is held for milliseconds, never for a send.
    drop(guard);

    send_reply(&ctx, CrackedMessage::Resume, true).await?;

    Ok(())
}
