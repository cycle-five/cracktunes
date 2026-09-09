//! "What's your song?" party game (`/gp`).
//!
//! Each round the bot posts a prompt from [`gp_prompts`] and opens a timed
//! window for everyone in the voice channel to secretly submit a song. The
//! round's songs then play back-to-back: guess the submitter from a dropdown,
//! 👍 the ones you like, and the submitter is revealed when the song ends.
//! Scores live in memory for the duration of the game, and are written to
//! Postgres at each submission and each song's end so a restart does not end
//! the game -- see [`gp_persist`](super::gp_persist).

use crate::{
    commands::cmd_check_music,
    commands::get_call_or_join_author,
    commands::music::gp_prompts::{draw_prompts, GpCategory},
    commands::music::skip::force_skip_top_track,
    db::GpOutcome,
    errors::CrackedError,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    messaging::messages::{
        GP_ABORTED, GP_FOOLED_EVERYONE, GP_FULL_SONG, GP_FULL_SONG_NOTE, GP_GAME_OVER,
        GP_GUESSED_RIGHT, GP_GUESS_CHANGED, GP_GUESS_RECORDED, GP_HOW_TO, GP_HOW_TO_TITLE,
        GP_LIKED, GP_LIKES, GP_LIKE_HINT, GP_LIKE_LABEL, GP_NOBODY_GUESSED, GP_NOBODY_YET,
        GP_PROMPT_CLOSES_EARLY, GP_PROMPT_CLOSES_TITLE, GP_PROMPT_HOW_TO, GP_PROMPT_HOW_TO_TITLE,
        GP_RESULTS_GUESSED_BY, GP_RESULTS_GUESSED_COUNT, GP_RESULTS_NOBODY_SCORED,
        GP_RESULTS_THIS_ROUND, GP_RESULTS_TITLE, GP_REVEAL, GP_REVEAL_HELD, GP_ROUND_HINT,
        GP_ROUND_TITLE, GP_RULES_TEXT, GP_SCOREBOARD, GP_SELECT_PLACEHOLDER, GP_SONG_TITLE,
        GP_STATUS_CLOSES, GP_STATUS_GUESSED, GP_STATUS_LIKES, GP_STATUS_PLAYING, GP_STATUS_PROMPT,
        GP_STATUS_SCORES, GP_STATUS_SUBMITTED, GP_STATUS_SUBMITTING, GP_TITLE, GP_TRACK_FAILED,
        GP_TRACK_FAILED_NOTE, GP_UNLIKED, GP_WINDOW_CLOSED, GP_WINDOW_CLOSED_SONGS,
        GP_WINDOW_EMPTY, GP_WINDOW_WARNING, GP_WINDOW_WARNING_IN, SPOTIFY_GP_ONE_SONG,
        SPOTIFY_NOTHING_PLAYABLE,
    },
    music::queue::build_track,
    poise_ext::PoiseContextExt,
    sources::sleevenote,
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
use songbird::tracks::{PlayMode, TrackHandle, TrackState};
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
pub const GP_REVEAL_PAUSE_SECS: u64 = 5;
pub const GP_POINTS_CORRECT: u32 = 100;
/// Points to the submitter when nobody guessed them.
pub const GP_POINTS_FOOLED_ALL: u32 = 100;
/// Points to the submitter per 👍 their song gets.
pub const GP_POINTS_PER_LIKE: u32 = 10;
pub const GP_DEFAULT_ROUNDS: u32 = 5;
pub const GP_DEFAULT_TIMER_SECS: u64 = 180;
/// Bounds for `/gp start`'s `rounds` and `timer`. poise's `#[min]`/`#[max]`
/// only accept literals, so the attributes on `gp_start` repeat these numbers
/// -- keep them in sync by hand.
pub const GP_MAX_ROUNDS: u32 = 20;
pub const GP_MIN_TIMER_SECS: u64 = 30;
pub const GP_MAX_TIMER_SECS: u64 = 600;
pub const GP_WARNING_SECS: u64 = 30;
/// Points to the submitter when the room votes to hear their song in full.
pub const GP_POINTS_FULL_SONG: u32 = 50;
/// Clips are on unless the host says otherwise: playing whole songs is what makes
/// a round drag, and the first thirty seconds are usually an intro that gives a
/// guesser nothing, so the default skips it.
pub const GP_DEFAULT_CLIPS: bool = true;
pub const GP_DEFAULT_CLIP_START_SECS: u64 = 30;
pub const GP_DEFAULT_CLIP_LENGTH_SECS: u64 = 45;
/// Bounds for `clip_start` and `clip_length`, same hand-kept-in-sync deal as the
/// bounds above. The length floor is deliberately well clear of "unguessable":
/// clip length is also the guessing window, since the dropdown dies with the song.
pub const GP_MAX_CLIP_START_SECS: u64 = 120;
pub const GP_MIN_CLIP_LENGTH_SECS: u64 = 20;
pub const GP_MAX_CLIP_LENGTH_SECS: u64 = 300;
/// How long `/gp end` waits for the `End` that `stop()` queued before removing the
/// parked game itself. Only a backstop: the global track-end handler normally
/// collects it within milliseconds.
pub const GP_PARK_GRACE_SECS: u64 = 10;
/// How long a game may be down before it is not brought back. Past this the
/// room has moved on, and a prompt reappearing twenty minutes later is worse
/// than nothing. Measured against the last moment the game is known to have
/// been alive: for an open submission window that is `closes_at`, which needs
/// no write at all; otherwise it is `last_seen_at`, which a graceful shutdown
/// stamps on the way down and which a hard crash leaves at the last song's end.
pub const GP_RESUME_WINDOW_SECS: i64 = 300;
/// How much of a song has to have played before a failure counts as a song the
/// room actually heard, and so as something to score. Below this an `Errored`
/// track is treated as a dead link: nobody could have guessed it, so nobody is
/// paid for it -- including the submitter, who would otherwise collect the
/// fooled-everyone bonus for a song that never really played.
///
/// This is the ceiling, not the whole rule -- see [`gp_min_played`]. Thirty
/// seconds of a four-minute song is a fair "the room heard it", but it is the
/// entire length of a thirty-second clip, and a clip that played to its end must
/// not be scored as a dead link.
pub const GP_MIN_PLAYED: Duration = Duration::from_secs(30);
/// The share of the intended play length that has to be heard when that length is
/// short enough for [`GP_MIN_PLAYED`] to be most or all of it.
pub const GP_MIN_PLAYED_DIVISOR: u32 = 2;

/// How much of `intended` has to play for the song to count as heard: half of it,
/// capped at [`GP_MIN_PLAYED`]. A full song keeps the flat thirty seconds; a
/// forty-five second clip needs twenty-two, not the whole thing minus fifteen.
///
/// The dead-link-versus-fooled-everyone split that #423 is about is a separate
/// question and stays where it is; this only stops clips landing on the wrong side
/// of the existing line.
pub fn gp_min_played(intended: Option<Duration>) -> Duration {
    match intended {
        Some(d) => GP_MIN_PLAYED.min(d / GP_MIN_PLAYED_DIVISOR),
        None => GP_MIN_PLAYED,
    }
}
/// Component custom ids look like `gp:<g|l>:<guild_id>:<round_idx>:<track_idx>`.
pub const GP_CUSTOM_ID_PREFIX: &str = "gp:";
/// Music commands refused while a game owns playback, because each would leave
/// playback in a state the game's own state machine never produced: injecting or
/// reordering tracks (`play`, `shuffle`, `remove`, ...), advancing or stalling the
/// round outside the game's control (`skip`, `seek`, `repeat`, `pause`), tearing
/// down voice (`leave`), or moving the bot out of [`GpGame::voice_channel`] so that
/// every guess and 👍 is then rejected against a channel nobody is in (`summon`).
/// The game's own `/gp skip` and `/gp voteskip` are the sanctioned ways to end a
/// song. Matched against the command's *qualified* name so `gp skip` is not caught
/// by `skip`.
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
    "voteskip",
    "leave",
    "seek",
    "repeat",
    "pause",
    "summon",
    "summonchannel",
];

/// The subset of [`GP_BLOCKED_COMMANDS`] that would stall the game outright: the
/// current track would never reach [`TrackEvent::End`], so no round would ever
/// advance. Named separately so the test can assert none of them is ever dropped.
#[cfg(test)]
pub const GP_STALLING_COMMANDS: &[&str] = &["pause", "repeat", "seek"];

/// Votes needed to end a song early: a strict majority of the *eligible* voters,
/// which is the players in the game's voice channel other than the song's own
/// submitter -- they do not vote on their own song, they simply pull it. Counting
/// anyone who cannot vote would put the bar out of reach of those who can. Never
/// fewer than one, so an uncached or empty voice channel cannot make zero enough.
pub fn gp_votes_required(eligible_voters: usize) -> usize {
    (eligible_voters / 2 + 1).max(1)
}

/// A round's order has to be a derangement of the last one, or at least this
/// many songs long before that is asked of it. Two songs have two orders, and
/// forbidding the one that repeats leaves exactly one -- so two-song rounds
/// would alternate, which is a tell of its own. They stay a plain shuffle.
pub const GP_DERANGE_MIN_SONGS: usize = 3;

/// Shuffle `items` into a play order, then keep drawing until nobody sits in
/// the slot they had in `previous` -- the previous round's order, by submitter.
///
/// A uniform shuffle is memoryless: with three players the next round repeats
/// the last one's order one time in six and keeps someone's slot two times in
/// three, and once a room has noticed, the position in the round says whose
/// song it is before a note has played. Rejecting every order with a fixed
/// point relative to the last round closes both. With at least
/// [`GP_DERANGE_MIN_SONGS`] songs a passing order always exists (each slot
/// forbids at most one player, and no player is forbidden from two slots), and
/// with three players a third of draws pass, so this is a handful of shuffles
/// at worst.
pub fn shuffle_against<T>(items: &mut [(UserId, T)], previous: &[UserId], rng: &mut impl Rng) {
    items.shuffle(rng);
    if items.len() < GP_DERANGE_MIN_SONGS || previous.is_empty() {
        return;
    }
    let keeps_a_slot =
        |items: &[(UserId, T)]| items.iter().zip(previous).any(|((id, _), prev)| id == prev);
    // Bounded only so a broken rng cannot spin forever; a real one leaves this
    // loop within a few draws, and the fallback order is still a shuffle.
    let mut tries = 0;
    while keeps_a_slot(items) && tries < 1000 {
        items.shuffle(rng);
        tries += 1;
    }
}

/// Unix seconds now; the game stores `closes_at` this way so the prompt embed
/// can show Discord's live `<t:..:R>` countdown with a single send.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ------------------------------------------------------------------
// State
// ------------------------------------------------------------------

/// How much of each song to play. `None` on a game means whole songs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpClip {
    /// How far into the song to start, so the clip skips the intro.
    pub start: Duration,
    /// How long to play for. Also the guessing window, since the dropdown lives
    /// on the song message and dies when the song does.
    pub length: Duration,
}

