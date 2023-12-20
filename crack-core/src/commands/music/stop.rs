use serenity::all::GuildId;

use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    utils::send_response_poise_text,
    Context, Data, Error,
};

/// Stop the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let res = cancel_autoplay(ctx.data(), guild_id).await;
    match res {
        Ok(_) => {}
        Err(e) => {
            tracing::error!("Failed to cancel autoplay: {}", e);
        }
    }
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    // Do we want to return an error here or just pritn and return/?
    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    send_response_poise_text(ctx, CrackedMessage::Stop).await?;
    Ok(())
}

/// Cancel autoplay
pub async fn cancel_autoplay(data: &Data, guild_id: GuildId) -> Result<(), Error> {
    data.guild_cache_map
        .lock()
        .unwrap()
        .entry(guild_id)
        .or_default()
        .autoplay = false;
    tracing::error!("Autoplay cancelled");
    Ok(())
}

/// Enable autoplay
pub async fn enable_autoplay(data: &Data, guild_id: GuildId) -> Result<(), Error> {
    data.guild_cache_map
        .lock()
        .unwrap()
        .entry(guild_id)
        .or_default()
        .autoplay = true;
    tracing::error!("Autoplay enabled");
    Ok(())
}
