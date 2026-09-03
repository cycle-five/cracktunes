//! "What's your song?" party game (`/gp`).
//!
//! The host picks a category (see [`gp_prompts`]) and a round count. Each
//! round the bot posts a prompt ("What song do you cry to?") and opens a timed
//! submission window: everyone in the voice channel secretly submits one song
//! with the ephemeral `/gp submit` (resubmitting replaces it). The window
//! closes on the timer, as soon as every non-bot member of the voice channel
//! has submitted, or when the host runs `/gp close`. The round's songs are
//! then shuffled and played back-to-back; while a song plays everyone picks,
//! from a dropdown of that round's submitters, who they think submitted it,
//! and can 👍 the song. When the track ends (naturally, or via `/gp skip`) the
//! submitter is revealed, points are awarded and the next song -- or the next
//! prompt -- follows. Scores live in memory for the game only.
//!
//! # Locking rule
//!
//! Game state lives in `Data::gp_games`, a `DashMap` keyed by guild. Every
//! `impl Data` helper in this file is **synchronous**: it takes the map entry,
//! mutates it, clones out whatever the caller needs, and releases the entry
//! before returning. Never hold a `DashMap` ref across an `.await` -- the round
//! end handler runs on songbird's event task and then takes the call lock, so a
//! held entry there is a deadlock waiting to happen.
//!
//! # Stale timers and double fires
//!
//! Every phase transition bumps `GpGame::generation`. The window timer captures
//! the generation it was spawned for and asks `gp_close_window_if` /
//! `gp_warning_if`, which are no-ops once it has moved (the host closed the
//! window, everyone submitted, the game ended). Track-end handlers likewise only
//! act on the *current* round/track, so `End` and `Error` firing for one track
//! is harmless.
//!
//! # Hiding the requester
//!
//! The game's tracks are enqueued **without** `.with_user_id(submitter)`, so the
//! track data carries the default `UserId::new(1)` sentinel which the now-playing
//! and queue embeds render as "(auto)". The real submitter exists only in the
//! game state.

use crate::{
    commands::cmd_check_music,
    commands::get_call_or_join_author,
    commands::music::gp_prompts::{draw_prompts, GpCategory},
    commands::music::skip::force_skip_top_track,
    errors::CrackedError,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    messaging::messages::{
        GP_FOOLED_EVERYONE, GP_GAME_OVER, GP_GUESSED_RIGHT, GP_GUESS_CHANGED, GP_GUESS_RECORDED,
        GP_HOW_TO, GP_HOW_TO_TITLE, GP_LIKED, GP_LIKES, GP_LIKE_HINT, GP_LIKE_LABEL,
        GP_NOBODY_GUESSED, GP_NOBODY_YET, GP_PROMPT_CLOSES_EARLY, GP_PROMPT_CLOSES_TITLE,
        GP_PROMPT_HOW_TO, GP_PROMPT_HOW_TO_TITLE, GP_REVEAL, GP_ROUND_HINT, GP_ROUND_TITLE,
        GP_RULES_TEXT, GP_SCOREBOARD, GP_SELECT_PLACEHOLDER, GP_SONG_TITLE, GP_STATUS_CLOSES,
        GP_STATUS_GUESSED, GP_STATUS_LIKES, GP_STATUS_PLAYING, GP_STATUS_PROMPT, GP_STATUS_SCORES,
        GP_STATUS_SUBMITTED, GP_STATUS_SUBMITTING, GP_TITLE, GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE,
        GP_UNLIKED, GP_WINDOW_CLOSED, GP_WINDOW_CLOSED_SONGS, GP_WINDOW_EMPTY, GP_WINDOW_WARNING,
        GP_WINDOW_WARNING_IN,
    },
    music::queue::build_track,
    poise_ext::PoiseContextExt,
    Context, CrackedResult, Data, Error,
};
use ::serenity::{
    all::{
        ButtonStyle, ChannelId, Colour, ComponentInteraction, ComponentInteractionDataKind,
        GenericChannelId, GuildId, Mentionable, MessageId, UserId,
    },
    async_trait,
    builder::{
        CreateActionRow, CreateButton, CreateComponent, CreateEmbed, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateMessage, CreateSelectMenu, CreateSelectMenuKind,
        CreateSelectMenuOption, EditMessage,
    },
    http::Http,
};
use crack_testing::ResolvedTrack;
use crack_types::QueryType;
use poise::serenity_prelude::Context as SerenityContext;
use rand::{seq::SliceRandom, Rng};
use songbird::tracks::PlayMode;
use songbird::{Call, Event, EventContext, EventHandler, TrackEvent};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

/// Discord caps a string select menu at 25 options, so at most 25 people can
/// submit in one round.
pub const GP_MAX_PLAYERS: usize = 25;
/// Pause between a reveal and the next song, so people can read it.
pub const GP_REVEAL_PAUSE_SECS: u64 = 5;
/// Points for guessing the submitter correctly.
pub const GP_POINTS_CORRECT: u32 = 100;
/// Points to the submitter when nobody guessed them.
pub const GP_POINTS_FOOLED_ALL: u32 = 100;
/// Points to the submitter per 👍 their song gets.
pub const GP_POINTS_PER_LIKE: u32 = 10;
/// Rounds per game when the host doesn't say.
pub const GP_DEFAULT_ROUNDS: u32 = 5;
/// Submission window when the host doesn't say.
pub const GP_DEFAULT_TIMER_SECS: u64 = 180;
/// Bounds for `/gp start`'s `rounds` and `timer`. poise's `#[min]`/`#[max]`
/// only accept literals, so the attributes on `gp_start` repeat these numbers
/// -- keep them in sync by hand.
pub const GP_MAX_ROUNDS: u32 = 20;
pub const GP_MIN_TIMER_SECS: u64 = 30;
pub const GP_MAX_TIMER_SECS: u64 = 600;
/// A heads-up is posted this long before the window closes.
pub const GP_WARNING_SECS: u64 = 30;
/// Component custom ids look like `gp:<g|l>:<guild_id>:<round_idx>:<track_idx>`.
pub const GP_CUSTOM_ID_PREFIX: &str = "gp:";
/// Music commands that would corrupt the round order while a game owns playback.
/// Matched against the command's *qualified* name so `gp skip` is not caught by `skip`.
pub const GP_BLOCKED_COMMANDS: &[&str] = &[
    "play",
    "playnext",
    "playfile",
    "playytplaylist",
    "optplay",
    "search",
    "clear",
    "stop",
    "shuffle",
    "remove",
    "movesong",
    "skip",
    "leave",
    "seek",
    "repeat",
];

/// Unix seconds now; the game stores `closes_at` this way so the prompt embed
/// can show Discord's live `<t:..:R>` countdown with a single send.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ------------------------------------------------------------------
// State
// ------------------------------------------------------------------

/// Which stage the game is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpPhase {
    /// A prompt is up and the submission window is open.
    Submitting,
    /// The round's songs are being played.
    Playing,
    /// Last song revealed; the game is torn down once the scoreboard is posted.
    Finished,
}

/// One song in a round, with everyone's guesses and likes.
#[derive(Clone, Debug)]
pub struct GpTrack {
    pub submitter: UserId,
    pub track: ResolvedTrack<'static>,
    /// guesser -> guessed submitter. Last guess wins.
    pub guesses: HashMap<UserId, UserId>,
    pub likes: HashSet<UserId>,
    /// The song message, so the reveal can edit it in place.
    pub message: Option<(GenericChannelId, MessageId)>,
}

/// One prompt: what people submitted while the window was open, then the
/// shuffled play order once it closed.
#[derive(Clone, Debug)]
pub struct GpRound {
    pub prompt: String,
    /// One song per player while the window is open; resubmitting replaces.
    pub submissions: HashMap<UserId, ResolvedTrack<'static>>,
    /// Filled (shuffled) when the window closes.
    pub tracks: Vec<GpTrack>,
    /// The prompt message, so the close can edit it in place.
    pub prompt_message: Option<(GenericChannelId, MessageId)>,
    /// Unix seconds; `Some` while the window is open.
    pub closes_at: Option<i64>,
}

impl GpRound {
    fn new(prompt: String) -> Self {
        Self {
            prompt,
            submissions: HashMap::new(),
            tracks: Vec::new(),
            prompt_message: None,
            closes_at: None,
        }
    }

    /// Distinct people with a song in this round (the only valid guesses).
    fn submitters(&self) -> Vec<UserId> {
        let mut ids: Vec<UserId> = if self.tracks.is_empty() {
            self.submissions.keys().copied().collect()
        } else {
            self.tracks.iter().map(|t| t.submitter).collect()
        };
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// A one-song round has nothing to guess: the dropdown is left out and no
    /// guess/fooled points are awarded (likes still count).
    fn guessable(&self) -> bool {
        self.tracks.len() >= 2
    }
}

/// The per-guild game.
#[derive(Clone, Debug)]
pub struct GpGame {
    pub host: UserId,
    pub voice_channel: ChannelId,
    pub text_channel: GenericChannelId,
    pub phase: GpPhase,
    pub category: GpCategory,
    /// Pre-drawn, one per prompt.
    pub rounds: Vec<GpRound>,
    pub current_round: usize,
    pub current_track: usize,
    pub timer_secs: u64,
    /// Bumped on every transition; see the module docs.
    pub generation: u64,
    /// Everyone who has submitted, guessed or liked, with the display name we saw.
    pub players: HashMap<UserId, String>,
    pub scores: HashMap<UserId, u32>,
}

impl GpGame {
    fn new(
        host: UserId,
        voice_channel: ChannelId,
        text_channel: GenericChannelId,
        category: GpCategory,
        prompts: Vec<String>,
        timer_secs: u64,
    ) -> Self {
        Self {
            host,
            voice_channel,
            text_channel,
            phase: GpPhase::Submitting,
            category,
            rounds: prompts.into_iter().map(GpRound::new).collect(),
            current_round: 0,
            current_track: 0,
            timer_secs,
            generation: 0,
            players: HashMap::new(),
            scores: HashMap::new(),
        }
    }

    fn name_of(&self, id: UserId) -> String {
        self.players
            .get(&id)
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }

    /// Submitters of `round` with their display names, sorted by name for a
    /// stable dropdown.
    fn submitter_names(&self, round: &GpRound) -> Vec<(UserId, String)> {
        let mut v: Vec<(UserId, String)> = round
            .submitters()
            .into_iter()
            .map(|id| (id, self.name_of(id)))
            .collect();
        v.sort_by_key(|(_, name)| name.to_lowercase());
        v
    }

