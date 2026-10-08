use super::state::*;
use super::ui::*;
use crate::{
    errors::CrackedError,
    messaging::messages::{
        GP_ABORTED, GP_GAME_OVER, GP_GUESS_CHANGED, GP_GUESS_RECORDED, GP_LIKED, GP_UNLIKED,
    },
    music::queue::{build_track, enqueue_track_back, preload_time, stop_queue},
    music::PlaybackOwner,
    CrackedResult, Data, Error,
};
use ::serenity::{
    all::{
        ComponentInteraction, ComponentInteractionDataKind, GenericChannelId, GuildId, MessageId,
        UserId,
    },
    async_trait,
    builder::{
        CreateComponent, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage,
        CreateMessage, EditMessage,
    },
    http::Http,
};
use poise::serenity_prelude::Context as SerenityContext;
use songbird::tracks::{PlayMode, TrackHandle, TrackState};
use songbird::{Call, Event, EventContext, EventHandler, TrackEvent};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

// ------------------------------------------------------------------
// Playback glue
// ------------------------------------------------------------------

/// What the playback side of a game needs: shared state, HTTP, the call, and
/// the guild. Cloned into every per-track handler and timer task.
#[derive(Clone)]
pub struct GpPlayback {
    pub data: Arc<Data>,
    pub http: Arc<Http>,
    pub call: Arc<Mutex<Call>>,
    pub guild_id: GuildId,
}

pub struct GpTrackEndHandler {
    pub pb: GpPlayback,
    pub round_idx: usize,
    pub track_idx: usize,
    /// How much of the song was meant to play: the clip's length, or `None` for a
    /// whole song. The dead-link bar scales with it, so a clip that ran to its end
    /// is not mistaken for a stream that never opened.
    pub intended: Option<Duration>,
}

/// Did this song reach nobody? songbird reports a stream it could never open as
/// an `End` whose state is still `Errored`: it goes `Preparing` -> `Errored`
/// without mixing a frame. Both halves matter. Without the `Errored` check the
/// game treats a dead link as a song everyone just listened to; without the
/// play-time check it does the opposite to a stream that dies part-way through,
/// throwing away the guesses and 👍 of a room that heard most of it.
///
/// The line is [`GP_MIN_PLAYED`], not "any frame at all". A stream that dies a
/// couple of hundred milliseconds in is a dead link as far as the room is
/// concerned, and paying the submitter the fooled-everyone bonus for a song
/// nobody could possibly have guessed is the bug 2ed923b set out to fix.
pub(in crate::commands::music::gp) fn never_played(
    state: &TrackState,
    intended: Option<Duration>,
) -> bool {
    matches!(state.playing, PlayMode::Errored(_)) && state.play_time < gp_min_played(intended)
}

/// Did the song fail instead of finish? The handler is registered for both
/// `End` and `Error`, so this decides which of the two reveal paths runs.
fn track_errored(ctx: &EventContext<'_>, intended: Option<Duration>) -> bool {
    match ctx {
        EventContext::Track(states) => states
            .iter()
            .any(|(state, _)| never_played(state, intended)),
        _ => false,
    }
}

#[async_trait]
impl EventHandler for GpTrackEndHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        // A vote to hear more of this song outranks the play-time bar: somebody
        // asked for more of it, so it reached the room whatever the stream did
        // afterwards. Without this a song that played, got voted up, and then
        // dropped its stream would be revealed as one that never played, and
        // everyone who guessed it would go unpaid.
        let failed =
            !self
                .pb
                .data
                .gp_heard_by_vote(self.pb.guild_id, self.round_idx, self.track_idx)
                && track_errored(ctx, self.intended);
        // Do the reveal off the driver's event task: it edits messages, sleeps,
        // and takes the call lock to enqueue the next song.
        gp_spawn_advance(self.pb.clone(), self.round_idx, self.track_idx, failed);
        Some(Event::Cancel)
    }
}

