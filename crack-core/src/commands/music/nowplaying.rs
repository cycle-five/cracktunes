use crate::poise_ext::{ContextExt, PoiseContextExt};
use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    messaging::interface::create_now_playing_embed,
    Context, Error,
};

/// Get the currently playing track.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only,
    aliases("np")
)]
pub async fn nowplaying(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    nowplaying_internal(ctx).await
}

/// Get the currently playing track. Internal function.
pub async fn nowplaying_internal(ctx: Context<'_>) -> Result<(), Error> {
    let call = ctx.get_call().await?;

    let handler = call.lock().await;
    let track = handler
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;

    let embed = create_now_playing_embed(track).await;
    let _ = ctx.send_embed_response(embed).await?;
    Ok(())
}