    /// Every player with their points, best first; ties broken by name so the
    /// order is stable between edits.
    fn sorted_scores(&self) -> Vec<(UserId, u32)> {
        let mut v: Vec<(UserId, u32)> = self
            .players
            .keys()
            .map(|id| (*id, self.scores.get(id).copied().unwrap_or(0)))
            .collect();
        v.sort_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| {
                self.name_of(a.0)
                    .to_lowercase()
                    .cmp(&self.name_of(b.0).to_lowercase())
            })
        });
        v
    }

    /// Open the submission window for `current_round`.
    fn open_window(&mut self, now: i64) -> GpWindowOpened {
        self.phase = GpPhase::Submitting;
        self.current_track = 0;
        self.generation += 1;
        let closes_at = now + self.timer_secs as i64;
        let idx = self.current_round;
        let total_rounds = self.rounds.len();
        let round = &mut self.rounds[idx];
        round.closes_at = Some(closes_at);
        GpWindowOpened {
            round_idx: idx,
            total_rounds,
            prompt: round.prompt.clone(),
            closes_at,
            timer_secs: self.timer_secs,
            generation: self.generation,
            text_channel: self.text_channel,
        }
    }

    /// Close the window: shuffle the submissions into play order and either
    /// start playing, skip an empty round, or finish.
    fn close_window(&mut self, rng: &mut impl Rng, now: i64) -> GpWindowClosed {
        self.generation += 1;
        let idx = self.current_round;
        let total_rounds = self.rounds.len();
        let round = &mut self.rounds[idx];
        // Sort before shuffling so a seeded rng gives the same order regardless
        // of HashMap iteration order.
        let mut subs: Vec<(UserId, ResolvedTrack<'static>)> = round.submissions.drain().collect();
        subs.sort_by_key(|(id, _)| *id);
        subs.shuffle(rng);
        round.tracks = subs
            .into_iter()
            .map(|(submitter, track)| GpTrack {
                submitter,
                track,
                guesses: HashMap::new(),
                likes: HashSet::new(),
                message: None,
            })
            .collect();
        round.closes_at = None;
        let count = round.tracks.len();
        let prompt = round.prompt.clone();
        let prompt_message = round.prompt_message;
        let next = if count == 0 {
            self.advance_round(now)
        } else {
            self.phase = GpPhase::Playing;
            self.current_track = 0;
            GpNext::Track(Box::new(self.track_start()))
        };
        GpWindowClosed {
            round_idx: idx,
            total_rounds,
            prompt,
            prompt_message,
            count,
            text_channel: self.text_channel,
            next,
        }
    }

    /// Move to the next prompt, or finish.
    fn advance_round(&mut self, now: i64) -> GpNext {
        self.current_round += 1;
        if self.current_round < self.rounds.len() {
            GpNext::Window(self.open_window(now))
        } else {
            self.phase = GpPhase::Finished;
            self.generation += 1;
            GpNext::Finished(self.sorted_scores())
        }
    }

    /// Everything needed to play `current_track` of `current_round`.
    fn track_start(&self) -> GpTrackStart {
        let round = &self.rounds[self.current_round];
        let t = &round.tracks[self.current_track];
        GpTrackStart {
            round_idx: self.current_round,
            total_rounds: self.rounds.len(),
            track_idx: self.current_track,
            total_tracks: round.tracks.len(),
            prompt: round.prompt.clone(),
            track: t.track.clone(),
            players: self.submitter_names(round),
            guessable: round.guessable(),
            text_channel: self.text_channel,
        }
    }
}

/// A submission window just opened.
#[derive(Clone, Debug)]
pub struct GpWindowOpened {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub closes_at: i64,
    pub timer_secs: u64,
    pub generation: u64,
    pub text_channel: GenericChannelId,
}

/// A song is about to play.
#[derive(Clone, Debug)]
pub struct GpTrackStart {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub track_idx: usize,
    pub total_tracks: usize,
    pub prompt: String,
    pub track: ResolvedTrack<'static>,
    /// Dropdown options: the round's submitters, sorted by name.
    pub players: Vec<(UserId, String)>,
    pub guessable: bool,
    pub text_channel: GenericChannelId,
}

/// What happens after a window closes or a song ends.
#[derive(Clone, Debug)]
pub enum GpNext {
    /// Boxed: a `ResolvedTrack` is a couple of KB and the other variants are tiny.
    Track(Box<GpTrackStart>),
    Window(GpWindowOpened),
    /// Final scores, sorted.
    Finished(Vec<(UserId, u32)>),
}

/// A submission window just closed.
#[derive(Clone, Debug)]
pub struct GpWindowClosed {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub prompt_message: Option<(GenericChannelId, MessageId)>,
    pub count: usize,
    pub text_channel: GenericChannelId,
    pub next: GpNext,
}

/// The 30-second heads-up.
#[derive(Clone, Debug)]
pub struct GpWindowWarning {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub count: usize,
    pub closes_at: i64,
    pub text_channel: GenericChannelId,
}

/// What `gp_submit` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpSubmitOutcome {
    pub replaced: bool,
    /// Songs in so far this round.
    pub submitted: usize,
    /// Every non-bot member of the voice channel has a song in.
    pub everyone_in: bool,
    /// Pass to `gp_close_window_if` to close early.
    pub generation: u64,
}

/// What `gp_record_guess` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpGuessOutcome {
    Recorded,
    Changed,
}

/// What `gp_toggle_like` did; the payload is the song's new like count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpLikeOutcome {
    Liked(usize),
    Unliked(usize),
}

/// Everything the reveal needs, cloned out of the map so no lock is held.
#[derive(Clone, Debug)]
pub struct GpTrackResult {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub track_idx: usize,
    pub total_tracks: usize,
    pub prompt: String,
    pub submitter: UserId,
    pub title: String,
    pub url: String,
    pub correct: Vec<UserId>,
    pub fooled_everyone: bool,
    pub likes: usize,
    pub guessable: bool,
    /// Sorted descending.
    pub scores: Vec<(UserId, u32)>,
    pub message: Option<(GenericChannelId, MessageId)>,
    pub text_channel: GenericChannelId,
    pub next: GpNext,
    /// The song never played: songbird failed to open the stream. Nothing was
    /// scored for it.
    pub failed: bool,
}

/// Snapshot for `/gp status`.
#[derive(Clone, Debug)]
pub enum GpStatus {
    Submitting {
        host: UserId,
        round: usize,
        total: usize,
        prompt: String,
        closes_at: i64,
        submitted: Vec<String>,
        scores: Vec<(UserId, u32)>,
    },
    Playing {
        round: usize,
        total: usize,
        track: usize,
        tracks: usize,
        prompt: String,
        guessed: Vec<String>,
        likes: usize,
        scores: Vec<(UserId, u32)>,
    },
}

// ------------------------------------------------------------------
// State helpers on Data (all synchronous, see module docs)
// ------------------------------------------------------------------

impl Data {
    /// Create the game and open round 0's window.
    #[allow(clippy::too_many_arguments)]
    pub fn gp_start(
        &self,
        guild_id: GuildId,
        host: UserId,
        host_name: String,
        voice_channel: ChannelId,
        text_channel: GenericChannelId,
        category: GpCategory,
        prompts: Vec<String>,
        timer_secs: u64,
        now: i64,
    ) -> CrackedResult<GpWindowOpened> {
        if self.gp_games.contains_key(&guild_id) {
            return Err(CrackedError::GameAlreadyRunning);
        }
        if prompts.is_empty() {
            return Err(CrackedError::Other("That category has no prompts."));
        }
        let mut game = GpGame::new(
            host,
            voice_channel,
            text_channel,
            category,
            prompts,
            timer_secs,
        );
        game.players.insert(host, host_name);
        let opened = game.open_window(now);
        self.gp_games.insert(guild_id, game);
        Ok(opened)
    }