impl GpClip {
    /// The clip as it applies to a song of `duration`. A song shorter than the
    /// offset is played from the top rather than seeked past its own end, which
    /// would come back as an immediate `End` and read as a dead link.
    pub fn for_duration(self, duration: Option<Duration>) -> Self {
        let Some(d) = duration else {
            return self;
        };
        if self.start + self.length <= d {
            return self;
        }
        // Take the last `length` of the song where we can, otherwise all of it.
        let start = d.saturating_sub(self.length);
        Self {
            start,
            length: self.length.min(d.saturating_sub(start)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpPhase {
    Submitting,
    Playing,
    /// Last song revealed; the game is torn down once the scoreboard is posted.
    Finished,
}

/// When the room learns whose song was whose. The first `#[name]` is what
/// Discord shows in the `/gp start` dropdown; the second is what a prefix
/// command types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, poise::ChoiceParameter)]
pub enum GpReveal {
    /// The submitter is named when their song ends, as the game has always done.
    #[name = "🎉 After each song"]
    #[name = "song"]
    #[default]
    Song,
    /// Nothing is named until the round's last song has played: the reveal is
    /// the round-results embed. With every song revealed as it ends, the last
    /// song of a round is never a guess -- everyone has one song in, so by the
    /// final one the room knows by elimination -- and with three players the
    /// second is a coin flip. Holding the names keeps every song a guess.
    #[name = "🤐 At the end of the round"]
    #[name = "round"]
    Round,
}

impl GpReveal {
    /// A stable name for storage. Not the display name, whose emoji and wording
    /// are free to change.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Round => "round",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        [Self::Song, Self::Round]
            .into_iter()
            .find(|r| r.slug() == slug)
    }
}

#[derive(Clone, Debug)]
pub struct GpTrack {
    pub submitter: UserId,
    pub track: ResolvedTrack<'static>,
    /// guesser -> guessed submitter. Last guess wins.
    pub guesses: HashMap<UserId, UserId>,
    pub likes: HashSet<UserId>,
    /// Votes to end this song early, from `/gp voteskip`. Cleared with the track.
    pub skip_votes: HashSet<UserId>,
    /// Votes to hear this song in full, from `/gp votefull`.
    pub full_votes: HashSet<UserId>,
    /// The room voted to hear it all: the clip timer stands down and the
    /// submitter takes [`GP_POINTS_FULL_SONG`] at the reveal.
    pub play_full: bool,
    /// The song never played -- songbird could not open the stream -- so
    /// nothing was scored for it. Set when the song ends, and kept so the
    /// round's results can say so and so its payout stays zero when re-derived.
    pub failed: bool,
    /// The song message, so the reveal can edit it in place.
    pub message: Option<(GenericChannelId, MessageId)>,
}

/// What one song paid out, and to whom. A pure function of the song as it
/// stands, so the reveal, the round's results and the visible scoreboard all
/// agree on it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpTrackScore {
    /// Everyone (other than the submitter) whose last guess was right.
    pub correct: Vec<UserId>,
    pub fooled_everyone: bool,
    /// (player, points), one entry per player paid.
    pub points: Vec<(UserId, u32)>,
}

impl GpTrack {
    fn new(submitter: UserId, track: ResolvedTrack<'static>) -> Self {
        Self {
            submitter,
            track,
            guesses: HashMap::new(),
            likes: HashSet::new(),
            skip_votes: HashSet::new(),
            full_votes: HashSet::new(),
            play_full: false,
            failed: false,
            message: None,
        }
    }

    /// The payout for this song. `guessable` is the round's: a one-song round
    /// has nothing to guess, so no guess or fooled points are paid. A song that
    /// never played pays nothing at all -- not even for its likes.
    fn score(&self, guessable: bool) -> GpTrackScore {
        if self.failed {
            return GpTrackScore::default();
        }
        let mut correct: Vec<UserId> = if guessable {
            self.guesses
                .iter()
                .filter(|(guesser, guessed)| {
                    **guesser != self.submitter && **guessed == self.submitter
                })
                .map(|(guesser, _)| *guesser)
                .collect()
        } else {
            Vec::new()
        };
        // A stable order, so two derivations of the same song agree exactly.
        correct.sort_unstable();
        let fooled_everyone = guessable && correct.is_empty();
        let mut points: Vec<(UserId, u32)> =
            correct.iter().map(|g| (*g, GP_POINTS_CORRECT)).collect();
        let mut own = 0;
        if fooled_everyone {
            own += GP_POINTS_FOOLED_ALL;
        }
        own += self.likes.len() as u32 * GP_POINTS_PER_LIKE;
        if self.play_full {
            own += GP_POINTS_FULL_SONG;
        }
        if own > 0 {
            points.push((self.submitter, own));
        }
        GpTrackScore {
            correct,
            fooled_everyone,
            points,
        }
    }
}

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

    /// Everything the first `n` songs of this round paid out, added up per
    /// player. Only songs that have ended are scored, and `n` is how many have:
    /// a song still playing would be scored on guesses that can still change.
    fn points_through(&self, n: usize) -> HashMap<UserId, u32> {
        let guessable = self.guessable();
        let mut total: HashMap<UserId, u32> = HashMap::new();
        for t in self.tracks.iter().take(n) {
            for (who, pts) in t.score(guessable).points {
                *total.entry(who).or_insert(0) += pts;
            }
        }
        total
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

#[derive(Clone, Debug)]
pub struct GpGame {
    /// The guild the game is in -- also the map key, carried here so a game
    /// removed from the map still knows where it was.
    pub guild_id: GuildId,
    /// Unix seconds of `/gp start`. With the guild, this names the game in the
    /// database across restarts.
    pub started_at: i64,
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
    /// `None` plays whole songs.
    pub clip: Option<GpClip>,
    /// When submitters are named: as each song ends, or only in the round's
    /// results.
    pub reveal: GpReveal,
    /// Bumped on every phase transition. Timers capture the generation they
    /// were spawned for and do nothing once it has moved on, so a window that
    /// closed early (host, or everyone submitted) leaves no stale fire behind.
    pub generation: u64,
    /// Set by `/gp end`. The game is finished but deliberately still in the map:
    /// `stop()` has queued an `End` the global [`TrackEndHandler`] must see a game
    /// for, or it treats it as an ordinary track ending and starts autoplay.
    /// Whoever handles that `End` removes the game.
    ///
    /// [`TrackEndHandler`]: crate::handlers::TrackEndHandler
    pub parked_for_end: bool,
    /// Everyone who has submitted, guessed or liked, with the display name we saw.
    pub players: HashMap<UserId, String>,
    pub scores: HashMap<UserId, u32>,
}

impl GpGame {
    #[allow(clippy::too_many_arguments)]
    fn new(
        guild_id: GuildId,
        host: UserId,
        voice_channel: ChannelId,
        text_channel: GenericChannelId,
        category: GpCategory,
        prompts: Vec<String>,
        timer_secs: u64,
        clip: Option<GpClip>,
        reveal: GpReveal,
        started_at: i64,
    ) -> Self {
        Self {
            guild_id,
            started_at,
            host,
            voice_channel,
            text_channel,
            phase: GpPhase::Submitting,
            category,
            rounds: prompts.into_iter().map(GpRound::new).collect(),
            current_round: 0,
            current_track: 0,
            timer_secs,
            clip,
            reveal,
            generation: 0,
            parked_for_end: false,
            players: HashMap::new(),
            scores: HashMap::new(),
        }
    }

    /// Has this user put a song into this game? Submitting is what makes someone
    /// a player, and it sticks: someone who submitted in round 1 is still a player
    /// through a round they sit out. `players` is not the same thing -- it also
    /// holds the host and anyone whose display name was seen while guessing.
    fn has_submitted(&self, user: UserId) -> bool {
        self.rounds.iter().any(|r| {
            r.submissions.contains_key(&user) || r.tracks.iter().any(|t| t.submitter == user)
        })
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
    pub(crate) fn sorted_scores(&self) -> Vec<(UserId, u32)> {
        self.sort_points(
            self.players
                .keys()
                .map(|id| (*id, self.scores.get(id).copied().unwrap_or(0)))
                .collect(),
        )
    }

    /// The scoreboard the room may see right now. The same as
    /// [`Self::sorted_scores`], except while a round is playing with the reveal
    /// held to its end: then the totals still carry what this round's finished
    /// songs paid out, and showing them would give the round away -- a player
    /// up a hundred after song one either guessed it or was the one nobody
    /// guessed. So the round's payout so far is taken back off, and the board
    /// reads as it did when the round began.
    pub(crate) fn visible_scores(&self) -> Vec<(UserId, u32)> {
        if self.reveal != GpReveal::Round || self.phase != GpPhase::Playing {
            return self.sorted_scores();
        }
        let held = self.rounds[self.current_round].points_through(self.current_track);
        self.sort_points(
            self.players
                .keys()
                .map(|id| {
                    let total = self.scores.get(id).copied().unwrap_or(0);
                    (
                        *id,
                        total.saturating_sub(held.get(id).copied().unwrap_or(0)),
                    )
                })
                .collect(),
        )
    }

    /// Best first, ties by name.
    fn sort_points(&self, mut v: Vec<(UserId, u32)>) -> Vec<(UserId, u32)> {
        v.sort_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| {
                self.name_of(a.0)
                    .to_lowercase()
                    .cmp(&self.name_of(b.0).to_lowercase())
            })
        });
        v
    }

    /// The round as it ended, for the results embed: every song with who
    /// submitted it and who got it, what the round paid each player, and the
    /// scoreboard after it.
    fn round_result(&self, round_idx: usize) -> GpRoundResult {
        let round = &self.rounds[round_idx];
        let guessable = round.guessable();
        let songs = round
            .tracks
            .iter()
            .map(|t| {
                let score = t.score(guessable);
                GpSongResult {
                    submitter: t.submitter,
                    title: t.track.get_title(),
                    correct: score.correct,
                    fooled_everyone: score.fooled_everyone,
                    likes: t.likes.len(),
                    played_full: t.play_full,
                    failed: t.failed,
                }
            })
            .collect();
        let points: Vec<(UserId, u32)> = round
            .points_through(round.tracks.len())
            .into_iter()
            .filter(|(_, pts)| *pts > 0)
            .collect();
        GpRoundResult {
            round_idx,
            total_rounds: self.rounds.len(),
            prompt: round.prompt.clone(),
            guessable,
            songs,
            points: self.sort_points(points),
            scores: self.sorted_scores(),
        }
    }

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
        // The last play order before this round, so this one can be drawn
        // against it. A round nobody submitted to has no order and is skipped
        // over, not treated as a blank slate.
        let previous: Vec<UserId> = self.rounds[..idx]
            .iter()
            .rev()
            .find(|r| !r.tracks.is_empty())
            .map(|r| r.tracks.iter().map(|t| t.submitter).collect())
            .unwrap_or_default();
        let round = &mut self.rounds[idx];
        // Sort before shuffling so a seeded rng gives the same order regardless
        // of HashMap iteration order.
        let mut subs: Vec<(UserId, ResolvedTrack<'static>)> = round.submissions.drain().collect();
        subs.sort_by_key(|(id, _)| *id);
        shuffle_against(&mut subs, &previous, rng);
        round.tracks = subs
            .into_iter()
            .map(|(submitter, track)| GpTrack::new(submitter, track))
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

    pub(crate) fn track_start(&self) -> GpTrackStart {
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
            clip: self
                .clip
                .map(|c| c.for_duration(t.track.get_metadata().and_then(|m| m.duration))),
            generation: self.generation,
            text_channel: self.text_channel,
        }
    }
}

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
    /// The clip to play, already fitted to this song's duration. `None` plays it
    /// whole.
    pub clip: Option<GpClip>,
    /// The generation this song started under, so its clip timer can tell whether
    /// the game has moved on since.
    pub generation: u64,
    pub text_channel: GenericChannelId,
}

#[derive(Clone, Debug)]
pub enum GpNext {
    /// Boxed: a `ResolvedTrack` is a couple of KB and the other variants are tiny.
    Track(Box<GpTrackStart>),
    Window(GpWindowOpened),
    /// Final scores, sorted.
    Finished(Vec<(UserId, u32)>),
}

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

#[derive(Clone, Debug)]
pub struct GpWindowWarning {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub count: usize,
    pub closes_at: i64,
    pub text_channel: GenericChannelId,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpGuessOutcome {
    Recorded,
    Changed,
}

/// What `gp_vote_skip` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpVoteSkipOutcome {
    /// Not there yet; `needed` more votes will end the song.
    Counted { votes: usize, needed: usize },
    /// The room agreed: the caller should stop the current song.
    Passed,
    /// The caller submitted this song, so it is pulled outright rather than voted
    /// on. The caller should stop the current song.
    OwnSong,
}

/// What `gp_vote_full` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpVoteFullOutcome {
    /// Not there yet; `needed` more votes will let it run on.
    Counted { votes: usize, needed: usize },
    /// The room wants the whole thing: the clip timer stands down.
    Passed,
    /// Already carried, by an earlier vote on this same song.
    AlreadyFull,
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
    /// The room voted this one up to its full length.
    pub played_full: bool,
    pub guessable: bool,
    /// Sorted descending.
    pub scores: Vec<(UserId, u32)>,
    pub message: Option<(GenericChannelId, MessageId)>,
    pub text_channel: GenericChannelId,
    pub next: GpNext,
    /// The song never played: songbird failed to open the stream. Nothing was
    /// scored for it.
    pub failed: bool,
    /// The game reveals at the end of the round, not now: the song's own message
    /// names nobody and shows no scores, and `round` does the revealing when it
    /// comes.
    pub held: bool,
    /// This was the round's last song: the round's results, to post after the
    /// reveal and before whatever comes next.
    pub round: Option<GpRoundResult>,
}

/// One song on the round-results embed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpSongResult {
    pub submitter: UserId,
    pub title: String,
    pub correct: Vec<UserId>,
    pub fooled_everyone: bool,
    pub likes: usize,
    pub played_full: bool,
    pub failed: bool,
}

/// A round as it ended, cloned out of the map for the results embed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpRoundResult {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub guessable: bool,
    /// In play order.
    pub songs: Vec<GpSongResult>,
    /// What this round paid each player who took anything, best first.
    pub points: Vec<(UserId, u32)>,
    /// The running totals after it, best first.
    pub scores: Vec<(UserId, u32)>,
}

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
// State helpers on Data
// ------------------------------------------------------------------

