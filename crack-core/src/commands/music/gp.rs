//! Guilty pleasure party game (`/gp`).
//!
//! Players in a voice channel privately submit one or more tracks with the
//! ephemeral `/gp submit` command. When the host runs `/gp begin` the bot plays
//! the submissions one per round; everyone in the voice channel picks, from a
//! dropdown of the players, who they think submitted the track. When the track
//! ends (naturally, or via `/gp skip`) the submitter is revealed, points are
//! awarded and the next round starts. Scores live in memory for the game only.
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
//! # Hiding the requester
//!
//! The game's tracks are enqueued **without** `.with_user_id(submitter)`, so the
//! track data carries the default `UserId::new(1)` sentinel which the now-playing
//! and queue embeds render as "(auto)". The real submitter exists only in the
//! game state.

use crate::{
    commands::cmd_check_music,
    commands::get_call_or_join_author,
    commands::music::skip::force_skip_top_track,
    errors::CrackedError,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    messaging::messages::{
        GP_FOOLED_EVERYONE, GP_GAME_OVER, GP_GUESS_CHANGED, GP_GUESS_RECORDED, GP_LOBBY_HOW_TO,
        GP_LOBBY_OPEN, GP_LOBBY_RULES, GP_NOBODY_GUESSED, GP_NO_GUESSES_YET, GP_REVEAL,
        GP_ROUND_HINT, GP_ROUND_TITLE, GP_RULES_TEXT, GP_SCOREBOARD, GP_SELECT_PLACEHOLDER,
        GP_STATUS_GUESSED, GP_STATUS_LOBBY, GP_STATUS_PLAYING, GP_STATUS_SCORES,
        GP_STATUS_SUBMITTERS,
    },
    music::queue::build_track,
    poise_ext::PoiseContextExt,
    Context, CrackedResult, Data, Error,
};
use ::serenity::{
    all::{
        ChannelId, Colour, ComponentInteraction, ComponentInteractionDataKind, GenericChannelId,
        GuildId, Mentionable, MessageId, UserId,
    },
    async_trait,
    builder::{
        CreateActionRow, CreateComponent, CreateEmbed, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateMessage, CreateSelectMenu, CreateSelectMenuKind,
        CreateSelectMenuOption, EditMessage,
    },
    http::Http,
};
use crack_testing::ResolvedTrack;
use crack_types::QueryType;
use poise::serenity_prelude::Context as SerenityContext;
use rand::{seq::SliceRandom, Rng};
use songbird::{Call, Event, EventContext, EventHandler, TrackEvent};
use std::{borrow::Cow, collections::HashMap, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;

/// Discord caps a string select menu at 25 options.
pub const GP_MAX_PLAYERS: usize = 25;
/// Pause between the reveal and the next round, so people can read it.
pub const GP_REVEAL_PAUSE_SECS: u64 = 5;
/// Points for guessing the submitter correctly.
pub const GP_POINTS_CORRECT: u32 = 1;
/// Points to the submitter when nobody guessed them.
pub const GP_POINTS_FOOLED_ALL: u32 = 1;
/// Component custom ids look like `gp:<guild_id>:<round_idx>`.
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

// ------------------------------------------------------------------
// State
// ------------------------------------------------------------------

/// Which stage the game is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpPhase {
    /// Accepting submissions.
    Lobby,
    /// Rounds are being played.
    Playing,
    /// Last round revealed; the game is torn down once the scoreboard is posted.
    Finished,
}

/// One submitted track.
#[derive(Clone, Debug)]
pub struct GpSubmission {
    pub submitter: UserId,
    pub track: ResolvedTrack<'static>,
}

/// One round: a track, its submitter, and everyone's guesses.
#[derive(Clone, Debug)]
pub struct GpRound {
    pub submitter: UserId,
    pub track: ResolvedTrack<'static>,
    /// guesser -> guessed submitter. Last guess wins.
    pub guesses: HashMap<UserId, UserId>,
    /// The round message, so the reveal can edit it in place.
    pub message: Option<(GenericChannelId, MessageId)>,
}

/// The per-guild game.
#[derive(Clone, Debug)]
pub struct GpGame {
    pub host: UserId,
    pub voice_channel: ChannelId,
    pub text_channel: GenericChannelId,
    pub phase: GpPhase,
    /// Everyone who has submitted or guessed, with the display name we saw at the time.
    pub players: HashMap<UserId, String>,
    /// Filled during the lobby, drained into `rounds` by `begin`.
    pub submissions: Vec<GpSubmission>,
    pub rounds: Vec<GpRound>,
    /// Index of the round currently playing.
    pub current: usize,
    pub scores: HashMap<UserId, u32>,
}

impl GpGame {
    fn new(host: UserId, voice_channel: ChannelId, text_channel: GenericChannelId) -> Self {
        Self {
            host,
            voice_channel,
            text_channel,
            phase: GpPhase::Lobby,
            players: HashMap::new(),
            submissions: Vec::new(),
            rounds: Vec::new(),
            current: 0,
            scores: HashMap::new(),
        }
    }

    /// Distinct users who submitted at least one track (the only valid answers).
    fn submitters(&self) -> Vec<UserId> {
        let mut ids: Vec<UserId> = match self.phase {
            GpPhase::Lobby => self.submissions.iter().map(|s| s.submitter).collect(),
            _ => self.rounds.iter().map(|r| r.submitter).collect(),
        };
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Submitters with their display names, sorted by name for a stable dropdown.
    fn submitter_names(&self) -> Vec<(UserId, String)> {
        let mut v: Vec<(UserId, String)> = self
            .submitters()
            .into_iter()
            .map(|id| {
                let name = self
                    .players
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| id.to_string());
                (id, name)
            })
            .collect();
        v.sort_by_key(|(_, name)| name.to_lowercase());
        v
    }