/// Send a game message, retrying once: a rate limit or a transient 5xx should
/// not cost the guild its game. Both attempts failing is treated as fatal by the
/// callers, because everything the round needs is armed after the send.
#[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
async fn gp_send(
    pb: &GpPlayback,
    channel: GenericChannelId,
    embed: CreateEmbed<'static>,
    components: Vec<CreateComponent<'static>>,
) -> Result<MessageId, Error> {
    let build = || {
        CreateMessage::new()
            .embed(embed.clone())
            .components(components.clone())
    };
    let first = match channel.send_message(&pb.http, build()).await {
        Ok(msg) => return Ok(msg.id),
        Err(e) => e,
    };
    tracing::warn!(
        "gp: send in {} failed ({first}), retrying once",
        pb.guild_id
    );
    Ok(channel.send_message(&pb.http, build()).await?.id)
}

/// Discard the game after an error it cannot come back from. Without this a
/// failed send leaves the guild with a game that no timer and no track handler
/// will ever advance, while [`GP_BLOCKED_COMMANDS`] keeps refusing its music
/// commands until somebody thinks to run `/gp end`.
async fn gp_abort(pb: &GpPlayback, text_channel: GenericChannelId, reason: &str) {
    if pb.data.gp_remove(pb.guild_id).is_none() {
        return;
    }
    tracing::warn!("gp: {reason} in {}, game discarded", pb.guild_id);
    // 🪤 ORDER MATTERS: lock as whoever holds the lease *at this line*.
    // `gp_remove` four lines above already released it, so the guild is `Free`
    // by the time we get here and `Free` is what must be passed. Locking as
    // `Game` would be refused, and a refusal is silent -- the queue would
    // simply play on under a game that no longer exists.
    match pb
        .data
        .lock_queue(
            pb.guild_id,
            PlaybackOwner::Free,
            crate::music::audit::Actor::bot(crate::music::audit::BotReason::Game),
        )
        .await
    {
        Ok(guard) => {
            let handler = pb.call.lock().await;
            #[expect(
                clippy::disallowed_methods,
                reason = "gp_abort is cleanup as the game owner: it stops the queue of a game that failed"
            )]
            stop_queue(&guard, &handler);
        },
        Err(e) => tracing::warn!(
            "gp: could not lock the queue to abort in {}: {e}",
            pb.guild_id
        ),
    }
    // Both the guard and the call lock are dropped above, before the Discord
    // round trip below: `stop()` fires `End` inline on songbird's event task,
    // and a handler awaiting `lock_queue` would park that task for the length
    // of this send. See `stop_queue`.
    // Best effort: the channel is usually what just failed.
    #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
    if let Err(e) = text_channel
        .send_message(&pb.http, CreateMessage::new().content(GP_ABORTED))
        .await
    {
        tracing::warn!("gp: could not announce the abort in {}: {e}", pb.guild_id);
    }
}

pub async fn gp_open_round(pb: &GpPlayback, opened: GpWindowOpened) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    if !pb.data.gp_is_active(guild_id) {
        return Ok(());
    }
    let msg_id = match gp_send(
        pb,
        opened.text_channel,
        gp_prompt_embed(&opened),
        Vec::new(),
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            gp_abort(
                pb,
                opened.text_channel,
                &format!(
                    "posting the round {} prompt failed: {e}",
                    opened.round_idx + 1
                ),
            )
            .await;
            return Ok(());
        },
    };
    // Losing the id only costs the close its in-place edit, which already falls
    // back to a new message -- never a reason to leave the window unarmed.
    if let Err(e) =
        pb.data
            .gp_set_prompt_message(guild_id, opened.round_idx, opened.text_channel, msg_id)
    {
        tracing::warn!("gp: recording the prompt message in {guild_id}: {e}");
    }
    gp_spawn_window_timer(pb.clone(), &opened);
    Ok(())
}

/// The window timer: a heads-up 30 s before the end, then close. Both steps
/// are no-ops if the window it was spawned for has already closed.
pub fn gp_spawn_window_timer(pb: GpPlayback, opened: &GpWindowOpened) {
    gp_spawn_window_timer_secs(
        pb,
        opened.generation,
        opened.text_channel,
        opened.timer_secs,
    );
}

