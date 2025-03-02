use crate::{
    commands::{cmd_check_music, help},
    messaging::message::CrackedMessage,
    utils::send_reply,
    Context, Error,
};
use crack_types::errors::CrackedError;
use songbird::error::JoinError;

/// Tell the bot to leave the voice channel it is in.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    aliases("dc", "fuckoff", "fuck off"),
    //subcommands("help"),
    check = "cmd_check_music"
)]
pub async fn leave(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    leave_internal(ctx).await
}

/// Leave a voice channel. Actually impl.
pub async fn leave_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = ctx.data().songbird.clone();
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