// Every helper below is synchronous: it takes the `gp_games` entry, mutates
// it, clones out what the caller needs and releases it before returning.
// Never hold a `DashMap` ref across an await -- the track end handler runs on
// songbird's event task and then takes the call lock, so an entry held there
// is a deadlock waiting to happen.
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
        clip: Option<GpClip>,
        reveal: GpReveal,
        now: i64,
    ) -> CrackedResult<GpWindowOpened> {
        // `contains_key` then `insert` would let two `/gp start` race through and
        // leave the loser's window timer running against the winner's game.
        let dashmap::mapref::entry::Entry::Vacant(slot) = self.gp_games.entry(guild_id) else {
            return Err(CrackedError::GameAlreadyRunning);
        };
        if prompts.is_empty() {
            return Err(CrackedError::Other("That category has no prompts."));
        }
        let mut game = GpGame::new(
            guild_id,
            host,
            voice_channel,
            text_channel,
            category,
            prompts,
            timer_secs,
            clip,
            reveal,
            now,
        );
        game.players.insert(host, host_name);
        let opened = game.open_window(now);
        slot.insert(game);
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
        // A submission is one of the two moments the game is written down.
        self.gp_snapshot(game);
        Ok(GpSubmitOutcome {
            replaced,
            submitted,
            everyone_in,
            generation: game.generation,
        })
    }

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
        let closed = game.close_window(rng, now);
        // Closing is a checkpoint: it is the moment a round stops being a set of
        // submissions and becomes a play order, and the only writer of `phase =
        // playing` and of `closes_at = None`. Until it lands the rows still say
        // the window is open until a `closes_at` that has passed, so a resume
        // during the round's first song would measure the outage from there and
        // write off a game that was only away for seconds.
        self.gp_snapshot(&game);
        Ok(closed)
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
        let closed = game.close_window(rng, now);
        // A checkpoint for the same reason as `gp_close_window`.
        self.gp_snapshot(&game);
        Some(closed)
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
    ///
    /// Also a checkpoint, and the only one taken while a song plays. The id is
    /// what a resume takes the pre-restart dropdown down by, and the checkpoint
    /// before this one was the end of the *previous* song, when this track had no
    /// message yet -- so without a write here the restarted game would never
    /// learn which message to close, and the old one would sit there with a live
    /// dropdown for the rest of the game, answering every click with a stale
    /// round.
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
        self.gp_snapshot(&game);
        Ok(())
    }

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
        if !game.has_submitted(guesser) {
            return Err(CrackedError::NotAGamePlayer);
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
        if !game.has_submitted(liker) {
            return Err(CrackedError::NotAGamePlayer);
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

    /// Count a vote to end the current song early. `vc_members` are the non-bot
    /// members of the game's voice channel; the song ends once a strict majority
    /// of them has voted, so a single player cannot skip for everyone else.
    pub fn gp_vote_skip(
        &self,
        guild_id: GuildId,
        voter: UserId,
        voter_name: String,
        vc_members: &[UserId],
    ) -> CrackedResult<GpVoteSkipOutcome> {
        let mut entry = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let game: &mut GpGame = &mut entry;
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying);
        }
        if !game.has_submitted(voter) {
            return Err(CrackedError::NotAGamePlayer);
        }
        let (round_idx, track_idx) = (game.current_round, game.current_track);
        let submitter = game
            .rounds
            .get(round_idx)
            .and_then(|r| r.tracks.get(track_idx))
            .ok_or(CrackedError::StaleRound)?
            .submitter;
        // The submitter is not a voter on their own song: it is theirs to pull.
        if voter == submitter {
            return Ok(GpVoteSkipOutcome::OwnSong);
        }
        // The denominator has to match the numerator. Only players may vote, so
        // counting the whole voice channel would set a bar that the people allowed
        // to vote cannot clear -- with one lurker in a channel of three, the single
        // eligible voter needs a second that nobody can cast, and the song becomes
        // unskippable. Which is the mixed channel the submitters-only rule creates.
        let eligible = vc_members
            .iter()
            .filter(|u| **u != submitter && game.has_submitted(**u))
            .count();
        let required = gp_votes_required(eligible);
        let t = &mut game.rounds[round_idx].tracks[track_idx];
        if !t.skip_votes.insert(voter) {
            return Err(CrackedError::AlreadyVotedSkip);
        }
        let votes = t.skip_votes.len();
        game.players.entry(voter).or_insert(voter_name);
        Ok(if votes >= required {
            GpVoteSkipOutcome::Passed
        } else {
            GpVoteSkipOutcome::Counted {
                votes,
                needed: required - votes,
            }
        })
    }

    /// Count a vote to hear the current song in full instead of as a clip. Same
    /// shape as [`Self::gp_vote_skip`] and the same pool, with one difference: the
    /// submitter is not merely excluded from voting, they have nothing to pull.
    /// A song already carried stays carried.
    pub fn gp_vote_full(
        &self,
        guild_id: GuildId,
        voter: UserId,
        voter_name: String,
        vc_members: &[UserId],
    ) -> CrackedResult<GpVoteFullOutcome> {
        let mut entry = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        let game: &mut GpGame = &mut entry;
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying);
        }
        if game.clip.is_none() {
            return Err(CrackedError::NotPlayingClips);
        }
        if !game.has_submitted(voter) {
            return Err(CrackedError::NotAGamePlayer);
        }
        let (round_idx, track_idx) = (game.current_round, game.current_track);
        let t = game
            .rounds
            .get(round_idx)
            .and_then(|r| r.tracks.get(track_idx))
            .ok_or(CrackedError::StaleRound)?;
        let submitter = t.submitter;
        // Voting to hear your own song in full is voting yourself the bonus.
        // Checked before the already-carried case so a submitter always gets the
        // same answer, the way `gp_vote_skip` orders it.
        if voter == submitter {
            return Err(CrackedError::CannotVoteOwnSongFull);
        }
        if t.play_full {
            return Ok(GpVoteFullOutcome::AlreadyFull);
        }
        let eligible = vc_members
            .iter()
            .filter(|u| **u != submitter && game.has_submitted(**u))
            .count();
        let required = gp_votes_required(eligible);
        let t = &mut game.rounds[round_idx].tracks[track_idx];
        if !t.full_votes.insert(voter) {
            return Err(CrackedError::AlreadyVotedFull);
        }
        let votes = t.full_votes.len();
        let carried = votes >= required;
        if carried {
            t.play_full = true;
        }
        game.players.entry(voter).or_insert(voter_name);
        Ok(if carried {
            GpVoteFullOutcome::Passed
        } else {
            GpVoteFullOutcome::Counted {
                votes,
                needed: required - votes,
            }
        })
    }

    /// Is the song a clip timer was spawned for still the one playing? Mirrors the
    /// generation guard the window timer uses.
    pub fn gp_clip_still_current(
        &self,
        guild_id: GuildId,
        generation: u64,
        round_idx: usize,
        track_idx: usize,
    ) -> bool {
        self.gp_games.get(&guild_id).is_some_and(|g| {
            g.phase == GpPhase::Playing
                && g.generation == generation
                && g.current_round == round_idx
                && g.current_track == track_idx
        })
    }

    /// Has the room voted this song up to its full length? The clip timer asks
    /// before stopping it, so a vote that lands mid-clip still counts.
    pub fn gp_plays_full(&self, guild_id: GuildId, round_idx: usize, track_idx: usize) -> bool {
        self.gp_games
            .get(&guild_id)
            .and_then(|g| {
                g.rounds
                    .get(round_idx)
                    .and_then(|r| r.tracks.get(track_idx))
                    .map(|t| t.play_full)
            })
            .unwrap_or(false)
    }

    /// Did anyone vote to hear more of this song? That is first-hand evidence the
    /// room was listening to something, which beats any inference from play time:
    /// a song someone asked to hear *more* of was audibly playing, whatever
    /// songbird went on to report about the stream.
    ///
    /// Deliberately only the full-song votes. A skip vote proves nothing of the
    /// sort -- silence from a dead link is exactly what makes people reach for
    /// `/gp voteskip` -- and counting those would score the songs this check
    /// exists to catch.
    pub fn gp_heard_by_vote(&self, guild_id: GuildId, round_idx: usize, track_idx: usize) -> bool {
        self.gp_games
            .get(&guild_id)
            .and_then(|g| {
                g.rounds
                    .get(round_idx)
                    .and_then(|r| r.tracks.get(track_idx))
                    .map(|t| !t.full_votes.is_empty())
            })
            .unwrap_or(false)
    }

    /// Score the song that just ended and advance. Returns `None` unless the
    /// game is playing and this is the current song, which makes it safe to
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
        let guessable = game.rounds[round_idx].guessable();
        let t = &mut game.rounds[round_idx].tracks[track_idx];
        t.failed = failed;
        let (submitter, likes, message) = (t.submitter, t.likes.len(), t.message);
        let played_full = t.play_full;
        let (title, url) = (t.track.get_title(), t.track.get_url());
        let GpTrackScore {
            correct,
            fooled_everyone,
            points,
        } = t.score(guessable);
        for (who, pts) in points {
            *game.scores.entry(who).or_insert(0) += pts;
        }
        game.generation += 1;
        game.current_track += 1;
        let total_tracks = game.rounds[round_idx].tracks.len();
        let last_of_round = game.current_track >= total_tracks;
        let next = if last_of_round {
            game.advance_round(now)
        } else {
            GpNext::Track(Box::new(game.track_start()))
        };
        let round = last_of_round.then(|| game.round_result(round_idx));
        // A song ending is the other moment the game is written down: the scores
        // it just paid out, and the position the game moves to.
        self.gp_snapshot(&game);
        Some(GpTrackResult {
            round_idx,
            total_rounds: game.rounds.len(),
            track_idx,
            total_tracks,
            prompt: game.rounds[round_idx].prompt.clone(),
            submitter,
            title,
            url,
            correct,
            fooled_everyone,
            likes,
            played_full,
            guessable,
            scores: game.visible_scores(),
            message,
            text_channel: game.text_channel,
            next,
            failed,
            held: game.reveal == GpReveal::Round,
            round,
        })
    }

    /// Park the game for teardown and hand back a snapshot for the scoreboard.
    /// Only the host (or someone who may manage the guild) may end a game.
    ///
    /// The game is deliberately *left in the map*, and it is not this caller's to
    /// remove. `TrackHandle::stop()` only queues a message for the driver task, so
    /// the `End` it causes arrives later; removing on the next line -- as this used
    /// to -- wins that race every time, and the `End` still reaches the global
    /// [`TrackEndHandler`] with no game to find, which is exactly the autoplay bug
    /// this was meant to fix. Collection belongs to whoever handles that `End`;
    /// [`Data::gp_remove_if_parked`] is how they do it. Moving out of `Playing` and
    /// bumping the generation is what stops the game's *own* handlers and timers
    /// acting on the same event.
    ///
    /// [`TrackEndHandler`]: crate::handlers::TrackEndHandler
    pub fn gp_park_for_end(
        &self,
        guild_id: GuildId,
        caller: UserId,
        caller_is_admin: bool,
    ) -> CrackedResult<GpGame> {
        let mut game = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.host != caller && !caller_is_admin {
            return Err(CrackedError::NotGameHost);
        }
        let snapshot = game.clone();
        game.phase = GpPhase::Finished;
        game.generation += 1;
        game.parked_for_end = true;
        Ok(snapshot)
    }

    /// Remove a game parked by `/gp end`, reporting whether there was one. Called
    /// from the global track-end handler once the `End` that `stop()` queued has
    /// actually arrived: autoplay has been kept away, so the game's work is done.
    /// A game that is merely finished is left alone -- only `/gp end` parks.
    pub fn gp_remove_if_parked(&self, guild_id: GuildId) -> bool {
        let parked = self
            .gp_games
            .get(&guild_id)
            .is_some_and(|g| g.parked_for_end);
        if parked {
            if let Some((_, game)) = self.gp_games.remove(&guild_id) {
                self.gp_mark_finished(&game, GpOutcome::Ended);
            }
        }
        parked
    }

    /// Remove the game unconditionally (game over, bot kicked from voice).
    ///
    /// The database is told the game is over, with why: a game that played out
    /// finished, one parked by `/gp end` was ended, and anything else was
    /// abandoned. Without this a game ended on purpose would still look live
    /// after a redeploy and come back.
    pub fn gp_remove(&self, guild_id: GuildId) -> Option<GpGame> {
        let (_, game) = self.gp_games.remove(&guild_id)?;
        let outcome = if game.parked_for_end {
            GpOutcome::Ended
        } else if game.phase == GpPhase::Finished {
            GpOutcome::Finished
        } else {
            GpOutcome::Abandoned
        };
        self.gp_mark_finished(&game, outcome);
        Some(game)
    }

    /// Put a game loaded from the database back into the map. `false` if the
    /// guild already has one, which means `/gp start` got there first.
    pub fn gp_restore(&self, guild_id: GuildId, game: GpGame) -> bool {
        match self.gp_games.entry(guild_id) {
            dashmap::mapref::entry::Entry::Vacant(slot) => {
                slot.insert(game);
                true
            },
            dashmap::mapref::entry::Entry::Occupied(_) => false,
        }
    }

    /// Start the current song over after a resume: the [`GpTrackStart`] to play
    /// and the message the song had before the restart, whose dropdown is still
    /// live and should be taken down before a new one is posted. Bumps the
    /// generation so nothing from before the restart could match, though nothing
    /// survived to try.
    pub fn gp_resume_playing(
        &self,
        guild_id: GuildId,
    ) -> Option<(GpTrackStart, Option<(GenericChannelId, MessageId)>)> {
        let mut game = self.gp_games.get_mut(&guild_id)?;
        if game.phase != GpPhase::Playing {
            return None;
        }
        game.generation += 1;
        let old = game
            .rounds
            .get(game.current_round)?
            .tracks
            .get(game.current_track)?
            .message;
        Some((game.track_start(), old))
    }

    /// Who may act in a running game: anyone who has submitted, plus the host, so
    /// that whoever is running the game can always see where it stands. The play
    /// actions themselves (guess, 👍, vote to skip) are stricter -- they require an
    /// actual submission, host or not.
    pub fn gp_require_player(&self, guild_id: GuildId, user: UserId) -> CrackedResult<()> {
        let game = self
            .gp_games
            .get(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.host == user || game.has_submitted(user) {
            Ok(())
        } else {
            Err(CrackedError::NotAGamePlayer)
        }
    }

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
                scores: game.visible_scores(),
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
                    scores: game.visible_scores(),
                }
            },
        })
    }

    /// True while a game owns playback for this guild -- from `/gp start`
    /// until the scoreboard is posted. There is no lobby any more: even while a
    /// window is open, a stray `/play` would land in front of the round's songs.
    pub fn gp_is_active(&self, guild_id: GuildId) -> bool {
        self.gp_games.contains_key(&guild_id)
    }

    pub fn gp_voice_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        self.gp_games.get(&guild_id).map(|g| g.voice_channel)
    }

    /// The channel the game plays out in: where the prompts and songs are posted,
    /// and so where a message to the room belongs.
    pub fn gp_text_channel(&self, guild_id: GuildId) -> Option<GenericChannelId> {
        self.gp_games.get(&guild_id).map(|g| g.text_channel)
    }
}