/// The same, for a window with `timer` seconds left on it -- the full timer
/// when a round opens, whatever remains when a game is resumed.
pub fn gp_spawn_window_timer_secs(
    pb: GpPlayback,
    generation: u64,
    text_channel: GenericChannelId,
    timer: u64,
) {
    tokio::spawn(async move {
        let guild_id = pb.guild_id;
        if timer > GP_WARNING_SECS {
            tokio::time::sleep(Duration::from_secs(timer - GP_WARNING_SECS)).await;
            let Some(warning) = pb.data.gp_warning_if(guild_id, generation) else {
                return;
            };
            #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
            if let Err(e) = text_channel
                .send_message(
                    &pb.http,
                    CreateMessage::new().content(gp_warning_text(&warning)),
                )
                .await
            {
                tracing::warn!("gp: window warning in {guild_id}: {e}");
            }
            tokio::time::sleep(Duration::from_secs(GP_WARNING_SECS)).await;
        } else {
            tokio::time::sleep(Duration::from_secs(timer)).await;
        }
        let closed = pb
            .data
            .gp_close_window_if(guild_id, generation, &mut rand::rng(), now());
        if let Some(closed) = closed {
            if let Err(e) = gp_after_close(pb, closed).await {
                tracing::warn!("gp: closing window in {guild_id}: {e}");
            }
        }
    });
}

pub async fn gp_after_close(pb: GpPlayback, closed: GpWindowClosed) -> Result<(), Error> {
    let embed = gp_prompt_closed_embed(&closed);
    #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
    let edited = match closed.prompt_message {
        Some((chan, msg_id)) => chan
            .edit_message(&pb.http, msg_id, EditMessage::new().embed(embed.clone()))
            .await
            .is_ok(),
        None => false,
    };
    if !edited {
        #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
        if let Err(e) = closed
            .text_channel
            .send_message(&pb.http, CreateMessage::new().embed(embed))
            .await
        {
            tracing::warn!("gp: posting the closed-window embed: {e}");
        }
    }
    gp_follow(pb, closed.next, closed.text_channel, false).await
}

/// Move the game on to `next`. `after_reveal` says something was just posted
/// that the room should get a beat to read -- a song's reveal, or a round's
/// results -- before the next song starts or the next prompt pushes it up the
/// channel. The close of a window has nothing to read and goes straight on.
async fn gp_follow(
    pb: GpPlayback,
    next: GpNext,
    text_channel: GenericChannelId,
    after_reveal: bool,
) -> Result<(), Error> {
    if after_reveal {
        tokio::time::sleep(Duration::from_secs(GP_REVEAL_PAUSE_SECS)).await;
    }
    match next {
        GpNext::Track(start) => gp_play_track(&pb, *start).await,
        GpNext::Window(opened) => gp_open_round(&pb, opened).await,
        GpNext::Finished(scores) => {
            // Remove first: a game that cannot post its scoreboard must still end,
            // or the guild keeps a finished game blocking its music commands.
            pb.data.gp_remove(pb.guild_id);
            #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
            text_channel
                .send_message(
                    &pb.http,
                    CreateMessage::new().embed(gp_scoreboard_embed(&scores, GP_GAME_OVER)),
                )
                .await?;
            Ok(())
        },
    }
}