    /// Cheap check before resolving a query: is a window open here?
    pub fn gp_window_open(&self, guild_id: GuildId) -> CrackedResult<()> {
        let game = self
            .gp_games
            .get(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.phase != GpPhase::Submitting {
            return Err(CrackedError::WindowClosed);
        }
        Ok(())
    }

    /// Record (or replace) this user's song for the current prompt.
    /// `vc_members` are the non-bot members of the game's voice channel, used
    /// to tell the caller whether everyone is in.
    pub fn gp_submit(
        &self,
        guild_id: GuildId,
        user: UserId,
        name: String,
        track: ResolvedTrack<'static>,
        vc_members: &[UserId],
    ) -> CrackedResult<GpSubmitOutcome> {
        let mut entry = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let game: &mut GpGame = &mut entry;
        if game.phase != GpPhase::Submitting {
            return Err(CrackedError::WindowClosed);
        }
        let idx = game.current_round;
        let round = &mut game.rounds[idx];
        let is_new = !round.submissions.contains_key(&user);
        if is_new && round.submissions.len() >= GP_MAX_PLAYERS {
            return Err(CrackedError::TooManyPlayers(GP_MAX_PLAYERS));
        }
        game.players.insert(user, name);
        let replaced = round.submissions.insert(user, track).is_some();
        let submitted = round.submissions.len();
        let everyone_in =
            !vc_members.is_empty() && vc_members.iter().all(|u| round.submissions.contains_key(u));
        Ok(GpSubmitOutcome {
            replaced,
            submitted,
            everyone_in,
            generation: game.generation,
        })
    }

    /// Host closes the window early.
    pub fn gp_close_window(
        &self,
        guild_id: GuildId,
        caller: UserId,
        rng: &mut impl Rng,
        now: i64,
    ) -> CrackedResult<GpWindowClosed> {
        let mut game = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.host != caller {
            return Err(CrackedError::NotGameHost);
        }
        if game.phase != GpPhase::Submitting {
            return Err(CrackedError::WindowClosed);
        }
        Ok(game.close_window(rng, now))
    }

    /// Close the window the timer (or "everyone submitted") was started for.
    /// `None` if there is no game, no window is open, or the window it was
    /// spawned for has already closed (the generation moved).
    pub fn gp_close_window_if(
        &self,
        guild_id: GuildId,
        generation: u64,
        rng: &mut impl Rng,
        now: i64,
    ) -> Option<GpWindowClosed> {
        let mut game = self.gp_games.get_mut(&guild_id)?;
        if game.phase != GpPhase::Submitting || game.generation != generation {
            return None;
        }
        Some(game.close_window(rng, now))
    }

    /// The 30-second heads-up, if the window it was spawned for is still open.
    pub fn gp_warning_if(&self, guild_id: GuildId, generation: u64) -> Option<GpWindowWarning> {
        let game = self.gp_games.get(&guild_id)?;
        if game.phase != GpPhase::Submitting || game.generation != generation {
            return None;
        }
        let round = &game.rounds[game.current_round];
        Some(GpWindowWarning {
            round_idx: game.current_round,
            total_rounds: game.rounds.len(),
            prompt: round.prompt.clone(),
            count: round.submissions.len(),
            closes_at: round.closes_at.unwrap_or(0),
            text_channel: game.text_channel,
        })
    }

    /// Remember where the prompt message went so the close can edit it.
    pub fn gp_set_prompt_message(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        channel: GenericChannelId,
        message_id: MessageId,
    ) -> CrackedResult<()> {
        let mut game = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let round = game
            .rounds
            .get_mut(round_idx)
            .ok_or(CrackedError::StaleRound)?;
        round.prompt_message = Some((channel, message_id));
        Ok(())
    }

    /// Remember where a song's message went so the reveal can edit it.
    pub fn gp_set_track_message(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        channel: GenericChannelId,
        message_id: MessageId,
    ) -> CrackedResult<()> {
        let mut game = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let t = game
            .rounds
            .get_mut(round_idx)
            .and_then(|r| r.tracks.get_mut(track_idx))
            .ok_or(CrackedError::StaleRound)?;
        t.message = Some((channel, message_id));
        Ok(())
    }

    /// Record (or change) a guess for the song that is playing.
    pub fn gp_record_guess(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        guesser: UserId,
        guesser_name: String,
        guessed: UserId,
    ) -> CrackedResult<GpGuessOutcome> {
        let mut entry = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let game: &mut GpGame = &mut entry;
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying);
        }
        if round_idx != game.current_round || track_idx != game.current_track {
            return Err(CrackedError::StaleRound);
        }
        let round = &mut game.rounds[round_idx];
        if !round.guessable() {
            return Err(CrackedError::NotGuessable);
        }
        if !round.submitters().contains(&guessed) {
            return Err(CrackedError::NotAPlayer);
        }
        game.players.entry(guesser).or_insert(guesser_name);
        let previous = round.tracks[track_idx].guesses.insert(guesser, guessed);
        Ok(match previous {
            Some(p) if p != guessed => GpGuessOutcome::Changed,
            _ => GpGuessOutcome::Recorded,
        })
    }

    /// Toggle a 👍 on the song that is playing.
    pub fn gp_toggle_like(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        liker: UserId,
        liker_name: String,
    ) -> CrackedResult<GpLikeOutcome> {
        let mut entry = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let game: &mut GpGame = &mut entry;
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying);
        }
        if round_idx != game.current_round || track_idx != game.current_track {
            return Err(CrackedError::StaleRound);
        }
        let t = &mut game.rounds[round_idx].tracks[track_idx];
        if t.submitter == liker {
            return Err(CrackedError::CannotLikeOwnSong);
        }
        game.players.entry(liker).or_insert(liker_name);
        Ok(if t.likes.remove(&liker) {
            GpLikeOutcome::Unliked(t.likes.len())
        } else {
            t.likes.insert(liker);
            GpLikeOutcome::Liked(t.likes.len())
        })
    }

    /// Score the song that just ended and advance. Returns `None` unless the
    /// game is playing **and** this is the current song, which makes it safe to
    /// call twice (End and Error can both fire for one track).
    pub fn gp_reveal_and_advance(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        now: i64,
    ) -> Option<GpTrackResult> {
        self.gp_finish_track(guild_id, round_idx, track_idx, now, false)
    }

    /// Like [`Self::gp_reveal_and_advance`], but for a song that never played:
    /// songbird could not open the stream, so the track went straight from
    /// `Preparing` to `Errored` without mixing a single frame. Nobody heard it,
    /// so nothing is scored -- no guess points, no fooled-everyone bonus and no
    /// like points -- and the game moves on to the next song.
    pub fn gp_fail_and_advance(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        now: i64,
    ) -> Option<GpTrackResult> {
        self.gp_finish_track(guild_id, round_idx, track_idx, now, true)
    }

    fn gp_finish_track(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        now: i64,
        failed: bool,
    ) -> Option<GpTrackResult> {
        let mut game = self.gp_games.get_mut(&guild_id)?;
        if game.phase != GpPhase::Playing
            || round_idx != game.current_round
            || track_idx != game.current_track
        {
            return None;
        }
        let round = &game.rounds[round_idx];
        let guessable = round.guessable();
        let t = round.tracks[track_idx].clone();
        let correct: Vec<UserId> = if guessable && !failed {
            t.guesses
                .iter()
                .filter(|(guesser, guessed)| **guesser != t.submitter && **guessed == t.submitter)
                .map(|(guesser, _)| *guesser)
                .collect()
        } else {
            Vec::new()
        };
        for g in &correct {
            *game.scores.entry(*g).or_insert(0) += GP_POINTS_CORRECT;
        }
        let fooled_everyone = guessable && !failed && correct.is_empty();
        if fooled_everyone {
            *game.scores.entry(t.submitter).or_insert(0) += GP_POINTS_FOOLED_ALL;
        }
        let likes = t.likes.len();
        if likes > 0 && !failed {
            *game.scores.entry(t.submitter).or_insert(0) += likes as u32 * GP_POINTS_PER_LIKE;
        }
        game.generation += 1;
        game.current_track += 1;
        let total_tracks = game.rounds[round_idx].tracks.len();
        let next = if game.current_track < total_tracks {
            GpNext::Track(Box::new(game.track_start()))
        } else {
            game.advance_round(now)
        };
        Some(GpTrackResult {
            round_idx,
            total_rounds: game.rounds.len(),
            track_idx,
            total_tracks,
            prompt: game.rounds[round_idx].prompt.clone(),
            submitter: t.submitter,
            title: t.track.get_title(),
            url: t.track.get_url(),
            correct,
            fooled_everyone,
            likes,
            guessable,
            scores: game.sorted_scores(),
            message: t.message,
            text_channel: game.text_channel,
            next,
            failed,
        })
    }

    /// Remove the game. Only the host (or someone who may manage the guild) can.
    pub fn gp_end(
        &self,
        guild_id: GuildId,
        caller: UserId,
        caller_is_admin: bool,
    ) -> CrackedResult<GpGame> {
        {
            let game = self
                .gp_games
                .get(&guild_id)
                .ok_or(CrackedError::NoGameInProgress)?;
            if game.host != caller && !caller_is_admin {
                return Err(CrackedError::NotGameHost);
            }
        }
        self.gp_games
            .remove(&guild_id)
            .map(|(_, g)| g)
            .ok_or(CrackedError::NoGameInProgress)
    }

    /// Remove the game unconditionally (game over, bot kicked from voice).
    pub fn gp_remove(&self, guild_id: GuildId) -> Option<GpGame> {
        self.gp_games.remove(&guild_id).map(|(_, g)| g)
    }

    /// Snapshot for the status command.
    pub fn gp_status(&self, guild_id: GuildId) -> CrackedResult<GpStatus> {
        let game = self
            .gp_games
            .get(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let total = game.rounds.len();
        let round_idx = game.current_round.min(total.saturating_sub(1));
        let round = &game.rounds[round_idx];
        let names = |ids: Vec<UserId>| {
            let mut v: Vec<String> = ids.into_iter().map(|id| game.name_of(id)).collect();
            v.sort_by_key(|n| n.to_lowercase());
            v
        };
        Ok(match game.phase {
            GpPhase::Submitting => GpStatus::Submitting {
                host: game.host,
                round: round_idx + 1,
                total,
                prompt: round.prompt.clone(),
                closes_at: round.closes_at.unwrap_or(0),
                submitted: names(round.submissions.keys().copied().collect()),
                scores: game.sorted_scores(),
            },
            GpPhase::Playing | GpPhase::Finished => {
                let t = round.tracks.get(game.current_track);
                GpStatus::Playing {
                    round: round_idx + 1,
                    total,
                    track: (game.current_track + 1).min(round.tracks.len()),
                    tracks: round.tracks.len(),
                    prompt: round.prompt.clone(),
                    guessed: names(
                        t.map(|t| t.guesses.keys().copied().collect())
                            .unwrap_or_default(),
                    ),
                    likes: t.map(|t| t.likes.len()).unwrap_or(0),
                    scores: game.sorted_scores(),
                }
            },
        })
    }

    /// True while a game owns playback for this guild -- from `/gp start`
    /// until the scoreboard is posted. There is no lobby any more: even while a
    /// window is open, a stray `/play` would land in front of the round's songs.
    pub fn gp_is_playing(&self, guild_id: GuildId) -> bool {
        self.gp_games.contains_key(&guild_id)
    }

    /// True if a game exists for the guild in any phase.
    pub fn gp_is_active(&self, guild_id: GuildId) -> bool {
        self.gp_games.contains_key(&guild_id)
    }

    /// The game's voice channel, if a game exists.
    pub fn gp_voice_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        self.gp_games.get(&guild_id).map(|g| g.voice_channel)
    }
}

// ------------------------------------------------------------------
// Components and embeds
// ------------------------------------------------------------------

/// Which control a custom id belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpComponent {
    /// The "who submitted this?" dropdown.
    Guess,
    /// The 👍 button.
    Like,
}

impl GpComponent {
    fn tag(self) -> &'static str {
        match self {
            Self::Guess => "g",
            Self::Like => "l",
        }
    }
}

/// Build a `gp:<g|l>:<guild>:<round>:<track>` custom id.
pub fn gp_custom_id(
    kind: GpComponent,
    guild_id: GuildId,
    round_idx: usize,
    track_idx: usize,
) -> String {
    format!(
        "{GP_CUSTOM_ID_PREFIX}{}:{}:{round_idx}:{track_idx}",
        kind.tag(),
        guild_id.get()
    )
}

/// Parse a `gp:<g|l>:<guild>:<round>:<track>` custom id.
pub fn parse_custom_id(custom_id: &str) -> Option<(GpComponent, GuildId, usize, usize)> {
    let rest = custom_id.strip_prefix(GP_CUSTOM_ID_PREFIX)?;
    let mut parts = rest.split(':');
    let kind = match parts.next()? {
        "g" => GpComponent::Guess,
        "l" => GpComponent::Like,
        _ => return None,
    };
    let guild = parts.next()?.parse::<u64>().ok().filter(|g| *g != 0)?;
    let round = parts.next()?.parse::<usize>().ok()?;
    let track = parts.next()?.parse::<usize>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((kind, GuildId::new(guild), round, track))
}

/// The controls under a playing song: the "who submitted this?" dropdown
/// (only when there is something to guess) and the 👍 button.
pub fn gp_components(
    guild_id: GuildId,
    round_idx: usize,
    track_idx: usize,
    players: &[(UserId, String)],
    guessable: bool,
) -> Vec<CreateComponent<'static>> {
    let mut rows = Vec::with_capacity(2);
    if guessable {
        let options: Vec<CreateSelectMenuOption<'static>> = players
            .iter()
            .take(GP_MAX_PLAYERS)
            .map(|(id, name)| CreateSelectMenuOption::new(name.clone(), id.to_string()))
            .collect();
        let menu = CreateSelectMenu::new(
            gp_custom_id(GpComponent::Guess, guild_id, round_idx, track_idx),
            CreateSelectMenuKind::String {
                options: Cow::Owned(options),
            },
        )
        .placeholder(GP_SELECT_PLACEHOLDER)
        .min_values(1)
        .max_values(1);
        rows.push(CreateComponent::ActionRow(CreateActionRow::SelectMenu(
            menu,
        )));
    }
    let like = CreateButton::new(gp_custom_id(
        GpComponent::Like,
        guild_id,
        round_idx,
        track_idx,
    ))
    .emoji('👍')
    .label(GP_LIKE_LABEL)
    .style(ButtonStyle::Secondary);
    rows.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
        Cow::Owned(vec![like]),
    )));
    rows
}

