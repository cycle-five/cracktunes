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
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    // Do we want to return an error here or just pritn and return/?
    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    cancel_track_end_handler(ctx.data(), guild_id).await?;

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    send_response_poise_text(ctx, CrackedMessage::Stop).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}

/// Cancel the track end handler.
pub async fn cancel_track_end_handler(data: &Data, guild_id: GuildId) -> Result<(), Error> {
    let mut guild_cache_map = data.guild_cache_map.lock().unwrap();
    let guild_cache = guild_cache_map.get_mut(&guild_id).unwrap();
    guild_cache.autoplay = false;
    Ok(())
}