pub async fn gp_play_track(pb: &GpPlayback, start: GpTrackStart) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    if !pb.data.gp_is_active(guild_id) {
        // Ended or torn down while we were between songs.
        return Ok(());
    }
    // `build_track` is lazy; the stream is fetched when songbird starts it.
    // Deliberately no `.with_user_id(submitter)`: the track keeps the default
    // sentinel, which the now-playing and queue embeds render as "(auto)", so
    // playback cannot leak the submitter before the reveal.
    let songbird_track = match build_track(&start.track, &pb.data.http_client) {
        Ok(t) => t,
        Err(e) => {
            // Nothing to enqueue means no End event will ever arrive, so drive the
            // same path a stream songbird cannot open takes: score nothing, move on.
            tracing::warn!(
                "gp: building round {} song {} in {guild_id}: {e}",
                start.round_idx,
                start.track_idx
            );
            gp_spawn_advance(pb.clone(), start.round_idx, start.track_idx, true);
            return Ok(());
        },
    };
    // Post and record the message *before* the song can start, so a stream that
    // dies immediately cannot beat the id into the map: the reveal would then
    // find no message to edit, post itself separately, and leave this one behind
    // still carrying a live dropdown.
    let msg_id = match gp_send(
        pb,
        start.text_channel,
        gp_track_embed(&start),
        gp_components(
            guild_id,
            start.round_idx,
            start.track_idx,
            &start.players,
            start.guessable,
        ),
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            // The song is audible but nobody can guess or 👍 it, and the reveal has
            // nothing to edit. Better to end cleanly than to play on unplayable.
            gp_abort(
                pb,
                start.text_channel,
                &format!("posting the song message failed: {e}"),
            )
            .await;
            return Ok(());
        },
    };
    // Only costs the reveal its in-place edit, which already falls back to a new
    // message -- not worth losing the game over.
    if let Err(e) = pb.data.gp_set_track_message(
        guild_id,
        start.round_idx,
        start.track_idx,
        start.text_channel,
        msg_id,
    ) {
        tracing::warn!("gp: recording the song message in {guild_id}: {e}");
    }

    let handle = {
        // The game still owns playback here: `gp_start` claimed the lease and
        // nothing on this path has released it, so lock as `Game`. `Free` would
        // be refused and the song would silently never be enqueued -- no `End`
        // would ever arrive and the round would hang forever.
        let guard = match pb
            .data
            .lock_queue(
                guild_id,
                PlaybackOwner::Game,
                crate::music::audit::Actor::bot(crate::music::audit::BotReason::Game),
            )
            .await
        {
            Ok(guard) => guard,
            Err(e) => {
                // Unreachable while `PlaybackOwner` has only `Free` and `Game`
                // -- locking as `Game` matches both -- but a third owner is
                // foreseeable, and a song that cannot be enqueued is fatal to
                // the round in exactly the way the two aborts below are.
                gp_abort(
                    pb,
                    start.text_channel,
                    &format!("locking the queue to play failed: {e}"),
                )
                .await;
                return Ok(());
            },
        };
        // The preload time is computed from metadata already in hand rather
        // than derived by songbird, which would spawn yt-dlp here, under the
        // guard -- see `preload_time`.
        enqueue_track_back(&guard, &pb.call, songbird_track, preload_time(&start.track)).await
        // The guard drops with this block, before the seek below. Both are slow
        // legs -- the enqueue above no longer is, now that it does not run
        // yt-dlp -- and `lease.rs` forbids holding exclusion across either.
        // Nothing after this point mutates the queue.
    };

    // Arm every handler before awaiting anything. The seek below is the first
    // await, and it is not a passive one: it forces songbird to create the
    // stream, which is when `Playable` fires and, if creation fails, when the
    // track is removed. Registering afterwards would race the first and find a
    // dead command channel after the second.
    for event in [TrackEvent::End, TrackEvent::Error] {
        if let Err(e) = crate::handlers::add_track_handler(
            &handle,
            Event::Track(event),
            GpTrackEndHandler {
                pb: pb.clone(),
                round_idx: start.round_idx,
                track_idx: start.track_idx,
                intended: start.clip.map(|c| c.length),
            },
        ) {
            // Unarmed, this song would play out and the round would never advance.
            gp_abort(
                pb,
                start.text_channel,
                &format!("arming the {event:?} handler failed: {e}"),
            )
            .await;
            return Ok(());
        }
    }

    if let Some(clip) = start.clip {
        // The clip is timed from when the song becomes audible, not from here.
        // `build_track` is lazy -- yt-dlp has not even spawned yet -- so timing it
        // from the enqueue would spend an unpredictable slice of the clip on
        // resolve latency, and would do it silently: the timer's `stop()` fires
        // `End`, not `Errored`, so `never_played` cannot catch a clip the room
        // barely heard.
        //
        // `TrackEvent::Playable`, not `Play`: `Play` explicitly does not fire when
        // a track first starts, only on a later pause -> play.
        if let Err(e) = crate::handlers::add_track_handler(
            &handle,
            Event::Track(TrackEvent::Playable),
            GpClipStartHandler {
                pb: pb.clone(),
                handle: handle.clone(),
                clip,
                round_idx: start.round_idx,
                track_idx: start.track_idx,
                generation: start.generation,
            },
        ) {
            gp_abort(
                pb,
                start.text_channel,
                &format!("arming the clip timer failed: {e}"),
            )
            .await;
            return Ok(());
        }

        if !clip.start.is_zero() {
            if let Err(e) = handle.seek(clip.start).result_async().await {
                // Not a fallback to playing from the top: songbird documents a
                // failed seek as fatal and *removes the track*, so there is no
                // song left and no `End` coming for it. Drive the same path a
                // stream songbird cannot open takes, rather than letting the dead
                // handle surface later as an abort of the whole game.
                tracing::warn!(
                    "gp: seeking round {} song {} in {guild_id} to {:?}: {e}",
                    start.round_idx,
                    start.track_idx,
                    clip.start
                );
                gp_spawn_advance(pb.clone(), start.round_idx, start.track_idx, true);
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Stop the song once its clip has played. Keyed to the generation the song
/// started under, the same way the submission-window timer is, so a timer left
/// over from an abandoned round cannot cut a later song short. Stands down if the
/// room has voted the song up to its full length in the meantime.
#[expect(
    clippy::disallowed_methods,
    reason = "the game ending its own clip; a track ending is not recorded"
)]
fn gp_spawn_clip_timer(
    pb: GpPlayback,
    handle: TrackHandle,
    clip: GpClip,
    round_idx: usize,
    track_idx: usize,
    generation: u64,
) {
    tokio::spawn(async move {
        tokio::time::sleep(clip.length).await;
        let guild_id = pb.guild_id;
        if !pb
            .data
            .gp_clip_still_current(guild_id, generation, round_idx, track_idx)
        {
            return;
        }
        if pb.data.gp_plays_full(guild_id, round_idx, track_idx) {
            tracing::trace!("gp: {guild_id} voted round {round_idx} song {track_idx} up to full");
            return;
        }
        // stop() fires TrackEvent::End, which is what runs the reveal.
        if let Err(e) = handle.stop() {
            tracing::warn!("gp: ending the clip in {guild_id}: {e}");
        }
    });
}

/// Starts the clip timer the moment the song becomes audible. One-shot: it
/// cancels itself once the timer is armed.
pub struct GpClipStartHandler {
    pub pb: GpPlayback,
    pub handle: TrackHandle,
    pub clip: GpClip,
    pub round_idx: usize,
    pub track_idx: usize,
    pub generation: u64,
}

#[async_trait]
impl EventHandler for GpClipStartHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        gp_spawn_clip_timer(
            self.pb.clone(),
            self.handle.clone(),
            self.clip,
            self.round_idx,
            self.track_idx,
            self.generation,
        );
        Some(Event::Cancel)
    }
}

