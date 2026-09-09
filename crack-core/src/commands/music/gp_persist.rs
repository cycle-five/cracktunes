//! Saving `/gp` games to Postgres, and picking them up after a restart.
//!
//! A game lives in [`Data::gp_games`], in memory, and that stays the source of
//! truth while it runs. It is written down at the four moments its shape
//! changes -- a song is submitted, the window closes on a round, a song's
//! message is posted, a song ends -- as a snapshot of the game into the `gp_*`
//! tables (see [`crate::db::gp`], which writes only the rounds that can have
//! changed since the last one). Guesses, likes and votes on the song that is
//! playing are not written until it ends: after a resume that song plays again
//! from the top and the room casts them again, so the worst a crash costs is one
//! song's worth of guesses that people can re-cast, never a scoreboard.
//!
//! Nothing on the interaction or songbird path waits on the database. The
//! `gp_*` mutations on [`Data`] clone the game and send it down a channel; one
//! writer task per process drains that channel in order, so a later snapshot
//! can never be overwritten by an earlier one. Shutdown drains the channel
//! before the pool closes, which fits in Docker's ten-second stop grace because
//! the queue is almost always already empty.
//!
//! A game leaves the live set with a tombstone rather than a snapshot: a
//! `finished_at` and an outcome. That write happens on every path that removes a
//! game from the map, `/gp end` included -- it is not a checkpoint, but without
//! it a game ended on purpose would still look live after a redeploy and come
//! back. There is no heartbeat: a graceful shutdown stamps `last_seen_at` on
//! the games this process is actually running as it drains, so for a redeploy
//! -- the case that prompted this -- the gap the resume measures is the outage
//! itself. A hard crash leaves `last_seen_at` at the last checkpoint, which
//! errs toward not resuming.
//!
//! On the way back up, [`gp_resume_guild`] runs from the guild-create handler for
//! each guild as it arrives. A live game is brought back only if it can still be
//! going: a submission window that is still open, or closed less than
//! [`GP_RESUME_WINDOW_SECS`] ago by its own `closes_at`; a song whose game was
//! seen within that long. Anything older, or whose voice channel is empty, is
//! marked lost and its scoreboard posted with a line saying why. Anything else is put back into the map, the bot
//! rejoins voice, and the game re-enters its phase: a submission window with
//! whatever time `closes_at` says is left (or closes at once if that passed), a
//! song from the top, with the pre-restart song message's dropdown taken down
//! so there are not two live ones for the same song.

use super::gp::{
    gp_after_close, gp_play_track, gp_scoreboard_embed, gp_spawn_window_timer_secs, now, GpClip,
    GpGame, GpPhase, GpPlayback, GpReveal, GpRound, GpTrack, GP_RESUME_WINDOW_SECS,
};
use super::gp_prompts::GpCategory;
use crate::commands::music_utils::set_global_handlers_with;
use crate::db::{
    gp_mark_finished, gp_touch_live, GpGameRow, GpOutcome, GpPlayerRow, GpRoundRow, GpSaved,
    GpTrackRow,
};
use crate::messaging::messages::{
    GP_LOST, GP_RESUMED, GP_RESUMED_SONG, GP_RESUMED_WINDOW, GP_RESUMED_WINDOW_CLOSED,
    GP_SCOREBOARD,
};
use crate::Data;
use ::serenity::{
    all::{ChannelId, GenericChannelId, Guild, GuildId, MessageId, UserId},
    builder::{CreateComponent, CreateMessage, EditMessage},
};
use crack_testing::ResolvedTrack;
use crack_types::SavedTrack;
use poise::serenity_prelude::Context as SerenityContext;
use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

const PHASE_SUBMITTING: &str = "submitting";
const PHASE_PLAYING: &str = "playing";
const PHASE_FINISHED: &str = "finished";

/// What the writer task is asked to do. Ordered: the channel is FIFO and there is
/// one consumer, so a `Flush` answers only once everything before it is written.
#[derive(Debug)]
pub enum GpPersist {
    /// The whole game as it stands.
    Snapshot(Box<GpSaved>),
    /// The game is over; take it out of the live set.
    Finished {
        guild_id: i64,
        started_at: i64,
        outcome: GpOutcome,
    },
    /// The process is going down: the games listed were alive until now.
    Stopping { guild_ids: Vec<i64> },
    /// Reply once everything sent before this has been written.
    Flush(oneshot::Sender<()>),
}

/// Start the writer. Unbounded on purpose: the senders are synchronous `DashMap`
/// mutations, some on songbird's event path, and must never wait; the queue is
/// bounded in practice by how fast a room can submit songs.
pub fn spawn_gp_writer(pool: PgPool) -> mpsc::UnboundedSender<GpPersist> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(run_gp_writer(rx, pool));
    tx
}

pub async fn run_gp_writer(mut rx: mpsc::UnboundedReceiver<GpPersist>, pool: PgPool) {
    while let Some(msg) = rx.recv().await {
        match msg {
            GpPersist::Snapshot(saved) => {
                let guild_id = saved.game.guild_id;
                match saved.save(&pool).await {
                    Ok(id) => tracing::trace!("gp: saved game {id} in {guild_id}"),
                    Err(e) => tracing::warn!("gp: saving the game in {guild_id}: {e}"),
                }
            },
            GpPersist::Finished {
                guild_id,
                started_at,
                outcome,
            } => match gp_mark_finished(&pool, guild_id, started_at, outcome).await {
                Ok(true) => tracing::trace!("gp: game in {guild_id} {}", outcome.as_str()),
                // Nobody ever submitted, so there was no row; or it was already over.
                Ok(false) => {},
                Err(e) => tracing::warn!("gp: marking the game in {guild_id} over: {e}"),
            },
            GpPersist::Stopping { guild_ids } => match gp_touch_live(&pool, &guild_ids).await {
                Ok(n) if n > 0 => tracing::info!("gp: {n} live game(s) stamped for the restart"),
                Ok(_) => {},
                Err(e) => tracing::warn!("gp: stamping live games at shutdown: {e}"),
            },
            GpPersist::Flush(ack) => {
                let _ = ack.send(());
            },
        }
    }
}

