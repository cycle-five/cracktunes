use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    utils::{create_response_poise_text, get_track_metadata},
    Context, Error,
};
use songbird::{tracks::TrackHandle, Call};
use std::cmp::min;
use tokio::sync::MutexGuard;

/// Skip the current track, or a number of tracks.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn skip(
    ctx: Context<'_>,
    #[description = "Number of tracks to skip"] tracks_to_skip: Option<usize>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = match manager.get(guild_id) {
        Some(call) => call,
        None => {
            // create_response_poise_text(&ctx, CrackedMessage::NotInVoiceChannel).await?;
            tracing::warn!(
                "Not in voice channel: manager.get({}) returned None",
                guild_id
            );
            return Ok(());
        }
    };

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
            let metadata = get_track_metadata(&track).await;
            create_response_poise_text(
                &ctx,
                CrackedMessage::SkipTo {
                    title: metadata.title.as_ref().unwrap().to_owned(),
                    url: metadata.source_url.as_ref().unwrap().to_owned(),
                },
            )
            .await
        }
        None => {
            if tracks_to_skip > 1 {
                create_response_poise_text(&ctx, CrackedMessage::SkipAll).await
            } else {
                create_response_poise_text(&ctx, CrackedMessage::Skip).await
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
    let _ = handler.queue().dequeue(0);
    handler.queue().resume().ok();

    Ok(handler.queue().current_queue())
}