/// Advance off the current task. Used by the track handlers and by a song that
/// could not be built, both of which must not run the reveal inline.
fn gp_spawn_advance(pb: GpPlayback, round_idx: usize, track_idx: usize, failed: bool) {
    tokio::spawn(async move {
        let guild_id = pb.guild_id;
        if let Err(e) = gp_advance_track(pb, round_idx, track_idx, failed).await {
            tracing::warn!("gp: advancing round {round_idx} song {track_idx} in {guild_id}: {e}");
        }
    });
}

pub async fn gp_advance_track(
    pb: GpPlayback,
    round_idx: usize,
    track_idx: usize,
    failed: bool,
) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    let advance = if failed {
        tracing::warn!("gp: round {round_idx} song {track_idx} in {guild_id} never played");
        Data::gp_fail_and_advance
    } else {
        Data::gp_reveal_and_advance
    };
    let Some(res) = advance(&pb.data, guild_id, round_idx, track_idx, now()) else {
        return Ok(());
    };

    let reveal = gp_reveal_embed(&res);
    #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
    let edited = match res.message {
        Some((chan, msg_id)) => chan
            .edit_message(
                &pb.http,
                msg_id,
                EditMessage::new()
                    .embed(reveal.clone())
                    .components(Vec::<CreateComponent<'_>>::new()),
            )
            .await
            .is_ok(),
        None => false,
    };
    if !edited {
        #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
        if let Err(e) = res
            .text_channel
            .send_message(&pb.http, CreateMessage::new().embed(reveal))
            .await
        {
            tracing::warn!("gp: posting the reveal: {e}");
        }
    }
    // The round's last song: sum the round up at the bottom of the channel
    // before the next prompt (or the final scoreboard) goes up. Never reached
    // for a round nobody submitted to, which ends at the close, not here. Once
    // it is up the round is marked as having had its results, and the game
    // written down again: the snapshot that ended the round went out before
    // this post, and a round left unmarked is posted by the next resume.
    #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
    if let Some(round) = &res.round {
        match res
            .text_channel
            .send_message(
                &pb.http,
                CreateMessage::new().embed(gp_round_results_embed(round)),
            )
            .await
        {
            Ok(_) => pb.data.gp_mark_results_posted(guild_id, round.round_idx),
            Err(e) => tracing::warn!(
                "gp: posting round {} results in {guild_id}: {e}",
                round.round_idx + 1
            ),
        }
    }
    // A beat before the next song always; before the next prompt only if the
    // results were posted, so a game without them moves on as it used to.
    let pause = matches!(res.next, GpNext::Track(_)) || res.round.is_some();
    gp_follow(pb, res.next, res.text_channel, pause).await
}

