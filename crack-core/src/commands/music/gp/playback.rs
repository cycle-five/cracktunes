use super::wrong_prompt;
use crate::{
    errors::CrackedError,
    messaging::{
        courier::{self, Destination},
        message::CrackedMessage,
        messages::{
            GP_ABORTED, GP_GAME_OVER, GP_GUESS_CHANGED, GP_GUESS_RECORDED, GP_LIKED, GP_UNLIKED,
        },
        render::RenderCx,
        transport::{DiscordPress, Press, Transport},
    },
    music::queue::{
        build_anonymous_track, build_track, enqueue_track_back, preload_time, stop_queue,
    },
    music::PlaybackOwner,
    CrackedResult, Data, Error,
};
use ::serenity::{
    all::{
        Cache, ChannelId, ComponentInteraction, ComponentInteractionDataKind, GenericChannelId,
        GuildId, MessageId, UserId,
    },
    async_trait,
};
use crack_gp::gp::{
    gp_min_played, gp_post, gp_rendered, gp_warning_text, now, parse_custom_id, user_refs, GpCard,
    GpClip, GpComponent, GpGuessOutcome, GpLikeOutcome, GpNext, GpPhase, GpPlayed, GpTrackStart,
    GpWindowClosed, GpWindowOpened, WrongPromptCard, WrongPromptClipState, GP_DEAD_LINK,
    GP_REVEAL_PAUSE_SECS, GP_WARNING_SECS,
};
use poise::serenity_prelude::Context as SerenityContext;
use songbird::tracks::{PlayMode, TrackHandle, TrackState};
use songbird::{Call, Event, EventContext, EventHandler, TrackEvent};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

// ------------------------------------------------------------------
// Playback glue
// ------------------------------------------------------------------

/// What the playback side of a game needs: shared state, the wire to Discord,
/// the call, and the guild. Cloned into every per-track handler and timer task.
#[derive(Clone)]
pub struct GpPlayback {
    pub data: Arc<Data>,
    pub transport: Arc<dyn Transport>,
    pub cache: Option<Arc<Cache>>,
    pub call: Arc<Mutex<Call>>,
    pub guild_id: GuildId,
}

pub(super) fn gp_voice_players(cache: &Cache, guild_id: GuildId, vc: ChannelId) -> Vec<UserId> {
    let me = cache.current_user().id;
    let Some(guild) = guild_id.to_guild_cached(cache) else {
        return Vec::new();
    };
    guild
        .voice_states
        .iter()
        .filter(|vs| vs.channel_id == Some(vc) && vs.user_id != me)
        .filter(|vs| guild.members.get(&vs.user_id).is_none_or(|m| !m.user.bot()))
        .map(|vs| vs.user_id)
        .collect()
}

pub struct GpTrackEndHandler {
    pub pb: GpPlayback,
    pub round_idx: usize,
    pub track_idx: usize,
    pub generation: u64,
    pub wrong_prompt: bool,
    /// How much of the song was meant to play: the clip's length, or `None` for a
    /// whole song. The dead-link bar scales with it, so a clip that ran to its end
    /// is not mistaken for a stream that never opened.
    pub intended: Option<Duration>,
}

/// How much of this song reached the room? songbird reports a stream it could
/// never open as an `End` whose state is still `Errored`: it goes `Preparing`
/// -> `Errored` without mixing a frame. Only an `Errored` track can fall short;
/// one that ended any other way was heard, whatever its play time.
pub(in crate::commands::music::gp) fn gp_played(
    state: &TrackState,
    intended: Option<Duration>,
) -> GpPlayed {
    if !matches!(state.playing, PlayMode::Errored(_)) {
        GpPlayed::Heard
    } else if state.play_time < GP_DEAD_LINK {
        GpPlayed::Never
    } else if state.play_time < gp_min_played(intended) {
        GpPlayed::CutShort
    } else {
        GpPlayed::Heard
    }
}

