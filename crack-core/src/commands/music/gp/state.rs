use crate::{
    commands::music::gp_prompts::{GpCategories, GpCategory, GpPrompt},
    db::GpOutcome,
    errors::CrackedError,
    music::PlaybackOwner,
    CrackedResult, Data,
};
use ::serenity::all::{ChannelId, GenericChannelId, GuildId, MessageId, UserId};
use crack_testing::ResolvedTrack;
use rand::{seq::SliceRandom, Rng};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

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
/// Custom ids on `/gp start`'s category picker. Deliberately not under
/// [`GP_CUSTOM_ID_PREFIX`]: the picker's own collector answers them, and the
/// global handler, finding no game, would answer them first.
pub const GP_PICK_MENU_ID: &str = "gppick:menu";
pub const GP_PICK_START_ID: &str = "gppick:start";
pub const GP_PICK_CANCEL_ID: &str = "gppick:cancel";
/// How long the category picker waits on the host's next click.
pub const GP_PICK_TIMEOUT_SECS: u64 = 120;
/// Music commands refused while a game owns playback, because each would leave
/// playback in a state the game's own state machine never produced: injecting or
/// reordering tracks (`play`, `shuffle`, `remove`, ...), advancing or stalling the
/// round outside the game's control (`skip`, `seek`, `repeat`, `pause`), tearing
/// down voice (`leave`), or moving the bot out of [`GpGame::voice_channel`] so that
/// every guess and 👍 is then rejected against a channel nobody is in (`summon`).
/// The game's own `/gp skip` and `/gp voteskip` are the sanctioned ways to end a
/// song. Matched against the command's *qualified* name so `gp skip` is not caught
/// by `skip`.
///
/// # What `blocklist_matches_registry` actually checks
///
/// Only that every name here is a registered music command that runs a check
/// -- not that this is the exact set of commands that take a [`QueueGuard`] as
/// [`PlaybackOwner::Free`]. The converse does not hold: `leave`, `summon`,
/// `summonchannel`, `seek` and `repeat` are on this list and take no guard at
/// all. See "What the funnel does NOT cover" in
/// `docs/superpowers/specs/2026-09-10-playback-ownership-lease-design.md` for
/// why voice-state and track-state commands are blocked here without one.
///
/// Both halves the test *does* check have already failed once. `remove` sat
/// here from #422 with no `check = "cmd_check_music"`, so its entry never
/// fired; `resume` took the guard but was absent from this list *and* from the
/// funnel, which is the one case where a user could still land a mutation on a
/// live round.
///
/// [`QueueGuard`]: crate::music::QueueGuard
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
    "resume",
    "summon",
    "summonchannel",
    // Mutates nothing, but names the song a round is hiding (the floating
    // status spec, rule 7). Matched on `qualified_name`, so `np` is covered.
    "nowplaying",
];

