use crate::{
    commands::{cmd_check_music, sub_help as help},
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
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
    subcommands("help"),
    guild_only
)]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    let queue = ctx.get_queue().await?;

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.pause(), CrackedError::Other("Failed to pause"))?;

    send_reply(&ctx, CrackedMessage::Pause, true).await?;
    Ok(())
}