fn scores_lines(scores: &[(UserId, u32)]) -> String {
    if scores.is_empty() {
        return "-".to_string();
    }
    scores
        .iter()
        .enumerate()
        .map(|(i, (id, pts))| format!("{}. {} — {pts}", i + 1, id.mention()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn round_title(round_idx: usize, total_rounds: usize) -> String {
    format!("{GP_ROUND_TITLE} {}/{total_rounds}", round_idx + 1)
}

fn song_title(
    round_idx: usize,
    total_rounds: usize,
    track_idx: usize,
    total_tracks: usize,
) -> String {
    format!(
        "{} · {GP_SONG_TITLE} {}/{total_tracks}",
        round_title(round_idx, total_rounds),
        track_idx + 1
    )
}

/// The rules / how-to embed used by `/gp`.
pub fn gp_rules_embed() -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(GP_TITLE)
        .description(GP_RULES_TEXT)
        .field(GP_HOW_TO_TITLE, GP_HOW_TO, false)
        .colour(Colour::FOOYOO)
}

/// The prompt message with its live countdown.
pub fn gp_prompt_embed(w: &GpWindowOpened) -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(round_title(w.round_idx, w.total_rounds))
        .description(format!("**{}**", w.prompt))
        .field(GP_PROMPT_HOW_TO_TITLE, GP_PROMPT_HOW_TO, false)
        .field(
            GP_PROMPT_CLOSES_TITLE,
            format!("<t:{}:R> {GP_PROMPT_CLOSES_EARLY}", w.closes_at),
            false,
        )
        .colour(Colour::FOOYOO)
}

/// The prompt message once the window has closed.
pub fn gp_prompt_closed_embed(c: &GpWindowClosed) -> CreateEmbed<'static> {
    let status = if c.count == 0 {
        GP_WINDOW_EMPTY.to_string()
    } else {
        format!("{GP_WINDOW_CLOSED} {} {GP_WINDOW_CLOSED_SONGS}", c.count)
    };
    CreateEmbed::new()
        .title(round_title(c.round_idx, c.total_rounds))
        .description(format!("**{}**\n\n{status}", c.prompt))
        .colour(Colour::DARKER_GREY)
}

/// The 30-second heads-up, as plain text.
pub fn gp_warning_text(w: &GpWindowWarning) -> String {
    format!(
        "{GP_WINDOW_WARNING} **{}** — {} {GP_WINDOW_WARNING_IN} <t:{}:R>",
        w.prompt, w.count, w.closes_at
    )
}

/// The song message: prompt, title and what to do. Never the submitter.
pub fn gp_track_embed(s: &GpTrackStart) -> CreateEmbed<'static> {
    let hint = if s.guessable {
        format!("{GP_ROUND_HINT}\n{GP_LIKE_HINT}")
    } else {
        GP_LIKE_HINT.to_string()
    };
    CreateEmbed::new()
        .title(song_title(
            s.round_idx,
            s.total_rounds,
            s.track_idx,
            s.total_tracks,
        ))
        .description(format!(
            "*{}*\n\n**[{}]({})**\n\n{hint}",
            s.prompt,
            s.track.get_title(),
            s.track.get_url()
        ))
        .colour(Colour::BLURPLE)
}

/// The reveal, written into the song message once the track is over.
pub fn gp_reveal_embed(res: &GpTrackResult) -> CreateEmbed<'static> {
    if res.failed {
        return CreateEmbed::new()
            .title(song_title(
                res.round_idx,
                res.total_rounds,
                res.track_idx,
                res.total_tracks,
            ))
            .description(format!(
                "*{}*\n\n**[{}]({})**\n\n{GP_REVEAL} {}",
                res.prompt,
                res.title,
                res.url,
                res.submitter.mention()
            ))
            .field(GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE, false)
            .field(GP_SCOREBOARD, scores_lines(&res.scores), false)
            .colour(Colour::RED);
    }
    let mut e = CreateEmbed::new()
        .title(song_title(
            res.round_idx,
            res.total_rounds,
            res.track_idx,
            res.total_tracks,
        ))
        .description(format!(
            "*{}*\n\n**[{}]({})**\n\n{GP_REVEAL} {}",
            res.prompt,
            res.title,
            res.url,
            res.submitter.mention()
        ))
        .colour(Colour::DARK_GREEN);
    if res.guessable {
        let correct = if res.correct.is_empty() {
            GP_NOBODY_GUESSED.to_string()
        } else {
            res.correct
                .iter()
                .map(|id| id.mention().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        e = e.field(GP_GUESSED_RIGHT, correct, false);
        if res.fooled_everyone {
            e = e.field(
                GP_FOOLED_EVERYONE,
                res.submitter.mention().to_string(),
                false,
            );
        }
    }
    e.field(GP_LIKES, res.likes.to_string(), true).field(
        GP_SCOREBOARD,
        scores_lines(&res.scores),
        false,
    )
}

/// The final scoreboard.
pub fn gp_scoreboard_embed(scores: &[(UserId, u32)], title: &str) -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(title.to_string())
        .description(scores_lines(scores))
        .colour(Colour::GOLD)
}

/// The status embed.
pub fn gp_status_embed(status: &GpStatus) -> CreateEmbed<'static> {
    let list = |names: &[String]| {
        if names.is_empty() {
            GP_NOBODY_YET.to_string()
        } else {
            names.join(", ")
        }
    };
    match status {
        GpStatus::Submitting {
            host,
            round,
            total,
            prompt,
            closes_at,
            submitted,
            scores,
        } => CreateEmbed::new()
            .title(format!("{GP_STATUS_SUBMITTING} {round}/{total}"))
            .field(GP_STATUS_PROMPT, prompt.clone(), false)
            .field("Host", host.mention().to_string(), true)
            .field(GP_STATUS_CLOSES, format!("<t:{closes_at}:R>"), true)
            .field(GP_STATUS_SUBMITTED, list(submitted), false)
            .field(GP_STATUS_SCORES, scores_lines(scores), false)
            .colour(Colour::FOOYOO),
        GpStatus::Playing {
            round,
            total,
            track,
            tracks,
            prompt,
            guessed,
            likes,
            scores,
        } => CreateEmbed::new()
            .title(format!(
                "{GP_STATUS_PLAYING} {round}/{total} · {GP_SONG_TITLE} {track}/{tracks}"
            ))
            .field(GP_STATUS_PROMPT, prompt.clone(), false)
            .field(GP_STATUS_GUESSED, list(guessed), false)
            .field(GP_STATUS_LIKES, likes.to_string(), true)
            .field(GP_STATUS_SCORES, scores_lines(scores), false)
            .colour(Colour::BLURPLE),
    }
}

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

/// Per-track handler: when a song ends, reveal and move on.
pub struct GpTrackEndHandler {
    pub pb: GpPlayback,
    pub round_idx: usize,
    pub track_idx: usize,
}

/// Did the song fail instead of finish? The handler is registered for both
/// `End` and `Error`, and songbird reports a stream it could never open as an
/// `End` whose state is still `Errored` -- it goes `Preparing` -> `Errored`
/// without mixing a frame, so `play_time` never leaves zero. Without this the
/// game treats a dead link as a song everyone just listened to.
fn track_errored(ctx: &EventContext<'_>) -> bool {
    match ctx {
        EventContext::Track(states) => states
            .iter()
            .any(|(state, _)| matches!(state.playing, PlayMode::Errored(_))),
        _ => false,
    }
}

#[async_trait]
impl EventHandler for GpTrackEndHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let (pb, round_idx, track_idx) = (self.pb.clone(), self.round_idx, self.track_idx);
        let failed = track_errored(ctx);
        // Do the reveal off the driver's event task: it edits messages, sleeps,
        // and takes the call lock to enqueue the next song.
        tokio::spawn(async move {
            let guild_id = pb.guild_id;
            if let Err(e) = gp_advance_track(pb, round_idx, track_idx, failed).await {
                tracing::warn!(
                    "gp: advancing round {round_idx} song {track_idx} in {guild_id}: {e}"
                );
            }
        });
        Some(Event::Cancel)
    }
}

/// Post the prompt and start the window timer.
pub async fn gp_open_round(pb: &GpPlayback, opened: GpWindowOpened) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    if !pb.data.gp_is_active(guild_id) {
        return Ok(());
    }
    let msg = opened
        .text_channel
        .send_message(
            &pb.http,
            CreateMessage::new().embed(gp_prompt_embed(&opened)),
        )
        .await?;
    pb.data
        .gp_set_prompt_message(guild_id, opened.round_idx, opened.text_channel, msg.id)?;
    gp_spawn_window_timer(pb.clone(), &opened);
    Ok(())
}

/// The window timer: a heads-up 30 s before the end, then close. Both steps
/// are no-ops if the window it was spawned for has already closed.
pub fn gp_spawn_window_timer(pb: GpPlayback, opened: &GpWindowOpened) {
    let (generation, timer, text_channel) =
        (opened.generation, opened.timer_secs, opened.text_channel);
    tokio::spawn(async move {
        let guild_id = pb.guild_id;
        if timer > GP_WARNING_SECS {
            tokio::time::sleep(Duration::from_secs(timer - GP_WARNING_SECS)).await;
            let Some(warning) = pb.data.gp_warning_if(guild_id, generation) else {
                return;
            };
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
        let closed =
            pb.data
                .gp_close_window_if(guild_id, generation, &mut rand::thread_rng(), now());
        if let Some(closed) = closed {
            if let Err(e) = gp_after_close(pb, closed).await {
                tracing::warn!("gp: closing window in {guild_id}: {e}");
            }
        }
    });
}

/// After a window closed (timer, everyone in, or the host): update the prompt
/// message, then play, skip, or finish.
pub async fn gp_after_close(pb: GpPlayback, closed: GpWindowClosed) -> Result<(), Error> {
    let embed = gp_prompt_closed_embed(&closed);
    let edited = match closed.prompt_message {
        Some((chan, msg_id)) => chan
            .edit_message(&pb.http, msg_id, EditMessage::new().embed(embed.clone()))
            .await
            .is_ok(),
        None => false,
    };
    if !edited {
        closed
            .text_channel
            .send_message(&pb.http, CreateMessage::new().embed(embed))
            .await?;
    }
    gp_follow(pb, closed.next, closed.text_channel, false).await
}

/// Take the next step of the game.
async fn gp_follow(
    pb: GpPlayback,
    next: GpNext,
    text_channel: GenericChannelId,
    pause_before_track: bool,
) -> Result<(), Error> {
    match next {
        GpNext::Track(start) => {
            if pause_before_track {
                tokio::time::sleep(Duration::from_secs(GP_REVEAL_PAUSE_SECS)).await;
            }
            gp_play_track(&pb, *start).await
        },
        GpNext::Window(opened) => gp_open_round(&pb, opened).await,
        GpNext::Finished(scores) => {
            text_channel
                .send_message(
                    &pb.http,
                    CreateMessage::new().embed(gp_scoreboard_embed(&scores, GP_GAME_OVER)),
                )
                .await?;
            pb.data.gp_remove(pb.guild_id);
            Ok(())
        },
    }
}

/// Enqueue one song, hook its end, and post the song message.
pub async fn gp_play_track(pb: &GpPlayback, start: GpTrackStart) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    if !pb.data.gp_is_active(guild_id) {
        // Ended or torn down while we were between songs.
        return Ok(());
    }
    // `build_track` is lazy; the stream is fetched when songbird starts it.
    // Note: no `.with_user_id` -- see the module docs.
    let songbird_track = build_track(&start.track, &pb.data.http_client)?;
    let handle = {
        let mut handler = pb.call.lock().await;
        handler.enqueue(songbird_track).await
    };
    for event in [TrackEvent::End, TrackEvent::Error] {
        handle.add_event(
            Event::Track(event),
            GpTrackEndHandler {
                pb: pb.clone(),
                round_idx: start.round_idx,
                track_idx: start.track_idx,
            },
        )?;
    }

    let msg = start
        .text_channel
        .send_message(
            &pb.http,
            CreateMessage::new()
                .embed(gp_track_embed(&start))
                .components(gp_components(
                    guild_id,
                    start.round_idx,
                    start.track_idx,
                    &start.players,
                    start.guessable,
                )),
        )
        .await?;
    pb.data.gp_set_track_message(
        guild_id,
        start.round_idx,
        start.track_idx,
        start.text_channel,
        msg.id,
    )?;
    Ok(())
}