/// What the track-end handler decides. A vote to hear more of this song
/// outranks the play time: somebody asked for more of it, so it reached the
/// room whatever the stream did afterwards. Without this a song that played,
/// got voted up, and then dropped its stream would be revealed as one that
/// never played, and everyone who guessed it would go unpaid. Otherwise the
/// least heard of the event's states stands.
pub(in crate::commands::music::gp) fn gp_played_at_end<'a>(
    heard_by_vote: bool,
    states: impl IntoIterator<Item = &'a TrackState>,
    intended: Option<Duration>,
) -> GpPlayed {
    if heard_by_vote {
        return GpPlayed::Heard;
    }
    states
        .into_iter()
        .map(|state| gp_played(state, intended))
        .max()
        .unwrap_or(GpPlayed::Heard)
}

#[async_trait]
impl EventHandler for GpTrackEndHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        // Registered for `End` and `Error`, which always carry the track's
        // state; anything else has nothing to say against it having played.
        let states = match ctx {
            EventContext::Track(states) => *states,
            _ => &[],
        };
        let voted = self
            .pb
            .data
            .gp_heard_by_vote(self.pb.guild_id, self.round_idx, self.track_idx);
        let played = if self.wrong_prompt {
            states
                .iter()
                .map(|(state, _)| match gp_played(state, self.intended) {
                    GpPlayed::Heard if state.play_time < GP_DEAD_LINK => GpPlayed::Never,
                    GpPlayed::Heard
                        if matches!(state.playing, PlayMode::Stop)
                            && state.play_time < gp_min_played(self.intended) =>
                    {
                        GpPlayed::CutShort
                    },
                    heard => heard,
                })
                .max()
                .unwrap_or(GpPlayed::Never)
        } else {
            gp_played_at_end(voted, states.iter().map(|(state, _)| *state), self.intended)
        };
        // Do the reveal off the driver's event task: it edits messages, sleeps,
        // and takes the call lock to enqueue the next song.
        gp_spawn_advance(
            self.pb.clone(),
            self.generation,
            self.round_idx,
            self.track_idx,
            played,
        );
        Some(Event::Cancel)
    }
}

/// Send a game message, retrying once: a rate limit or a transient 5xx should
/// not cost the guild its game. Both attempts failing is treated as fatal by the
/// callers, because everything the round needs is armed after the send.
pub(super) async fn gp_send(
    pb: &GpPlayback,
    channel: GenericChannelId,
    card: GpCard,
) -> Result<MessageId, Error> {
    let out = gp_rendered(card);
    let first = match courier::post_message(&*pb.transport, channel, &out).await {
        Ok(id) => return Ok(id),
        Err(e) => e,
    };
    tracing::warn!(
        "gp: send in {} failed ({first}), retrying once",
        pb.guild_id
    );
    Ok(courier::post_message(&*pb.transport, channel, &out).await?)
}

/// Edit `card` into `message`, or post it if there is no message or the edit
/// fails (deleted by hand, or by `/clean`). Best effort.
pub(super) async fn gp_edit_or_post(
    pb: &GpPlayback,
    message: Option<(GenericChannelId, MessageId)>,
    channel: GenericChannelId,
    card: GpCard,
) {
    let msg: CrackedMessage = card.into();
    if let Some((chan, id)) = message {
        match courier::edit_message(&*pb.transport, chan, id, &msg).await {
            Ok(()) => return,
            Err(e) => tracing::warn!("gp: editing {id} in {chan}: {e}; posting instead"),
        }
    }
    courier::post(
        &pb.data,
        &*pb.transport,
        Destination::Channel(channel),
        &msg,
        &RenderCx::now(),
    )
    .await;
}

/// Answer a dropdown pick or a 👍: one ephemeral response, worked out from
/// memory before this is called -- well inside Discord's three seconds, so
/// there is no acknowledge first (the spec, section 3).
pub(in crate::commands::music::gp) async fn gp_answer_component(
    press: &dyn Press,
    text: String,
) -> Result<(), Error> {
    courier::respond(press, &GpCard::Line(text).into(), &RenderCx::now(), true).await?;
    Ok(())
}