impl Data {
    fn gp_persist_send(&self, msg: GpPersist) {
        if let Some(tx) = &self.gp_persist {
            if tx.send(msg).is_err() {
                tracing::warn!("gp: the persistence writer is gone; game state is not being saved");
            }
        }
    }

    /// Write the game down as it stands now. A no-op without a database.
    pub(crate) fn gp_snapshot(&self, game: &GpGame) {
        if self.gp_persist.is_none() {
            return;
        }
        self.gp_persist_send(GpPersist::Snapshot(Box::new(game.to_saved())));
    }

    /// The game is over. A no-op without a database.
    pub(crate) fn gp_mark_finished(&self, game: &GpGame, outcome: GpOutcome) {
        if self.gp_persist.is_none() {
            return;
        }
        self.gp_persist_send(GpPersist::Finished {
            guild_id: game_id(game.guild_id),
            started_at: game.started_at,
            outcome,
        });
    }

    /// The process is shutting down: stamp this process's live games as seen
    /// now, then wait until everything sent so far has been written, or
    /// `timeout` passes. For the shutdown handler, which has a budget.
    pub async fn gp_shutdown(&self, timeout: Duration) {
        let Some(tx) = &self.gp_persist else {
            return;
        };
        // Only the games in the map: a live row without one is a leftover, and
        // vouching for it would keep a game nobody is playing resumable forever.
        let guild_ids: Vec<i64> = self.gp_games.iter().map(|g| game_id(*g.key())).collect();
        if tx.send(GpPersist::Stopping { guild_ids }).is_err() {
            return;
        }
        let (ack, done) = oneshot::channel();
        if tx.send(GpPersist::Flush(ack)).is_err() {
            return;
        }
        if tokio::time::timeout(timeout, done).await.is_err() {
            tracing::warn!("gp: the game writer did not drain within {timeout:?}");
        }
    }
}

fn game_id(guild_id: GuildId) -> i64 {
    guild_id.get() as i64
}

fn user(id: i64) -> UserId {
    UserId::new(id as u64)
}

fn chan(id: i64) -> GenericChannelId {
    GenericChannelId::new(id as u64)
}

fn message(channel: Option<i64>, msg: Option<i64>) -> Option<(GenericChannelId, MessageId)> {
    Some((chan(channel?), MessageId::new(msg? as u64)))
}

/// Why a saved game could not be turned back into one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpLoadError(pub String);

impl fmt::Display for GpLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GpLoadError {}

