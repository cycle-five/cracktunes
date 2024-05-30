use crate::{
    errors::CrackedError, messaging::interface::create_now_playing_embed,
    utils::send_embed_response_poise, Context, Error,
};
use serenity::all::GuildId;
use serenity::prelude::Mutex;
use songbird::{Call, Songbird};
use std::sync::Arc;

/// Get the currently playing track.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only, aliases("np"))]
pub async fn nowplaying(ctx: Context<'_>) -> Result<(), Error> {
    let (guild_id, _manager, call) = get_guild_id_and_songbird_call(ctx).await?;

    let handler = call.lock().await;
    let track = handler
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;

    let embed = create_now_playing_embed(&track).await;
    let msg = send_embed_response_poise(ctx, embed).await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}

/// Gets the guild id and songbird manager and call structs.
#[cfg(not(tarpaulin_include))]
pub async fn get_guild_id_and_songbird_call(
    ctx: Context<'_>,
) -> Result<(GuildId, Arc<Songbird>, Arc<Mutex<Call>>), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::Other("Songbird manager not found."))?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;
    Ok((guild_id, manager, call))
}