/// Reveal the song that just ended and take the next step.
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
        res.text_channel
            .send_message(&pb.http, CreateMessage::new().embed(reveal))
            .await?;
    }
    gp_follow(pb, res.next, res.text_channel, true).await
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

/// The user must be in the game's voice channel to guess or like.
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

// ------------------------------------------------------------------
// Commands
// ------------------------------------------------------------------

async fn author_display_name(ctx: Context<'_>) -> String {
    match ctx.author_member().await {
        Some(m) => m.display_name().to_string(),
        None => ctx.author().name.to_string(),
    }
}

/// Non-bot members of `vc`, from the cache. Empty if the guild isn't cached
/// (then the window simply waits for the timer or the host).
fn gp_vc_members(ctx: Context<'_>, vc: ChannelId) -> Vec<UserId> {
    let Some(guild) = ctx.guild() else {
        return Vec::new();
    };
    guild
        .voice_states
        .iter()
        .filter(|vs| vs.channel_id == Some(vc))
        .filter(|vs| guild.members.get(&vs.user_id).is_none_or(|m| !m.user.bot()))
        .map(|vs| vs.user_id)
        .collect()
}

fn gp_playback(ctx: Context<'_>, call: Arc<Mutex<Call>>, guild_id: GuildId) -> GpPlayback {
    GpPlayback {
        data: ctx.data().clone(),
        http: ctx.serenity_context().http.clone(),
        call,
        guild_id,
    }
}

/// "What's your song?" party game: a prompt, secret submissions, guess whose is whose.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    aliases("guiltypleasure"),
    subcommands("gp_start", "gp_submit", "gp_close", "gp_skip", "gp_status", "gp_end")
)]
pub async fn gp(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send_embed_response(gp_rules_embed()).await?;
    Ok(())
}

/// Start a game in your voice channel: pick a category, rounds, and the submission timer.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "start",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_start(
    ctx: Context<'_>,
    #[description = "Prompt category (or Mixed)."] category: GpCategory,
    #[description = "Number of rounds (default 5)."]
    #[min = 1]
    #[max = 20]
    rounds: Option<u32>,
    #[description = "Seconds to submit each round (default 180)."]
    #[min = 30]
    #[max = 600]
    timer: Option<u32>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    if data.gp_is_active(guild_id) {
        return Err(CrackedError::GameAlreadyRunning.into());
    }
    let vc = ctx.author_vc().ok_or(CrackedError::NotConnected)?;
    let host = ctx.author().id;
    let host_name = author_display_name(ctx).await;

    // Join (or reuse) the call, and make sure it is the host's channel.
    let call = get_call_or_join_author(ctx).await?;
    {
        let handler = call.lock().await;
        if let Some(chan) = handler.current_channel() {
            if chan.get() != vc.get() {
                return Err(CrackedError::WrongVoiceChannel.into());
            }
        }
    }

    let rounds = rounds.unwrap_or(GP_DEFAULT_ROUNDS).clamp(1, GP_MAX_ROUNDS) as usize;
    let timer_secs = timer
        .map(u64::from)
        .unwrap_or(GP_DEFAULT_TIMER_SECS)
        .clamp(GP_MIN_TIMER_SECS, GP_MAX_TIMER_SECS);
    let prompts = draw_prompts(category, rounds, &mut rand::thread_rng());

    // Create the game first so the global TrackEndHandler ignores the End
    // event that stopping an existing queue fires.
    let opened = data.gp_start(
        guild_id,
        host,
        host_name,
        vc,
        ctx.channel_id(),
        category,
        prompts,
        timer_secs,
        now(),
    )?;
    let cleared_queue = {
        let handler = call.lock().await;
        let non_empty = !handler.queue().is_empty();
        if non_empty {
            handler.queue().stop();
        }
        non_empty
    };

    ctx.send_reply(
        CrackedMessage::GpStarted {
            category: category.display(),
            rounds: opened.total_rounds,
            timer_secs,
            cleared_queue,
        },
        true,
    )
    .await?;

    let pb = gp_playback(ctx, call, guild_id);
    gp_open_round(&pb, opened).await
}

/// Secretly submit your song for the current prompt (link or search). Resubmitting replaces it.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "submit",
    category = "Games",
    slash_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_submit(
    ctx: Context<'_>,
    #[description = "song link or search query."] query: String,
) -> Result<(), Error> {
    // Errors are answered here, ephemerally, rather than through the framework's
    // public error reply.
    let msg = match gp_submit_internal(ctx, query).await {
        Ok(m) => m,
        Err(e) => CrackedMessage::CrackedError(e),
    };
    ctx.send_message(SendMessageParams::new(msg).with_ephemeral(true))
        .await?;
    Ok(())
}

/// Resolve the query and store it; close the window if everyone is in.
#[cfg(not(tarpaulin_include))]
pub async fn gp_submit_internal(ctx: Context<'_>, query: String) -> CrackedResult<CrackedMessage> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = data
        .gp_voice_channel(guild_id)
        .ok_or(CrackedError::NoGameInProgress)?;
    if ctx.author_vc() != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel);
    }
    data.gp_window_open(guild_id)?;
    let query = query.trim();
    if query.is_empty() {
        return Err(CrackedError::NoQuery);
    }
    let query_type = QueryType::from_str(query).map_err(CrackedError::TrackResolveError)?;
    let track = data
        .ct_client
        .resolve_track(query_type)
        .await
        .map_err(CrackedError::TrackFail)?;
    let name = author_display_name(ctx).await;
    let title = track.get_title();
    let vc_members = gp_vc_members(ctx, game_vc);
    let outcome = data.gp_submit(guild_id, ctx.author().id, name, track, &vc_members)?;

    if outcome.everyone_in {
        let closed =
            data.gp_close_window_if(guild_id, outcome.generation, &mut rand::thread_rng(), now());
        if let (Some(closed), Some(call)) = (closed, data.songbird.get(guild_id)) {
            let pb = gp_playback(ctx, call, guild_id);
            // Off this task so the ephemeral confirmation isn't held up.
            tokio::spawn(async move {
                if let Err(e) = gp_after_close(pb, closed).await {
                    tracing::warn!("gp: closing window in {guild_id}: {e}");
                }
            });
        }
    }
    Ok(CrackedMessage::GpSubmitted {
        title,
        replaced: outcome.replaced,
        submitted: outcome.submitted,
        of: vc_members.len(),
    })
}

/// Close submissions early and start playing this round (host only).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "close",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_close(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let closed = data.gp_close_window(guild_id, ctx.author().id, &mut rand::thread_rng(), now())?;
    ctx.send_reply(
        CrackedMessage::GpWindowClosed {
            count: closed.count,
        },
        true,
    )
    .await?;
    let call = data
        .songbird
        .get(guild_id)
        .ok_or(CrackedError::NotConnected)?;
    gp_after_close(gp_playback(ctx, call, guild_id), closed).await
}

/// End the current song early (host only).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "skip",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_skip(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    {
        let game = data
            .gp_games
            .get(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.host != ctx.author().id {
            return Err(CrackedError::NotGameHost.into());
        }
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying.into());
        }
    }
    let call = data
        .songbird
        .get(guild_id)
        .ok_or(CrackedError::NotConnected)?;
    {
        let handler = call.lock().await;
        if handler.queue().is_empty() {
            return Err(CrackedError::NothingPlaying.into());
        }
        // stop() fires TrackEvent::End, which is what advances the game.
        force_skip_top_track(&handler).await?;
    }
    ctx.send_reply(CrackedMessage::GpRoundSkipped, true).await?;
    Ok(())
}

/// The prompt, who has submitted or guessed, likes, and the scores so far.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "status",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let status = ctx.data().gp_status(guild_id)?;
    ctx.send_embed_response(gp_status_embed(&status)).await?;
    Ok(())
}

/// Abort the game (host, or anyone who can manage the server).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "end",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_end(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let is_admin = ctx
        .author_permissions()
        .await
        .map(|p| p.manage_guild())
        .unwrap_or(false);
    // Remove the game *before* stopping playback, so the End event finds no
    // game and the track-end handler (and any window timer) is a no-op.
    let game = data.gp_end(guild_id, ctx.author().id, is_admin)?;
    if let Some(call) = data.songbird.get(guild_id) {
        call.lock().await.queue().stop();
    }
    let by = author_display_name(ctx).await;
    ctx.send_reply(CrackedMessage::GpEnded { by }, true).await?;
    let nothing_played = game.phase == GpPhase::Submitting && game.current_round == 0;
    if !nothing_played {
        ctx.send_embed_response(gp_scoreboard_embed(&game.sorted_scores(), GP_SCOREBOARD))
            .await?;
    }
    Ok(())
}