/// The subset of [`GP_BLOCKED_COMMANDS`] that would stall the game outright: the
/// current track would never reach [`songbird::TrackEvent::End`], so no round would ever
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
    /// Nothing is named until the round's last song has played: the reveal is
    /// the round-results embed. The default. With every song revealed as it
    /// ends, the last song of a round is never a guess -- everyone has one song
    /// in, so by the final one the room knows by elimination -- and with three
    /// players the second is a coin flip. Holding the names keeps every song a
    /// guess.
    #[name = "🤐 At the end of the round"]
    #[name = "round"]
    #[default]
    Round,
    /// The submitter is named when their song ends, as the game originally did.
    #[name = "🎉 After each song"]
    #[name = "song"]
    Song,
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
        [Self::Round, Self::Song]
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
    pub(in crate::commands::music::gp) fn new(
        submitter: UserId,
        track: ResolvedTrack<'static>,
    ) -> Self {
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
    pub(in crate::commands::music::gp) fn score(&self, guessable: bool) -> GpTrackScore {
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
    /// The category the prompt was drawn from. `None` only on a round saved,
    /// in a game of several categories, before rounds had one.
    pub category: Option<GpCategory>,
    /// One song per player while the window is open; resubmitting replaces.
    pub submissions: HashMap<UserId, ResolvedTrack<'static>>,
    /// Filled (shuffled) when the window closes.
    pub tracks: Vec<GpTrack>,
    /// The prompt message, so the close can edit it in place.
    pub prompt_message: Option<(GenericChannelId, MessageId)>,
    /// Unix seconds; `Some` while the window is open.
    pub closes_at: Option<i64>,
    /// The round's results embed has reached the channel. The snapshot that
    /// moves the game past a round is written *before* its results are posted,
    /// so a restart in between would otherwise lose them -- and with the reveal
    /// held to the round's end, that embed is the only place the round's
    /// submitters are ever named. A resume posts the results of any round the
    /// game has moved past that does not have this set.
    pub results_posted: bool,
}

impl GpRound {
    fn new(prompt: GpPrompt) -> Self {
        Self {
            prompt: prompt.text,
            category: Some(prompt.category),
            submissions: HashMap::new(),
            tracks: Vec::new(),
            prompt_message: None,
            closes_at: None,
            results_posted: false,
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
    pub categories: GpCategories,
    /// Pre-drawn, one per prompt.
    pub rounds: Vec<GpRound>,
    pub current_round: usize,
    pub current_track: usize,
    pub timer_secs: u64,
    /// `None` plays whole songs.
    pub clip: Option<GpClip>,
    /// When submitters are named: only in the round's results (the default), or
    /// as each song ends.
    pub reveal: GpReveal,
    /// Post the round-results embed when a round ends. Off, the game is as it
    /// was before there was one: each song's reveal and nothing summing them
    /// up. Ignored -- always on -- when `reveal` is held to the round's end,
    /// since then the results embed is the reveal.
    pub round_results: bool,
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
    pub(in crate::commands::music::gp) fn new(
        guild_id: GuildId,
        host: UserId,
        voice_channel: ChannelId,
        text_channel: GenericChannelId,
        categories: GpCategories,
        prompts: Vec<GpPrompt>,
        timer_secs: u64,
        clip: Option<GpClip>,
        reveal: GpReveal,
        round_results: bool,
        started_at: i64,
    ) -> Self {
        Self {
            guild_id,
            started_at,
            host,
            voice_channel,
            text_channel,
            phase: GpPhase::Submitting,
            categories,
            rounds: prompts.into_iter().map(GpRound::new).collect(),
            current_round: 0,
            current_track: 0,
            timer_secs,
            clip,
            reveal,
            round_results,
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

    /// Whether this game posts a round's results: a host may turn them off,
    /// unless the reveal is held to the round's end, in which case they are the
    /// reveal and there is no game without them.
    pub(crate) fn posts_results(&self) -> bool {
        self.round_results || self.reveal == GpReveal::Round
    }

    /// The rounds the game has moved past whose results never reached the
    /// channel: the bot went down between the snapshot that ended the round and
    /// the post. Whatever else becomes of the game, the room is owed these. A
    /// round nobody submitted to has no results and is not.
    pub(crate) fn unposted_results(&self) -> Vec<usize> {
        if !self.posts_results() {
            return Vec::new();
        }
        self.rounds
            .iter()
            .take(self.current_round)
            .enumerate()
            .filter(|(_, r)| !r.tracks.is_empty() && !r.results_posted)
            .map(|(idx, _)| idx)
            .collect()
    }

    /// The round as it ended, for the results embed: every song with who
    /// submitted it and who got it, what the round paid each player, and the
    /// scoreboard after it.
    pub(crate) fn round_result(&self, round_idx: usize) -> GpRoundResult {
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

    /// The category to show with round `idx`'s prompt: only in a game of
    /// several, where it changes from round to round. A game of one said which
    /// when it started.
    fn shown_category(&self, idx: usize) -> Option<GpCategory> {
        if self.categories.as_slice().len() > 1 {
            self.rounds[idx].category
        } else {
            None
        }
    }

    fn open_window(&mut self, now: i64) -> GpWindowOpened {
        self.phase = GpPhase::Submitting;
        self.current_track = 0;
        self.generation += 1;
        let closes_at = now + self.timer_secs as i64;
        let idx = self.current_round;
        let total_rounds = self.rounds.len();
        let category = self.shown_category(idx);
        let round = &mut self.rounds[idx];
        round.closes_at = Some(closes_at);
        GpWindowOpened {
            round_idx: idx,
            total_rounds,
            prompt: round.prompt.clone(),
            category,
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
        let category = self.shown_category(idx);
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
            category,
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
    /// The prompt's category, to show above it; `None` when there is nothing to
    /// show (see `GpGame::shown_category`).
    pub category: Option<GpCategory>,
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
    /// As on [`GpWindowOpened`]: the closed embed replaces the open one.
    pub category: Option<GpCategory>,
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
    /// This was the round's last song and the game posts results: the round's
    /// results, to post after the reveal and before whatever comes next.
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
        categories: GpCategories,
        prompts: Vec<GpPrompt>,
        timer_secs: u64,
        clip: Option<GpClip>,
        reveal: GpReveal,
        round_results: bool,
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
            categories,
            prompts,
            timer_secs,
            clip,
            reveal,
            round_results,
            now,
        );
        game.players.insert(host, host_name);
        let opened = game.open_window(now);
        // 🔑 Claimed here, inside the branch that inserts, so the lease and the
        // map move together. See music/lease.rs on why this must not be called
        // from anywhere else.
        self.claim_playback(guild_id, PlaybackOwner::Game)?;
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
        let round = (last_of_round && game.posts_results()).then(|| game.round_result(round_idx));
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
                self.release_playback(guild_id);
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
        self.release_playback(guild_id);
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
                // Reclaimed before anything is restored into the guild -- the
                // arbitration #431 needed, so a resumed game and a restored
                // queue cannot both take the voice channel.
                //
                // 🪤 This branch is unreachable today: `claim_playback` only
                // errs for a *different* owner, and `Game` is the only owner
                // that exists. It stays as deliberate defensive coding for
                // when a second `PlaybackOwner` variant arrives -- not dead
                // code to be simplified away.
                if self.claim_playback(guild_id, PlaybackOwner::Game).is_err() {
                    return false;
                }
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

    /// The round's results embed is in the channel. Written down at once: it is
    /// what a resume reads to know the round is not still owed them, and
    /// the snapshot that ended the round went out before the post.
    pub fn gp_mark_results_posted(&self, guild_id: GuildId, round_idx: usize) {
        let Some(mut game) = self.gp_games.get_mut(&guild_id) else {
            return;
        };
        let Some(round) = game.rounds.get_mut(round_idx) else {
            return;
        };
        round.results_posted = true;
        self.gp_snapshot(&game);
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