/// Discard the game after an error it cannot come back from. Without this a
/// failed send leaves the guild with a game that no timer and no track handler
/// will ever advance, while [`GP_BLOCKED_COMMANDS`] keeps refusing its music
/// commands until somebody thinks to run `/gp end`.
pub(super) async fn gp_abort(pb: &GpPlayback, text_channel: GenericChannelId, reason: &str) {
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
    gp_post(
        &*pb.transport,
        text_channel,
        GpCard::Line(GP_ABORTED.into()),
    )
    .await;
}

pub async fn gp_open_round(pb: &GpPlayback, mut opened: GpWindowOpened) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    if !pb.data.gp_is_active(guild_id) {
        return Ok(());
    }
    let card = if opened.wrong_prompt {
        let Some(vc) = pb.data.gp_voice_channel(guild_id) else {
            return Ok(());
        };
        let members = pb
            .cache
            .as_ref()
            .map_or_else(Vec::new, |cache| gp_voice_players(cache, guild_id, vc));
        let gp = pb.data.gp_data();
        let prepared = gp.gp_prepare_wrong_prompt(
            guild_id,
            opened.generation,
            members,
            &mut rand::rng(),
            now(),
        );
        match prepared {
            Ok(deadline) => opened.closes_at = deadline,
            Err(CrackedError::NoGameInProgress | CrackedError::StaleRound) => return Ok(()),
            Err(err) => {
                if let Some(scores) =
                    gp.gp_stop_wrong_prompt_without_players(guild_id, opened.generation)
                {
                    gp_post(
                        &*pb.transport,
                        opened.text_channel,
                        GpCard::Line(err.to_string()),
                    )
                    .await;
                    gp_post_final_scores(pb, opened.text_channel, scores, true).await?;
                }
                return Ok(());
            },
        }
        wrong_prompt::prompt_card(pb, &opened)
    } else {
        GpCard::Prompt((&opened).into())
    };
    let msg_id = match gp_send(pb, opened.text_channel, card).await {
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
    if opened.wrong_prompt {
        if !pb.data.gp_data().gp_set_wrong_prompt_prompt_message(
            guild_id,
            opened.generation,
            (opened.text_channel, msg_id),
        ) {
            let _ = pb
                .transport
                .clear_components(opened.text_channel, msg_id)
                .await;
            return Ok(());
        }
    } else if let Err(e) =
        pb.data
            .gp_set_prompt_message(guild_id, opened.round_idx, opened.text_channel, msg_id)
    {
        tracing::warn!("gp: recording the prompt message in {guild_id}: {e}");
    }
    if opened.wrong_prompt {
        opened.timer_secs = opened.closes_at.saturating_sub(now()).max(0) as u64;
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
            gp_post(
                &*pb.transport,
                text_channel,
                GpCard::Line(gp_warning_text(&warning)),
            )
            .await;
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
    if closed.wrong_prompt {
        wrong_prompt::close_prompt(&pb, &closed).await;
    } else {
        gp_edit_or_post(
            &pb,
            closed.prompt_message,
            closed.text_channel,
            GpCard::PromptClosed((&closed).into()),
        )
        .await;
    }
    gp_follow(pb, closed.next, closed.text_channel, false).await
}

/// Move the game on to `next`. `after_reveal` says something was just posted
/// that the room should get a beat to read -- a song's reveal, or a round's
/// results -- before the next song starts or the next prompt pushes it up the
/// channel. The close of a window has nothing to read and goes straight on.
pub(super) async fn gp_follow(
    pb: GpPlayback,
    mut next: GpNext,
    text_channel: GenericChannelId,
    after_reveal: bool,
) -> Result<(), Error> {
    // Capture the finished game before any awaited reveal or retry notice. A
    // host can end it and start another game while those messages go out.
    let finished_game = pb.data.gp_games.get(&pb.guild_id).and_then(|game| {
        (game.phase == GpPhase::Finished).then_some((game.generation, game.wrong_prompt.is_some()))
    });
    if after_reveal {
        tokio::time::sleep(Duration::from_secs(GP_REVEAL_PAUSE_SECS)).await;
    }
    while let GpNext::Retry {
        round_idx,
        total_rounds,
        reason,
        next: retry,
    } = next
    {
        gp_post(
            &*pb.transport,
            text_channel,
            GpCard::WrongPrompt(WrongPromptCard::Voided {
                round_idx,
                total_rounds,
                reason,
            }),
        )
        .await;
        next = *retry;
    }
    match next {
        GpNext::Track(start) => gp_play_track(&pb, *start).await,
        GpNext::Window(opened) => gp_open_round(&pb, opened).await,
        GpNext::Discuss(window) => {
            wrong_prompt::gp_resume_wrong_prompt(pb, window, GpPhase::Discussing).await
        },
        GpNext::Finished(scores) => {
            // Remove first: a game that cannot post its scoreboard must still end,
            // or the guild keeps a finished game blocking its music commands.
            if let Some((generation, wrong_prompt)) = finished_game {
                if pb
                    .data
                    .gp_data()
                    .gp_remove_finished(pb.guild_id, generation)
                    .is_some()
                {
                    gp_post_final_scores(&pb, text_channel, scores, wrong_prompt).await?;
                }
            }
            Ok(())
        },
        GpNext::Retry { .. } => unreachable!("retries are handled before advancing"),
    }
}

pub(super) async fn gp_post_final_scores(
    pb: &GpPlayback,
    text_channel: GenericChannelId,
    scores: Vec<(UserId, u32)>,
    wrong_prompt: bool,
) -> Result<(), Error> {
    let card = if wrong_prompt {
        GpCard::WrongPrompt(WrongPromptCard::Final { scores })
    } else {
        GpCard::Scoreboard {
            scores: user_refs(&scores),
            title: GP_GAME_OVER,
            lead: None,
        }
    };
    courier::post_message(&*pb.transport, text_channel, &gp_rendered(card)).await?;
    Ok(())
}

pub async fn gp_play_track(pb: &GpPlayback, start: GpTrackStart) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    if !pb
        .data
        .gp_clip_still_current(guild_id, start.generation, start.round_idx, start.track_idx)
    {
        // Ended or torn down while we were between songs.
        return Ok(());
    }
    // `build_track` is lazy; the stream is fetched when songbird starts it.
    // Deliberately no `.with_user_id(submitter)`: the track keeps the default
    // sentinel, which the now-playing and queue embeds render as "(auto)", so
    // playback cannot leak the submitter before the reveal.
    let built = if start.wrong_prompt {
        // Queue, now-playing and audit readers all see the same anonymous label.
        build_anonymous_track(
            &start.track,
            &pb.data.http_client,
            format!("Wrong Prompt clip {}", start.track_idx + 1),
        )
    } else {
        build_track(&start.track, &pb.data.http_client)
    };
    let songbird_track = match built {
        Ok(t) => t,
        Err(e) => {
            // Nothing to enqueue means no End event will ever arrive, so drive the
            // same path a stream songbird cannot open takes: score nothing, move on.
            tracing::warn!(
                "gp: building round {} song {} in {guild_id}: {e}",
                start.round_idx,
                start.track_idx
            );
            gp_spawn_advance(
                pb.clone(),
                start.generation,
                start.round_idx,
                start.track_idx,
                GpPlayed::Never,
            );
            return Ok(());
        },
    };
    // Post and record the message *before* the song can start, so a stream that
    // dies immediately cannot beat the id into the map: the reveal would then
    // find no message to edit, post itself separately, and leave this one behind
    // still carrying a live dropdown.
    let card = if start.wrong_prompt {
        GpCard::WrongPrompt(WrongPromptCard::Clip {
            round_idx: start.round_idx,
            total_rounds: start.total_rounds,
            track_idx: start.track_idx,
            total_tracks: start.total_tracks,
            state: WrongPromptClipState::Playing,
        })
    } else {
        GpCard::Song((&start, guild_id).into())
    };
    let msg_id = match gp_send(pb, start.text_channel, card).await {
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
    if !pb
        .data
        .gp_clip_still_current(guild_id, start.generation, start.round_idx, start.track_idx)
    {
        let _ = pb
            .transport
            .clear_components(start.text_channel, msg_id)
            .await;
        return Ok(());
    }
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
        if !pb.data.gp_clip_still_current(
            guild_id,
            start.generation,
            start.round_idx,
            start.track_idx,
        ) {
            return Ok(());
        }
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
                generation: start.generation,
                wrong_prompt: start.wrong_prompt,
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
                gp_spawn_advance(
                    pb.clone(),
                    start.generation,
                    start.round_idx,
                    start.track_idx,
                    GpPlayed::Never,
                );
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
fn gp_spawn_advance(
    pb: GpPlayback,
    generation: u64,
    round_idx: usize,
    track_idx: usize,
    played: GpPlayed,
) {
    tokio::spawn(async move {
        let guild_id = pb.guild_id;
        if let Err(e) = gp_advance_track_if(pb, generation, round_idx, track_idx, played).await {
            tracing::warn!("gp: advancing round {round_idx} song {track_idx} in {guild_id}: {e}");
        }
    });
}

pub async fn gp_advance_track(
    pb: GpPlayback,
    round_idx: usize,
    track_idx: usize,
    played: GpPlayed,
) -> Result<(), Error> {
    let Some(generation) = pb
        .data
        .gp_games
        .get(&pb.guild_id)
        .map(|game| game.generation)
    else {
        return Ok(());
    };
    gp_advance_track_if(pb, generation, round_idx, track_idx, played).await
}

async fn gp_advance_track_if(
    pb: GpPlayback,
    generation: u64,
    round_idx: usize,
    track_idx: usize,
    played: GpPlayed,
) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    match played {
        GpPlayed::Heard => {},
        GpPlayed::CutShort => {
            tracing::warn!("gp: round {round_idx} song {track_idx} in {guild_id} was cut short");
        },
        GpPlayed::Never => {
            tracing::warn!("gp: round {round_idx} song {track_idx} in {guild_id} never played");
        },
    }
    let Some(res) = pb.data.gp_data().gp_finish_track_if(
        guild_id,
        generation,
        round_idx,
        track_idx,
        now(),
        played,
    ) else {
        return Ok(());
    };

    let card = if res.wrong_prompt {
        GpCard::WrongPrompt(WrongPromptCard::Clip {
            round_idx: res.round_idx,
            total_rounds: res.total_rounds,
            track_idx: res.track_idx,
            total_tracks: res.total_tracks,
            state: if played != GpPlayed::Heard {
                WrongPromptClipState::Failed
            } else {
                WrongPromptClipState::Played
            },
        })
    } else {
        GpCard::Reveal((&res).into())
    };
    gp_edit_or_post(&pb, res.message, res.text_channel, card).await;
    // The round's last song: sum the round up at the bottom of the channel
    // before the next prompt (or the final scoreboard) goes up. Never reached
    // for a round nobody submitted to, which ends at the close, not here. Once
    // it is up the round is marked as having had its results, and the game
    // written down again: the snapshot that ended the round went out before
    // this post, and a round left unmarked is posted by the next resume.
    if let Some(round) = &res.round {
        let out = gp_rendered(GpCard::RoundResults(round.into()));
        match courier::post_message(&*pb.transport, res.text_channel, &out).await {
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
    if let Some((action, guild_id, generation)) = wrong_prompt::parse_component(&mci.data.custom_id)
    {
        return wrong_prompt::handle_component(data, ctx, mci, action, guild_id, generation).await;
    }
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
    let press = DiscordPress {
        http: &ctx.http,
        interaction: mci,
    };
    gp_answer_component(&press, content).await
}

pub(super) fn gp_component_vc_check(
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
