use poise::serenity_prelude::{ChannelId, Message};
use serenity::{builder::CreateMessage, http::Http};
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{errors::CrackedError, utils::create_now_playing_embed, Context, Error};

/// Have the current song sent to your DMs.
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

    let _ = send_now_playing(
        channel.id,
        ctx.serenity_context().http.clone(),
        call.clone(),
    )
    .await?;

    ctx.say("Sent you a DM with the current track").await?;

    Ok(())
}

/// Send the current track information as an ebmed to the given channel.
#[cfg(not(tarpaulin_include))]
pub async fn send_now_playing(
    channel: ChannelId,
    http: Arc<Http>,
    handler: Arc<Mutex<Call>>,
) -> Result<Message, Error> {
    let handler = handler.lock().await;
    match handler.queue().current() {
        Some(track_handle) => {
            let embed = create_now_playing_embed(&track_handle).await;
            channel
                .send_message(http, CreateMessage::new().embed(embed))
                .await
                .map_err(|e| e.into())
        }
        None => channel
            .say(http, "Nothing playing!")
            .await
            .map_err(|e| e.into()),
    }
}
