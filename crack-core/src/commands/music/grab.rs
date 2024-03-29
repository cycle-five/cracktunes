use poise::serenity_prelude::{ChannelId, Message};
use serenity::{builder::CreateMessage, http::Http};
use songbird::input::AuxMetadata;
use songbird::Call;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::utils::create_now_playing_embed_metadata;
use crate::utils::get_requesting_user;
use crate::{errors::CrackedError, interface::create_now_playing_embed, Context, Error};

/// Send the current tack to your DMs.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, aliases("save"), guild_only)]
pub async fn grab(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;
    let channel = ctx
        .author()
        .create_dm_channel(&ctx.serenity_context().http)
        .await?;

    let msg = send_now_playing(
        channel.id,
        ctx.serenity_context().http.clone(),
        call.clone(),
        None,
        None,
    )
    .await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    let reply_handle = ctx.say("Sent you a DM with the current track").await?;

    let msg = reply_handle.into_message().await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}

/// Send the current track information as an ebmed to the given channel.
#[cfg(not(tarpaulin_include))]
pub async fn send_now_playing(
    channel: ChannelId,
    http: Arc<Http>,
    call: Arc<Mutex<Call>>,
    cur_position: Option<Duration>,
    metadata: Option<AuxMetadata>,
) -> Result<Message, Error> {
    tracing::warn!("locking mutex");
    let mutex_guard = call.lock().await;
    tracing::warn!("mutex locked");
    let msg: CreateMessage = match mutex_guard.queue().current() {
        Some(track_handle) => {
            tracing::warn!("track handle found, dropping mutex guard");
            drop(mutex_guard);
            let requesting_user = get_requesting_user(&track_handle).await;
            let embed = if let Some(metadata2) = metadata {
                create_now_playing_embed_metadata(
                    requesting_user.ok(),
                    cur_position,
                    crate::commands::MyAuxMetadata::Data(metadata2),
                )
            } else {
                create_now_playing_embed(&track_handle).await
            };
            CreateMessage::new().embed(embed)
        }
        None => {
            tracing::warn!("track handle not found, dropping mutex guard");
            drop(mutex_guard);
            CreateMessage::new().content("Nothing playing")
        }
    };
    tracing::warn!("sending message: {:?}", msg);
    channel
        .send_message(Arc::clone(&http), msg)
        .await
        .map_err(|e| e.into())
}