    /// Every player (submitters *and* guessers) with their points, best first;
    /// ties broken by name so the order is stable between edits.
    fn sorted_scores(&self) -> Vec<(UserId, u32)> {
        let mut ids: Vec<UserId> = self.players.keys().copied().collect();
        ids.extend(self.submitters());
        ids.sort_unstable();
        ids.dedup();
        let mut v: Vec<(UserId, u32)> = ids
            .into_iter()
            .map(|id| (id, self.scores.get(&id).copied().unwrap_or(0)))
            .collect();
        v.sort_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| {
                let name = |id: &UserId| {
                    self.players
                        .get(id)
                        .map(|n| n.to_lowercase())
                        .unwrap_or_default()
                };
                name(&a.0).cmp(&name(&b.0))
            })
        });
        v
    }
}

/// What `gp_begin` hands back: round 0's track, the dropdown options and the round count.
#[derive(Clone, Debug)]
pub struct GpBegun {
    pub first_track: ResolvedTrack<'static>,
    pub players: Vec<(UserId, String)>,
    pub total: usize,
}

/// What `gp_record_guess` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpGuessOutcome {
    Recorded,
    Changed,
}

/// Everything the reveal needs, cloned out of the map so no lock is held.
#[derive(Clone, Debug)]
pub struct GpRoundResult {
    pub round_idx: usize,
    pub total: usize,
    pub submitter: UserId,
    pub title: String,
    pub url: String,
    pub correct: Vec<UserId>,
    pub fooled_everyone: bool,
    /// Sorted descending.
    pub scores: Vec<(UserId, u32)>,
    pub message: Option<(GenericChannelId, MessageId)>,
    pub text_channel: GenericChannelId,
    /// The next round's track, or `None` when the game is over.
    pub next: Option<ResolvedTrack<'static>>,
    /// Dropdown options for the next round.
    pub players: Vec<(UserId, String)>,
}

/// Snapshot for `/gp status`.
#[derive(Clone, Debug)]
pub enum GpStatus {
    Lobby {
        host: UserId,
        submitters: Vec<(String, usize)>,
    },
    Playing {
        round: usize,
        total: usize,
        guessed: Vec<String>,
        scores: Vec<(UserId, u32)>,
    },
}

// ------------------------------------------------------------------
// State helpers on Data (all synchronous, see module docs)
// ------------------------------------------------------------------

impl Data {
    /// Open a lobby for `guild_id`.
    pub fn gp_start(
        &self,
        guild_id: GuildId,
        host: UserId,
        host_name: String,
        voice_channel: ChannelId,
        text_channel: GenericChannelId,
    ) -> CrackedResult<()> {
        if self.gp_games.contains_key(&guild_id) {
            return Err(CrackedError::GameAlreadyRunning);
        }
        let mut game = GpGame::new(host, voice_channel, text_channel);
        game.players.insert(host, host_name);
        self.gp_games.insert(guild_id, game);
        Ok(())
    }

    /// Record a submission. Returns how many tracks this user has submitted.
    pub fn gp_submit(
        &self,
        guild_id: GuildId,
        user: UserId,
        name: String,
        track: ResolvedTrack<'static>,
    ) -> CrackedResult<usize> {
        let mut game = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.phase != GpPhase::Lobby {
            return Err(CrackedError::SubmissionsClosed);
        }
        let is_new = !game.submissions.iter().any(|s| s.submitter == user);
        if is_new && game.submitters().len() >= GP_MAX_PLAYERS {
            return Err(CrackedError::TooManyPlayers(GP_MAX_PLAYERS));
        }
        game.players.insert(user, name);
        game.submissions.push(GpSubmission {
            submitter: user,
            track,
        });
        Ok(game
            .submissions
            .iter()
            .filter(|s| s.submitter == user)
            .count())
    }

    /// Close submissions, shuffle them into rounds and return round 0's track
    /// together with the dropdown options.
    pub fn gp_begin(
        &self,
        guild_id: GuildId,
        caller: UserId,
        rng: &mut impl Rng,
    ) -> CrackedResult<GpBegun> {
        let mut game = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.host != caller {
            return Err(CrackedError::NotGameHost);
        }
        if game.phase != GpPhase::Lobby {
            return Err(CrackedError::SubmissionsClosed);
        }
        if game.submitters().len() < 2 {
            return Err(CrackedError::NotEnoughPlayers);
        }
        let mut submissions = std::mem::take(&mut game.submissions);
        submissions.shuffle(rng);
        game.rounds = submissions
            .into_iter()
            .map(|s| GpRound {
                submitter: s.submitter,
                track: s.track,
                guesses: HashMap::new(),
                message: None,
            })
            .collect();
        game.current = 0;
        game.phase = GpPhase::Playing;
        let total = game.rounds.len();
        Ok(GpBegun {
            first_track: game.rounds[0].track.clone(),
            players: game.submitter_names(),
            total,
        })
    }

    /// Remember where the round message went so the reveal can edit it.
    pub fn gp_set_round_message(
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
        round.message = Some((channel, message_id));
        Ok(())
    }