// ------------------------------------------------------------------
// Tests: pure state logic, no Discord
// ------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use crate::DataInner;
    use crack_types::AuxMetadata;
    use rand::{rngs::StdRng, SeedableRng};

    const G: GuildId = GuildId::new(1);
    const VC: ChannelId = ChannelId::new(10);
    const TC: GenericChannelId = GenericChannelId::new(20);
    const A: UserId = UserId::new(100);
    const B: UserId = UserId::new(200);
    const C: UserId = UserId::new(300);
    const NOW: i64 = 1_700_000_000;
    const TIMER: u64 = 120;

    fn data() -> Data {
        Data(Arc::new(DataInner {
            ..Default::default()
        }))
    }

    fn track(title: &str) -> ResolvedTrack<'static> {
        ResolvedTrack::new(QueryType::VideoLink(format!(
            "https://www.youtube.com/watch?v={title}"
        )))
        .with_metadata(AuxMetadata {
            title: Some(title.to_string()),
            source_url: Some(format!("https://www.youtube.com/watch?v={title}")),
            ..Default::default()
        })
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0)
    }

    fn prompts(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// A game hosted by alice with the given prompts; round 0's window is open.
    fn game_with(data: &Data, prompt_list: &[&str]) -> GpWindowOpened {
        data.gp_start(
            G,
            A,
            "alice".into(),
            VC,
            TC,
            GpCategory::Nostalgia,
            prompts(prompt_list),
            TIMER,
            NOW,
        )
        .unwrap()
    }

    fn submit(data: &Data, user: UserId, name: &str, title: &str) -> GpSubmitOutcome {
        data.gp_submit(G, user, name.into(), track(title), &[])
            .unwrap()
    }

    fn game(data: &Data) -> GpGame {
        data.gp_games.get(&G).unwrap().clone()
    }

    #[test]
    fn start_opens_round_one() {
        let data = data();
        assert_eq!(
            data.gp_submit(G, A, "a".into(), track("x"), &[])
                .unwrap_err(),
            CrackedError::NoGameInProgress
        );
        let opened = game_with(&data, &["p1", "p2"]);
        assert_eq!((opened.round_idx, opened.total_rounds), (0, 2));
        assert_eq!(opened.prompt, "p1");
        assert_eq!(opened.closes_at, NOW + TIMER as i64);
        assert_eq!(opened.generation, 1);
        assert_eq!(opened.text_channel, TC);
        let g = game(&data);
        assert_eq!(g.phase, GpPhase::Submitting);
        assert_eq!(g.rounds[0].closes_at, Some(NOW + TIMER as i64));
        // The game owns playback from the start; there is no lobby.
        assert!(data.gp_is_playing(G));
        assert!(data.gp_is_active(G));
        assert_eq!(data.gp_voice_channel(G), Some(VC));
        assert!(data.gp_window_open(G).is_ok());
        assert_eq!(
            data.gp_start(
                G,
                B,
                "bob".into(),
                VC,
                TC,
                GpCategory::Mixed,
                prompts(&["x"]),
                TIMER,
                NOW
            )
            .unwrap_err(),
            CrackedError::GameAlreadyRunning
        );
        assert_eq!(
            data.gp_start(
                GuildId::new(2),
                B,
                "bob".into(),
                VC,
                TC,
                GpCategory::Mixed,
                vec![],
                TIMER,
                NOW
            )
            .unwrap_err(),
            CrackedError::Other("That category has no prompts.")
        );
    }

    #[test]
    fn resubmit_replaces() {
        let data = data();
        game_with(&data, &["p1"]);
        let first = submit(&data, B, "bob", "one");
        assert_eq!(
            first,
            GpSubmitOutcome {
                replaced: false,
                submitted: 1,
                everyone_in: false,
                generation: 1
            }
        );
        let second = submit(&data, B, "bob", "two");
        assert!(second.replaced);
        assert_eq!(second.submitted, 1);
        let g = game(&data);
        assert_eq!(g.rounds[0].submissions.len(), 1);
        assert_eq!(g.rounds[0].submissions[&B].get_title(), "two");
        assert_eq!(g.players.get(&B).map(String::as_str), Some("bob"));
    }

    #[test]
    fn everyone_in_is_set_based() {
        let data = data();
        game_with(&data, &["p1"]);
        // Nobody known in the VC (cache miss): never closes early.
        assert!(!submit(&data, A, "alice", "a").everyone_in);
        // Bob is in the VC and hasn't submitted.
        let out = data
            .gp_submit(G, A, "alice".into(), track("a2"), &[A, B])
            .unwrap();
        assert!(!out.everyone_in);
        // Bob submits; carol (a leaver) isn't in the VC list, so she doesn't block.
        let out = data
            .gp_submit(G, B, "bob".into(), track("b"), &[A, B])
            .unwrap();
        assert!(out.everyone_in);
        // A newcomer who hasn't submitted blocks again.
        let out = data
            .gp_submit(G, B, "bob".into(), track("b2"), &[A, B, C])
            .unwrap();
        assert!(!out.everyone_in);
    }

    #[test]
    fn too_many_players_per_round() {
        let data = data();
        game_with(&data, &["p1"]);
        for i in 0..GP_MAX_PLAYERS as u64 {
            submit(&data, UserId::new(1000 + i), &format!("u{i}"), "t");
        }
        // An existing submitter may still swap their song...
        assert!(submit(&data, UserId::new(1000), "u0", "t2").replaced);
        // ...but a 26th distinct submitter may not.
        assert_eq!(
            data.gp_submit(G, UserId::new(5000), "new".into(), track("t"), &[])
                .unwrap_err(),
            CrackedError::TooManyPlayers(GP_MAX_PLAYERS)
        );
    }

    #[test]
    fn close_shuffles_and_plays() {
        let data = data();
        game_with(&data, &["p1", "p2"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");
        let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        assert_eq!((closed.round_idx, closed.total_rounds), (0, 2));
        assert_eq!(closed.prompt, "p1");
        assert_eq!(closed.count, 3);
        let GpNext::Track(start) = &closed.next else {
            panic!("expected a track, got {:?}", closed.next);
        };
        assert_eq!(
            (start.round_idx, start.track_idx, start.total_tracks),
            (0, 0, 3)
        );
        assert!(start.guessable);
        assert_eq!(start.prompt, "p1");
        assert_eq!(
            start.players,
            vec![
                (A, "alice".to_string()),
                (B, "bob".to_string()),
                (C, "carol".to_string())
            ]
        );
        let g = game(&data);
        assert_eq!(g.phase, GpPhase::Playing);
        assert_eq!(g.generation, 2);
        assert!(g.rounds[0].submissions.is_empty());
        assert_eq!(g.rounds[0].closes_at, None);
        let mut submitters: Vec<UserId> = g.rounds[0].tracks.iter().map(|t| t.submitter).collect();
        submitters.sort_unstable();
        assert_eq!(submitters, vec![A, B, C]);
        // Seeded shuffles are reproducible.
        let data2 = data_with_same_round();
        let closed2 = data2.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let GpNext::Track(start2) = closed2.next else {
            unreachable!()
        };
        assert_eq!(start2.track.get_title(), start.track.get_title());
        // Submissions are closed now.
        assert_eq!(
            data.gp_submit(G, A, "alice".into(), track("late"), &[])
                .unwrap_err(),
            CrackedError::WindowClosed
        );
        assert_eq!(
            data.gp_window_open(G).unwrap_err(),
            CrackedError::WindowClosed
        );
    }

    fn data_with_same_round() -> Data {
        let data = data();
        game_with(&data, &["p1", "p2"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");
        data
    }

    #[test]
    fn close_zero_submissions_skips_to_next_window() {
        let data = data();
        game_with(&data, &["p1", "p2"]);
        let closed = data.gp_close_window(G, A, &mut rng(), NOW + 5).unwrap();
        assert_eq!(closed.count, 0);
        let GpNext::Window(opened) = &closed.next else {
            panic!("expected next window, got {:?}", closed.next);
        };
        assert_eq!(opened.round_idx, 1);
        assert_eq!(opened.prompt, "p2");
        assert_eq!(opened.closes_at, NOW + 5 + TIMER as i64);
        let g = game(&data);
        assert_eq!(g.phase, GpPhase::Submitting);
        assert_eq!(g.current_round, 1);
    }

    #[test]
    fn close_zero_submissions_finishes_on_last_round() {
        let data = data();
        game_with(&data, &["only"]);
        let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        assert!(matches!(closed.next, GpNext::Finished(_)));
        let g = game(&data);
        assert_eq!(g.phase, GpPhase::Finished);
        assert!(data.gp_is_playing(G));
        // No window to close any more.
        assert_eq!(
            data.gp_close_window(G, A, &mut rng(), NOW).unwrap_err(),
            CrackedError::WindowClosed
        );
        data.gp_remove(G);
        assert!(!data.gp_is_playing(G));
    }

    #[test]
    fn close_if_generation_guard() {
        let data = data();
        let opened = game_with(&data, &["p1", "p2"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        // A timer from a generation that never existed does nothing.
        assert!(data.gp_close_window_if(G, 99, &mut rng(), NOW).is_none());
        assert!(data.gp_warning_if(G, 99).is_none());
        // The right generation warns and closes.
        let w = data.gp_warning_if(G, opened.generation).unwrap();
        assert_eq!(
            (w.round_idx, w.count, w.closes_at),
            (0, 2, opened.closes_at)
        );
        assert_eq!(w.prompt, "p1");
        let closed = data
            .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
            .unwrap();
        assert_eq!(closed.count, 2);
        // The same timer firing again (or the host) is a no-op / error now.
        assert!(data
            .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
            .is_none());
        assert!(data.gp_warning_if(G, opened.generation).is_none());
        assert_eq!(
            data.gp_close_window(G, A, &mut rng(), NOW).unwrap_err(),
            CrackedError::WindowClosed
        );
        // No game at all.
        assert!(data
            .gp_close_window_if(GuildId::new(9), 1, &mut rng(), NOW)
            .is_none());
    }

    #[test]
    fn host_close_permissions() {
        let data = data();
        game_with(&data, &["p1"]);
        assert_eq!(
            data.gp_close_window(G, B, &mut rng(), NOW).unwrap_err(),
            CrackedError::NotGameHost
        );
        assert_eq!(
            data.gp_close_window(GuildId::new(9), A, &mut rng(), NOW)
                .unwrap_err(),
            CrackedError::NoGameInProgress
        );
    }

    #[test]
    fn single_submission_is_likes_only() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let GpNext::Track(start) = &closed.next else {
            unreachable!()
        };
        assert!(!start.guessable);
        assert_eq!(start.total_tracks, 1);
        // No dropdown, so no guessing...
        assert_eq!(
            data.gp_record_guess(G, 0, 0, B, "bob".into(), A)
                .unwrap_err(),
            CrackedError::NotGuessable
        );
        // ...but likes still count.
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, B, "bob".into()).unwrap(),
            GpLikeOutcome::Liked(1)
        );
        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert!(!res.guessable);
        assert!(!res.fooled_everyone);
        assert!(res.correct.is_empty());
        assert_eq!(res.likes, 1);
        assert_eq!(game(&data).scores.get(&A), Some(&GP_POINTS_PER_LIKE));
        assert!(matches!(res.next, GpNext::Finished(_)));
    }

    #[test]
    fn guesses_likes_and_scoring() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let g = game(&data);
        let s0 = g.rounds[0].tracks[0].submitter;
        let other = if s0 == A { B } else { A };

        // Wrong round / track are stale.
        assert_eq!(
            data.gp_record_guess(G, 1, 0, C, "carol".into(), A)
                .unwrap_err(),
            CrackedError::StaleRound
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 1, C, "carol".into(), A)
                .unwrap_err(),
            CrackedError::StaleRound
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 1, C, "carol".into()).unwrap_err(),
            CrackedError::StaleRound
        );
        // Only submitters are valid answers.
        assert_eq!(
            data.gp_record_guess(G, 0, 0, C, "carol".into(), C)
                .unwrap_err(),
            CrackedError::NotAPlayer
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 0, C, "carol".into(), other)
                .unwrap(),
            GpGuessOutcome::Recorded
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 0, C, "carol".into(), s0)
                .unwrap(),
            GpGuessOutcome::Changed
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 0, C, "carol".into(), s0)
                .unwrap(),
            GpGuessOutcome::Recorded
        );
        // The submitter may pick on their own song; it never scores.
        data.gp_record_guess(G, 0, 0, s0, "self".into(), s0)
            .unwrap();
        // The other player guesses wrong.
        data.gp_record_guess(G, 0, 0, other, "other".into(), other)
            .unwrap();
        // Likes: own song rejected, others toggle.
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, s0, "self".into()).unwrap_err(),
            CrackedError::CannotLikeOwnSong
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
            GpLikeOutcome::Liked(1)
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, C, "carol".into()).unwrap(),
            GpLikeOutcome::Liked(2)
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
            GpLikeOutcome::Unliked(1)
        );

        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert_eq!(res.submitter, s0);
        assert_eq!(res.correct, vec![C]);
        assert!(!res.fooled_everyone);
        assert_eq!(res.likes, 1);
        assert!(res.guessable);
        assert!(matches!(res.next, GpNext::Track(_)));
        let g = game(&data);
        assert_eq!(g.scores.get(&C), Some(&GP_POINTS_CORRECT));
        assert_eq!(g.scores.get(&s0), Some(&GP_POINTS_PER_LIKE));
        assert_eq!(g.scores.get(&other), None);
        assert_eq!(g.current_track, 1);

        // Second call for the same song is a no-op.
        assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());
        // Guessing on the finished song is stale now.
        assert_eq!(
            data.gp_record_guess(G, 0, 0, C, "carol".into(), s0)
                .unwrap_err(),
            CrackedError::StaleRound
        );

        // Song 2: nobody guesses -> fooled everyone, game finishes.
        let s1 = g.rounds[0].tracks[1].submitter;
        let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
        assert!(res.fooled_everyone);
        assert_eq!(res.likes, 0);
        assert!(matches!(res.next, GpNext::Finished(_)));
        let g = game(&data);
        assert_eq!(g.phase, GpPhase::Finished);
        assert_ne!(s1, s0);
        assert_eq!(g.scores.get(&s1), Some(&GP_POINTS_FOOLED_ALL));
        assert_eq!(
            data.gp_record_guess(G, 0, 1, C, "carol".into(), A)
                .unwrap_err(),
            CrackedError::GameNotPlaying
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 1, C, "carol".into()).unwrap_err(),
            CrackedError::GameNotPlaying
        );
    }

    /// A song that never played must not be scored. songbird reports a stream
    /// it could not open as an `End` whose state is still `Errored`, and before
    /// this the game happily revealed it and paid out guesses and likes for a
    /// song nobody had heard.
    #[test]
    fn failed_track_scores_nothing_and_advances() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let submitter = game(&data).rounds[0].tracks[0].submitter;
        let guesser = if submitter == A { B } else { A };

        // A correct guess and a like, both of which would normally pay out.
        data.gp_record_guess(G, 0, 0, guesser, "guesser".into(), submitter)
            .unwrap();
        data.gp_toggle_like(G, 0, 0, guesser, "guesser".into())
            .unwrap();

        let res = data.gp_fail_and_advance(G, 0, 0, NOW).unwrap();
        assert!(res.failed);
        assert!(res.correct.is_empty(), "a correct guess must not count");
        assert!(!res.fooled_everyone, "an unheard song fools nobody");
        assert!(res.scores.iter().all(|(_, points)| *points == 0));
        // The game still moves on to the next song.
        assert!(matches!(res.next, GpNext::Track(ref s) if s.track_idx == 1));

        // The surviving song scores normally, so only the failure is skipped.
        let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
        assert!(!res.failed);
        assert!(res.scores.iter().any(|(_, points)| *points > 0));
    }

    /// The reveal for a failed song says so, and drops the guess/like fields
    /// rather than reporting zeroes for a song that never played.
    #[test]
    fn failed_reveal_embed_json() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let res = data.gp_fail_and_advance(G, 0, 0, NOW).unwrap();

        let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2, "failure note and scoreboard only");
        assert_eq!(fields[0]["name"], GP_TRACK_FAILED);
        assert_eq!(fields[0]["value"], GP_TRACK_FAILED_NOTE);
        assert_eq!(fields[1]["name"], GP_SCOREBOARD);
        let names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();
        assert!(!names.contains(&GP_GUESSED_RIGHT), "{names:?}");
        assert!(!names.contains(&GP_LIKES), "{names:?}");
    }

    #[test]
    fn tracks_then_next_prompt() {
        let data = data();
        game_with(&data, &["p1", "p2"]);
        for round in 0..2 {
            submit(&data, A, "alice", &format!("a{round}"));
            submit(&data, B, "bob", &format!("b{round}"));
            let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
            assert!(matches!(closed.next, GpNext::Track(_)));
            let first = data.gp_reveal_and_advance(G, round, 0, NOW).unwrap();
            assert!(matches!(first.next, GpNext::Track(ref s) if s.track_idx == 1));
            let second = data.gp_reveal_and_advance(G, round, 1, NOW).unwrap();
            if round == 0 {
                assert!(
                    matches!(second.next, GpNext::Window(ref w) if w.round_idx == 1 && w.prompt == "p2")
                );
                assert_eq!(game(&data).phase, GpPhase::Submitting);
            } else {
                let GpNext::Finished(scores) = second.next else {
                    panic!("expected finished");
                };
                assert_eq!(scores.len(), 2);
            }
        }
        assert_eq!(game(&data).phase, GpPhase::Finished);
    }

    #[test]
    fn end_permissions_and_missing_game() {
        let data = data();
        game_with(&data, &["p1"]);
        assert_eq!(
            data.gp_end(G, B, false).unwrap_err(),
            CrackedError::NotGameHost
        );
        let g = data.gp_end(G, C, true).unwrap();
        assert_eq!(g.host, A);
        assert!(!data.gp_is_active(G));
        assert_eq!(
            data.gp_end(G, A, false).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert_eq!(
            data.gp_status(G).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert_eq!(
            data.gp_window_open(G).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert!(data.gp_remove(G).is_none());
        assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());
        assert_eq!(data.gp_voice_channel(G), None);
        assert_eq!(
            data.gp_record_guess(G, 0, 0, A, "a".into(), B).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, A, "a".into()).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert_eq!(
            data.gp_set_prompt_message(G, 0, TC, MessageId::new(1))
                .unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert!(!data.gp_is_playing(G));
    }

    #[test]
    fn message_bookkeeping() {
        let data = data();
        game_with(&data, &["p1"]);
        assert_eq!(
            data.gp_set_prompt_message(G, 3, TC, MessageId::new(1))
                .unwrap_err(),
            CrackedError::StaleRound
        );
        data.gp_set_prompt_message(G, 0, TC, MessageId::new(7))
            .unwrap();
        assert_eq!(
            data.gp_set_track_message(G, 0, 0, TC, MessageId::new(1))
                .unwrap_err(),
            CrackedError::StaleRound,
            "no tracks before the window closes"
        );
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        assert_eq!(closed.prompt_message, Some((TC, MessageId::new(7))));
        data.gp_set_track_message(G, 0, 0, TC, MessageId::new(42))
            .unwrap();
        assert_eq!(
            data.gp_set_track_message(G, 0, 5, TC, MessageId::new(1))
                .unwrap_err(),
            CrackedError::StaleRound
        );
        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert_eq!(res.message, Some((TC, MessageId::new(42))));
        assert_eq!(res.text_channel, TC);
        let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
        assert_eq!(res.message, None);
    }

    #[test]
    fn scores_tie_break_by_name() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, B, "bob", "b");
        submit(&data, A, "alice", "a");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        // Carol guesses right both songs; alice and bob tie at 0.
        for i in 0..2 {
            let s = game(&data).rounds[0].tracks[i].submitter;
            data.gp_record_guess(G, 0, i, C, "carol".into(), s).unwrap();
            data.gp_reveal_and_advance(G, 0, i, NOW).unwrap();
        }
        assert_eq!(
            game(&data).sorted_scores(),
            vec![(C, 2 * GP_POINTS_CORRECT), (A, 0), (B, 0)]
        );
    }

    #[test]
    fn status_snapshots() {
        let data = data();
        game_with(&data, &["p1", "p2"]);
        submit(&data, B, "bob", "b");
        match data.gp_status(G).unwrap() {
            GpStatus::Submitting {
                host,
                round,
                total,
                prompt,
                closes_at,
                submitted,
                scores,
            } => {
                assert_eq!(host, A);
                assert_eq!((round, total), (1, 2));
                assert_eq!(prompt, "p1");
                assert_eq!(closes_at, NOW + TIMER as i64);
                assert_eq!(submitted, vec!["bob".to_string()]);
                assert_eq!(scores.len(), 2);
            },
            other => panic!("expected submitting, got {other:?}"),
        }
        submit(&data, A, "alice", "a");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        data.gp_record_guess(G, 0, 0, C, "carol".into(), A).unwrap();
        data.gp_toggle_like(G, 0, 0, C, "carol".into()).unwrap();
        match data.gp_status(G).unwrap() {
            GpStatus::Playing {
                round,
                total,
                track,
                tracks,
                prompt,
                guessed,
                likes,
                scores,
            } => {
                assert_eq!((round, total, track, tracks), (1, 2, 1, 2));
                assert_eq!(prompt, "p1");
                assert_eq!(guessed, vec!["carol".to_string()]);
                assert_eq!(likes, 1);
                assert_eq!(scores.len(), 3);
            },
            other => panic!("expected playing, got {other:?}"),
        }
    }

    #[test]
    fn custom_ids() {
        assert_eq!(
            parse_custom_id("gp:g:1:0:0"),
            Some((GpComponent::Guess, GuildId::new(1), 0, 0))
        );
        assert_eq!(
            parse_custom_id("gp:l:1:2:7"),
            Some((GpComponent::Like, GuildId::new(1), 2, 7))
        );
        assert_eq!(
            parse_custom_id(&gp_custom_id(GpComponent::Guess, G, 3, 4)),
            Some((GpComponent::Guess, G, 3, 4))
        );
        assert_eq!(parse_custom_id("gp:x:1:0:0"), None);
        assert_eq!(parse_custom_id("gp:g:0:0:0"), None);
        assert_eq!(parse_custom_id("gp:g:1:0"), None);
        assert_eq!(parse_custom_id("gp:g:1:0:0:9"), None);
        assert_eq!(parse_custom_id("gp:1:0"), None, "the v1 shape is rejected");
        assert_eq!(parse_custom_id("song_select"), None);
    }

    /// The controls as Discord will receive them: a string select (only when
    /// guessable) and a 👍 button, each in its own action row.
    #[test]
    fn components_json() {
        let players = vec![(B, "bob".to_string()), (A, "alice".to_string())];
        let rows = gp_components(G, 1, 2, &players, true);
        assert_eq!(rows.len(), 2);
        let v = serde_json::to_value(&rows).unwrap();
        let menu = &v[0]["components"][0];
        assert_eq!(menu["custom_id"], "gp:g:1:1:2");
        assert_eq!(menu["placeholder"], GP_SELECT_PLACEHOLDER);
        assert_eq!(menu["min_values"], 1);
        assert_eq!(menu["max_values"], 1);
        let options = menu["options"].as_array().unwrap();
        assert_eq!(options.len(), 2);
        assert_eq!(options[0]["label"], "bob");
        assert_eq!(options[0]["value"], "200");
        let button = &v[1]["components"][0];
        assert_eq!(button["custom_id"], "gp:l:1:1:2");
        assert_eq!(button["label"], GP_LIKE_LABEL);
        assert_eq!(button["emoji"]["name"], "👍");
        assert_eq!(button["style"], 2, "secondary");

        // Not guessable: only the like button.
        let rows = gp_components(G, 0, 0, &players, false);
        assert_eq!(rows.len(), 1);
        let v = serde_json::to_value(&rows).unwrap();
        assert_eq!(v[0]["components"][0]["custom_id"], "gp:l:1:0:0");

        // Options are capped at 25.
        let many: Vec<(UserId, String)> = (1..=40u64)
            .map(|i| (UserId::new(i), format!("u{i}")))
            .collect();
        let v = serde_json::to_value(gp_components(G, 0, 0, &many, true)).unwrap();
        assert_eq!(
            v[0]["components"][0]["options"].as_array().unwrap().len(),
            GP_MAX_PLAYERS
        );
    }

    #[test]
    fn prompt_embeds_json() {
        let opened = GpWindowOpened {
            round_idx: 1,
            total_rounds: 3,
            prompt: "What song do you cry to?".into(),
            closes_at: NOW,
            timer_secs: TIMER,
            generation: 4,
            text_channel: TC,
        };
        let v = serde_json::to_value(gp_prompt_embed(&opened)).unwrap();
        assert_eq!(v["title"], format!("{GP_ROUND_TITLE} 2/3"));
        assert_eq!(v["description"], "**What song do you cry to?**");
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields[0]["name"], GP_PROMPT_HOW_TO_TITLE);
        assert_eq!(fields[1]["name"], GP_PROMPT_CLOSES_TITLE);
        assert!(fields[1]["value"]
            .as_str()
            .unwrap()
            .starts_with(&format!("<t:{NOW}:R>")));

        let closed = GpWindowClosed {
            round_idx: 1,
            total_rounds: 3,
            prompt: "p".into(),
            prompt_message: None,
            count: 4,
            text_channel: TC,
            next: GpNext::Finished(vec![]),
        };
        let v = serde_json::to_value(gp_prompt_closed_embed(&closed)).unwrap();
        let desc = v["description"].as_str().unwrap();
        assert!(
            desc.contains(&format!("{GP_WINDOW_CLOSED} 4 {GP_WINDOW_CLOSED_SONGS}")),
            "{desc}"
        );
        let empty = GpWindowClosed { count: 0, ..closed };
        let v = serde_json::to_value(gp_prompt_closed_embed(&empty)).unwrap();
        assert!(v["description"].as_str().unwrap().contains(GP_WINDOW_EMPTY));

        let w = GpWindowWarning {
            round_idx: 0,
            total_rounds: 1,
            prompt: "p".into(),
            count: 2,
            closes_at: NOW,
            text_channel: TC,
        };
        let text = gp_warning_text(&w);
        assert!(text.starts_with(GP_WINDOW_WARNING));
        assert!(text.contains(&format!("<t:{NOW}:R>")));
    }

    /// The song message must show the prompt and the track but never a mention.
    #[test]
    fn track_embed_hides_submitter() {
        let start = GpTrackStart {
            round_idx: 0,
            total_rounds: 2,
            track_idx: 1,
            total_tracks: 3,
            prompt: "Cry song.".into(),
            track: track("secret"),
            players: vec![],
            guessable: true,
            text_channel: TC,
        };
        let v = serde_json::to_value(gp_track_embed(&start)).unwrap();
        assert_eq!(
            v["title"],
            format!("{GP_ROUND_TITLE} 1/2 · {GP_SONG_TITLE} 2/3")
        );
        let desc = v["description"].as_str().unwrap();
        assert!(desc.contains("*Cry song.*"), "{desc}");
        assert!(desc.contains("secret"), "{desc}");
        assert!(desc.contains(GP_ROUND_HINT), "{desc}");
        assert!(desc.contains(GP_LIKE_HINT), "{desc}");
        assert!(!desc.contains("<@"), "must not mention anyone: {desc}");

        let solo = GpTrackStart {
            guessable: false,
            ..start
        };
        let desc = serde_json::to_value(gp_track_embed(&solo)).unwrap()["description"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!desc.contains(GP_ROUND_HINT), "{desc}");
        assert!(desc.contains(GP_LIKE_HINT), "{desc}");
    }

    #[test]
    fn reveal_embed_json() {
        let res = GpTrackResult {
            round_idx: 0,
            total_rounds: 2,
            track_idx: 0,
            total_tracks: 2,
            prompt: "Cry song.".into(),
            submitter: A,
            title: "song".into(),
            url: "https://example.invalid/song".into(),
            correct: vec![B, C],
            fooled_everyone: false,
            likes: 3,
            guessable: true,
            scores: vec![(B, 100), (C, 100), (A, 30)],
            message: None,
            text_channel: TC,
            next: GpNext::Finished(vec![]),
            failed: false,
        };
        let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
        assert_eq!(
            v["title"],
            format!("{GP_ROUND_TITLE} 1/2 · {GP_SONG_TITLE} 1/2")
        );
        let desc = v["description"].as_str().unwrap();
        assert!(desc.contains("*Cry song.*"), "{desc}");
        assert!(
            desc.contains("[song](https://example.invalid/song)"),
            "{desc}"
        );
        assert!(desc.contains(&format!("{GP_REVEAL} <@100>")), "{desc}");
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3, "guessed right, likes, scoreboard");
        assert_eq!(fields[0]["name"], GP_GUESSED_RIGHT);
        assert_eq!(fields[0]["value"], "<@200>, <@300>");
        assert_eq!(fields[1]["name"], GP_LIKES);
        assert_eq!(fields[1]["value"], "3");
        assert_eq!(fields[2]["name"], GP_SCOREBOARD);
        assert_eq!(
            fields[2]["value"],
            "1. <@200> — 100\n2. <@300> — 100\n3. <@100> — 30"
        );

        let fooled = GpTrackResult {
            correct: vec![],
            fooled_everyone: true,
            likes: 0,
            ..res.clone()
        };
        let v = serde_json::to_value(gp_reveal_embed(&fooled)).unwrap();
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0]["value"], GP_NOBODY_GUESSED);
        assert_eq!(fields[1]["name"], GP_FOOLED_EVERYONE);
        assert_eq!(fields[1]["value"], "<@100>");

        // Not guessable: no guess fields at all.
        let solo = GpTrackResult {
            correct: vec![],
            guessable: false,
            ..res
        };
        let v = serde_json::to_value(gp_reveal_embed(&solo)).unwrap();
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["name"], GP_LIKES);
    }

    #[test]
    fn scoreboard_and_status_embeds_json() {
        let v = serde_json::to_value(gp_scoreboard_embed(&[], GP_GAME_OVER)).unwrap();
        assert_eq!(v["title"], GP_GAME_OVER);
        assert_eq!(v["description"], "-");

        let submitting = GpStatus::Submitting {
            host: A,
            round: 1,
            total: 5,
            prompt: "p".into(),
            closes_at: NOW,
            submitted: vec!["alice".into(), "bob".into()],
            scores: vec![(A, 0)],
        };
        let v = serde_json::to_value(gp_status_embed(&submitting)).unwrap();
        assert_eq!(v["title"], format!("{GP_STATUS_SUBMITTING} 1/5"));
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields[0]["value"], "p");
        assert_eq!(fields[1]["value"], "<@100>");
        assert_eq!(fields[2]["value"], format!("<t:{NOW}:R>"));
        assert_eq!(fields[3]["value"], "alice, bob");
        // Titles never appear in status output.
        assert!(!serde_json::to_string(&v).unwrap().contains("watch?v="));

        let playing = GpStatus::Playing {
            round: 2,
            total: 3,
            track: 1,
            tracks: 4,
            prompt: "p".into(),
            guessed: vec![],
            likes: 2,
            scores: vec![(A, 110)],
        };
        let v = serde_json::to_value(gp_status_embed(&playing)).unwrap();
        assert_eq!(
            v["title"],
            format!("{GP_STATUS_PLAYING} 2/3 · {GP_SONG_TITLE} 1/4")
        );
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields[1]["value"], GP_NOBODY_YET);
        assert_eq!(fields[2]["value"], "2");
        assert_eq!(fields[3]["value"], "1. <@100> — 110");
    }

    /// Every blocked name must be a real top-level music command (so the list
    /// cannot silently rot), and none of the game's own subcommands may be
    /// caught by it. poise fills `qualified_name` only at framework start, so
    /// top-level `name`s are what we compare against here.
    #[cfg(not(tarpaulin_include))]
    #[test]
    fn blocklist_matches_registry() {
        let music: Vec<String> = crate::commands::music::music_commands()
            .into_iter()
            .map(|c| c.name.to_string())
            .collect();
        for blocked in GP_BLOCKED_COMMANDS {
            assert!(
                music.contains(&blocked.to_string()),
                "{blocked} is not a registered music command"
            );
        }
        assert!(!GP_BLOCKED_COMMANDS.contains(&"voteskip"));
        assert!(!GP_BLOCKED_COMMANDS.contains(&"gp"));
        for sub in &gp().subcommands {
            let qualified = format!("gp {}", sub.name);
            assert!(!GP_BLOCKED_COMMANDS.contains(&qualified.as_str()));
        }
    }

    /// Registration and the attributes the design relies on.
    #[cfg(not(tarpaulin_include))]
    #[test]
    fn command_registration() {
        let all = crate::commands::all_commands();
        let registered = crate::commands::commands_to_register();
        for list in [&all, &registered] {
            assert!(list.iter().any(|c| c.name == "gp"), "gp not registered");
        }

        let cmd = gp();
        assert_eq!(cmd.category.as_deref(), Some("Games"));
        assert!(cmd.guild_only);
        assert!(cmd.aliases.iter().any(|a| a == "guiltypleasure"));
        assert!(cmd.slash_action.is_some() && cmd.prefix_action.is_some());

        let mut names: Vec<&str> = cmd.subcommands.iter().map(|c| c.name.as_ref()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["close", "end", "skip", "start", "status", "submit"]
        );
        for sub in &cmd.subcommands {
            assert!(sub.guild_only, "{} must be guild_only", sub.name);
            assert!(
                !sub.checks.is_empty(),
                "{} must carry cmd_check_music",
                sub.name
            );
            assert!(
                sub.slash_action.is_some(),
                "{} needs a slash form",
                sub.name
            );
            if sub.name == "submit" {
                assert!(sub.ephemeral, "submit replies must be ephemeral");
                assert!(
                    sub.prefix_action.is_none(),
                    "submit must be slash-only so the query never lands in the channel"
                );
            } else {
                assert!(
                    sub.prefix_action.is_some(),
                    "{} should work as a prefix command",
                    sub.name
                );
            }
            if sub.name == "start" {
                let params: Vec<&str> = sub.parameters.iter().map(|p| p.name.as_ref()).collect();
                assert_eq!(params, vec!["category", "rounds", "timer"]);
                assert!(sub.parameters[0].required);
                assert_eq!(
                    sub.parameters[0].choices.len(),
                    crate::commands::music::gp_prompts::GP_PROMPTS.len() + 1,
                    "every category + Mixed"
                );
                assert!(!sub.parameters[1].required && !sub.parameters[2].required);
            }
        }
    }
}
