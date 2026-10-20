use crate::{
    commands::{cmd_check_music, connected_call},
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    messaging::{
        courier,
        message::CrackedMessage,
        messages::{
            DOWNVOTED_AND_SKIPPED, DOWNVOTED_BUT_MOVED_ON, MOVED_ON_NOT_SKIPPED, SKIPPED_NOT_LOGGED,
        },
    },
    music::ops::{self, OpCx, OpRefused},
    poise_ext::PoiseContextExt,
    utils::get_track_handle_metadata,
    Context, Error,
};
use serenity::all::Message;

/// Skip the current track, or a number of tracks.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn skip(
    ctx: Context<'_>,
    #[description = "Number of tracks to skip"] num_tracks: Option<u32>,
) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let private = crate::messaging::status::reply_privately(
        ctx.data().get_ephemeral_replies(cx.guild_id).await,
        ctx.is_prefix(),
    );
    let done = ops::skip(&cx, num_tracks.unwrap_or(1) as usize, None)
        .await
        .map_err(CrackedError::from)?;
    // The op holds no lease or Call lock by now; the reply is a Discord round trip.
    let reply = send_skip_reply(ctx, done.outcome().message(), private).await?;
    // A visible reply is the floor: its gateway echo may not have reached
    // the cache yet, and the status must still land below it. An ephemeral
    // reply is not a channel message and must not move the status.
    let anchor = (!private).then_some((reply.channel_id, reply.id));
    done.settle_after(&cx, anchor).await;
    Ok(())
}

/// Send the response to discord for skipping a track.
// Why don't we need to defer here?
#[cfg(not(tarpaulin_include))]
pub async fn send_skip_reply(
    ctx: Context<'_>,
    send_msg: CrackedMessage,
    private: bool,
) -> Result<Message, CrackedError> {
    courier::reply_as(ctx, send_msg, private)
        .await?
        .into_message()
        .await
        .map_err(|e| e.into())
}

/// Skip the current track and hide it from /playlog
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn downvote(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    // Never joins: downvoting is about the track playing right now.
    let call = connected_call(&ctx.data().songbird, guild_id, None)
        .await
        .ok_or(CrackedError::NotConnected)?;

    // The downvoted track must be the one skipped. The DB write sits between
    // the read and the skip, with no lease or Call lock held across it, so the
    // skip carries the id it read: if the track moved on in between, the op
    // refuses `Stale` and nothing else is skipped.
    let (current_id, source_url) = {
        let handler = call.lock().await;
        let current = handler
            .queue()
            .current()
            .ok_or(CrackedError::NothingPlaying)?;
        let metadata = get_track_handle_metadata(&current).await?;
        (
            current.uuid(),
            metadata.source_url.ok_or(CrackedError::NoMetadata)?,
        )
    };
    // Autoplay picks are never logged, so there may be nothing to downvote.
    // That must not stop the skip; every other failure still does.
    let hidden = match ctx.data().downvote_track(guild_id, &source_url).await {
        Ok(reaction) => {
            tracing::info!("downvoted track: {:#?}", reaction);
            true
        },
        Err(CrackedError::NoPlayLogged) => {
            tracing::info!("downvote: no play logged for {source_url}; skipping only");
            false
        },
        Err(e) => return Err(e.into()),
    };

    let cx = OpCx::from_ctx(&ctx)?;
    let private = crate::messaging::status::reply_privately(
        ctx.data().get_ephemeral_replies(cx.guild_id).await,
        ctx.is_prefix(),
    );
    match ops::skip(&cx, 1, Some(current_id)).await {
        Ok(done) => {
            let text = if hidden {
                DOWNVOTED_AND_SKIPPED
            } else {
                SKIPPED_NOT_LOGGED
            };
            let reply =
                send_skip_reply(ctx, CrackedMessage::Other(text.to_string()), private).await?;
            // As in `/skip`: a visible reply anchors the status below it.
            let anchor = (!private).then_some((reply.channel_id, reply.id));
            done.settle_after(&cx, anchor).await;
        },
        Err(OpRefused::Stale) => {
            tracing::warn!("downvoted a track that had already moved on; skipped nothing");
            let text = if hidden {
                DOWNVOTED_BUT_MOVED_ON
            } else {
                MOVED_ON_NOT_SKIPPED
            };
            send_skip_reply(ctx, CrackedMessage::Other(text.to_string()), private).await?;
        },
        Err(e) => return Err(CrackedError::from(e).into()),
    }
    Ok(())
}
