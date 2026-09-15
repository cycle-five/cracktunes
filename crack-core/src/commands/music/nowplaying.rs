use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::status::{
    now_playing_pointer, pointer_goes_first, reply_privately, show_now_playing,
};
use crate::poise_ext::{ContextExt, PoiseContextExt};
use crate::utils::get_track_handle_metadata;
use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    Context, Error,
};
use poise::CreateReply;

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
///
/// The status message shows the track; this replies with a one-line pointer
/// to it (spec: `/nowplaying` ordering).
pub async fn nowplaying_internal(ctx: Context<'_>) -> Result<(), Error> {
    let call = ctx.get_call().await?;
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // 🔑 The Call lock is released at the end of this statement: the status
    // update below takes it.
    let track = call
        .lock()
        .await
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;
    let title = get_track_handle_metadata(&track)
        .await
        .ok()
        .and_then(|meta| meta.title)
        .unwrap_or_default();

    let data = ctx.data();
    let private = reply_privately(data.get_ephemeral_replies(guild_id).await, ctx.is_prefix());
    let music_channel = data.get_music_channel(guild_id).await;
    let serenity_ctx = ctx.serenity_context();

    if pointer_goes_first(private, music_channel, ctx.channel_id()) {
        ctx.send(
            CreateReply::default()
                .content(now_playing_pointer(&title, None))
                .ephemeral(private),
        )
        .await?;
        show_now_playing(
            &data,
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
        )
        .await;
    } else {
        let shown = show_now_playing(
            &data,
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
        )
        .await;
        let link = shown.map(|status| status.id.link(status.channel, Some(guild_id)));
        ctx.send(
            CreateReply::default()
                .content(now_playing_pointer(&title, link))
                .ephemeral(private),
        )
        .await?;
    }
    Ok(())
}