/// Handle a dropdown pick or a 👍. Called from `SerenityHandler::dispatch` for
/// every component interaction whose custom id starts with
/// [`GP_CUSTOM_ID_PREFIX`]. Every branch answers the interaction (ephemerally),
/// otherwise Discord shows "This interaction failed".
pub async fn handle_gp_component(
    data: &Data,
    ctx: &SerenityContext,
    mci: &ComponentInteraction,
) -> Result<(), Error> {
    let Some((kind, guild_id, round_idx, track_idx)) = parse_custom_id(&mci.data.custom_id) else {
        return Ok(());
    };
    if mci.guild_id != Some(guild_id) {
        return Ok(());
    }

    let content = match gp_component_vc_check(data, ctx, mci, guild_id).and_then(|()| match kind {
        GpComponent::Guess => {
            gp_guess_outcome(data, mci, guild_id, round_idx, track_idx).map(|o| match o {
                GpGuessOutcome::Recorded => GP_GUESS_RECORDED.to_string(),
                GpGuessOutcome::Changed => GP_GUESS_CHANGED.to_string(),
            })
        },
        GpComponent::Like => {
            gp_like_outcome(data, mci, guild_id, round_idx, track_idx).map(|o| match o {
                GpLikeOutcome::Liked(n) => format!("{GP_LIKED} ({n})"),
                GpLikeOutcome::Unliked(n) => format!("{GP_UNLIKED} ({n})"),
            })
        },
    }) {
        Ok(text) => text,
        Err(e) => e.to_string(),
    };
    #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
    mci.create_response(
        &ctx.http,
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(content)
                .ephemeral(true),
        ),
    )
    .await?;
    Ok(())
}

fn gp_component_vc_check(
    data: &Data,
    ctx: &SerenityContext,
    mci: &ComponentInteraction,
    guild_id: GuildId,
) -> CrackedResult<()> {
    let game_vc = data
        .gp_voice_channel(guild_id)
        .ok_or(CrackedError::NoGameInProgress)?;
    let user_vc = {
        let guild = guild_id
            .to_guild_cached(&ctx.cache)
            .ok_or(CrackedError::NoGuildCached)?;
        guild
            .voice_states
            .get(&mci.user.id)
            .and_then(|vs| vs.channel_id)
    };
    if user_vc != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel);
    }
    Ok(())
}

fn interaction_display_name(mci: &ComponentInteraction) -> String {
    mci.member
        .as_ref()
        .map(|m| m.display_name().to_string())
        .unwrap_or_else(|| mci.user.name.to_string())
}

/// The synchronous part of a guess.
fn gp_guess_outcome(
    data: &Data,
    mci: &ComponentInteraction,
    guild_id: GuildId,
    round_idx: usize,
    track_idx: usize,
) -> CrackedResult<GpGuessOutcome> {
    let guessed = match &mci.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values
            .first()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v != 0)
            .map(UserId::new)
            .ok_or(CrackedError::NotAPlayer)?,
        _ => return Err(CrackedError::NotAPlayer),
    };
    data.gp_record_guess(
        guild_id,
        round_idx,
        track_idx,
        mci.user.id,
        interaction_display_name(mci),
        guessed,
    )
}

/// The synchronous part of a 👍.
fn gp_like_outcome(
    data: &Data,
    mci: &ComponentInteraction,
    guild_id: GuildId,
    round_idx: usize,
    track_idx: usize,
) -> CrackedResult<GpLikeOutcome> {
    if !matches!(mci.data.kind, ComponentInteractionDataKind::Button) {
        return Err(CrackedError::StaleRound);
    }
    data.gp_toggle_like(
        guild_id,
        round_idx,
        track_idx,
        mci.user.id,
        interaction_display_name(mci),
    )
}