impl GpGame {
    /// The game as rows.
    pub fn to_saved(&self) -> GpSaved {
        let phase = match self.phase {
            GpPhase::Submitting => PHASE_SUBMITTING,
            GpPhase::Playing => PHASE_PLAYING,
            GpPhase::Finished => PHASE_FINISHED,
        };
        let mut players: Vec<GpPlayerRow> = self
            .players
            .iter()
            .map(|(id, name)| GpPlayerRow {
                user_id: game_user(*id),
                display_name: name.clone(),
                score: self.scores.get(id).copied().unwrap_or(0) as i32,
            })
            .collect();
        players.sort_by_key(|p| p.user_id);

        let rounds = self
            .rounds
            .iter()
            .enumerate()
            .map(|(idx, r)| GpRoundRow {
                round_idx: idx as i32,
                prompt: r.prompt.clone(),
                closes_at: r.closes_at,
                prompt_channel_id: r.prompt_message.map(|(c, _)| c.get() as i64),
                prompt_message_id: r.prompt_message.map(|(_, m)| m.get() as i64),
            })
            .collect();

        let mut tracks = Vec::new();
        for (idx, r) in self.rounds.iter().enumerate() {
            let mut subs: Vec<(&UserId, &ResolvedTrack<'static>)> = r.submissions.iter().collect();
            subs.sort_by_key(|(id, _)| **id);
            for (submitter, track) in subs {
                let saved = SavedTrack::from(track);
                tracks.push(GpTrackRow {
                    round_idx: idx as i32,
                    submitter_id: game_user(*submitter),
                    position: None,
                    url: saved.url.clone(),
                    title: saved.title.clone(),
                    artist: saved.artist.clone(),
                    duration_secs: saved.duration_secs(),
                    play_full: false,
                    failed: false,
                    message_channel_id: None,
                    message_id: None,
                    guesses: Vec::new(),
                    likes: Vec::new(),
                    skip_votes: Vec::new(),
                    full_votes: Vec::new(),
                });
            }
            for (pos, t) in r.tracks.iter().enumerate() {
                let saved = SavedTrack::from(&t.track);
                let mut guesses: Vec<(i64, i64)> = t
                    .guesses
                    .iter()
                    .map(|(a, b)| (game_user(*a), game_user(*b)))
                    .collect();
                guesses.sort_unstable();
                tracks.push(GpTrackRow {
                    round_idx: idx as i32,
                    submitter_id: game_user(t.submitter),
                    position: Some(pos as i32),
                    url: saved.url.clone(),
                    title: saved.title.clone(),
                    artist: saved.artist.clone(),
                    duration_secs: saved.duration_secs(),
                    play_full: t.play_full,
                    failed: t.failed,
                    message_channel_id: t.message.map(|(c, _)| c.get() as i64),
                    message_id: t.message.map(|(_, m)| m.get() as i64),
                    guesses,
                    likes: sorted_users(&t.likes),
                    skip_votes: sorted_users(&t.skip_votes),
                    full_votes: sorted_users(&t.full_votes),
                });
            }
        }

        GpSaved {
            game: GpGameRow {
                guild_id: game_id(self.guild_id),
                started_at: self.started_at,
                host_id: game_user(self.host),
                voice_channel_id: self.voice_channel.get() as i64,
                text_channel_id: self.text_channel.get() as i64,
                category: self.category.slug().to_string(),
                phase: phase.to_string(),
                current_round: self.current_round as i32,
                current_track: self.current_track as i32,
                timer_secs: self.timer_secs as i64,
                clip_start_secs: self.clip.map(|c| c.start.as_secs() as i64),
                clip_length_secs: self.clip.map(|c| c.length.as_secs() as i64),
                reveal: self.reveal.slug().to_string(),
                round_results: self.round_results,
                generation: self.generation as i64,
            },
            players,
            rounds,
            tracks,
        }
    }

    /// A game from its rows. Fails only on rows this code did not write: an
    /// unknown category or phase, a round index with no round, a position past
    /// the end of its round.
    pub fn from_saved(saved: &GpSaved) -> Result<GpGame, GpLoadError> {
        let g = &saved.game;
        let category = GpCategory::from_slug(&g.category)
            .ok_or_else(|| GpLoadError(format!("unknown category {:?}", g.category)))?;
        let phase = match g.phase.as_str() {
            PHASE_SUBMITTING => GpPhase::Submitting,
            PHASE_PLAYING => GpPhase::Playing,
            PHASE_FINISHED => GpPhase::Finished,
            other => return Err(GpLoadError(format!("unknown phase {other:?}"))),
        };
        let reveal = GpReveal::from_slug(&g.reveal)
            .ok_or_else(|| GpLoadError(format!("unknown reveal {:?}", g.reveal)))?;
        let clip = match (g.clip_start_secs, g.clip_length_secs) {
            (Some(start), Some(length)) => Some(GpClip {
                start: Duration::from_secs(start.max(0) as u64),
                length: Duration::from_secs(length.max(0) as u64),
            }),
            _ => None,
        };

        let mut rounds: Vec<GpRound> = Vec::with_capacity(saved.rounds.len());
        let mut sorted_rounds: Vec<&GpRoundRow> = saved.rounds.iter().collect();
        sorted_rounds.sort_by_key(|r| r.round_idx);
        for (expected, r) in sorted_rounds.iter().enumerate() {
            if r.round_idx as usize != expected {
                return Err(GpLoadError(format!(
                    "round {} where round {expected} was expected",
                    r.round_idx
                )));
            }
            rounds.push(GpRound {
                prompt: r.prompt.clone(),
                submissions: HashMap::new(),
                tracks: Vec::new(),
                prompt_message: message(r.prompt_channel_id, r.prompt_message_id),
                closes_at: r.closes_at,
            });
        }

        // Tracks in play order: `load_live` orders by position, but do not rely
        // on it -- `to_saved` may not be the only writer forever.
        let mut ordered: Vec<&GpTrackRow> = saved.tracks.iter().collect();
        ordered.sort_by_key(|t| (t.round_idx, t.position.unwrap_or(i32::MAX), t.submitter_id));
        for t in ordered {
            let round = rounds.get_mut(t.round_idx as usize).ok_or_else(|| {
                GpLoadError(format!("track in round {} which has no round", t.round_idx))
            })?;
            let submitter = user(t.submitter_id);
            // The submitter is the game's, kept on the `GpTrack` and as the
            // submissions key; it is deliberately never put on the track itself.
            let resolved = ResolvedTrack::from_saved(&SavedTrack::from_secs(
                t.url.clone(),
                t.title.clone(),
                t.artist.clone(),
                t.duration_secs,
            ));
            match t.position {
                None => {
                    round.submissions.insert(submitter, resolved);
                },
                Some(pos) => {
                    if pos as usize != round.tracks.len() {
                        return Err(GpLoadError(format!(
                            "round {} track at position {pos} where {} was expected",
                            t.round_idx,
                            round.tracks.len()
                        )));
                    }
                    round.tracks.push(GpTrack {
                        submitter,
                        track: resolved,
                        guesses: t
                            .guesses
                            .iter()
                            .map(|(a, b)| (user(*a), user(*b)))
                            .collect(),
                        likes: t.likes.iter().map(|u| user(*u)).collect(),
                        skip_votes: t.skip_votes.iter().map(|u| user(*u)).collect(),
                        full_votes: t.full_votes.iter().map(|u| user(*u)).collect(),
                        play_full: t.play_full,
                        failed: t.failed,
                        message: message(t.message_channel_id, t.message_id),
                    });
                },
            }
        }

        let current_round = g.current_round.max(0) as usize;
        if phase != GpPhase::Finished && current_round >= rounds.len() {
            return Err(GpLoadError(format!(
                "current round {current_round} of {}",
                rounds.len()
            )));
        }
        let current_track = g.current_track.max(0) as usize;
        if phase == GpPhase::Playing && current_track >= rounds[current_round].tracks.len() {
            return Err(GpLoadError(format!(
                "current track {current_track} of {} in round {current_round}",
                rounds[current_round].tracks.len()
            )));
        }

        let mut players = HashMap::new();
        let mut scores = HashMap::new();
        for p in &saved.players {
            players.insert(user(p.user_id), p.display_name.clone());
            if p.score > 0 {
                scores.insert(user(p.user_id), p.score as u32);
            }
        }

        Ok(GpGame {
            guild_id: GuildId::new(g.guild_id as u64),
            started_at: g.started_at,
            host: user(g.host_id),
            voice_channel: ChannelId::new(g.voice_channel_id as u64),
            text_channel: chan(g.text_channel_id),
            phase,
            category,
            rounds,
            current_round,
            current_track,
            timer_secs: g.timer_secs.max(0) as u64,
            clip,
            reveal,
            round_results: g.round_results,
            generation: g.generation.max(0) as u64,
            parked_for_end: false,
            players,
            scores,
        })
    }
}

fn game_user(id: UserId) -> i64 {
    id.get() as i64
}

fn sorted_users(set: &HashSet<UserId>) -> Vec<i64> {
    let mut v: Vec<i64> = set.iter().map(|u| game_user(*u)).collect();
    v.sort_unstable();
    v
}

/// The non-bot members of `vc`, from the guild as the gateway just described it.
///
/// `me` is checked by id rather than left to the member lookup: `guild.members`
/// arrives truncated for a large guild, and an absent member is read as human so
/// that a player the payload left out still counts. The bot's own voice state
/// from the session that just died is usually still in the payload, and counting
/// it would let the "nobody is left in the channel" check pass on the bot's own
/// ghost -- resuming the game to an empty room.
fn vc_members(guild: &Guild, vc: ChannelId, me: UserId) -> usize {
    guild
        .voice_states
        .iter()
        .filter(|vs| vs.channel_id == Some(vc))
        .filter(|vs| vs.user_id != me)
        .filter(|vs| guild.members.get(&vs.user_id).is_none_or(|m| !m.user.bot()))
        .count()
}

/// Bring back the guild's live game, if it has one and it is worth bringing
/// back. Called for every guild as it arrives after a (re)connect; a guild with
/// a game already in memory, or with nothing saved, is a quick no.
pub async fn gp_resume_guild(data: &Data, ctx: &SerenityContext, guild: &Guild) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    let guild_id = guild.id;
    if data.gp_is_active(guild_id) {
        return;
    }
    let loaded = match GpSaved::load_live(pool, game_id(guild_id)).await {
        Ok(Some(l)) => l,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("gp: looking for a saved game in {guild_id}: {e}");
            return;
        },
    };
    let started_at = loaded.saved.game.started_at;
    let game = match GpGame::from_saved(&loaded.saved) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(
                "gp: the saved game in {guild_id} could not be loaded ({e}); dropping it"
            );
            if let Err(e) =
                gp_mark_finished(pool, game_id(guild_id), started_at, GpOutcome::Lost).await
            {
                tracing::warn!("gp: marking the unloadable game in {guild_id} lost: {e}");
            }
            return;
        },
    };
    let text_channel = game.text_channel;

    // Alive until when? An open window says so itself: it was open until
    // `closes_at` whatever happened to the bot. A song has no such clock, so the
    // last time the bot wrote or stamped the game stands in.
    let alive_until = match game.phase {
        GpPhase::Submitting => game.rounds[game.current_round].closes_at.unwrap_or(0),
        _ => now() - loaded.last_seen_secs_ago,
    };
    let down_for = now() - alive_until;
    let too_late = down_for > GP_RESUME_WINDOW_SECS;
    let nobody = vc_members(guild, game.voice_channel, ctx.cache.current_user().id) == 0;
    if too_late || nobody || game.phase == GpPhase::Finished {
        let why = if too_late {
            format!("gone for {down_for}s")
        } else if nobody {
            "the voice channel is empty".to_string()
        } else {
            "it had already finished".to_string()
        };
        tracing::info!("gp: not resuming the game in {guild_id}: {why}");
        // Say so only once. `on_guild_create` runs again on every reconnect, and
        // while the tombstone is not written the row is still live and still
        // hopeless -- so posting regardless would put a fresh scoreboard in the
        // channel each time the gateway blinked.
        let marked =
            match gp_mark_finished(pool, game_id(guild_id), started_at, GpOutcome::Lost).await {
                Ok(_) => true,
                Err(e) => {
                    tracing::warn!("gp: marking the game in {guild_id} lost: {e}");
                    false
                },
            };
        if marked && game.phase != GpPhase::Finished {
            let scores = game.sorted_scores();
            let msg = CreateMessage::new()
                .content(GP_LOST)
                .embed(gp_scoreboard_embed(&scores, GP_SCOREBOARD));
            if let Err(e) = text_channel.send_message(&ctx.http, msg).await {
                tracing::warn!("gp: posting the lost game's scoreboard in {guild_id}: {e}");
            }
        }
        return;
    }

    let (phase, voice_channel, current_round) =
        (game.phase, game.voice_channel, game.current_round);
    let closes_at = game.rounds[current_round].closes_at;
    if !data.gp_restore(guild_id, game) {
        // `/gp start` beat us to it; that game's first snapshot closes this row.
        return;
    }
    let call = match data.songbird.join(guild_id, voice_channel).await {
        Ok(call) => call,
        Err(e) => {
            tracing::warn!("gp: rejoining {voice_channel} in {guild_id} to resume: {e}");
            data.gp_games.remove(&guild_id);
            if let Err(e) =
                gp_mark_finished(pool, game_id(guild_id), started_at, GpOutcome::Lost).await
            {
                tracing::warn!("gp: marking the game in {guild_id} lost: {e}");
            }
            let _ = text_channel
                .send_message(&ctx.http, CreateMessage::new().content(GP_LOST))
                .await;
            return;
        },
    };
    set_global_handlers_with(
        ctx,
        Arc::new(data.clone()),
        call.clone(),
        guild_id,
        text_channel,
    )
    .await;
    let pb = GpPlayback {
        data: Arc::new(data.clone()),
        http: ctx.http.clone(),
        call,
        guild_id,
    };
    tracing::info!("gp: resuming the game in {guild_id} in {phase:?}, round {current_round}");

    match phase {
        GpPhase::Submitting => {
            let remaining = closes_at.unwrap_or(0) - now();
            let round_no = current_round + 1;
            // The generation the restore left in the map is the one the timer
            // must match; read it back rather than trusting the copy we had.
            let generation = data
                .gp_games
                .get(&guild_id)
                .map(|g| g.generation)
                .unwrap_or(0);
            if remaining <= 0 {
                announce(
                    &pb,
                    text_channel,
                    &GP_RESUMED_WINDOW_CLOSED.replace("{round}", &round_no.to_string()),
                )
                .await;
                // Bound first: a `ThreadRng` temporary is not `Send`, and the
                // condition of an `if let` lives across the body's await.
                let closed = data.gp_close_window_if(guild_id, generation, &mut rand::rng(), now());
                if let Some(closed) = closed {
                    if let Err(e) = gp_after_close(pb, closed).await {
                        tracing::warn!("gp: closing the resumed window in {guild_id}: {e}");
                    }
                }
            } else {
                announce(
                    &pb,
                    text_channel,
                    &format!(
                        "{GP_RESUMED_WINDOW} {round_no}, <t:{}:R>.",
                        closes_at.unwrap_or(0)
                    ),
                )
                .await;
                gp_spawn_window_timer_secs(pb, generation, text_channel, remaining as u64);
            }
        },
        GpPhase::Playing => {
            let Some((start, old_message)) = data.gp_resume_playing(guild_id) else {
                return;
            };
            announce(&pb, text_channel, GP_RESUMED_SONG).await;
            // Restore the one-live-dropdown invariant: the reveal only edits the
            // message the track remembers, which is about to be the new one.
            if let Some((c, m)) = old_message {
                if let Err(e) = c
                    .edit_message(
                        &pb.http,
                        m,
                        EditMessage::new().components(Vec::<CreateComponent<'_>>::new()),
                    )
                    .await
                {
                    tracing::warn!(
                        "gp: taking down the pre-restart song message in {guild_id}: {e}"
                    );
                }
            }
            if let Err(e) = gp_play_track(&pb, start).await {
                tracing::warn!("gp: restarting the song in {guild_id}: {e}");
            }
        },
        GpPhase::Finished => unreachable!("finished games are not restored"),
    }
}