    /// Record (or change) a guess for the current round.
    pub fn gp_record_guess(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        guesser: UserId,
        guesser_name: String,
        guessed: UserId,
    ) -> CrackedResult<GpGuessOutcome> {
        let mut game = self
            .gp_games
            .get_mut(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying);
        }
        if round_idx != game.current {
            return Err(CrackedError::StaleRound);
        }
        if !game.submitters().contains(&guessed) {
            return Err(CrackedError::NotAPlayer);
        }
        game.players.entry(guesser).or_insert(guesser_name);
        let current = game.current;
        let previous = game.rounds[current].guesses.insert(guesser, guessed);
        Ok(match previous {
            Some(p) if p != guessed => GpGuessOutcome::Changed,
            _ => GpGuessOutcome::Recorded,
        })
    }

    /// Score the round `round_idx` and advance. Returns `None` unless the game
    /// is playing **and** `round_idx` is the current round, which makes it safe
    /// to call twice (End and Error can both fire for one track).
    pub fn gp_reveal_and_advance(
        &self,
        guild_id: GuildId,
        round_idx: usize,
    ) -> Option<GpRoundResult> {
        let mut game = self.gp_games.get_mut(&guild_id)?;
        if game.phase != GpPhase::Playing || round_idx != game.current {
            return None;
        }
        let round = game.rounds[round_idx].clone();
        let correct: Vec<UserId> = round
            .guesses
            .iter()
            .filter(|(guesser, guessed)| {
                **guesser != round.submitter && **guessed == round.submitter
            })
            .map(|(guesser, _)| *guesser)
            .collect();
        for g in &correct {
            *game.scores.entry(*g).or_insert(0) += GP_POINTS_CORRECT;
        }
        let fooled_everyone = correct.is_empty();
        if fooled_everyone {
            *game.scores.entry(round.submitter).or_insert(0) += GP_POINTS_FOOLED_ALL;
        }
        game.current += 1;
        let total = game.rounds.len();
        let next = if game.current < total {
            Some(game.rounds[game.current].track.clone())
        } else {
            game.phase = GpPhase::Finished;
            None
        };
        Some(GpRoundResult {
            round_idx,
            total,
            submitter: round.submitter,
            title: round.track.get_title(),
            url: round.track.get_url(),
            correct,
            fooled_everyone,
            scores: game.sorted_scores(),
            message: round.message,
            text_channel: game.text_channel,
            next,
            players: game.submitter_names(),
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
        Ok(match game.phase {
            GpPhase::Lobby => {
                let submitters = game
                    .submitter_names()
                    .into_iter()
                    .map(|(id, name)| {
                        let n = game
                            .submissions
                            .iter()
                            .filter(|s| s.submitter == id)
                            .count();
                        (name, n)
                    })
                    .collect();
                GpStatus::Lobby {
                    host: game.host,
                    submitters,
                }
            },
            GpPhase::Playing | GpPhase::Finished => {
                let guessed = game
                    .rounds
                    .get(game.current)
                    .map(|r| {
                        let mut names: Vec<String> = r
                            .guesses
                            .keys()
                            .map(|id| {
                                game.players
                                    .get(id)
                                    .cloned()
                                    .unwrap_or_else(|| id.to_string())
                            })
                            .collect();
                        names.sort();
                        names
                    })
                    .unwrap_or_default();
                GpStatus::Playing {
                    round: (game.current + 1).min(game.rounds.len()),
                    total: game.rounds.len(),
                    guessed,
                    scores: game.sorted_scores(),
                }
            },
        })
    }

    /// True while a game owns playback for this guild: rounds are being played,
    /// or the last round has been revealed and the scoreboard is on its way.
    /// A lobby does **not** own playback, so normal music keeps working until `begin`.
    pub fn gp_is_playing(&self, guild_id: GuildId) -> bool {
        self.gp_games
            .get(&guild_id)
            .map(|g| matches!(g.phase, GpPhase::Playing | GpPhase::Finished))
            .unwrap_or(false)
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

/// Build the `gp:<guild>:<round>` custom id.
pub fn gp_custom_id(guild_id: GuildId, round_idx: usize) -> String {
    format!("{GP_CUSTOM_ID_PREFIX}{}:{round_idx}", guild_id.get())
}

/// Parse a `gp:<guild>:<round>` custom id.
pub fn parse_custom_id(custom_id: &str) -> Option<(GuildId, usize)> {
    let rest = custom_id.strip_prefix(GP_CUSTOM_ID_PREFIX)?;
    let (guild, round) = rest.split_once(':')?;
    let guild = guild.parse::<u64>().ok().filter(|g| *g != 0)?;
    let round = round.parse::<usize>().ok()?;
    Some((GuildId::new(guild), round))
}

/// The "who submitted this?" dropdown for one round.
pub fn gp_select_menu(
    guild_id: GuildId,
    round_idx: usize,
    players: &[(UserId, String)],
) -> CreateComponent<'static> {
    let options: Vec<CreateSelectMenuOption<'static>> = players
        .iter()
        .take(GP_MAX_PLAYERS)
        .map(|(id, name)| CreateSelectMenuOption::new(name.clone(), id.to_string()))
        .collect();
    let menu = CreateSelectMenu::new(
        gp_custom_id(guild_id, round_idx),
        CreateSelectMenuKind::String {
            options: Cow::Owned(options),
        },
    )
    .placeholder(GP_SELECT_PLACEHOLDER)
    .min_values(1)
    .max_values(1);
    CreateComponent::ActionRow(CreateActionRow::SelectMenu(menu))
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

/// The rules / how-to embed used by `/gp` and `/gp start`.
pub fn gp_rules_embed(host: Option<UserId>) -> CreateEmbed<'static> {
    let mut e = CreateEmbed::new()
        .title(GP_LOBBY_OPEN)
        .description(GP_RULES_TEXT)
        .field(GP_LOBBY_RULES, GP_LOBBY_HOW_TO, false)
        .colour(Colour::FOOYOO);
    if let Some(host) = host {
        e = e.field("Host", host.mention().to_string(), true);
    }
    e
}

/// The round message: which round, the title, and the prompt. No submitter.
pub fn gp_round_embed(
    round_idx: usize,
    total: usize,
    track: &ResolvedTrack<'_>,
) -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(format!("{GP_ROUND_TITLE} {}/{total}", round_idx + 1))
        .description(format!(
            "**[{}]({})**\n\n{GP_ROUND_HINT}",
            track.get_title(),
            track.get_url()
        ))
        .colour(Colour::BLURPLE)
}

/// The reveal, written into the round message once the track is over.
pub fn gp_reveal_embed(res: &GpRoundResult) -> CreateEmbed<'static> {
    let correct = if res.correct.is_empty() {
        GP_NOBODY_GUESSED.to_string()
    } else {
        res.correct
            .iter()
            .map(|id| id.mention().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut e = CreateEmbed::new()
        .title(format!(
            "{GP_ROUND_TITLE} {}/{}",
            res.round_idx + 1,
            res.total
        ))
        .description(format!(
            "**[{}]({})**\n\n{GP_REVEAL} {}",
            res.title,
            res.url,
            res.submitter.mention()
        ))
        .field("Guessed right", correct, false)
        .field(GP_SCOREBOARD, scores_lines(&res.scores), false)
        .colour(Colour::DARK_GREEN);
    if res.fooled_everyone {
        e = e.field(
            GP_FOOLED_EVERYONE,
            res.submitter.mention().to_string(),
            false,
        );
    }
    e
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
    match status {
        GpStatus::Lobby { host, submitters } => {
            let list = if submitters.is_empty() {
                "-".to_string()
            } else {
                submitters
                    .iter()
                    .map(|(name, n)| format!("{name} ({n})"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            CreateEmbed::new()
                .title(GP_STATUS_LOBBY)
                .field("Host", host.mention().to_string(), true)
                .field(GP_STATUS_SUBMITTERS, list, false)
                .colour(Colour::FOOYOO)
        },
        GpStatus::Playing {
            round,
            total,
            guessed,
            scores,
        } => {
            let guessed = if guessed.is_empty() {
                GP_NO_GUESSES_YET.to_string()
            } else {
                guessed.join(", ")
            };
            CreateEmbed::new()
                .title(format!("{GP_STATUS_PLAYING} {round}/{total}"))
                .field(GP_STATUS_GUESSED, guessed, false)
                .field(GP_STATUS_SCORES, scores_lines(scores), false)
                .colour(Colour::BLURPLE)
        },
    }
}

// ------------------------------------------------------------------
// Playback glue
// ------------------------------------------------------------------

/// What the playback side of a game needs: shared state, HTTP, the call, and
/// the guild. Cloned into every per-track handler.
#[derive(Clone)]
pub struct GpPlayback {
    pub data: Arc<Data>,
    pub http: Arc<Http>,
    pub call: Arc<Mutex<Call>>,
    pub guild_id: GuildId,
}

/// Per-track handler: when the round's track ends, reveal and move on.
pub struct GpRoundEndHandler {
    pub pb: GpPlayback,
    pub round_idx: usize,
}

#[async_trait]
impl EventHandler for GpRoundEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let (pb, round_idx) = (self.pb.clone(), self.round_idx);
        // Do the reveal off the driver's event task: it edits messages, sleeps,
        // and takes the call lock to enqueue the next round.
        tokio::spawn(async move {
            let guild_id = pb.guild_id;
            if let Err(e) = gp_advance_round(pb, round_idx).await {
                tracing::warn!("gp: advancing round {round_idx} in {guild_id}: {e}");
            }
        });
        Some(Event::Cancel)
    }
}

/// Enqueue one round's track, hook its end, and post the round message.
pub async fn gp_play_round(
    pb: &GpPlayback,
    round_idx: usize,
    total: usize,
    track: ResolvedTrack<'static>,
    players: &[(UserId, String)],
    text_channel: GenericChannelId,
) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    if !pb.data.gp_is_playing(guild_id) {
        // Ended or torn down while we were between rounds.
        return Ok(());
    }
    // `build_track` is lazy; the stream is fetched when songbird starts it.
    // Note: no `.with_user_id` -- see the module docs.
    let songbird_track = build_track(&track, &pb.data.http_client)?;
    let handle = {
        let mut handler = pb.call.lock().await;
        handler.enqueue(songbird_track).await
    };
    for event in [TrackEvent::End, TrackEvent::Error] {
        handle.add_event(
            Event::Track(event),
            GpRoundEndHandler {
                pb: pb.clone(),
                round_idx,
            },
        )?;
    }

    let msg = text_channel
        .send_message(
            &pb.http,
            CreateMessage::new()
                .embed(gp_round_embed(round_idx, total, &track))
                .components(vec![gp_select_menu(guild_id, round_idx, players)]),
        )
        .await?;
    pb.data
        .gp_set_round_message(guild_id, round_idx, text_channel, msg.id)?;
    Ok(())
}

/// Reveal the round that just ended and start the next one (or finish).
pub async fn gp_advance_round(pb: GpPlayback, round_idx: usize) -> Result<(), Error> {
    let guild_id = pb.guild_id;
    let Some(res) = pb.data.gp_reveal_and_advance(guild_id, round_idx) else {
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

    tokio::time::sleep(Duration::from_secs(GP_REVEAL_PAUSE_SECS)).await;

    match res.next {
        Some(track) => {
            gp_play_round(
                &pb,
                res.round_idx + 1,
                res.total,
                track,
                &res.players,
                res.text_channel,
            )
            .await
        },
        None => {
            res.text_channel
                .send_message(
                    &pb.http,
                    CreateMessage::new().embed(gp_scoreboard_embed(&res.scores, GP_GAME_OVER)),
                )
                .await?;
            pb.data.gp_remove(guild_id);
            Ok(())
        },
    }
}

/// Handle a dropdown pick. Called from `SerenityHandler::dispatch` for every
/// component interaction whose custom id starts with [`GP_CUSTOM_ID_PREFIX`].
/// Every branch answers the interaction (ephemerally), otherwise Discord shows
/// "This interaction failed".
pub async fn handle_gp_guess(
    data: &Data,
    ctx: &SerenityContext,
    mci: &ComponentInteraction,
) -> Result<(), Error> {
    let Some((guild_id, round_idx)) = parse_custom_id(&mci.data.custom_id) else {
        return Ok(());
    };
    if mci.guild_id != Some(guild_id) {
        return Ok(());
    }

    let outcome = gp_guess_outcome(data, ctx, mci, guild_id, round_idx);
    let content = match outcome {
        Ok(GpGuessOutcome::Recorded) => GP_GUESS_RECORDED.to_string(),
        Ok(GpGuessOutcome::Changed) => GP_GUESS_CHANGED.to_string(),
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

/// The synchronous part of a guess: voice-channel check + record.
fn gp_guess_outcome(
    data: &Data,
    ctx: &SerenityContext,
    mci: &ComponentInteraction,
    guild_id: GuildId,
    round_idx: usize,
) -> CrackedResult<GpGuessOutcome> {
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
    let guessed = match &mci.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values
            .first()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v != 0)
            .map(UserId::new)
            .ok_or(CrackedError::NotAPlayer)?,
        _ => return Err(CrackedError::NotAPlayer),
    };
    let name = mci
        .member
        .as_ref()
        .map(|m| m.display_name().to_string())
        .unwrap_or_else(|| mci.user.name.to_string());
    data.gp_record_guess(guild_id, round_idx, mci.user.id, name, guessed)
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

/// Guilty pleasure party game: submit tracks in secret, guess who queued what.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    aliases("guiltypleasure"),
    subcommands("gp_start", "gp_submit", "gp_begin", "gp_skip", "gp_status", "gp_end")
)]
pub async fn gp(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send_embed_response(gp_rules_embed(None)).await?;
    Ok(())
}

/// Open a guilty pleasure lobby in your voice channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "start",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_start(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let vc = ctx.author_vc().ok_or(CrackedError::NotConnected)?;
    let host = ctx.author().id;
    let host_name = author_display_name(ctx).await;
    ctx.data()
        .gp_start(guild_id, host, host_name, vc, ctx.channel_id())?;
    ctx.send_embed_response(gp_rules_embed(Some(host))).await?;
    Ok(())
}

/// Secretly submit a guilty pleasure track (link or search).
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

/// Resolve the query and store it. Returns the confirmation message.
#[cfg(not(tarpaulin_include))]
pub async fn gp_submit_internal(ctx: Context<'_>, query: String) -> CrackedResult<CrackedMessage> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let game_vc = ctx
        .data()
        .gp_voice_channel(guild_id)
        .ok_or(CrackedError::NoGameInProgress)?;
    if ctx.author_vc() != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel);
    }
    let query = query.trim();
    if query.is_empty() {
        return Err(CrackedError::NoQuery);
    }
    let query_type = QueryType::from_str(query).map_err(CrackedError::TrackResolveError)?;
    let track = ctx
        .data()
        .ct_client
        .resolve_track(query_type)
        .await
        .map_err(CrackedError::TrackFail)?;
    let name = author_display_name(ctx).await;
    let title = track.get_title();
    let count = ctx
        .data()
        .gp_submit(guild_id, ctx.author().id, name, track)?;
    Ok(CrackedMessage::GpSubmitted { title, count })
}

