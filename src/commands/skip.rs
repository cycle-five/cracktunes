use crate::{
    errors::{verify, CrackedError},
    messaging::message::ParrotMessage,
    utils::{create_response_poise_text, get_guild_id},
    Context, Error,
};
use songbird::{tracks::TrackHandle, Call};
use std::cmp::min;
use tokio::sync::MutexGuard;

#[poise::command(prefix_command, slash_command)]
pub async fn skip(
    ctx: Context<'_>,
    #[description = "Number of tracks to skip"] tracks_to_skip: Option<usize>,
) -> Result<(), Error> {
    let guild_id = get_guild_id(&ctx).unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let to_skip = tracks_to_skip.unwrap_or(1);

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;

    let tracks_to_skip = min(to_skip, queue.len());

    handler.queue().modify_queue(|v| {
        v.drain(1..tracks_to_skip);
    });

    force_skip_top_track(&handler).await?;
    create_skip_response_poise(ctx, &handler, tracks_to_skip).await
}

pub async fn create_skip_response_poise(
    ctx: Context<'_>,
    handler: &MutexGuard<'_, Call>,
    tracks_to_skip: usize,
) -> Result<(), Error> {
    //ctx.defer().await?;
    //let mut interaction = get_interaction(ctx).unwrap();

    create_skip_response(ctx, handler, tracks_to_skip).await
}

pub async fn create_skip_response(
    ctx: Context<'_>,
    handler: &MutexGuard<'_, Call>,
    tracks_to_skip: usize,
) -> Result<(), Error> {
    match handler.queue().current() {
        Some(track) => {
            create_response_poise_text(
                &ctx,
                ParrotMessage::SkipTo {
                    title: track.metadata().title.as_ref().unwrap().to_owned(),
                    url: track.metadata().source_url.as_ref().unwrap().to_owned(),
                },
            )
            .await
        }
        None => {
            if tracks_to_skip > 1 {
                create_response_poise_text(&ctx, ParrotMessage::SkipAll).await
            } else {
                create_response_poise_text(&ctx, ParrotMessage::Skip).await
            }
        }
    }
}

pub async fn force_skip_top_track(
    handler: &MutexGuard<'_, Call>,
) -> Result<Vec<TrackHandle>, CrackedError> {
    // this is an odd sequence of commands to ensure the queue is properly updated
    // apparently, skipping/stopping a track takes a while to remove it from the queue
    // also, manually removing tracks doesn't trigger the next track to play
    // so first, stop the top song, manually remove it and then resume playback
    handler.queue().current().unwrap().stop().ok();
    handler.queue().dequeue(0);
    handler.queue().resume().ok();

    Ok(handler.queue().current_queue())
}