async fn announce(pb: &GpPlayback, text_channel: GenericChannelId, what: &str) {
    if let Err(e) = text_channel
        .send_message(
            &pb.http,
            CreateMessage::new().content(format!("{GP_RESUMED} {what}")),
        )
        .await
    {
        tracing::warn!("gp: announcing the resume in {}: {e}", pb.guild_id);
    }
}

// ------------------------------------------------------------------
// Tests: what gets written, and that rows come back as the same game
// ------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use crate::DataInner;
    use crack_types::{AuxMetadata, QueryType};
    use rand::{rngs::StdRng, SeedableRng};

    const G: GuildId = GuildId::new(1);
    const VC: ChannelId = ChannelId::new(10);
    const TC: GenericChannelId = GenericChannelId::new(20);
    const A: UserId = UserId::new(100);
    const B: UserId = UserId::new(200);
    const C: UserId = UserId::new(300);
    const NOW: i64 = 1_700_000_000;

    /// A `Data` whose writes land in a channel instead of a database.
    fn recording() -> (Data, mpsc::UnboundedReceiver<GpPersist>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let data = Data(Arc::new(DataInner {
            gp_persist: Some(tx),
            ..Default::default()
        }));
        (data, rx)
    }

    fn track(title: &str) -> ResolvedTrack<'static> {
        ResolvedTrack::new(QueryType::VideoLink(format!(
            "https://www.youtube.com/watch?v={title}"
        )))
        .with_metadata(AuxMetadata {
            title: Some(title.to_string()),
            artist: Some("artist".into()),
            duration: Some(Duration::from_secs(180)),
            source_url: Some(format!("https://www.youtube.com/watch?v={title}")),
            ..Default::default()
        })
    }

    fn start(data: &Data, prompts: &[&str]) {
        data.gp_start(
            G,
            A,
            "alice".into(),
            VC,
            TC,
            GpCategory::Nostalgia,
            prompts.iter().map(|s| s.to_string()).collect(),
            120,
            Some(GpClip {
                start: Duration::from_secs(30),
                length: Duration::from_secs(45),
            }),
            GpReveal::Song,
            true,
            NOW,
        )
        .unwrap();
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<GpPersist>) -> Vec<GpPersist> {
        let mut v = Vec::new();
        while let Ok(m) = rx.try_recv() {
            v.push(m);
        }
        v
    }

    fn snapshot(m: &GpPersist) -> &GpSaved {
        match m {
            GpPersist::Snapshot(s) => s,
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    #[test]
    fn a_submission_is_written_and_nothing_before_it_is() {
        let (data, mut rx) = recording();
        start(&data, &["p0", "p1"]);
        assert!(drain(&mut rx).is_empty(), "starting a game writes nothing");

        data.gp_submit(G, B, "bob".into(), track("b"), &[]).unwrap();
        let msgs = drain(&mut rx);
        assert_eq!(msgs.len(), 1);
        let s = snapshot(&msgs[0]);
        assert_eq!(s.game.guild_id, 1);
        assert_eq!(s.game.started_at, NOW);
        assert_eq!(s.game.phase, PHASE_SUBMITTING);
        assert_eq!(s.rounds.len(), 2);
        assert_eq!(s.rounds[0].closes_at, Some(NOW + 120));
        assert_eq!(s.tracks.len(), 1);
        assert_eq!(s.tracks[0].position, None, "still a submission");
        assert_eq!(s.tracks[0].submitter_id, 200);
        assert_eq!(s.tracks[0].title.as_deref(), Some("b"));
        assert_eq!(s.players.len(), 2, "host and bob");

        // Resubmitting replaces: still one track, new title.
        data.gp_submit(G, B, "bob".into(), track("b2"), &[])
            .unwrap();
        let msgs = drain(&mut rx);
        let s = snapshot(&msgs[0]);
        assert_eq!(s.tracks.len(), 1);
        assert_eq!(s.tracks[0].title.as_deref(), Some("b2"));
    }

    #[test]
    fn closing_and_the_song_message_are_written_but_guesses_wait_for_the_song() {
        let (data, mut rx) = recording();
        start(&data, &["p0"]);
        data.gp_submit(G, A, "alice".into(), track("a"), &[])
            .unwrap();
        data.gp_submit(G, B, "bob".into(), track("b"), &[]).unwrap();
        drain(&mut rx);

        // Closing writes the play order and the phase, so a resume during the
        // round's first song does not read the passed `closes_at` as an outage.
        let closed = data
            .gp_close_window(G, A, &mut StdRng::seed_from_u64(0), NOW)
            .unwrap();
        assert_eq!(closed.count, 2);
        let msgs = drain(&mut rx);
        assert_eq!(msgs.len(), 1);
        let s = snapshot(&msgs[0]);
        assert_eq!(s.game.phase, PHASE_PLAYING);
        assert_eq!(s.rounds[0].closes_at, None, "the window is not open");
        assert_eq!(
            s.tracks.iter().filter(|t| t.position.is_some()).count(),
            2,
            "both songs have their place in the round"
        );

        // And the song's message, which is what a resume takes the pre-restart
        // dropdown down by.
        data.gp_set_track_message(G, 0, 0, TC, MessageId::new(555))
            .unwrap();
        let msgs = drain(&mut rx);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            snapshot(&msgs[0])
                .tracks
                .iter()
                .find(|t| t.position == Some(0))
                .unwrap()
                .message_id,
            Some(555)
        );

        let first = data.gp_games.get(&G).unwrap().rounds[0].tracks[0].submitter;
        let guesser = if first == A { B } else { A };
        data.gp_record_guess(G, 0, 0, guesser, "x".into(), first)
            .unwrap();
        data.gp_toggle_like(G, 0, 0, guesser, "x".into()).unwrap();
        assert!(
            drain(&mut rx).is_empty(),
            "guessing and liking are not checkpoints"
        );

        data.gp_reveal_and_advance(G, 0, 0, NOW + 200).unwrap();
        let msgs = drain(&mut rx);
        assert_eq!(msgs.len(), 1);
        let s = snapshot(&msgs[0]);
        assert_eq!(s.game.phase, PHASE_PLAYING);
        assert_eq!(s.game.current_track, 1);
        assert_eq!(s.tracks.len(), 2);
        let t0 = s.tracks.iter().find(|t| t.position == Some(0)).unwrap();
        assert_eq!(t0.submitter_id, first.get() as i64);
        assert_eq!(t0.guesses, vec![(guesser.get() as i64, first.get() as i64)]);
        assert_eq!(t0.likes, vec![guesser.get() as i64]);
        assert_eq!(t0.message_id, Some(555));
        let scores: HashMap<i64, i32> = s.players.iter().map(|p| (p.user_id, p.score)).collect();
        assert_eq!(scores[&(guesser.get() as i64)], 100, "correct guess paid");
        assert_eq!(scores[&(first.get() as i64)], 10, "one like paid");

        // The last song: the snapshot says finished, and removing the game
        // afterwards records that it finished.
        data.gp_reveal_and_advance(G, 0, 1, NOW + 300).unwrap();
        let msgs = drain(&mut rx);
        assert_eq!(snapshot(&msgs[0]).game.phase, PHASE_FINISHED);
        data.gp_remove(G);
        match &drain(&mut rx)[..] {
            [GpPersist::Finished {
                guild_id: 1,
                started_at: NOW,
                outcome: GpOutcome::Finished,
            }] => {},
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ending_a_game_leaves_a_tombstone_however_it_goes() {
        // `/gp end` mid-song: parked, then collected by the track-end handler.
        let (data, mut rx) = recording();
        start(&data, &["p0"]);
        data.gp_submit(G, A, "alice".into(), track("a"), &[])
            .unwrap();
        drain(&mut rx);
        data.gp_park_for_end(G, A, false).unwrap();
        assert!(drain(&mut rx).is_empty(), "parking is not a checkpoint");
        assert!(data.gp_remove_if_parked(G));
        match &drain(&mut rx)[..] {
            [GpPersist::Finished {
                outcome: GpOutcome::Ended,
                ..
            }] => {},
            other => panic!("{other:?}"),
        }

        // `/gp end` with nothing playing: removed directly, still "ended".
        let (data, mut rx) = recording();
        start(&data, &["p0"]);
        data.gp_park_for_end(G, A, false).unwrap();
        data.gp_remove(G);
        match &drain(&mut rx)[..] {
            [GpPersist::Finished {
                outcome: GpOutcome::Ended,
                ..
            }] => {},
            other => panic!("{other:?}"),
        }

        // Torn down for any other reason.
        let (data, mut rx) = recording();
        start(&data, &["p0"]);
        data.gp_remove(G);
        match &drain(&mut rx)[..] {
            [GpPersist::Finished {
                outcome: GpOutcome::Abandoned,
                ..
            }] => {},
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn without_a_database_nothing_is_sent_and_nothing_panics() {
        let data = Data::default();
        start(&data, &["p0"]);
        data.gp_submit(G, A, "alice".into(), track("a"), &[])
            .unwrap();
        data.gp_shutdown(Duration::from_millis(10)).await;
        data.gp_remove(G);
    }

    #[tokio::test]
    async fn shutdown_stamps_this_process_games_and_waits_for_the_writer() {
        let (data, mut rx) = recording();
        // Nobody is draining: the flush times out rather than hanging shutdown.
        let started = std::time::Instant::now();
        data.gp_shutdown(Duration::from_millis(50)).await;
        assert!(started.elapsed() >= Duration::from_millis(50));
        match &drain(&mut rx)[..] {
            [GpPersist::Stopping { guild_ids }, GpPersist::Flush(_)] => {
                assert!(guild_ids.is_empty(), "no games, nothing to vouch for");
            },
            other => panic!("{other:?}"),
        }

        // With a game running, that guild -- and only that guild -- is stamped.
        start(&data, &["p0"]);
        data.gp_shutdown(Duration::from_millis(10)).await;
        match &drain(&mut rx)[..] {
            [GpPersist::Stopping { guild_ids }, GpPersist::Flush(_)] => {
                assert_eq!(guild_ids, &[1]);
            },
            other => panic!("{other:?}"),
        }
        data.gp_remove(G);
        drain(&mut rx);
        // With the writer's end gone the flush returns at once.
        drop(rx);
        let started = std::time::Instant::now();
        data.gp_shutdown(Duration::from_secs(5)).await;
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// Build a game through the real API into a mid-game state, then check the
    /// rows come back as the same game -- compared as rows, since neither
    /// `GpGame` nor `ResolvedTrack` compares.
    #[test]
    fn a_mid_game_round_trips_through_its_rows() {
        let (data, mut rx) = recording();
        start(&data, &["p0", "p1", "p2"]);
        data.gp_submit(G, A, "alice".into(), track("a0"), &[])
            .unwrap();
        data.gp_submit(G, B, "bob".into(), track("b0"), &[])
            .unwrap();
        data.gp_submit(G, C, "carol".into(), track("c0"), &[])
            .unwrap();
        data.gp_set_prompt_message(G, 0, TC, MessageId::new(1))
            .unwrap();
        data.gp_close_window(G, A, &mut StdRng::seed_from_u64(7), NOW)
            .unwrap();
        let order: Vec<UserId> = data.gp_games.get(&G).unwrap().rounds[0]
            .tracks
            .iter()
            .map(|t| t.submitter)
            .collect();
        // Song 0: everyone else guesses right, one like, then it ends.
        for u in [A, B, C].into_iter().filter(|u| *u != order[0]) {
            data.gp_record_guess(G, 0, 0, u, "n".into(), order[0])
                .unwrap();
        }
        let liker = [A, B, C].into_iter().find(|u| *u != order[0]).unwrap();
        data.gp_toggle_like(G, 0, 0, liker, "n".into()).unwrap();
        data.gp_set_track_message(G, 0, 0, TC, MessageId::new(2))
            .unwrap();
        data.gp_reveal_and_advance(G, 0, 0, NOW + 100).unwrap();
        // Song 1 is playing: a vote to hear it in full, and a guess nobody has
        // seen paid yet.
        let voter = [A, B, C].into_iter().find(|u| *u != order[1]).unwrap();
        data.gp_vote_full(G, voter, "n".into(), &[A, B, C]).unwrap();
        data.gp_record_guess(G, 0, 1, voter, "n".into(), order[0])
            .unwrap();
        data.gp_set_track_message(G, 0, 1, TC, MessageId::new(3))
            .unwrap();
        drain(&mut rx);

        let game = data.gp_games.get(&G).unwrap().clone();
        let saved = game.to_saved();
        let back = GpGame::from_saved(&saved).expect("loads");
        assert_eq!(back.to_saved(), saved);

        // And the things the resume reads off it are what they were.
        assert_eq!(back.guild_id, G);
        assert_eq!(back.phase, GpPhase::Playing);
        assert_eq!((back.current_round, back.current_track), (0, 1));
        assert_eq!(back.rounds.len(), 3);
        assert_eq!(back.rounds[0].prompt_message, Some((TC, MessageId::new(1))));
        assert_eq!(
            back.rounds[0].tracks[1].message,
            Some((TC, MessageId::new(3)))
        );
        assert_eq!(back.rounds[0].tracks[1].full_votes.len(), 1);
        assert_eq!(back.rounds[0].tracks[0].likes.len(), 1);
        assert_eq!(back.rounds[0].tracks[0].guesses.len(), 2);
        assert_eq!(back.scores, game.scores);
        assert_eq!(back.players, game.players);
        assert_eq!(back.clip, game.clip);
        assert_eq!(back.reveal, GpReveal::Song);
        assert!(!back.parked_for_end);
        let t = &back.rounds[0].tracks[0].track;
        assert_eq!(t.get_title(), game.rounds[0].tracks[0].track.get_title());
        assert_eq!(t.get_url(), game.rounds[0].tracks[0].track.get_url());
        assert_eq!(
            t.get_metadata().and_then(|m| m.duration),
            Some(Duration::from_secs(180))
        );
        // The rebuilt song still knows how long it is, so the clip fits to it.
        let start = back.track_start();
        assert_eq!(start.clip.map(|c| c.length), Some(Duration::from_secs(45)));
        assert_eq!(start.players.len(), 3);
    }

    /// The reveal is the whole game, and a song that names its submitter gives
    /// it away. The live path deliberately leaves the requester off a game's
    /// tracks -- the now-playing embed renders the sentinel as "(auto)" -- and a
    /// song rebuilt after a restart must not put it back, or `/np` would out
    /// every submitter for the rest of the game.
    #[test]
    fn a_song_rebuilt_from_its_rows_does_not_name_its_submitter() {
        let (data, _rx) = recording();
        start(&data, &["p0"]);
        data.gp_submit(G, B, "bob".into(), track("b0"), &[])
            .unwrap();
        data.gp_close_window(G, A, &mut StdRng::seed_from_u64(3), NOW)
            .unwrap();
        let saved = data.gp_games.get(&G).unwrap().to_saved();

        let back = GpGame::from_saved(&saved).unwrap();
        let t = &back.rounds[0].tracks[0];
        assert_eq!(t.submitter, B, "the game still knows whose song it is");
        assert_eq!(
            t.track.get_requesting_user(),
            UserId::new(1),
            "the song itself does not"
        );
    }

    /// A song that never played paid nothing, and must still pay nothing when
    /// the round's results are derived after a resume; and a game that holds
    /// its reveal to the end of the round has to come back still holding it.
    #[test]
    fn what_a_resume_needs_to_keep_the_round_honest_round_trips() {
        let (data, _rx) = recording();
        data.gp_start(
            G,
            A,
            "alice".into(),
            VC,
            TC,
            GpCategory::Nostalgia,
            vec!["p0".into()],
            120,
            None,
            GpReveal::Round,
            false,
            NOW,
        )
        .unwrap();
        data.gp_submit(G, A, "alice".into(), track("a"), &[])
            .unwrap();
        data.gp_submit(G, B, "bob".into(), track("b"), &[]).unwrap();
        data.gp_close_window(G, A, &mut StdRng::seed_from_u64(0), NOW)
            .unwrap();
        data.gp_fail_and_advance(G, 0, 0, NOW).unwrap();

        let saved = data.gp_games.get(&G).unwrap().to_saved();
        assert_eq!(saved.game.reveal, "round");
        assert!(!saved.game.round_results);
        let t0 = saved.tracks.iter().find(|t| t.position == Some(0)).unwrap();
        assert!(t0.failed);
        assert!(
            !saved
                .tracks
                .iter()
                .find(|t| t.position == Some(1))
                .unwrap()
                .failed
        );

        let back = GpGame::from_saved(&saved).unwrap();
        assert_eq!(back.to_saved(), saved);
        assert_eq!(back.reveal, GpReveal::Round);
        assert!(!back.round_results);
        assert!(back.rounds[0].tracks[0].failed);
        assert!(!back.rounds[0].tracks[1].failed);
        // Held: the board the resumed game shows is still the empty one.
        assert!(back.visible_scores().iter().all(|(_, p)| *p == 0));
    }

    #[test]
    fn a_submitting_game_round_trips_with_its_open_window() {
        let (data, _rx) = recording();
        start(&data, &["p0", "p1"]);
        data.gp_submit(G, B, "bob".into(), track("b0"), &[])
            .unwrap();
        let game = data.gp_games.get(&G).unwrap().clone();
        let saved = game.to_saved();
        let back = GpGame::from_saved(&saved).unwrap();
        assert_eq!(back.to_saved(), saved);
        assert_eq!(back.phase, GpPhase::Submitting);
        assert_eq!(
            back.rounds[0].submissions[&B].get_requesting_user(),
            UserId::new(1),
            "a submission does not name its submitter either"
        );
        assert_eq!(back.rounds[0].closes_at, Some(NOW + 120));
        assert_eq!(back.rounds[0].submissions.len(), 1);
        assert!(back.rounds[0].tracks.is_empty());
        assert_eq!(back.rounds[1].closes_at, None);
    }

    #[test]
    fn rows_this_code_would_not_write_are_refused_not_trusted() {
        let (data, _rx) = recording();
        start(&data, &["p0"]);
        data.gp_submit(G, B, "bob".into(), track("b0"), &[])
            .unwrap();
        let good = data.gp_games.get(&G).unwrap().to_saved();

        let mut s = good.clone();
        s.game.category = "🎲 Mixed".into();
        assert!(
            GpGame::from_saved(&s).is_err(),
            "display name is not the slug"
        );

        let mut s = good.clone();
        s.game.phase = "paused".into();
        assert!(GpGame::from_saved(&s).is_err());

        let mut s = good.clone();
        s.game.reveal = "never".into();
        assert!(GpGame::from_saved(&s).is_err());

        let mut s = good.clone();
        s.tracks[0].round_idx = 5;
        assert!(
            GpGame::from_saved(&s).is_err(),
            "track in a round that is not there"
        );

        let mut s = good.clone();
        s.game.phase = PHASE_PLAYING.into();
        assert!(
            GpGame::from_saved(&s).is_err(),
            "playing with no track in play order"
        );

        let mut s = good.clone();
        s.rounds.remove(0);
        s.tracks.clear();
        assert!(
            GpGame::from_saved(&s).is_err(),
            "current round past the end"
        );

        // Finished games may point past the last round; that is what finished means.
        let mut s = good;
        s.game.phase = PHASE_FINISHED.into();
        s.game.current_round = 1;
        assert!(GpGame::from_saved(&s).is_ok());
    }
}