// ------------------------------------------------------------------
// Components and embeds
// ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpComponent {
    Guess,
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

pub fn gp_rules_embed() -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(GP_TITLE)
        .description(GP_RULES_TEXT)
        .field(GP_HOW_TO_TITLE, GP_HOW_TO, false)
        .colour(Colour::FOOYOO)
}

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

/// The song message once the song has ended. Names the submitter and shows what
/// they scored -- unless the game reveals at the end of the round, in which
/// case it names nobody and shows no scores: only that the song is over, and
/// its likes, which give nothing away.
pub fn gp_reveal_embed(res: &GpTrackResult) -> CreateEmbed<'static> {
    if res.held {
        let e = CreateEmbed::new()
            .title(song_title(
                res.round_idx,
                res.total_rounds,
                res.track_idx,
                res.total_tracks,
            ))
            .description(format!(
                "*{}*\n\n**[{}]({})**\n\n{GP_REVEAL_HELD}",
                res.prompt, res.title, res.url
            ));
        return if res.failed {
            e.field(GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE, false)
                .colour(Colour::RED)
        } else {
            e.field(GP_LIKES, res.likes.to_string(), true)
                .colour(Colour::DARKER_GREY)
        };
    }
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
    if res.played_full {
        e = e.field(GP_FULL_SONG, GP_FULL_SONG_NOTE, false);
    }
    e.field(GP_LIKES, res.likes.to_string(), true).field(
        GP_SCOREBOARD,
        scores_lines(&res.scores),
        false,
    )
}

pub fn gp_scoreboard_embed(scores: &[(UserId, u32)], title: &str) -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(title.to_string())
        .description(scores_lines(scores))
        .colour(Colour::GOLD)
}

/// Discord's ceiling on an embed description.
pub const GP_EMBED_DESCRIPTION_MAX: usize = 4096;