/// Close submissions and start playing the rounds (host only).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "begin",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_begin(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = data
        .gp_voice_channel(guild_id)
        .ok_or(CrackedError::NoGameInProgress)?;
    if ctx.author_vc() != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel.into());
    }

    // Join (or reuse) the call, and make sure it is the game's channel.
    let call = get_call_or_join_author(ctx).await?;
    {
        let handler = call.lock().await;
        if let Some(chan) = handler.current_channel() {
            if chan.get() != game_vc.get() {
                return Err(CrackedError::WrongVoiceChannel.into());
            }
        }
    }

    // Flip the phase first so the global TrackEndHandler ignores the End event
    // that stopping the existing queue fires.
    let GpBegun {
        first_track,
        players,
        total,
    } = data.gp_begin(guild_id, ctx.author().id, &mut rand::thread_rng())?;
    let cleared_queue = {
        let handler = call.lock().await;
        let non_empty = !handler.queue().is_empty();
        if non_empty {
            handler.queue().stop();
        }
        non_empty
    };

    ctx.send_reply(
        CrackedMessage::GpBegan {
            rounds: total,
            players: players.len(),
            cleared_queue,
        },
        true,
    )
    .await?;

    let pb = GpPlayback {
        data: data.clone(),
        http: ctx.serenity_context().http.clone(),
        call,
        guild_id,
    };
    gp_play_round(&pb, 0, total, first_track, &players, ctx.channel_id()).await
}

