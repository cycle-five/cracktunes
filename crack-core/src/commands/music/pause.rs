use crate::{
    commands::cmd_check_music,
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    music::{queue::pause_queue, PlaybackOwner},
    poise_ext::ContextExt,
    utils::send_reply,
    {Context, Error},
};

/// Pause the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    // `get_call_guild_id`, not `get_queue`: a cloned `TrackQueue` is a handle to
    // the same queue with no `Call` attached, and the guard-taking helpers need
    // the `Call`. Cloning it was also what hid this mutation from every
    // verification grep in this branch.
    let (call, guild_id) = ctx.get_call_guild_id().await?;

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    {
        let handler = call.lock().await;

        verify(!handler.queue().is_empty(), CrackedError::NothingPlaying)?;
        verify(
            pause_queue(&guard, &handler),
            CrackedError::Other("Failed to pause"),
        )?;
    }
    // Held for the mutation, not across the reply below.
    drop(guard);

    send_reply(&ctx, CrackedMessage::Pause, true).await?;
    Ok(())
}