fn points_lines(points: &[(UserId, u32)]) -> String {
    if points.is_empty() {
        return GP_RESULTS_NOBODY_SCORED.to_string();
    }
    points
        .iter()
        .enumerate()
        .map(|(i, (id, pts))| format!("{}. {} — +{pts}", i + 1, id.mention()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line of the results per song. `names` lists who guessed right by
/// mention; otherwise it is a count, for a round too big for the names to fit.
fn song_result_line(i: usize, s: &GpSongResult, guessable: bool, names: bool) -> String {
    let mut line = format!("{}. **{}** · {}", i + 1, s.title, s.submitter.mention());
    if s.failed {
        line.push_str(&format!(" · {GP_TRACK_FAILED}"));
        return line;
    }
    if guessable {
        let guessed = if s.correct.is_empty() {
            GP_NOBODY_GUESSED.to_string()
        } else if names {
            s.correct
                .iter()
                .map(|id| id.mention().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            format!("{} {GP_RESULTS_GUESSED_COUNT}", s.correct.len())
        };
        line.push_str(&format!(" · {GP_RESULTS_GUESSED_BY} {guessed}"));
    }
    line.push_str(&format!(" · 👍 {}", s.likes));
    if s.fooled_everyone {
        line.push_str(&format!(" · {GP_FOOLED_EVERYONE}"));
    }
    if s.played_full {
        line.push_str(&format!(" · {GP_FULL_SONG}"));
    }
    line
}

/// The round summed up, posted at the bottom of the channel once its last
/// song has been revealed: every song with who submitted it and who got it,
/// what the round paid out, and the scoreboard. Each song's own reveal is an
/// edit of a message somewhere up the channel, and after five songs nobody
/// finds them; this is the one place the round's results are together. In a
/// game that reveals at the end of the round it is also the reveal itself.
pub fn gp_round_results_embed(r: &GpRoundResult) -> CreateEmbed<'static> {
    let lines = |names: bool| {
        let songs = r
            .songs
            .iter()
            .enumerate()
            .map(|(i, s)| song_result_line(i, s, r.guessable, names))
            .collect::<Vec<_>>()
            .join("\n");
        format!("**{}**\n\n{songs}", r.prompt)
    };
    // Twenty-five songs each naming two dozen guessers by mention is well past
    // what a description holds; fall back to counting the guessers, and past
    // that cut the list rather than have Discord refuse the whole embed.
    let mut description = lines(true);
    if description.chars().count() > GP_EMBED_DESCRIPTION_MAX {
        description = lines(false);
    }
    if description.chars().count() > GP_EMBED_DESCRIPTION_MAX {
        description = description
            .chars()
            .take(GP_EMBED_DESCRIPTION_MAX - 1)
            .collect::<String>()
            + "…";
    }
    CreateEmbed::new()
        .title(format!(
            "{} {GP_RESULTS_TITLE}",
            round_title(r.round_idx, r.total_rounds)
        ))
        .description(description)
        .field(GP_RESULTS_THIS_ROUND, points_lines(&r.points), false)
        .field(GP_SCOREBOARD, scores_lines(&r.scores), false)
        .colour(Colour::DARK_GOLD)
}

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
fn never_played(state: &TrackState, intended: Option<Duration>) -> bool {
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
    pb.call.lock().await.queue().stop();
    // Best effort: the channel is usually what just failed.
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
    let edited = match closed.prompt_message {
        Some((chan, msg_id)) => chan
            .edit_message(&pb.http, msg_id, EditMessage::new().embed(embed.clone()))
            .await
            .is_ok(),
        None => false,
    };
    if !edited {
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
        let mut handler = pb.call.lock().await;
        handler.enqueue(songbird_track).await
    };

    // Arm every handler before awaiting anything. The seek below is the first
    // await, and it is not a passive one: it forces songbird to create the
    // stream, which is when `Playable` fires and, if creation fails, when the
    // track is removed. Registering afterwards would race the first and find a
    // dead command channel after the second.
    for event in [TrackEvent::End, TrackEvent::Error] {
        if let Err(e) = handle.add_event(
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
        if let Err(e) = handle.add_event(
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
        if let Err(e) = res
            .text_channel
            .send_message(&pb.http, CreateMessage::new().embed(reveal))
            .await
        {
            tracing::warn!("gp: posting the reveal: {e}");
        }
    }
    // The round's last song: sum the round up at the bottom of the channel
    // before the next prompt (or the final scoreboard) goes up. Not fatal if it
    // cannot be posted -- the reveals above still carry it -- and never reached
    // for a round nobody submitted to, which ends at the close, not here.
    if let Some(round) = &res.round {
        if let Err(e) = res
            .text_channel
            .send_message(
                &pb.http,
                CreateMessage::new().embed(gp_round_results_embed(round)),
            )
            .await
        {
            tracing::warn!(
                "gp: posting round {} results in {guild_id}: {e}",
                round.round_idx + 1
            );
        }
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
/// (then the window simply waits for the timer or the host). The bot is always
/// in this channel and is excluded by id, not just by the member lookup, which
/// answers "human" for anyone the cache is missing.
fn gp_vc_members(ctx: Context<'_>, vc: ChannelId) -> Vec<UserId> {
    // Read before the guild: no cache lock is held across the other lookup.
    let me = ctx.serenity_context().cache.current_user().id;
    let Some(guild) = ctx.guild() else {
        return Vec::new();
    };
    guild
        .voice_states
        .iter()
        .filter(|vs| vs.channel_id == Some(vc))
        .filter(|vs| vs.user_id != me)
        .filter(|vs| guild.members.get(&vs.user_id).is_none_or(|m| !m.user.bot()))
        .map(|vs| vs.user_id)
        .collect()
}

/// The game is for the people in the room: acting on a running game means being in
/// its voice channel. `/gp submit` needs only this much -- submitting is how you
/// join -- while everything else also goes through [`gp_require_player`].
fn gp_require_in_game_vc(ctx: Context<'_>, guild_id: GuildId) -> CrackedResult<ChannelId> {
    let game_vc = ctx
        .data()
        .gp_voice_channel(guild_id)
        .ok_or(CrackedError::NoGameInProgress)?;
    if ctx.author_vc() != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel);
    }
    Ok(game_vc)
}

/// In the voice channel *and* actually playing. `/gp end` is the deliberate
/// exception to both, so an admin can always kill a game from outside it.
fn gp_require_player(ctx: Context<'_>, guild_id: GuildId) -> CrackedResult<ChannelId> {
    let game_vc = gp_require_in_game_vc(ctx, guild_id)?;
    ctx.data().gp_require_player(guild_id, ctx.author().id)?;
    Ok(game_vc)
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
    subcommands(
        "gp_start",
        "gp_submit",
        "gp_close",
        "gp_skip",
        "gp_voteskip",
        "gp_votefull",
        "gp_status",
        "gp_end"
    )
)]
pub async fn gp(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send_embed_response(gp_rules_embed()).await?;
    Ok(())
}

/// Start a game in your voice channel: pick a category, rounds, and the submission timer.
#[cfg(not(tarpaulin_include))]
#[allow(clippy::too_many_arguments)]
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
    #[description = "Play a clip of each song instead of all of it (default yes)."] clips: Option<
        bool,
    >,
    #[description = "Seconds into each song the clip starts (default 30, needs clips)."]
    #[min = 0]
    #[max = 120]
    clip_start: Option<u32>,
    #[description = "Seconds of each song to play (default 45, needs clips)."]
    #[min = 20]
    #[max = 300]
    clip_length: Option<u32>,
    #[description = "Name submitters after each song (default), or only at the end of the round."]
    reveal: Option<GpReveal>,
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
    // The two dials only mean anything with clips on; off, they are ignored
    // rather than quietly turning clips back on for a host who asked for whole
    // songs.
    let clip = clips.unwrap_or(GP_DEFAULT_CLIPS).then(|| GpClip {
        start: Duration::from_secs(
            clip_start
                .map(u64::from)
                .unwrap_or(GP_DEFAULT_CLIP_START_SECS)
                .min(GP_MAX_CLIP_START_SECS),
        ),
        length: Duration::from_secs(
            clip_length
                .map(u64::from)
                .unwrap_or(GP_DEFAULT_CLIP_LENGTH_SECS)
                .clamp(GP_MIN_CLIP_LENGTH_SECS, GP_MAX_CLIP_LENGTH_SECS),
        ),
    });
    let reveal = reveal.unwrap_or_default();
    let prompts = draw_prompts(category, rounds, &mut rand::rng());

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
        clip,
        reveal,
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
            clip,
            reveal,
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

/// Resolve a Spotify link for `/gp submit`, which takes exactly one song.
///
/// An album or playlist is refused rather than quietly reduced to its first
/// track: *which* song a player submits is the whole game, so choosing one for
/// them would replace their move with ours and they would never know.
async fn gp_spotify_query(url: &str) -> CrackedResult<QueryType> {
    let resolution = sleevenote::resolve_spotify(url).await?;
    if resolution.media_type.is_collection() {
        return Err(CrackedError::Other(SPOTIFY_GP_ONE_SONG));
    }
    let query = resolution
        .queries()
        .into_iter()
        .next()
        .ok_or(CrackedError::Other(SPOTIFY_NOTHING_PLAYABLE))?;
    Ok(QueryType::Keywords(query))
}

#[cfg(not(tarpaulin_include))]
pub async fn gp_submit_internal(ctx: Context<'_>, query: String) -> CrackedResult<CrackedMessage> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_in_game_vc(ctx, guild_id)?;
    data.gp_window_open(guild_id)?;
    let query = query.trim();
    if query.is_empty() {
        return Err(CrackedError::NoQuery);
    }
    let query_type = match QueryType::from_str(query).map_err(CrackedError::TrackResolveError)? {
        QueryType::SpotifyLink(url) => {
            // Resolving a Spotify link is an HTTP round trip that takes ten
            // seconds or more on a cold cache, well past Discord's
            // three-second interaction deadline -- so defer before doing any
            // of it, or the command fails before the answer exists. Only this
            // path defers: a search or a YouTube link answers fast enough that
            // a "thinking" state would be a downgrade.
            ctx.defer_ephemeral().await?;
            gp_spotify_query(&url).await?
        },
        other => other,
    };
    let track = data
        .ct_client
        .resolve_track(query_type)
        .await
        .map_err(CrackedError::TrackFail)?;
    let name = author_display_name(ctx).await;
    let title = track.get_title();
    let vc_members = gp_vc_members(ctx, game_vc);
    let outcome = data.gp_submit(guild_id, ctx.author().id, name, track, &vc_members)?;

    // Take the call first: closing commits the shuffle and moves the game to
    // `Playing`, so discovering there is nothing to play on afterwards would
    // strand the round with nothing ever enqueued to advance it.
    if outcome.everyone_in {
        if let Some(call) = data.songbird.get(guild_id) {
            if let Some(closed) =
                data.gp_close_window_if(guild_id, outcome.generation, &mut rand::rng(), now())
            {
                let pb = gp_playback(ctx, call, guild_id);
                // Off this task so the ephemeral confirmation isn't held up.
                tokio::spawn(async move {
                    if let Err(e) = gp_after_close(pb, closed).await {
                        tracing::warn!("gp: closing window in {guild_id}: {e}");
                    }
                });
            }
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
    gp_require_player(ctx, guild_id)?;
    // Take the call *before* closing: `gp_close_window` commits the shuffle and
    // moves the game to `Playing`, and bailing after that would leave the round
    // closed with nothing ever enqueued to advance it.
    let call = data
        .songbird
        .get(guild_id)
        .ok_or(CrackedError::NotConnected)?;
    let closed = data.gp_close_window(guild_id, ctx.author().id, &mut rand::rng(), now())?;
    if let Err(e) = ctx
        .send_reply(
            CrackedMessage::GpWindowClosed {
                count: closed.count,
            },
            true,
        )
        .await
    {
        tracing::warn!("gp: acknowledging the close in {guild_id}: {e}");
    }
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
    gp_require_player(ctx, guild_id)?;
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

/// What a vote command says: `mine` goes to the voter alone, `room` -- if there
/// is anything the room should hear -- to the game's channel, naming nobody.
type GpVoteAnswer = (CrackedMessage, Option<CrackedMessage>);

/// Answer a vote. The voter's confirmation is ephemeral, the same as
/// `/gp submit`'s: a slash reply lands under "*name* used `/gp voteskip`", and
/// who wants a song gone is nobody's business in a game whose whole premise is
/// that people submitted something embarrassing. What the room is told goes to
/// the game's channel as a plain message, so the last voter is not named on
/// that either.
async fn gp_answer_vote(ctx: Context<'_>, (mine, room): GpVoteAnswer) -> Result<(), Error> {
    ctx.send_message(SendMessageParams::new(mine).with_ephemeral(true))
        .await?;
    let Some(room) = room else {
        return Ok(());
    };
    // The vote may have ended the game's last song; then there is no game and
    // nothing to tell anyone.
    let Some(channel) = ctx
        .guild_id()
        .and_then(|guild_id| ctx.data().gp_text_channel(guild_id))
    else {
        return Ok(());
    };
    channel
        .send_message(
            &ctx.serenity_context().http,
            CreateMessage::new().content(room.to_string()),
        )
        .await?;
    Ok(())
}

/// Vote to end the current song early -- a majority of the voice channel ends it.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "voteskip",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_voteskip(ctx: Context<'_>) -> Result<(), Error> {
    // Errors are answered here, ephemerally, rather than through the framework's
    // public error reply: "you already voted to skip this" names the voter as
    // surely as the vote would have.
    let answer = match gp_voteskip_internal(ctx).await {
        Ok(a) => a,
        Err(e) => (CrackedMessage::CrackedError(e), None),
    };
    gp_answer_vote(ctx, answer).await
}

#[cfg(not(tarpaulin_include))]
async fn gp_voteskip_internal(ctx: Context<'_>) -> CrackedResult<GpVoteAnswer> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_player(ctx, guild_id)?;
    let vc_members = gp_vc_members(ctx, game_vc);
    let name = author_display_name(ctx).await;
    let outcome = data.gp_vote_skip(guild_id, ctx.author().id, name, &vc_members)?;
    let answer = match outcome {
        GpVoteSkipOutcome::Counted { votes, needed } => (
            CrackedMessage::GpVoteSkipCounted { votes, needed },
            Some(CrackedMessage::GpVoteSkipRoom { needed }),
        ),
        GpVoteSkipOutcome::Passed => (
            CrackedMessage::GpVoteSkipCarried,
            Some(CrackedMessage::GpVoteSkipPassed),
        ),
        GpVoteSkipOutcome::OwnSong => (
            CrackedMessage::GpVoteSkipOwnSong,
            Some(CrackedMessage::GpVoteSkipPulled),
        ),
    };
    if !matches!(outcome, GpVoteSkipOutcome::Counted { .. }) {
        let call = data
            .songbird
            .get(guild_id)
            .ok_or(CrackedError::NotConnected)?;
        let handler = call.lock().await;
        if handler.queue().is_empty() {
            return Err(CrackedError::NothingPlaying);
        }
        // stop() fires TrackEvent::End, which is what advances the game.
        force_skip_top_track(&handler).await?;
    }
    Ok(answer)
}

/// Vote to hear the whole song instead of just the clip -- a majority lets it run.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "votefull",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_votefull(ctx: Context<'_>) -> Result<(), Error> {
    // Same split as `/gp voteskip`, for the same reason: one pattern, not two.
    let answer = match gp_votefull_internal(ctx).await {
        Ok(a) => a,
        Err(e) => (CrackedMessage::CrackedError(e), None),
    };
    gp_answer_vote(ctx, answer).await
}

#[cfg(not(tarpaulin_include))]
async fn gp_votefull_internal(ctx: Context<'_>) -> CrackedResult<GpVoteAnswer> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_player(ctx, guild_id)?;
    let vc_members = gp_vc_members(ctx, game_vc);
    let name = author_display_name(ctx).await;
    Ok(
        match data.gp_vote_full(guild_id, ctx.author().id, name, &vc_members)? {
            GpVoteFullOutcome::Counted { votes, needed } => (
                CrackedMessage::GpVoteFullCounted { votes, needed },
                Some(CrackedMessage::GpVoteFullRoom { needed }),
            ),
            // Nothing to do to the track: the clip timer checks `play_full` before
            // it stops anything, so letting it run on is simply not stopping it.
            GpVoteFullOutcome::Passed => (
                CrackedMessage::GpVoteFullCarried,
                Some(CrackedMessage::GpVoteFullPassed),
            ),
            // Already carried by an earlier vote: the room heard about it then.
            GpVoteFullOutcome::AlreadyFull => (CrackedMessage::GpVoteFullAlready, None),
        },
    )
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
    gp_require_player(ctx, guild_id)?;
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
    // Park and stop, but do not remove: `stop()` only queues the `End`, so removing
    // here would beat it to the map and hand the event to autoplay. The global
    // track-end handler collects the parked game when the `End` actually lands.
    let game = data.gp_park_for_end(guild_id, ctx.author().id, is_admin)?;
    let was_playing = match data.songbird.get(guild_id) {
        Some(call) => {
            let handler = call.lock().await;
            let playing = !handler.queue().is_empty();
            handler.queue().stop();
            playing
        },
        None => false,
    };
    if was_playing {
        // If that `End` never reaches the handler, the parked game would outlive
        // the command and keep the guild's music commands blocked.
        let data = data.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(GP_PARK_GRACE_SECS)).await;
            if data.gp_remove_if_parked(guild_id) {
                tracing::warn!("gp: parked game in {guild_id} was never collected, removing");
            }
        });
    } else {
        // Nothing was playing, so no `End` is coming to collect it.
        data.gp_remove(guild_id);
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
    /// Never submits anything, so never a player.
    const D: UserId = UserId::new(400);
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

    /// A clip that never fires in the pure-state tests, but proves the setting is
    /// carried from `/gp start` all the way to `GpTrackStart`.
    fn clip() -> GpClip {
        GpClip {
            start: Duration::from_secs(GP_DEFAULT_CLIP_START_SECS),
            length: Duration::from_secs(GP_DEFAULT_CLIP_LENGTH_SECS),
        }
    }

    /// A game hosted by alice with the given prompts; round 0's window is open.
    fn game_with(data: &Data, prompt_list: &[&str]) -> GpWindowOpened {
        game_with_clip(data, prompt_list, None)
    }

    /// As [`game_with`], with an explicit clip setting.
    fn game_with_clip(data: &Data, prompt_list: &[&str], clip: Option<GpClip>) -> GpWindowOpened {
        game_with_reveal(data, prompt_list, clip, GpReveal::Song)
    }

    /// As [`game_with_clip`], with an explicit reveal setting.
    fn game_with_reveal(
        data: &Data,
        prompt_list: &[&str],
        clip: Option<GpClip>,
        reveal: GpReveal,
    ) -> GpWindowOpened {
        data.gp_start(
            G,
            A,
            "alice".into(),
            VC,
            TC,
            GpCategory::Nostalgia,
            prompts(prompt_list),
            TIMER,
            clip,
            reveal,
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
                None,
                GpReveal::Song,
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
                None,
                GpReveal::Song,
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
        assert!(data.gp_is_active(G));
        // No window to close any more.
        assert_eq!(
            data.gp_close_window(G, A, &mut rng(), NOW).unwrap_err(),
            CrackedError::WindowClosed
        );
        data.gp_remove(G);
        assert!(!data.gp_is_active(G));
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
        // Round 0 gets both of them in, so bob is a player for round 1 even
        // though he sits that one out.
        game_with(&data, &["p1", "p2"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();

        submit(&data, A, "alice", "a2");
        let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let GpNext::Track(start) = &closed.next else {
            unreachable!()
        };
        assert!(!start.guessable);
        assert_eq!(start.total_tracks, 1);
        // No dropdown, so no guessing -- even for a player.
        assert_eq!(
            data.gp_record_guess(G, 1, 0, B, "bob".into(), A)
                .unwrap_err(),
            CrackedError::NotGuessable
        );
        // Watching is still not playing.
        assert_eq!(
            data.gp_toggle_like(G, 1, 0, D, "dave".into()).unwrap_err(),
            CrackedError::NotAGamePlayer
        );
        // ...but a player's likes still count.
        assert_eq!(
            data.gp_toggle_like(G, 1, 0, B, "bob".into()).unwrap(),
            GpLikeOutcome::Liked(1)
        );
        let before = game(&data).scores.get(&A).copied().unwrap_or(0);
        let res = data.gp_reveal_and_advance(G, 1, 0, NOW).unwrap();
        assert!(!res.guessable);
        assert!(!res.fooled_everyone);
        assert!(res.correct.is_empty());
        assert_eq!(res.likes, 1);
        assert_eq!(
            game(&data).scores.get(&A),
            Some(&(before + GP_POINTS_PER_LIKE))
        );
        assert!(matches!(res.next, GpNext::Finished(_)));
    }

    /// Submitting is what makes someone a player, and it sticks for the rest of
    /// the game: sitting a round out does not put them back outside it.
    #[test]
    fn membership_is_earned_by_submitting_and_sticks() {
        let data = data();
        game_with(&data, &["p1", "p2"]);
        // The host counts before submitting so they can watch their own game...
        assert!(data.gp_require_player(G, A).is_ok());
        // ...but nobody else does.
        assert_eq!(
            data.gp_require_player(G, B).unwrap_err(),
            CrackedError::NotAGamePlayer
        );
        submit(&data, B, "bob", "b");
        assert!(data.gp_require_player(G, B).is_ok());
        assert_eq!(
            data.gp_require_player(G, D).unwrap_err(),
            CrackedError::NotAGamePlayer
        );

        // Play round 0 out; bob submits nothing in round 1 but stays a player.
        submit(&data, A, "alice", "a");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
        assert_eq!(game(&data).current_round, 1);
        assert!(data.gp_require_player(G, B).is_ok());
        assert_eq!(
            data.gp_require_player(G, D).unwrap_err(),
            CrackedError::NotAGamePlayer
        );
    }

    #[test]
    fn votes_required_is_a_majority() {
        // Strict majority of the eligible voters (everyone in the channel bar the
        // song's submitter), so one player can never skip for the whole room.
        assert_eq!(gp_votes_required(4), 3);
        assert_eq!(gp_votes_required(3), 2);
        assert_eq!(gp_votes_required(2), 2);
        assert_eq!(gp_votes_required(1), 1);
        // An empty or uncached voice channel must not make zero votes enough.
        assert_eq!(gp_votes_required(0), 1);
    }

    #[test]
    fn vote_skip_needs_a_majority() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");

        // Nothing is playing yet.
        assert_eq!(
            data.gp_vote_skip(G, A, "alice".into(), &[A, B, C])
                .unwrap_err(),
            CrackedError::GameNotPlaying
        );

        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let vc = [A, B, C];
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        let voters: Vec<UserId> = vc.iter().copied().filter(|u| *u != s0).collect();

        // Watching is not playing.
        assert_eq!(
            data.gp_vote_skip(G, D, "dave".into(), &vc).unwrap_err(),
            CrackedError::NotAGamePlayer
        );
        // The submitter does not vote on their own song -- they pull it, and the
        // pull is not recorded as a vote.
        assert_eq!(
            data.gp_vote_skip(G, s0, "self".into(), &vc).unwrap(),
            GpVoteSkipOutcome::OwnSong
        );
        assert!(game(&data).rounds[0].tracks[0].skip_votes.is_empty());
        // Pulling is idempotent: it never trips the already-voted guard.
        assert_eq!(
            data.gp_vote_skip(G, s0, "self".into(), &vc).unwrap(),
            GpVoteSkipOutcome::OwnSong
        );

        // The submitter is out of the pool, so the other two carry the vote.
        assert_eq!(
            data.gp_vote_skip(G, voters[0], "v0".into(), &vc).unwrap(),
            GpVoteSkipOutcome::Counted {
                votes: 1,
                needed: 1
            }
        );
        // Voting twice does not carry the vote.
        assert_eq!(
            data.gp_vote_skip(G, voters[0], "v0".into(), &vc)
                .unwrap_err(),
            CrackedError::AlreadyVotedSkip
        );
        assert_eq!(
            data.gp_vote_skip(G, voters[1], "v1".into(), &vc).unwrap(),
            GpVoteSkipOutcome::Passed
        );

        // Votes belong to the song, so the next one starts clean.
        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert!(matches!(res.next, GpNext::Track(_)));
        assert!(game(&data).rounds[0].tracks[1].skip_votes.is_empty());
        let s1 = game(&data).rounds[0].tracks[1].submitter;
        let next_voter = vc.iter().copied().find(|u| *u != s1).unwrap();
        assert_eq!(
            data.gp_vote_skip(G, next_voter, "v".into(), &vc).unwrap(),
            GpVoteSkipOutcome::Counted {
                votes: 1,
                needed: 1
            }
        );
    }

    /// `/gp end` leaves the game in the map on purpose: `stop()` only queues the
    /// `End`, so removing it there would beat the event and hand it to autoplay.
    /// Collection is the track-end handler's job, and only a *parked* game is its
    /// to collect.
    #[test]
    fn parked_game_is_collected_by_the_track_end_and_not_before() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();

        // A game that is merely running is not anyone's to collect.
        assert!(!data.gp_remove_if_parked(G));
        assert!(data.gp_is_active(G));

        data.gp_park_for_end(G, A, false).unwrap();
        // Still present, which is the whole point -- the global handler has to see
        // a game when the End that stop() queued finally lands.
        assert!(data.gp_is_active(G));
        // ...and inert, so the game's own handlers do not reveal or advance on it.
        assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());

        assert!(data.gp_remove_if_parked(G));
        assert!(!data.gp_is_active(G));
        // Idempotent: a second End, or the command's backstop, finds nothing.
        assert!(!data.gp_remove_if_parked(G));
    }

    /// A dead link and a stream that dies part-way through are not the same thing.
    /// Only the first reached nobody, and only the first should skip the scoring.
    #[test]
    fn never_played_needs_errored_and_too_little_play_time() {
        use songbird::tracks::PlayMode;
        let errored = |play_time| TrackState {
            playing: PlayMode::Errored(songbird::tracks::PlayError::Create(Arc::new(
                songbird::input::AudioStreamError::Unsupported,
            ))),
            play_time,
            ..Default::default()
        };
        // Preparing -> Errored without mixing a frame: nobody heard it.
        assert!(never_played(&errored(Duration::ZERO), None));
        // Died 200ms in: a dead link as far as the room is concerned. Scoring it
        // would hand the submitter the fooled-everyone bonus for a song nobody
        // could have guessed.
        assert!(never_played(&errored(Duration::from_millis(200)), None));
        // Either side of the line.
        assert!(never_played(
            &errored(GP_MIN_PLAYED - Duration::from_millis(1)),
            None
        ));
        assert!(!never_played(&errored(GP_MIN_PLAYED), None));
        // Died two minutes in: the room heard it, so it scores like any other song.
        assert!(!never_played(&errored(Duration::from_secs(120)), None));
        // A 45s clip is judged against its own length, not the flat thirty: it
        // needs 22.5s, so 25s counts as heard where a whole song would not.
        let clip45 = Some(Duration::from_secs(45));
        assert!(!never_played(&errored(Duration::from_secs(25)), clip45));
        assert!(never_played(&errored(Duration::from_secs(20)), clip45));
        // A song that simply finished is not a failure at any play time.
        assert!(!never_played(
            &TrackState {
                playing: PlayMode::End,
                play_time: Duration::ZERO,
                ..Default::default()
            },
            None
        ));
        assert!(!never_played(&TrackState::default(), None));
    }

    /// A vote to hear more of a song is first-hand evidence it was playing, and
    /// outranks whatever the play time says afterwards. A skip vote is not: a dead
    /// link is silent, and silence is what makes people reach for `/gp voteskip`.
    #[test]
    fn a_full_song_vote_beats_the_played_threshold() {
        let data = data();
        game_with_clip(&data, &["p1"], Some(clip()));
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let vc = [A, B, C];
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        let voter = vc.iter().copied().find(|u| *u != s0).unwrap();

        assert!(!data.gp_heard_by_vote(G, 0, 0));

        // A skip vote is not evidence of anything being audible.
        let skipper = vc
            .iter()
            .copied()
            .find(|u| *u != s0 && *u != voter)
            .unwrap();
        data.gp_vote_skip(G, skipper, "s".into(), &vc).unwrap();
        assert!(!data.gp_heard_by_vote(G, 0, 0));

        // One vote for more of it is, even before the vote carries.
        assert_eq!(
            data.gp_vote_full(G, voter, "v".into(), &vc).unwrap(),
            GpVoteFullOutcome::Counted {
                votes: 1,
                needed: 1
            }
        );
        assert!(data.gp_heard_by_vote(G, 0, 0));
        assert!(!data.gp_plays_full(G, 0, 0), "not carried yet");

        // Out of range, and an absent game, are not evidence either.
        assert!(!data.gp_heard_by_vote(G, 0, 99));
        assert!(!data.gp_heard_by_vote(GuildId::new(9), 0, 0));
    }

    /// A submitter gets the same answer whether or not the song has already been
    /// voted up: it is never theirs to vote on.
    #[test]
    fn vote_full_tells_the_submitter_the_same_thing_either_way() {
        let data = data();
        game_with_clip(&data, &["p1"], Some(clip()));
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let vc = [A, B, C];
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        let voters: Vec<UserId> = vc.iter().copied().filter(|u| *u != s0).collect();

        assert_eq!(
            data.gp_vote_full(G, s0, "self".into(), &vc).unwrap_err(),
            CrackedError::CannotVoteOwnSongFull
        );
        // Carry it, then ask again: still their own song, still the same answer.
        data.gp_vote_full(G, voters[0], "v0".into(), &vc).unwrap();
        data.gp_vote_full(G, voters[1], "v1".into(), &vc).unwrap();
        assert!(data.gp_plays_full(G, 0, 0));
        assert_eq!(
            data.gp_vote_full(G, s0, "self".into(), &vc).unwrap_err(),
            CrackedError::CannotVoteOwnSongFull
        );
    }

    /// A clip has to fit the song. Seeking past the end comes back as an immediate
    /// `End`, which the game would read as a dead link and score nobody for.
    #[test]
    fn clip_fits_itself_to_the_song() {
        let c = GpClip {
            start: Duration::from_secs(30),
            length: Duration::from_secs(45),
        };
        let secs = |s| Some(Duration::from_secs(s));

        // Comfortably long enough: untouched.
        assert_eq!(c.for_duration(secs(240)), c);
        // Exactly long enough: still untouched.
        assert_eq!(c.for_duration(secs(75)), c);
        // Unknown duration: trust the offset, nothing better to go on.
        assert_eq!(c.for_duration(None), c);

        // Too short for start+length: take the last `length` instead of seeking
        // past the end.
        assert_eq!(
            c.for_duration(secs(60)),
            GpClip {
                start: Duration::from_secs(15),
                length: Duration::from_secs(45)
            }
        );
        // Shorter than the clip itself: play all of it, from the top.
        assert_eq!(
            c.for_duration(secs(20)),
            GpClip {
                start: Duration::ZERO,
                length: Duration::from_secs(20)
            }
        );
    }

    /// The "did the room hear it" bar has to scale with what was meant to play, or
    /// a clip that ran to its end is scored as a dead link.
    #[test]
    fn min_played_scales_with_the_intended_length() {
        // A whole song keeps the flat thirty seconds.
        assert_eq!(gp_min_played(Some(Duration::from_secs(240))), GP_MIN_PLAYED);
        assert_eq!(gp_min_played(None), GP_MIN_PLAYED);
        // A 45s clip needs 22.5s, not 30 -- which would be most of the clip.
        assert_eq!(
            gp_min_played(Some(Duration::from_secs(45))),
            Duration::from_millis(22_500)
        );
        // A 20s clip needs 10s. Under the old absolute rule it could never clear
        // the bar at all, so every short clip scored nobody.
        assert_eq!(
            gp_min_played(Some(Duration::from_secs(20))),
            Duration::from_secs(10)
        );
    }

    /// Voting a song up to full length: same pool as voteskip, but the submitter
    /// is barred outright rather than given a pull -- it is their own bonus.
    #[test]
    fn vote_full_carries_on_a_majority() {
        let data = data();
        game_with_clip(&data, &["p1"], Some(clip()));
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let vc = [A, B, C];
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        let voters: Vec<UserId> = vc.iter().copied().filter(|u| *u != s0).collect();

        assert!(!data.gp_plays_full(G, 0, 0));
        // Not a player, and not the submitter's to vote for.
        assert_eq!(
            data.gp_vote_full(G, D, "dave".into(), &vc).unwrap_err(),
            CrackedError::NotAGamePlayer
        );
        assert_eq!(
            data.gp_vote_full(G, s0, "self".into(), &vc).unwrap_err(),
            CrackedError::CannotVoteOwnSongFull
        );

        assert_eq!(
            data.gp_vote_full(G, voters[0], "v0".into(), &vc).unwrap(),
            GpVoteFullOutcome::Counted {
                votes: 1,
                needed: 1
            }
        );
        assert_eq!(
            data.gp_vote_full(G, voters[0], "v0".into(), &vc)
                .unwrap_err(),
            CrackedError::AlreadyVotedFull
        );
        assert!(!data.gp_plays_full(G, 0, 0));

        assert_eq!(
            data.gp_vote_full(G, voters[1], "v1".into(), &vc).unwrap(),
            GpVoteFullOutcome::Passed
        );
        // The clip timer asks this before it stops anything.
        assert!(data.gp_plays_full(G, 0, 0));
        assert_eq!(
            data.gp_vote_full(G, voters[0], "v0".into(), &vc).unwrap(),
            GpVoteFullOutcome::AlreadyFull
        );

        // The submitter is paid for it at the reveal.
        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert!(res.played_full);
        let scored = game(&data).scores.get(&s0).copied().unwrap_or(0);
        assert!(
            scored >= GP_POINTS_FULL_SONG,
            "submitter should have the full-song bonus, got {scored}"
        );
    }

    /// A game already playing whole songs has nothing to vote up.
    #[test]
    fn vote_full_needs_a_game_playing_clips() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        assert_eq!(
            data.gp_vote_full(G, A, "alice".into(), &[A, B])
                .unwrap_err(),
            CrackedError::NotPlayingClips
        );
    }

    /// The clip setting has to survive from `/gp start` to the song that plays.
    #[test]
    fn clip_setting_reaches_the_track() {
        let data = data();
        game_with_clip(&data, &["p1"], Some(clip()));
        submit(&data, A, "alice", "a");
        let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let GpNext::Track(start) = &closed.next else {
            unreachable!()
        };
        assert_eq!(start.clip, Some(clip()));
        // The timer keys off this, the same way the window timer does.
        assert_eq!(start.generation, game(&data).generation);
        assert!(data.gp_clip_still_current(G, start.generation, 0, 0));
        assert!(!data.gp_clip_still_current(G, start.generation + 1, 0, 0));
        assert!(!data.gp_clip_still_current(G, start.generation, 0, 1));
    }

    /// The pool the majority is measured against has to be the people who may
    /// actually vote. Counting a lurker sets a bar the eligible voters cannot
    /// clear, and the song becomes unskippable -- in exactly the mixed channel
    /// that submitters-only voting creates.
    #[test]
    fn vote_skip_pool_counts_players_not_bystanders() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        let other = if s0 == A { B } else { A };

        // Three in the channel, two of them players, the song belongs to one of
        // those two: D cannot vote, so the one eligible voter has to be enough.
        assert_eq!(
            data.gp_vote_skip(G, D, "dave".into(), &[A, B, D])
                .unwrap_err(),
            CrackedError::NotAGamePlayer
        );
        assert_eq!(
            data.gp_vote_skip(G, other, "other".into(), &[A, B, D])
                .unwrap(),
            GpVoteSkipOutcome::Passed
        );
    }

    /// Excluding the submitter from the pool is what keeps a song skippable: a
    /// two-person channel would otherwise need two votes with only one eligible
    /// voter, and the song could never be voted off.
    #[test]
    fn vote_skip_excludes_the_submitter_from_the_pool() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        let other = if s0 == A { B } else { A };
        // Two in the channel, one of them the submitter: the other one decides.
        assert_eq!(
            data.gp_vote_skip(G, other, "other".into(), &[A, B])
                .unwrap(),
            GpVoteSkipOutcome::Passed
        );
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
            data.gp_record_guess(G, 1, 0, other, "other".into(), A)
                .unwrap_err(),
            CrackedError::StaleRound
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 1, other, "other".into(), A)
                .unwrap_err(),
            CrackedError::StaleRound
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 1, other, "other".into())
                .unwrap_err(),
            CrackedError::StaleRound
        );
        // Watching is not playing: no guessing and no 👍 without a song in.
        assert_eq!(
            data.gp_record_guess(G, 0, 0, D, "dave".into(), s0)
                .unwrap_err(),
            CrackedError::NotAGamePlayer
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, D, "dave".into()).unwrap_err(),
            CrackedError::NotAGamePlayer
        );
        // Only submitters are valid answers.
        assert_eq!(
            data.gp_record_guess(G, 0, 0, other, "other".into(), D)
                .unwrap_err(),
            CrackedError::NotAPlayer
        );
        // A guess can be changed until the song ends.
        assert_eq!(
            data.gp_record_guess(G, 0, 0, other, "other".into(), other)
                .unwrap(),
            GpGuessOutcome::Recorded
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 0, other, "other".into(), s0)
                .unwrap(),
            GpGuessOutcome::Changed
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 0, other, "other".into(), s0)
                .unwrap(),
            GpGuessOutcome::Recorded
        );
        // The submitter may pick on their own song; it never scores.
        data.gp_record_guess(G, 0, 0, s0, "self".into(), s0)
            .unwrap();
        // Likes: own song rejected, everyone else toggles.
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, s0, "self".into()).unwrap_err(),
            CrackedError::CannotLikeOwnSong
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
            GpLikeOutcome::Liked(1)
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
            GpLikeOutcome::Unliked(0)
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
            GpLikeOutcome::Liked(1)
        );

        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert_eq!(res.submitter, s0);
        assert_eq!(res.correct, vec![other]);
        assert!(!res.fooled_everyone);
        assert_eq!(res.likes, 1);
        assert!(res.guessable);
        assert!(matches!(res.next, GpNext::Track(_)));
        let g = game(&data);
        assert_eq!(g.scores.get(&other), Some(&GP_POINTS_CORRECT));
        assert_eq!(g.scores.get(&s0), Some(&GP_POINTS_PER_LIKE));
        assert_eq!(g.current_track, 1);

        // Second call for the same song is a no-op.
        assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());
        // Guessing on the finished song is stale now.
        assert_eq!(
            data.gp_record_guess(G, 0, 0, other, "other".into(), s0)
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
        assert_eq!(
            g.scores.get(&s1),
            Some(&(GP_POINTS_CORRECT + GP_POINTS_FOOLED_ALL))
        );
        assert_eq!(
            data.gp_record_guess(G, 0, 1, other, "other".into(), A)
                .unwrap_err(),
            CrackedError::GameNotPlaying
        );
        assert_eq!(
            data.gp_toggle_like(G, 0, 1, other, "other".into())
                .unwrap_err(),
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
            data.gp_park_for_end(G, B, false).unwrap_err(),
            CrackedError::NotGameHost
        );
        // Parking hands back the game as it was, but leaves it in the map so the
        // caller can stop playback before the global handler stops seeing a game.
        let g = data.gp_park_for_end(G, C, true).unwrap();
        assert_eq!(g.host, A);
        assert_eq!(g.phase, GpPhase::Submitting);
        assert!(data.gp_is_active(G));
        // Parked, the game's own handlers and timers are already inert.
        assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());
        assert!(data
            .gp_close_window_if(G, g.generation, &mut rng(), NOW)
            .is_none());
        assert!(data.gp_warning_if(G, g.generation).is_none());

        assert!(data.gp_remove(G).is_some());
        assert!(!data.gp_is_active(G));
        assert_eq!(
            data.gp_park_for_end(G, A, false).unwrap_err(),
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
        assert!(!data.gp_is_active(G));
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
        submit(&data, C, "carol", "c");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        // Carol guesses every song but her own right, so alice and bob are
        // never fooled and tie at 0; carol takes 100 a guess plus the fooled
        // bonus for the song nobody pinned on her.
        for i in 0..3 {
            let s = game(&data).rounds[0].tracks[i].submitter;
            if s != C {
                data.gp_record_guess(G, 0, i, C, "carol".into(), s).unwrap();
            }
            data.gp_reveal_and_advance(G, 0, i, NOW).unwrap();
        }
        assert_eq!(
            game(&data).sorted_scores(),
            vec![
                (C, 2 * GP_POINTS_CORRECT + GP_POINTS_FOOLED_ALL),
                (A, 0),
                (B, 0)
            ]
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
        submit(&data, C, "carol", "c");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        // Carol is a player, so she may guess and 👍 -- unless it is her song.
        let liker = if s0 == C { A } else { C };
        data.gp_record_guess(G, 0, 0, liker, "carol".into(), A)
            .unwrap();
        data.gp_toggle_like(G, 0, 0, liker, "carol".into()).unwrap();
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
                assert_eq!((round, total, track, tracks), (1, 2, 1, 3));
                assert_eq!(prompt, "p1");
                assert_eq!(guessed, vec!["carol".to_string()]);
                assert_eq!(likes, 1);
                assert_eq!(scores.len(), 3);
            },
            other => panic!("expected playing, got {other:?}"),
        }
    }

    /// Play the current round out with no guesses, so the game moves on to the
    /// next window (or finishes). Returns the play order it had.
    fn play_out(data: &Data, round_idx: usize) -> Vec<UserId> {
        let order: Vec<UserId> = game(data).rounds[round_idx]
            .tracks
            .iter()
            .map(|t| t.submitter)
            .collect();
        for i in 0..order.len() {
            data.gp_reveal_and_advance(G, round_idx, i, NOW).unwrap();
        }
        order
    }

    /// The order of one round must never repeat the order of the round before
    /// it -- stronger, nobody may keep the slot they had -- or the position in
    /// the round says whose song it is before a note has played (#450).
    #[test]
    fn nobody_keeps_their_slot_from_one_round_to_the_next() {
        let players = [(A, "alice"), (B, "bob"), (C, "carol")];
        for seed in 0..40u64 {
            let data = data();
            game_with(&data, &["p0", "p1", "p2", "p3"]);
            let mut rng = StdRng::seed_from_u64(seed);
            let mut previous: Option<Vec<UserId>> = None;
            for round in 0..4 {
                for (id, name) in players {
                    submit(&data, id, name, &format!("{name}{round}"));
                }
                data.gp_close_window(G, A, &mut rng, NOW).unwrap();
                let order = play_out(&data, round);
                if let Some(prev) = &previous {
                    for (slot, (now, then)) in order.iter().zip(prev).enumerate() {
                        assert_ne!(
                            now, then,
                            "seed {seed}, round {round}: slot {slot} kept between rounds"
                        );
                    }
                }
                previous = Some(order);
            }
        }
    }

    /// A round nobody submitted to has no order; the constraint reaches back
    /// past it to the last round that was actually played.
    #[test]
    fn the_previous_order_skips_an_empty_round() {
        for seed in 0..40u64 {
            let data = data();
            game_with(&data, &["p0", "p1", "p2"]);
            let mut rng = StdRng::seed_from_u64(seed);
            for (id, name) in [(A, "alice"), (B, "bob"), (C, "carol")] {
                submit(&data, id, name, name);
            }
            data.gp_close_window(G, A, &mut rng, NOW).unwrap();
            let first = play_out(&data, 0);
            // Round 1: nobody submits, straight on to round 2's window.
            let closed = data.gp_close_window(G, A, &mut rng, NOW).unwrap();
            assert!(matches!(closed.next, GpNext::Window(_)));
            for (id, name) in [(A, "alice"), (B, "bob"), (C, "carol")] {
                submit(&data, id, name, name);
            }
            data.gp_close_window(G, A, &mut rng, NOW).unwrap();
            let third: Vec<UserId> = game(&data).rounds[2]
                .tracks
                .iter()
                .map(|t| t.submitter)
                .collect();
            assert!(
                first.iter().zip(&third).all(|(a, b)| a != b),
                "seed {seed}: {first:?} then {third:?}"
            );
        }
    }

    /// Two songs have two orders, and forbidding the repeat would leave one:
    /// the rounds would alternate, which is a tell of its own. So two-song
    /// rounds are a plain shuffle, and over enough seeds one of them repeats.
    #[test]
    fn two_song_rounds_are_not_deranged() {
        let mut repeats = 0;
        for seed in 0..40u64 {
            let data = data();
            game_with(&data, &["p0", "p1"]);
            let mut rng = StdRng::seed_from_u64(seed);
            submit(&data, A, "alice", "a0");
            submit(&data, B, "bob", "b0");
            data.gp_close_window(G, A, &mut rng, NOW).unwrap();
            let first = play_out(&data, 0);
            submit(&data, A, "alice", "a1");
            submit(&data, B, "bob", "b1");
            data.gp_close_window(G, A, &mut rng, NOW).unwrap();
            let second: Vec<UserId> = game(&data).rounds[1]
                .tracks
                .iter()
                .map(|t| t.submitter)
                .collect();
            if first == second {
                repeats += 1;
            }
        }
        assert!(repeats > 0, "a two-song round should be free to repeat");
        assert!(repeats < 40, "and free not to");
    }

    /// The constraint only reaches as far as the previous order does: slots the
    /// previous round did not have, and players who were not in it, are free.
    #[test]
    fn shuffle_against_constrains_only_the_slots_it_can() {
        let mut rng = rng();
        let ids: Vec<UserId> = (1..=5).map(UserId::new).collect();
        // A shorter previous round: only its slots are constrained.
        for _ in 0..200 {
            let mut items: Vec<(UserId, ())> = ids.iter().map(|id| (*id, ())).collect();
            shuffle_against(&mut items, &ids[..2], &mut rng);
            assert_ne!(items[0].0, ids[0]);
            assert_ne!(items[1].0, ids[1]);
        }
        // A previous round of strangers constrains nothing, and every order is
        // still reachable.
        let strangers: Vec<UserId> = (100..=104).map(UserId::new).collect();
        let mut items: Vec<(UserId, ())> = ids.iter().map(|id| (*id, ())).collect();
        shuffle_against(&mut items, &strangers, &mut rng);
        let mut seen: Vec<UserId> = items.iter().map(|(id, _)| *id).collect();
        seen.sort_unstable();
        assert_eq!(seen, ids);
        // No previous round at all: a plain shuffle.
        let mut items: Vec<(UserId, ())> = ids.iter().map(|id| (*id, ())).collect();
        shuffle_against(&mut items, &[], &mut rng);
        assert_eq!(items.len(), 5);
        // Fewer than the minimum: unconstrained even against the same players.
        let mut same = 0;
        for _ in 0..100 {
            let mut items: Vec<(UserId, ())> = ids[..2].iter().map(|id| (*id, ())).collect();
            shuffle_against(&mut items, &ids[..2], &mut rng);
            if items[0].0 == ids[0] {
                same += 1;
            }
        }
        assert!(same > 0 && same < 100);
    }

    /// One scoring rule, derived from the song as it stands, so the reveal, the
    /// round's results and the held-back scoreboard cannot disagree.
    #[test]
    fn a_song_scores_from_what_the_room_did_to_it() {
        let mut t = GpTrack::new(A, track("a"));
        // Nothing happened: nobody guessed, so the submitter fooled everyone.
        assert_eq!(
            t.score(true),
            GpTrackScore {
                correct: vec![],
                fooled_everyone: true,
                points: vec![(A, GP_POINTS_FOOLED_ALL)],
            }
        );
        // Two right guesses (in a stable order), one wrong, the submitter's own
        // pick ignored; two likes; voted up to full length.
        t.guesses.insert(C, A);
        t.guesses.insert(B, A);
        t.guesses.insert(D, B);
        t.guesses.insert(A, A);
        t.likes.insert(B);
        t.likes.insert(C);
        t.play_full = true;
        assert_eq!(
            t.score(true),
            GpTrackScore {
                correct: vec![B, C],
                fooled_everyone: false,
                points: vec![
                    (B, GP_POINTS_CORRECT),
                    (C, GP_POINTS_CORRECT),
                    (A, 2 * GP_POINTS_PER_LIKE + GP_POINTS_FULL_SONG),
                ],
            }
        );
        // A one-song round: guesses and the fooled bonus are off, the rest stands.
        assert_eq!(
            t.score(false),
            GpTrackScore {
                correct: vec![],
                fooled_everyone: false,
                points: vec![(A, 2 * GP_POINTS_PER_LIKE + GP_POINTS_FULL_SONG)],
            }
        );
        // A song that never played pays nobody, whatever was cast on it.
        t.failed = true;
        assert_eq!(t.score(true), GpTrackScore::default());
    }

    /// The round's last reveal carries the round summed up: every song, what
    /// the round paid, and the board after it. Earlier reveals carry nothing.
    #[test]
    fn the_last_song_of_a_round_carries_the_rounds_results() {
        let data = data();
        game_with(&data, &["p1", "p2"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let order: Vec<UserId> = game(&data).rounds[0]
            .tracks
            .iter()
            .map(|t| t.submitter)
            .collect();
        let before = game(&data).sorted_scores();

        // Song 0: everyone else guesses it, one like. Song 1: nobody does.
        // Song 2: never played.
        for u in [A, B, C].into_iter().filter(|u| *u != order[0]) {
            data.gp_record_guess(G, 0, 0, u, "n".into(), order[0])
                .unwrap();
        }
        let liker = [A, B, C].into_iter().find(|u| *u != order[0]).unwrap();
        data.gp_toggle_like(G, 0, 0, liker, "n".into()).unwrap();
        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert!(res.round.is_none(), "not the last song");
        assert!(!res.held);
        let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
        assert!(res.round.is_none());
        let res = data.gp_fail_and_advance(G, 0, 2, NOW).unwrap();
        let round = res.round.expect("the last song carries the results");
        assert!(matches!(res.next, GpNext::Window(_)));

        assert_eq!((round.round_idx, round.total_rounds), (0, 2));
        assert_eq!(round.prompt, "p1");
        assert!(round.guessable);
        assert_eq!(round.songs.len(), 3);
        let s0 = &round.songs[0];
        assert_eq!(s0.submitter, order[0]);
        let mut guessers: Vec<UserId> = [A, B, C].into_iter().filter(|u| *u != order[0]).collect();
        guessers.sort_unstable();
        assert_eq!(s0.correct, guessers);
        assert!(!s0.fooled_everyone);
        assert_eq!(s0.likes, 1);
        assert!(!s0.failed);
        let s1 = &round.songs[1];
        assert_eq!(s1.submitter, order[1]);
        assert!(s1.correct.is_empty());
        assert!(s1.fooled_everyone);
        let s2 = &round.songs[2];
        assert!(s2.failed);
        assert!(!s2.fooled_everyone, "an unheard song fools nobody");

        // What the round paid is exactly the change in the totals.
        let after = game(&data).sorted_scores();
        let paid: HashMap<UserId, u32> = round.points.iter().copied().collect();
        for (id, total) in &after {
            let was = before
                .iter()
                .find(|(i, _)| i == id)
                .map(|(_, p)| *p)
                .unwrap_or(0);
            assert_eq!(total - was, paid.get(id).copied().unwrap_or(0), "{id}");
        }
        assert!(
            round.points.iter().all(|(_, p)| *p > 0),
            "only players who took something"
        );
        assert_eq!(round.points[0].1, GP_POINTS_CORRECT + GP_POINTS_FOOLED_ALL);
        assert_eq!(round.scores, after);

        // The last round's results come with the finish.
        submit(&data, A, "alice", "a2");
        submit(&data, B, "bob", "b2");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        data.gp_reveal_and_advance(G, 1, 0, NOW).unwrap();
        let res = data.gp_reveal_and_advance(G, 1, 1, NOW).unwrap();
        assert!(matches!(res.next, GpNext::Finished(_)));
        assert_eq!(res.round.unwrap().round_idx, 1);
    }

    /// With the reveal held to the end of the round, a song's end names nobody
    /// and moves no visible score: the totals still carry the round's payout,
    /// and showing them would say who guessed right and who fooled the room.
    #[test]
    fn a_held_reveal_names_nobody_and_moves_no_visible_score() {
        let data = data();
        game_with_reveal(&data, &["p1", "p2"], None, GpReveal::Round);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        submit(&data, C, "carol", "c");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let s0 = game(&data).rounds[0].tracks[0].submitter;
        let guesser = [A, B, C].into_iter().find(|u| *u != s0).unwrap();
        data.gp_record_guess(G, 0, 0, guesser, "n".into(), s0)
            .unwrap();
        data.gp_toggle_like(G, 0, 0, guesser, "n".into()).unwrap();

        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert!(res.held);
        assert!(res.scores.iter().all(|(_, p)| *p == 0), "{:?}", res.scores);
        let g = game(&data);
        assert!(g.scores.values().sum::<u32>() > 0, "paid, just not shown");
        assert!(g.visible_scores().iter().all(|(_, p)| *p == 0));
        assert!(g.sorted_scores().iter().any(|(_, p)| *p > 0));
        // `/gp status` shows the same held-back board.
        let GpStatus::Playing { scores, .. } = data.gp_status(G).unwrap() else {
            panic!("playing");
        };
        assert!(scores.iter().all(|(_, p)| *p == 0));

        let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
        let desc = v["description"].as_str().unwrap();
        assert!(desc.contains(GP_REVEAL_HELD), "{desc}");
        assert!(!desc.contains("<@"), "must name nobody: {desc}");
        assert!(!desc.contains(GP_REVEAL), "{desc}");
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 1, "likes only, no scoreboard: {fields:?}");
        assert_eq!(fields[0]["name"], GP_LIKES);
        assert_eq!(fields[0]["value"], "1");

        // A song that never played, held: says so, still names nobody.
        let res = data.gp_fail_and_advance(G, 0, 1, NOW).unwrap();
        assert!(res.held && res.failed);
        let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
        assert!(!serde_json::to_string(&v).unwrap().contains("<@"));
        assert_eq!(v["fields"][0]["name"], GP_TRACK_FAILED);

        // The round's end is the reveal: the results carry names and totals, and
        // from then on the board is whole again.
        let res = data.gp_reveal_and_advance(G, 0, 2, NOW).unwrap();
        let round = res.round.unwrap();
        assert!(round.scores.iter().any(|(_, p)| *p > 0));
        assert_eq!(round.scores, game(&data).sorted_scores());
        assert_eq!(game(&data).visible_scores(), game(&data).sorted_scores());
        let GpStatus::Submitting { scores, .. } = data.gp_status(G).unwrap() else {
            panic!("submitting");
        };
        assert!(scores.iter().any(|(_, p)| *p > 0));
    }

    /// In the default game the visible board is the board; nothing is held.
    #[test]
    fn a_song_reveal_shows_the_running_total() {
        let data = data();
        game_with(&data, &["p1"]);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        assert!(!res.held);
        assert!(res.scores.iter().any(|(_, p)| *p > 0), "fooled everyone");
        assert_eq!(res.scores, game(&data).sorted_scores());
        assert_eq!(game(&data).visible_scores(), game(&data).sorted_scores());
    }

    #[test]
    fn round_results_embed_json() {
        let song = |submitter, title: &str, correct: Vec<UserId>| GpSongResult {
            submitter,
            title: title.into(),
            correct,
            fooled_everyone: false,
            likes: 2,
            played_full: false,
            failed: false,
        };
        let r = GpRoundResult {
            round_idx: 1,
            total_rounds: 5,
            prompt: "Cry song.".into(),
            guessable: true,
            songs: vec![
                song(A, "one", vec![B, C]),
                GpSongResult {
                    fooled_everyone: true,
                    played_full: true,
                    ..song(B, "two", vec![])
                },
                GpSongResult {
                    failed: true,
                    ..song(C, "three", vec![A])
                },
            ],
            points: vec![(B, 250), (C, 100), (A, 20)],
            scores: vec![(B, 400), (A, 300), (C, 100)],
        };
        let v = serde_json::to_value(gp_round_results_embed(&r)).unwrap();
        assert_eq!(
            v["title"],
            format!("{GP_ROUND_TITLE} 2/5 {GP_RESULTS_TITLE}")
        );
        let desc = v["description"].as_str().unwrap();
        let lines: Vec<&str> = desc.lines().collect();
        assert_eq!(lines[0], "**Cry song.**");
        assert_eq!(
            lines[2],
            format!("1. **one** · <@100> · {GP_RESULTS_GUESSED_BY} <@200>, <@300> · 👍 2")
        );
        assert_eq!(
            lines[3],
            format!(
                "2. **two** · <@200> · {GP_RESULTS_GUESSED_BY} {GP_NOBODY_GUESSED} · 👍 2 · {GP_FOOLED_EVERYONE} · {GP_FULL_SONG}"
            )
        );
        assert_eq!(
            lines[4],
            format!("3. **three** · <@300> · {GP_TRACK_FAILED}")
        );
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["name"], GP_RESULTS_THIS_ROUND);
        assert_eq!(
            fields[0]["value"],
            "1. <@200> — +250\n2. <@300> — +100\n3. <@100> — +20"
        );
        assert_eq!(fields[1]["name"], GP_SCOREBOARD);
        assert_eq!(
            fields[1]["value"],
            "1. <@200> — 400\n2. <@100> — 300\n3. <@300> — 100"
        );

        // A one-song round has no guessing to report, and nobody may have scored.
        let solo = GpRoundResult {
            guessable: false,
            songs: vec![song(A, "only", vec![])],
            points: vec![],
            ..r.clone()
        };
        let v = serde_json::to_value(gp_round_results_embed(&solo)).unwrap();
        let desc = v["description"].as_str().unwrap();
        assert!(!desc.contains(GP_RESULTS_GUESSED_BY), "{desc}");
        assert!(desc.contains("**only** · <@100> · 👍 2"), "{desc}");
        assert_eq!(v["fields"][0]["value"], GP_RESULTS_NOBODY_SCORED);

        // A full room: 25 songs each guessed by the other 24 is more mentions
        // than a description holds, so the guessers are counted instead.
        let ids: Vec<UserId> = (1..=25).map(|i| UserId::new(1_000_000_000 + i)).collect();
        let big = GpRoundResult {
            songs: ids
                .iter()
                .map(|id| {
                    song(
                        *id,
                        "a song with a fairly long title",
                        ids.iter().copied().filter(|o| o != id).collect(),
                    )
                })
                .collect(),
            ..r
        };
        let v = serde_json::to_value(gp_round_results_embed(&big)).unwrap();
        let desc = v["description"].as_str().unwrap();
        assert!(
            desc.chars().count() <= GP_EMBED_DESCRIPTION_MAX,
            "{}",
            desc.len()
        );
        assert!(
            desc.contains(&format!("24 {GP_RESULTS_GUESSED_COUNT}")),
            "{desc}"
        );
        assert_eq!(desc.lines().count(), 27, "every song is still there");
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
            clip: None,
            generation: 1,
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
            played_full: false,
            guessable: true,
            scores: vec![(B, 100), (C, 100), (A, 30)],
            message: None,
            text_channel: TC,
            next: GpNext::Finished(vec![]),
            failed: false,
            held: false,
            round: None,
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
        for stalling in GP_STALLING_COMMANDS {
            assert!(
                GP_BLOCKED_COMMANDS.contains(stalling),
                "{stalling} stalls the game and must stay blocked"
            );
        }
        // Moving the bot strands `game.voice_channel`, after which every guess and
        // 👍 is rejected against a channel nobody is in.
        for moving in ["summon", "summonchannel"] {
            assert!(GP_BLOCKED_COMMANDS.contains(&moving), "{moving}");
        }
        // The game has its own `/gp voteskip`; the music one bypasses the majority.
        assert!(GP_BLOCKED_COMMANDS.contains(&"voteskip"));
        assert!(!GP_BLOCKED_COMMANDS.contains(&"gp"));
        assert!(!GP_BLOCKED_COMMANDS.contains(&"resume"), "the escape hatch");
        for sub in &gp().subcommands {
            let qualified = format!("gp {}", sub.name);
            assert!(!GP_BLOCKED_COMMANDS.contains(&qualified.as_str()));
        }
    }

    #[cfg(not(tarpaulin_include))]
    #[test]
    fn command_registration() {
        let all = crate::commands::all_commands();
        let registered = crate::commands::commands_to_register();
        for list in [&all, &registered] {
            assert!(list.iter().any(|c| c.name == "gp"), "gp not registered");
            // `gp` is registered on its own, not by chaining `game_commands()`:
            // coinflip and rolldice are deliberately still unregistered, and
            // pulling them in as a side effect of shipping `gp` is the mistake
            // this guards against.
            for unregistered in ["coinflip", "rolldice"] {
                assert!(
                    !list.iter().any(|c| c.name == unregistered),
                    "{unregistered} must stay unregistered"
                );
            }
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
            vec!["close", "end", "skip", "start", "status", "submit", "votefull", "voteskip"]
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
            // A vote is nobody's business but the voter's: a public reply would
            // land under "*name* used `/gp voteskip`" and say exactly who wants
            // the song gone.
            if sub.name == "voteskip" || sub.name == "votefull" {
                assert!(sub.ephemeral, "{} replies must be ephemeral", sub.name);
            }
            if sub.name == "start" {
                let params: Vec<&str> = sub.parameters.iter().map(|p| p.name.as_ref()).collect();
                assert_eq!(
                    params,
                    vec![
                        "category",
                        "rounds",
                        "timer",
                        "clips",
                        "clip_start",
                        "clip_length",
                        "reveal"
                    ]
                );
                assert!(sub.parameters[0].required);
                let reveal = sub.parameters.last().unwrap();
                assert_eq!(reveal.choices.len(), 2, "after each song, or at the end");
                assert_eq!(
                    sub.parameters[0].choices.len(),
                    crate::commands::music::gp_prompts::GP_PROMPTS.len() + 1,
                    "every category + Mixed"
                );
                // Only the category is required; everything else has a default.
                assert!(sub.parameters[1..].iter().all(|p| !p.required));
            }
        }
    }
}