/// End the current round early (host only).
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
        // stop() fires TrackEvent::End, which is what advances the round.
        force_skip_top_track(&handler).await?;
    }
    ctx.send_reply(CrackedMessage::GpRoundSkipped, true).await?;
    Ok(())
}

/// Who has submitted / guessed, and the scores so far.
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
    // game and the round-end handler is a no-op.
    let game = data.gp_end(guild_id, ctx.author().id, is_admin)?;
    if matches!(game.phase, GpPhase::Playing | GpPhase::Finished) {
        if let Some(call) = data.songbird.get(guild_id) {
            call.lock().await.queue().stop();
        }
    }
    let by = author_display_name(ctx).await;
    ctx.send_reply(CrackedMessage::GpEnded { by }, true).await?;
    if !game.rounds.is_empty() {
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

    fn lobby_with(data: &Data, users: &[(UserId, &str, usize)]) {
        data.gp_start(G, A, "alice".into(), VC, TC).unwrap();
        for (id, name, n) in users {
            for i in 0..*n {
                data.gp_submit(G, *id, name.to_string(), track(&format!("{name}{i}")))
                    .unwrap();
            }
        }
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0)
    }

    #[test]
    fn start_twice_and_submit_without_game() {
        let data = data();
        assert_eq!(
            data.gp_submit(G, A, "a".into(), track("x")).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        data.gp_start(G, A, "alice".into(), VC, TC).unwrap();
        assert_eq!(
            data.gp_start(G, B, "bob".into(), VC, TC).unwrap_err(),
            CrackedError::GameAlreadyRunning
        );
        assert!(data.gp_is_active(G));
        assert!(!data.gp_is_playing(G));
    }

    #[test]
    fn begin_needs_two_submitters_and_shuffles_all() {
        let data = data();
        lobby_with(&data, &[(A, "alice", 2)]);
        assert_eq!(
            data.gp_begin(G, A, &mut rng()).unwrap_err(),
            CrackedError::NotEnoughPlayers
        );
        data.gp_submit(G, B, "bob".into(), track("bob0")).unwrap();
        assert_eq!(
            data.gp_begin(G, B, &mut rng()).unwrap_err(),
            CrackedError::NotGameHost
        );
        let GpBegun { players, total, .. } = data.gp_begin(G, A, &mut rng()).unwrap();
        assert_eq!(total, 3);
        assert_eq!(players.len(), 2);
        assert!(data.gp_is_playing(G));
        let game = data.gp_games.get(&G).unwrap().clone();
        assert_eq!(game.phase, GpPhase::Playing);
        assert!(game.submissions.is_empty());
        assert_eq!(game.rounds.len(), 3);
        assert_eq!(
            data.gp_submit(G, C, "carol".into(), track("c"))
                .unwrap_err(),
            CrackedError::SubmissionsClosed
        );
    }

    #[test]
    fn too_many_players() {
        let data = data();
        data.gp_start(G, A, "alice".into(), VC, TC).unwrap();
        for i in 0..GP_MAX_PLAYERS as u64 {
            data.gp_submit(G, UserId::new(1000 + i), format!("u{i}"), track("t"))
                .unwrap();
        }
        // An existing player may still add tracks...
        data.gp_submit(G, UserId::new(1000), "u0".into(), track("t2"))
            .unwrap();
        // ...but a 26th distinct player may not.
        assert_eq!(
            data.gp_submit(G, UserId::new(5000), "new".into(), track("t"))
                .unwrap_err(),
            CrackedError::TooManyPlayers(GP_MAX_PLAYERS)
        );
    }

    #[test]
    fn guesses_and_scoring() {
        let data = data();
        lobby_with(&data, &[(A, "alice", 1), (B, "bob", 1)]);
        data.gp_begin(G, A, &mut rng()).unwrap();
        let submitter0 = data.gp_games.get(&G).unwrap().rounds[0].submitter;
        let other = if submitter0 == A { B } else { A };

        assert_eq!(
            data.gp_record_guess(G, 1, C, "carol".into(), A)
                .unwrap_err(),
            CrackedError::StaleRound
        );
        assert_eq!(
            data.gp_record_guess(G, 0, C, "carol".into(), C)
                .unwrap_err(),
            CrackedError::NotAPlayer
        );
        assert_eq!(
            data.gp_record_guess(G, 0, C, "carol".into(), other)
                .unwrap(),
            GpGuessOutcome::Recorded
        );
        assert_eq!(
            data.gp_record_guess(G, 0, C, "carol".into(), submitter0)
                .unwrap(),
            GpGuessOutcome::Changed
        );
        // The submitter may guess on their own track; it never scores.
        data.gp_record_guess(G, 0, submitter0, "self".into(), submitter0)
            .unwrap();
        // The other player guesses wrong.
        data.gp_record_guess(G, 0, other, "other".into(), other)
            .unwrap();

        let res = data.gp_reveal_and_advance(G, 0).unwrap();
        assert_eq!(res.submitter, submitter0);
        assert_eq!(res.correct, vec![C]);
        assert!(!res.fooled_everyone);
        assert!(res.next.is_some());
        let game = data.gp_games.get(&G).unwrap().clone();
        assert_eq!(game.scores.get(&C), Some(&GP_POINTS_CORRECT));
        assert_eq!(game.scores.get(&submitter0), None);
        assert_eq!(game.scores.get(&other), None);

        // Second call for the same round is a no-op.
        assert!(data.gp_reveal_and_advance(G, 0).is_none());

        // Round 1: nobody guesses -> submitter is rewarded, game finishes.
        let submitter1 = game.rounds[1].submitter;
        let res = data.gp_reveal_and_advance(G, 1).unwrap();
        assert!(res.fooled_everyone);
        assert!(res.next.is_none());
        let game = data.gp_games.get(&G).unwrap().clone();
        assert_eq!(game.phase, GpPhase::Finished);
        assert_eq!(game.scores.get(&submitter1), Some(&GP_POINTS_FOOLED_ALL));
        // Finished still owns playback until the scoreboard removes it.
        assert!(data.gp_is_playing(G));
        assert_eq!(
            data.gp_record_guess(G, 1, C, "carol".into(), A)
                .unwrap_err(),
            CrackedError::GameNotPlaying
        );
        data.gp_remove(G);
        assert!(!data.gp_is_playing(G));
        assert!(data.gp_reveal_and_advance(G, 2).is_none());
    }

    #[test]
    fn end_permissions() {
        let data = data();
        lobby_with(&data, &[(A, "alice", 1), (B, "bob", 1)]);
        assert_eq!(
            data.gp_end(G, B, false).unwrap_err(),
            CrackedError::NotGameHost
        );
        data.gp_end(G, C, true).unwrap();
        assert!(!data.gp_is_active(G));
        assert_eq!(
            data.gp_end(G, A, false).unwrap_err(),
            CrackedError::NoGameInProgress
        );
    }

    #[test]
    fn status_snapshots() {
        let data = data();
        lobby_with(&data, &[(A, "alice", 2), (B, "bob", 1)]);
        match data.gp_status(G).unwrap() {
            GpStatus::Lobby { host, submitters } => {
                assert_eq!(host, A);
                assert_eq!(
                    submitters,
                    vec![("alice".to_string(), 2), ("bob".to_string(), 1)]
                );
            },
            other => panic!("expected lobby, got {other:?}"),
        }
        data.gp_begin(G, A, &mut rng()).unwrap();
        data.gp_record_guess(G, 0, C, "carol".into(), A).unwrap();
        match data.gp_status(G).unwrap() {
            GpStatus::Playing {
                round,
                total,
                guessed,
                scores,
            } => {
                assert_eq!((round, total), (1, 3));
                assert_eq!(guessed, vec!["carol".to_string()]);
                // alice, bob (submitters) and carol (guesser only)
                assert_eq!(scores.len(), 3);
            },
            other => panic!("expected playing, got {other:?}"),
        }
    }

    #[test]
    fn custom_ids_and_select_menu() {
        assert_eq!(parse_custom_id("gp:1:0"), Some((GuildId::new(1), 0)));
        assert_eq!(parse_custom_id("gp:1:7"), Some((GuildId::new(1), 7)));
        assert_eq!(parse_custom_id(&gp_custom_id(G, 3)), Some((G, 3)));
        assert_eq!(parse_custom_id("gp:0:1"), None);
        assert_eq!(parse_custom_id("gp:x:1"), None);
        assert_eq!(parse_custom_id("song_select"), None);

        let players = vec![(A, "alice".to_string()), (B, "bob".to_string())];
        let component = gp_select_menu(G, 0, &players);
        assert!(matches!(
            component,
            CreateComponent::ActionRow(CreateActionRow::SelectMenu(_))
        ));
    }

    /// The dropdown as Discord will receive it: one string select in one action
    /// row, custom id `gp:<guild>:<round>`, one option per player (label = name,
    /// value = user id), single choice, capped at 25.
    #[test]
    fn select_menu_json() {
        let players = vec![(B, "bob".to_string()), (A, "alice".to_string())];
        let v = serde_json::to_value(gp_select_menu(G, 4, &players)).unwrap();
        let menu = &v["components"][0];
        assert_eq!(menu["custom_id"], "gp:1:4");
        assert_eq!(menu["placeholder"], GP_SELECT_PLACEHOLDER);
        assert_eq!(menu["min_values"], 1);
        assert_eq!(menu["max_values"], 1);
        let options = menu["options"].as_array().unwrap();
        assert_eq!(options.len(), 2);
        // Order is whatever the caller passed (submitter_names() pre-sorts).
        assert_eq!(options[0]["label"], "bob");
        assert_eq!(options[0]["value"], "200");
        assert_eq!(options[1]["label"], "alice");
        assert_eq!(options[1]["value"], "100");

        let many: Vec<(UserId, String)> = (1..=40u64)
            .map(|i| (UserId::new(i), format!("u{i}")))
            .collect();
        let v = serde_json::to_value(gp_select_menu(G, 0, &many)).unwrap();
        assert_eq!(
            v["components"][0]["options"].as_array().unwrap().len(),
            GP_MAX_PLAYERS
        );
    }

    /// The round message must name the track but never the submitter.
    #[test]
    fn round_embed_hides_submitter() {
        let t = track("secret");
        let v = serde_json::to_value(gp_round_embed(1, 5, &t)).unwrap();
        assert_eq!(v["title"], format!("{GP_ROUND_TITLE} 2/5"));
        let desc = v["description"].as_str().unwrap();
        assert!(desc.contains("secret"), "{desc}");
        assert!(desc.contains(GP_ROUND_HINT), "{desc}");
        assert!(
            !desc.contains("<@"),
            "round embed must not mention anyone: {desc}"
        );
        assert!(v.get("fields").is_none() || v["fields"].as_array().unwrap().is_empty());
    }

    #[test]
    fn reveal_embed_json() {
        let res = GpRoundResult {
            round_idx: 0,
            total: 2,
            submitter: A,
            title: "song".into(),
            url: "https://example.invalid/song".into(),
            correct: vec![B, C],
            fooled_everyone: false,
            scores: vec![(B, 1), (C, 1), (A, 0)],
            message: None,
            text_channel: TC,
            next: None,
            players: vec![],
        };
        let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
        assert_eq!(v["title"], format!("{GP_ROUND_TITLE} 1/2"));
        let desc = v["description"].as_str().unwrap();
        assert!(
            desc.contains("[song](https://example.invalid/song)"),
            "{desc}"
        );
        assert!(desc.contains(&format!("{GP_REVEAL} <@100>")), "{desc}");
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(
            fields.len(),
            2,
            "no 'fooled everyone' field when someone was right"
        );
        assert_eq!(fields[0]["name"], "Guessed right");
        assert_eq!(fields[0]["value"], "<@200>, <@300>");
        assert_eq!(fields[1]["name"], GP_SCOREBOARD);
        assert_eq!(
            fields[1]["value"],
            "1. <@200> — 1\n2. <@300> — 1\n3. <@100> — 0"
        );

        let fooled = GpRoundResult {
            correct: vec![],
            fooled_everyone: true,
            scores: vec![(A, 1)],
            ..res
        };
        let v = serde_json::to_value(gp_reveal_embed(&fooled)).unwrap();
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0]["value"], GP_NOBODY_GUESSED);
        assert_eq!(fields[2]["name"], GP_FOOLED_EVERYONE);
        assert_eq!(fields[2]["value"], "<@100>");
    }

    #[test]
    fn scoreboard_and_status_embeds_json() {
        let v = serde_json::to_value(gp_scoreboard_embed(&[], GP_GAME_OVER)).unwrap();
        assert_eq!(v["title"], GP_GAME_OVER);
        assert_eq!(v["description"], "-");

        let lobby = GpStatus::Lobby {
            host: A,
            submitters: vec![("alice".into(), 2), ("bob".into(), 1)],
        };
        let v = serde_json::to_value(gp_status_embed(&lobby)).unwrap();
        assert_eq!(v["title"], GP_STATUS_LOBBY);
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields[0]["value"], "<@100>");
        assert_eq!(fields[1]["value"], "alice (2)\nbob (1)");
        // Titles never appear in status output.
        assert!(!serde_json::to_string(&v).unwrap().contains("alice0"));

        let playing = GpStatus::Playing {
            round: 2,
            total: 3,
            guessed: vec![],
            scores: vec![(A, 2)],
        };
        let v = serde_json::to_value(gp_status_embed(&playing)).unwrap();
        assert_eq!(v["title"], format!("{GP_STATUS_PLAYING} 2/3"));
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields[0]["value"], GP_NO_GUESSES_YET);
        assert_eq!(fields[1]["value"], "1. <@100> — 2");
    }

    #[test]
    fn round_message_bookkeeping() {
        let data = data();
        lobby_with(&data, &[(A, "alice", 1), (B, "bob", 1)]);
        assert_eq!(
            data.gp_set_round_message(G, 0, TC, MessageId::new(1))
                .unwrap_err(),
            CrackedError::StaleRound,
            "no rounds exist before begin"
        );
        data.gp_begin(G, A, &mut rng()).unwrap();
        assert_eq!(
            data.gp_set_round_message(G, 5, TC, MessageId::new(1))
                .unwrap_err(),
            CrackedError::StaleRound
        );
        data.gp_set_round_message(G, 0, TC, MessageId::new(42))
            .unwrap();

        // Only the *current* round can be revealed; a future index is ignored.
        assert!(data.gp_reveal_and_advance(G, 1).is_none());
        let res = data.gp_reveal_and_advance(G, 0).unwrap();
        assert_eq!(res.message, Some((TC, MessageId::new(42))));
        assert_eq!(res.text_channel, TC);
        assert_eq!((res.round_idx, res.total), (0, 2));
        // Dropdown options for the next round are sorted by name.
        assert_eq!(
            res.players,
            vec![(A, "alice".to_string()), (B, "bob".to_string())]
        );
        // The next round has no message yet.
        let res = data.gp_reveal_and_advance(G, 1).unwrap();
        assert_eq!(res.message, None);
        assert!(res.next.is_none());
    }

    #[test]
    fn scores_accumulate_and_tie_break_by_name() {
        let data = data();
        lobby_with(&data, &[(A, "alice", 1), (B, "bob", 2)]);
        data.gp_begin(G, A, &mut rng()).unwrap();
        let rounds: Vec<UserId> = data
            .gp_games
            .get(&G)
            .unwrap()
            .rounds
            .iter()
            .map(|r| r.submitter)
            .collect();
        // Carol (never submitted) guesses right every round; nobody else guesses.
        for (i, submitter) in rounds.iter().enumerate() {
            data.gp_record_guess(G, i, C, "carol".into(), *submitter)
                .unwrap();
            let res = data.gp_reveal_and_advance(G, i).unwrap();
            assert_eq!(res.correct, vec![C]);
        }
        let game = data.gp_games.get(&G).unwrap().clone();
        assert_eq!(game.phase, GpPhase::Finished);
        assert_eq!(game.scores.get(&C), Some(&(3 * GP_POINTS_CORRECT)));
        // Carol first; alice and bob tie at 0 and sort by name.
        assert_eq!(game.sorted_scores(), vec![(C, 3), (A, 0), (B, 0)]);
    }

    #[test]
    fn missing_game_paths() {
        let data = data();
        assert_eq!(
            data.gp_status(G).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert!(data.gp_remove(G).is_none());
        assert!(data.gp_reveal_and_advance(G, 0).is_none());
        assert_eq!(data.gp_voice_channel(G), None);
        assert_eq!(
            data.gp_record_guess(G, 0, A, "a".into(), B).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert_eq!(
            data.gp_begin(G, A, &mut rng()).unwrap_err(),
            CrackedError::NoGameInProgress
        );
        assert!(!data.gp_is_playing(G));
        assert!(!data.gp_is_active(G));
    }

    #[test]
    fn lobby_does_not_own_playback() {
        let data = data();
        lobby_with(&data, &[(A, "alice", 1), (B, "bob", 1)]);
        assert_eq!(data.gp_voice_channel(G), Some(VC));
        // Normal music keeps working until `begin`.
        assert!(!data.gp_is_playing(G));
        data.gp_begin(G, A, &mut rng()).unwrap();
        assert!(data.gp_is_playing(G));
        let game = data.gp_end(G, A, false).unwrap();
        assert_eq!(game.rounds.len(), 2);
        assert!(!data.gp_is_playing(G));
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
            vec!["begin", "end", "skip", "start", "status", "submit"]
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
        }
    }
}
