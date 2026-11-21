use super::state::{
    GpClip, GpPhase, GpReveal, GpRoundResult, GpStatus, GpTrackResult, GpTrackStart,
    GpWindowClosed, GpWindowOpened, GpWindowWarning, GP_CUSTOM_ID_PREFIX, GP_MAX_PLAYERS,
    GP_PICK_CANCEL_ID, GP_PICK_MENU_ID, GP_PICK_START_ID,
};
use crate::gp_prompts::{GpCategory, WrongPromptTheme};
use ::serenity::all::{GenericChannelId, GuildId, UserId};
use crack_common::format::duration_to_string;
use crack_common::messages::{
    GP_FOOLED_EVERYONE, GP_FULL_SONG, GP_FULL_SONG_NOTE, GP_GUESSED_RIGHT, GP_HOW_TO,
    GP_HOW_TO_TITLE, GP_LIKES, GP_LIKE_HINT, GP_LIKE_LABEL, GP_NOBODY_GUESSED, GP_NOBODY_YET,
    GP_PICK_CANCEL, GP_PICK_PLACEHOLDER, GP_PICK_START, GP_PICK_TITLE, GP_PROMPT_CLOSES_EARLY,
    GP_PROMPT_CLOSES_TITLE, GP_PROMPT_HOW_TO, GP_PROMPT_HOW_TO_TITLE, GP_QUEUE_CLEARED,
    GP_RESULTS_GUESSED_BY, GP_RESULTS_GUESSED_COUNT, GP_RESULTS_NOBODY_SCORED,
    GP_RESULTS_THIS_ROUND, GP_RESULTS_TITLE, GP_REVEAL, GP_REVEAL_HELD, GP_ROUND_HINT,
    GP_ROUND_TITLE, GP_RULES_TEXT, GP_SCOREBOARD, GP_SELECT_PLACEHOLDER, GP_SONG_TITLE, GP_STARTED,
    GP_STARTED_CLIP, GP_STARTED_CLIP_FROM, GP_STARTED_NO_RESULTS, GP_STARTED_REVEAL_SONG,
    GP_STARTED_ROUNDS, GP_STARTED_TIMER, GP_STATUS_CLOSES, GP_STATUS_GUESSED, GP_STATUS_LIKES,
    GP_STATUS_PLAYING, GP_STATUS_PROMPT, GP_STATUS_SCORES, GP_STATUS_SUBMITTED,
    GP_STATUS_SUBMITTING, GP_TITLE, GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE, GP_WINDOW_CLOSED,
    GP_WINDOW_CLOSED_SONGS, GP_WINDOW_EMPTY, GP_WINDOW_WARNING, GP_WINDOW_WARNING_IN,
};
use crack_common::{
    courier,
    format::{clip, escape, TrackLabel, DESCRIPTION_MAX, FIELD_MAX},
    message::CrackedMessage,
    render::{render, RenderCx, Rendered},
    transport::Transport,
};
use crack_model::{
    colour, Button, ButtonStyle, Card, CommunityRef, Field, Mention, Row, Select, SelectOption,
    UserRef,
};
use crack_types::discord::ToRef;

/// A submitted song's title is third-party text; YouTube caps titles at 100,
/// so a real one is never cut and only a pathological one is.
pub const GP_TITLE_MAX: usize = 100;

/// Every message `/gp` sends, as data. [`render_card`] turns one into Discord
/// output, and `messaging::render` reaches it through `CrackedMessage::Gp`, so
/// `/gp` has no way to Discord but the one renderer.
#[derive(Debug, Clone)]
pub enum GpCard {
    Rules,
    Status(GpStatusView),
    /// The category picker; its menu and Start/Cancel only while `open`.
    Picker {
        text: String,
        picked: Vec<GpCategory>,
        open: bool,
    },
    WrongPromptPicker {
        text: String,
        picked: Vec<WrongPromptTheme>,
        open: bool,
    },
    Prompt(GpPromptView),
    PromptClosed(GpClosedView),
    /// The song message and its guess/👍 controls.
    Song(GpSongView),
    Reveal(GpRevealView),
    RoundResults(GpRoundView),
    WrongPrompt(WrongPromptCard),
    /// `lead` rides as content above the embed (a lost game's `GP_LOST`).
    Scoreboard {
        scores: Vec<(UserRef, u32)>,
        title: &'static str,
        lead: Option<&'static str>,
    },
    /// Plain text, as `/gp` has always sent its one-liners.
    Line(String),
    /// `/gp start`'s notice: what was opened, and how.
    Started {
        category: String,
        rounds: usize,
        timer_secs: u64,
        clip: Option<GpClip>,
        reveal: GpReveal,
        round_results: bool,
        cleared_queue: bool,
    },
}

/// `UserId`s as refs, for the views below.
pub fn user_refs(scores: &[(UserId, u32)]) -> Vec<(UserRef, u32)> {
    scores.iter().map(|(id, n)| (id.to_ref(), *n)).collect()
}

/// A prompt window opening, with only what its card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpPromptView {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub category: Option<GpCategory>,
    pub closes_at: i64,
}

impl From<&GpWindowOpened> for GpPromptView {
    fn from(w: &GpWindowOpened) -> Self {
        Self {
            round_idx: w.round_idx,
            total_rounds: w.total_rounds,
            prompt: w.prompt.clone(),
            category: w.category,
            closes_at: w.closes_at,
        }
    }
}

/// A prompt window closing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpClosedView {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub category: Option<GpCategory>,
    pub count: usize,
}

impl From<&GpWindowClosed> for GpClosedView {
    fn from(c: &GpWindowClosed) -> Self {
        Self {
            round_idx: c.round_idx,
            total_rounds: c.total_rounds,
            prompt: c.prompt.clone(),
            category: c.category,
            count: c.count,
        }
    }
}

/// A song starting: its text and its dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpSongView {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub track_idx: usize,
    pub total_tracks: usize,
    pub prompt: String,
    pub title: String,
    pub url: String,
    pub players: Vec<(UserRef, String)>,
    pub guessable: bool,
    pub guild: CommunityRef,
}

impl From<(&GpTrackStart, GuildId)> for GpSongView {
    fn from((s, guild): (&GpTrackStart, GuildId)) -> Self {
        Self {
            round_idx: s.round_idx,
            total_rounds: s.total_rounds,
            track_idx: s.track_idx,
            total_tracks: s.total_tracks,
            prompt: s.prompt.clone(),
            title: s.track.get_title(),
            url: s.track.get_url(),
            players: s
                .players
                .iter()
                .map(|(id, name)| (id.to_ref(), name.clone()))
                .collect(),
            guessable: s.guessable,
            guild: guild.to_ref(),
        }
    }
}

/// A song that has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpRevealView {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub track_idx: usize,
    pub total_tracks: usize,
    pub prompt: String,
    pub submitter: UserRef,
    pub title: String,
    pub url: String,
    pub correct: Vec<UserRef>,
    pub fooled_everyone: bool,
    pub likes: usize,
    pub played_full: bool,
    pub guessable: bool,
    pub scores: Vec<(UserRef, u32)>,
    pub failed: bool,
    pub held: bool,
}

impl From<&GpTrackResult> for GpRevealView {
    fn from(r: &GpTrackResult) -> Self {
        Self {
            round_idx: r.round_idx,
            total_rounds: r.total_rounds,
            track_idx: r.track_idx,
            total_tracks: r.total_tracks,
            prompt: r.prompt.clone(),
            submitter: r.submitter.to_ref(),
            title: r.title.clone(),
            url: r.url.clone(),
            correct: r.correct.iter().map(ToRef::to_ref).collect(),
            fooled_everyone: r.fooled_everyone,
            likes: r.likes,
            played_full: r.played_full,
            guessable: r.guessable,
            scores: user_refs(&r.scores),
            failed: r.failed,
            held: r.held,
        }
    }
}

/// One song on the round-results card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpSongRow {
    pub submitter: UserRef,
    pub title: String,
    pub correct: Vec<UserRef>,
    pub fooled_everyone: bool,
    pub likes: usize,
    pub played_full: bool,
    pub failed: bool,
}

/// A round as it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpRoundView {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub prompt: String,
    pub guessable: bool,
    pub songs: Vec<GpSongRow>,
    pub points: Vec<(UserRef, u32)>,
    pub scores: Vec<(UserRef, u32)>,
}

impl From<&GpRoundResult> for GpRoundView {
    fn from(r: &GpRoundResult) -> Self {
        Self {
            round_idx: r.round_idx,
            total_rounds: r.total_rounds,
            prompt: r.prompt.clone(),
            guessable: r.guessable,
            songs: r
                .songs
                .iter()
                .map(|s| GpSongRow {
                    submitter: s.submitter.to_ref(),
                    title: s.title.clone(),
                    correct: s.correct.iter().map(ToRef::to_ref).collect(),
                    fooled_everyone: s.fooled_everyone,
                    likes: s.likes,
                    played_full: s.played_full,
                    failed: s.failed,
                })
                .collect(),
            points: user_refs(&r.points),
            scores: user_refs(&r.scores),
        }
    }
}

/// `/gp status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpStatusView {
    Submitting {
        host: UserRef,
        round: usize,
        total: usize,
        prompt: String,
        closes_at: i64,
        submitted: Vec<String>,
        scores: Vec<(UserRef, u32)>,
    },
    Playing {
        round: usize,
        total: usize,
        track: usize,
        tracks: usize,
        prompt: String,
        guessed: Vec<String>,
        likes: usize,
        scores: Vec<(UserRef, u32)>,
    },
    WrongPrompt {
        round: usize,
        total: usize,
        phase: GpPhase,
        closes_at: Option<i64>,
        count: usize,
        scores: Vec<(UserRef, u32)>,
    },
}

impl From<&GpStatus> for GpStatusView {
    fn from(status: &GpStatus) -> Self {
        match status {
            GpStatus::Submitting {
                host,
                round,
                total,
                prompt,
                closes_at,
                submitted,
                scores,
            } => Self::Submitting {
                host: host.to_ref(),
                round: *round,
                total: *total,
                prompt: prompt.clone(),
                closes_at: *closes_at,
                submitted: submitted.clone(),
                scores: user_refs(scores),
            },
            GpStatus::Playing {
                round,
                total,
                track,
                tracks,
                prompt,
                guessed,
                likes,
                scores,
            } => Self::Playing {
                round: *round,
                total: *total,
                track: *track,
                tracks: *tracks,
                prompt: prompt.clone(),
                guessed: guessed.clone(),
                likes: *likes,
                scores: user_refs(scores),
            },
            GpStatus::WrongPrompt {
                round,
                total,
                phase,
                closes_at,
                count,
                scores,
            } => Self::WrongPrompt {
                round: *round,
                total: *total,
                phase: *phase,
                closes_at: *closes_at,
                count: *count,
                scores: user_refs(scores),
            },
        }
    }
}

impl From<GpCard> for CrackedMessage {
    fn from(card: GpCard) -> Self {
        CrackedMessage::Gp(Box::new(card))
    }
}

/// The Wrong Prompt round cards. Public cards carry neither assignments nor
/// ballot counts until the result, and private cards are sent as interactions.
#[derive(Debug, Clone)]
pub enum WrongPromptCard {
    Opening {
        round_idx: usize,
        total_rounds: usize,
        closes_at: i64,
        prompt_id: Option<String>,
    },
    PrivatePrompt {
        prompt: String,
    },
    Clip {
        round_idx: usize,
        total_rounds: usize,
        track_idx: usize,
        total_tracks: usize,
        state: WrongPromptClipState,
    },
    Discussion {
        round_idx: usize,
        total_rounds: usize,
        vote_at: i64,
    },
    Voting {
        round_idx: usize,
        total_rounds: usize,
        closes_at: i64,
        ballot_id: Option<String>,
    },
    Ballot {
        select_id: String,
        track_indices: Vec<usize>,
    },
    Results(WrongPromptRevealView),
    Final {
        scores: Vec<(UserId, u32)>,
    },
    Voided {
        round_idx: usize,
        total_rounds: usize,
        reason: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrongPromptClipState {
    Playing,
    Played,
    Failed,
}

#[derive(Debug, Clone)]
pub struct WrongPromptRevealView {
    pub round_idx: usize,
    pub total_rounds: usize,
    pub majority: String,
    pub alternate: String,
    pub odd_player: UserId,
    pub caught: bool,
    pub songs: Vec<WrongPromptSongView>,
    pub points: Vec<(UserId, u32)>,
    pub scores: Vec<(UserId, u32)>,
}

#[derive(Debug, Clone)]
pub struct WrongPromptSongView {
    pub track_idx: usize,
    pub title: String,
    pub submitter: UserId,
    pub votes: Option<usize>,
}

/// Build the reveal from a scored round. An ordinary GP round has no paired prompts.
pub fn wrong_prompt_results(result: &GpRoundResult) -> Option<WrongPromptRevealView> {
    let outcome = result.wrong_prompt.as_ref()?;
    Some(WrongPromptRevealView {
        round_idx: result.round_idx,
        total_rounds: result.total_rounds,
        majority: outcome.majority.clone(),
        alternate: outcome.alternate.clone(),
        odd_player: outcome.odd_player,
        caught: outcome.caught,
        songs: result
            .songs
            .iter()
            .enumerate()
            .map(|(track_idx, song)| WrongPromptSongView {
                track_idx,
                title: song.title.clone(),
                submitter: song.submitter,
                votes: if song.failed {
                    None
                } else {
                    outcome.votes.get(track_idx).copied()
                },
            })
            .collect(),
        points: result.points.clone(),
        scores: result.scores.clone(),
    })
}

fn wrong_prompt_title(round_idx: usize, total_rounds: usize) -> String {
    format!("Wrong Prompt · Round {}/{total_rounds}", round_idx + 1)
}

fn wrong_prompt_card(title: String, description: String, colour: u32) -> Rendered {
    Rendered::card(Card {
        title: Some(title),
        description: Some(description),
        colour: Some(colour),
        ..Card::default()
    })
}

fn wrong_prompt_button(id: &str, label: &str) -> Vec<Row> {
    vec![Row::Buttons(vec![Button {
        id: id.to_owned(),
        label: Some(label.to_owned()),
        emoji: None,
        style: ButtonStyle::Primary,
        disabled: false,
    }])]
}

impl WrongPromptCard {
    fn render(&self) -> Rendered {
        match self {
            Self::Opening {
                round_idx,
                total_rounds,
                closes_at,
                prompt_id,
            } => {
                let title = wrong_prompt_title(*round_idx, *total_rounds);
                match prompt_id {
                    Some(id) => wrong_prompt_card(
                        title,
                        format!(
                            "Open your prompt and keep it private. Pick a song with **/gp submit**.\n\nSubmissions close <t:{closes_at}:R>."
                        ),
                        colour::FOOYOO,
                    )
                    .with_rows(wrong_prompt_button(id, "See my prompt")),
                    None => wrong_prompt_card(
                        title,
                        "Submissions are closed.".into(),
                        colour::DARKER_GREY,
                    ),
                }
            },
            Self::PrivatePrompt { prompt } => wrong_prompt_card(
                "Your prompt".into(),
                format!("**{}**\n\nPick a song with **/gp submit**.", escape(prompt)),
                colour::FOOYOO,
            ),
            Self::Clip {
                round_idx,
                total_rounds,
                track_idx,
                total_tracks,
                state,
            } => {
                let title = format!(
                    "{} · Clip {}/{total_tracks}",
                    wrong_prompt_title(*round_idx, *total_rounds),
                    track_idx + 1,
                );
                let (description, colour) = match state {
                    WrongPromptClipState::Playing => ("Listen for the odd pick.", colour::BLURPLE),
                    WrongPromptClipState::Played => ("Clip ended.", colour::DARKER_GREY),
                    WrongPromptClipState::Failed => {
                        ("This clip is unavailable for voting.", colour::DARKER_GREY)
                    },
                };
                wrong_prompt_card(title, description.into(), colour)
            },
            Self::Discussion {
                round_idx,
                total_rounds,
                vote_at,
            } => wrong_prompt_card(
                wrong_prompt_title(*round_idx, *total_rounds),
                format!(
                    "Which clip came from the other prompt? Compare what you heard and keep your prompt wording private.\n\nVoting opens <t:{vote_at}:R>."
                ),
                colour::BLURPLE,
            ),
            Self::Voting {
                round_idx,
                total_rounds,
                closes_at,
                ballot_id,
            } => {
                let title = wrong_prompt_title(*round_idx, *total_rounds);
                match ballot_id {
                    Some(id) => wrong_prompt_card(
                        title,
                        format!(
                            "Which clip came from the other prompt? Open your private ballot to vote.\n\nVoting closes <t:{closes_at}:R>."
                        ),
                        colour::BLURPLE,
                    )
                    .with_rows(wrong_prompt_button(id, "Vote")),
                    None => wrong_prompt_card(title, "Voting is closed.".into(), colour::DARKER_GREY),
                }
            },
            Self::Ballot {
                select_id,
                track_indices,
            } => wrong_prompt_card(
                "Your ballot".into(),
                "Choose the clip you think came from the other prompt.".into(),
                colour::BLURPLE,
            )
            .with_rows(vec![Row::Select(Select {
                id: select_id.clone(),
                placeholder: Some("Pick a clip".into()),
                options: track_indices
                    .iter()
                    .map(|idx| SelectOption {
                        label: format!("Clip {}", idx + 1),
                        value: idx.to_string(),
                        default: false,
                    })
                    .collect(),
                min: Some(1),
                max: Some(1),
            })]),
            Self::Results(result) => Rendered::card(wrong_prompt_reveal_embed(result)),
            Self::Final { scores } => Rendered::card(wrong_prompt_final_embed(scores)),
            Self::Voided {
                round_idx,
                total_rounds,
                reason,
            } => wrong_prompt_card(
                wrong_prompt_title(*round_idx, *total_rounds),
                format!("This round didn’t count. {reason}"),
                colour::DARKER_GREY,
            ),
        }
    }
}

fn wrong_prompt_reveal_embed(result: &WrongPromptRevealView) -> Card {
    let outcome = if result.caught {
        "The room caught the odd pick."
    } else {
        "The odd pick slipped through."
    };
    let songs = result
        .songs
        .iter()
        .map(|song| {
            let submitter = mention(&song.submitter.to_ref());
            let title = clip(&song_name(&song.title), 84);
            let votes = match song.votes {
                Some(1) => "1 vote".into(),
                Some(votes) => format!("{votes} votes"),
                None => "Unavailable for voting".into(),
            };
            let odd = if song.submitter == result.odd_player {
                " · **Odd pick**"
            } else {
                ""
            };
            format!(
                "**Clip {}** · {title} · {submitter} · {votes}{odd}",
                song.track_idx + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Card {
        title: Some(format!(
            "{} · Results",
            wrong_prompt_title(result.round_idx, result.total_rounds)
        )),
        description: Some(clip(
            &format!(
                "**{outcome}**\n\n**Majority prompt** · {}\n**Odd prompt** · {}\n\n{songs}",
                escape(&result.majority),
                escape(&result.alternate),
            ),
            DESCRIPTION_MAX,
        )),
        fields: vec![
            field(
                GP_RESULTS_THIS_ROUND,
                clip(&points_lines(&user_refs(&result.points)), FIELD_MAX),
                false,
            ),
            field(
                GP_SCOREBOARD,
                clip(&scores_lines(&user_refs(&result.scores)), FIELD_MAX),
                false,
            ),
        ],
        colour: Some(colour::DARK_GOLD),
        ..Card::default()
    }
}

fn wrong_prompt_final_embed(scores: &[(UserId, u32)]) -> Card {
    let description = match scores.first() {
        Some((_, high_score)) if *high_score > 0 => {
            let winners: Vec<_> = scores
                .iter()
                .take_while(|(_, points)| points == high_score)
                .map(|(who, _)| mention(&who.to_ref()))
                .collect();
            let unit = if *high_score == 1 { "point" } else { "points" };
            match winners.as_slice() {
                [winner] => format!("{winner} wins with **{high_score} {unit}**."),
                _ if winners.join(", ").chars().count() > DESCRIPTION_MAX - 100 => {
                    format!(
                        "{} players share the win with **{high_score} {unit}** each.",
                        winners.len()
                    )
                },
                _ => format!(
                    "{} share the win with **{high_score} {unit}** each.",
                    winners.join(", ")
                ),
            }
        },
        _ => "No rounds were scored.".into(),
    };
    let totals = scores
        .iter()
        .map(|(who, points)| format!("{} · {points}", mention(&who.to_ref())))
        .collect::<Vec<_>>()
        .join("\n");
    Card {
        title: Some("Wrong Prompt · Final scores".into()),
        description: Some(description),
        fields: if totals.is_empty() {
            Vec::new()
        } else {
            vec![field(GP_SCOREBOARD, clip(&totals, FIELD_MAX), false)]
        },
        colour: Some(colour::GOLD),
        ..Card::default()
    }
}

/// A line is its text, everything else its kind. `Started`'s text is the
/// user-visible embed description (`render_card` uses it), not just a log line.
impl std::fmt::Display for GpCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Line(text) => return f.write_str(text),
            Self::Started {
                category,
                rounds,
                timer_secs,
                clip,
                reveal,
                round_results,
                cleared_queue,
            } => {
                return f.write_str(&format!(
                    "{} {} {} {} — {} {}{}{}{}{}",
                    GP_STARTED,
                    rounds,
                    GP_STARTED_ROUNDS,
                    category,
                    duration_to_string(std::time::Duration::from_secs(*timer_secs)),
                    GP_STARTED_TIMER,
                    match clip {
                        Some(c) => format!(
                            " {} {} {} {}",
                            GP_STARTED_CLIP,
                            duration_to_string(c.length),
                            GP_STARTED_CLIP_FROM,
                            duration_to_string(c.start)
                        ),
                        None => String::new(),
                    },
                    match reveal {
                        GpReveal::Song => format!(" {}", GP_STARTED_REVEAL_SONG),
                        GpReveal::Round => String::new(),
                    },
                    if *round_results {
                        String::new()
                    } else {
                        format!(" {}", GP_STARTED_NO_RESULTS)
                    },
                    if *cleared_queue {
                        format!(" {}", GP_QUEUE_CLEARED)
                    } else {
                        String::new()
                    }
                ))
            },
            Self::Rules => "rules",
            Self::Status(_) => "status",
            Self::Picker { .. } => "picker",
            Self::WrongPromptPicker { .. } => "Wrong Prompt theme picker",
            Self::Prompt(_) => "prompt",
            Self::PromptClosed(_) => "prompt closed",
            Self::Song { .. } => "song",
            Self::Reveal(_) => "reveal",
            Self::RoundResults(_) => "round results",
            Self::WrongPrompt(_) => "Wrong Prompt",
            Self::Scoreboard { .. } => "scoreboard",
        };
        write!(f, "gp: {kind}")
    }
}

/// What a card looks like in Discord. Reached through `messaging::render`.
pub fn render_card(card: &GpCard, _cx: &RenderCx) -> Rendered {
    match card {
        GpCard::Rules => Rendered::card(gp_rules_embed()),
        GpCard::Status(status) => Rendered::card(gp_status_embed(status)),
        GpCard::Picker { text, picked, open } => {
            let out = Rendered::card(gp_pick_embed(text));
            if *open {
                out.with_rows(gp_pick_components(picked))
            } else {
                out
            }
        },
        GpCard::WrongPromptPicker { text, picked, open } => {
            let out = wrong_prompt_card(
                "Pick Wrong Prompt themes".into(),
                text.clone(),
                colour::FOOYOO,
            );
            if *open {
                out.with_rows(wrong_prompt_pick_components(picked))
            } else {
                out
            }
        },
        GpCard::Prompt(w) => Rendered::card(gp_prompt_embed(w)),
        GpCard::PromptClosed(c) => Rendered::card(gp_prompt_closed_embed(c)),
        GpCard::Song(song) => Rendered::card(gp_track_embed(song)).with_rows(gp_components(
            &song.guild,
            song.round_idx,
            song.track_idx,
            &song.players,
            song.guessable,
        )),
        GpCard::Reveal(res) => Rendered::card(gp_reveal_embed(res)),
        GpCard::RoundResults(r) => Rendered::card(gp_round_results_embed(r)),
        GpCard::WrongPrompt(card) => card.render(),
        GpCard::Scoreboard {
            scores,
            title,
            lead,
        } => {
            let out = Rendered::card(gp_scoreboard_embed(scores, title));
            match lead {
                Some(lead) => out.with_content(*lead),
                None => out,
            }
        },
        GpCard::Line(text) => Rendered::text(text.clone()),
        GpCard::Started { .. } => Rendered::card(Card {
            description: Some(clip(&card.to_string(), DESCRIPTION_MAX)),
            colour: Some(colour::BLUE),
            ..Card::default()
        }),
    }
}

impl crack_common::render::Payload for GpCard {
    fn render(&self, cx: &RenderCx) -> Rendered {
        render_card(self, cx)
    }
}

/// Post a card to a channel. Best effort: a failure is logged with the card.
pub async fn gp_post(transport: &dyn Transport, channel: GenericChannelId, card: GpCard) {
    if let Err(e) = courier::post_message(transport, channel, &gp_rendered(card.clone())).await {
        tracing::warn!("gp: posting \"{card}\" to {channel} failed: {e}");
    }
}

/// A card as a ready body, through the one renderer.
pub fn gp_rendered(card: GpCard) -> Rendered {
    render(&card.into(), &RenderCx::now())
}

/// A submitted song as `[**title**](url)`: escaped, capped at
/// [`GP_TITLE_MAX`], and a link only for an http(s) URL.
fn song_link(title: &str, url: &str) -> String {
    TrackLabel {
        title: Some(title.to_owned()),
        url: Some(url.to_owned()),
        duration: None,
    }
    .linked(GP_TITLE_MAX)
}

/// A submitted song's title alone: escaped and capped.
fn song_name(title: &str) -> String {
    TrackLabel {
        title: Some(title.to_owned()),
        url: None,
        duration: None,
    }
    .title_text(GP_TITLE_MAX)
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

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only a test builds a custom id from a GuildId; the game uses custom_id_for"
    )
)]
pub fn gp_custom_id(
    kind: GpComponent,
    guild_id: GuildId,
    round_idx: usize,
    track_idx: usize,
) -> String {
    custom_id_for(kind, &guild_id.get().to_string(), round_idx, track_idx)
}

/// [`gp_custom_id`] for a guild already written as its snowflake.
fn custom_id_for(kind: GpComponent, guild: &str, round_idx: usize, track_idx: usize) -> String {
    format!(
        "{GP_CUSTOM_ID_PREFIX}{}:{guild}:{round_idx}:{track_idx}",
        kind.tag()
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
    guild: &CommunityRef,
    round_idx: usize,
    track_idx: usize,
    players: &[(UserRef, String)],
    guessable: bool,
) -> Vec<Row> {
    let mut rows = Vec::with_capacity(2);
    if guessable {
        let options = players
            .iter()
            .take(GP_MAX_PLAYERS)
            .map(|(id, name)| SelectOption {
                label: name.clone(),
                value: id.raw().as_str().to_string(),
                default: false,
            })
            .collect();
        rows.push(Row::Select(Select {
            id: custom_id_for(
                GpComponent::Guess,
                guild.raw().as_str(),
                round_idx,
                track_idx,
            ),
            placeholder: Some(GP_SELECT_PLACEHOLDER.to_string()),
            options,
            min: Some(1),
            max: Some(1),
        }));
    }
    rows.push(Row::Buttons(vec![Button {
        id: custom_id_for(
            GpComponent::Like,
            guild.raw().as_str(),
            round_idx,
            track_idx,
        ),
        label: Some(GP_LIKE_LABEL.to_string()),
        emoji: Some("👍".to_string()),
        style: ButtonStyle::Secondary,
        disabled: false,
    }]));
    rows
}

/// `/gp start`'s category picker: a menu of every category with `picked`
/// ticked, then Start -- greyed out until something is -- and Cancel.
pub fn gp_pick_components(picked: &[GpCategory]) -> Vec<Row> {
    let options = GpCategory::CATEGORIES
        .iter()
        .filter_map(|c| {
            Some(SelectOption {
                label: c.display().to_string(),
                value: c.key()?.to_string(),
                default: picked.contains(c),
            })
        })
        .collect();
    gp_pick_rows(GP_PICK_PLACEHOLDER, options)
}

fn wrong_prompt_pick_components(picked: &[WrongPromptTheme]) -> Vec<Row> {
    let options = WrongPromptTheme::ALL
        .iter()
        .map(|theme| SelectOption {
            label: theme.label().to_owned(),
            value: theme.key().to_owned(),
            default: picked.contains(theme),
        })
        .collect();
    gp_pick_rows("Themes to draw prompt pairs from", options)
}

fn gp_pick_rows(placeholder: &str, options: Vec<SelectOption>) -> Vec<Row> {
    let start_disabled = !options.iter().any(|option| option.default);
    let menu = Select {
        id: GP_PICK_MENU_ID.to_string(),
        placeholder: Some(placeholder.to_string()),
        min: Some(1),
        max: Some(options.len() as u8),
        options,
    };
    let button = |id: &str, label: &str, style, disabled| Button {
        id: id.to_string(),
        label: Some(label.to_string()),
        emoji: None,
        style,
        disabled,
    };
    vec![
        Row::Select(menu),
        Row::Buttons(vec![
            button(
                GP_PICK_START_ID,
                GP_PICK_START,
                ButtonStyle::Success,
                start_disabled,
            ),
            button(
                GP_PICK_CANCEL_ID,
                GP_PICK_CANCEL,
                ButtonStyle::Secondary,
                false,
            ),
        ]),
    ]
}

/// The categories ticked in the picker's menu, from its option values.
pub fn gp_picked<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<GpCategory> {
    values
        .into_iter()
        .filter_map(GpCategory::from_key)
        .collect()
}

/// A user written into message text.
fn mention(id: &UserRef) -> String {
    Mention::User(id.clone()).to_string()
}

fn field(name: impl Into<String>, value: impl Into<String>, inline: bool) -> Field {
    Field {
        name: name.into(),
        value: value.into(),
        inline,
    }
}

pub(in crate::gp) fn gp_pick_embed(text: &str) -> Card {
    Card {
        title: Some(GP_PICK_TITLE.to_string()),
        description: Some(text.to_string()),
        colour: Some(colour::FOOYOO),
        ..Card::default()
    }
}

fn scores_lines(scores: &[(UserRef, u32)]) -> String {
    if scores.is_empty() {
        return "-".to_string();
    }
    scores
        .iter()
        .enumerate()
        .map(|(i, (id, pts))| format!("{}. {} — {pts}", i + 1, mention(id)))
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

pub fn gp_rules_embed() -> Card {
    Card {
        title: Some(GP_TITLE.to_string()),
        description: Some(GP_RULES_TEXT.to_string()),
        fields: vec![field(GP_HOW_TO_TITLE, GP_HOW_TO, false)],
        colour: Some(colour::FOOYOO),
        ..Card::default()
    }
}

/// The prompt in bold, under its category when there is one to show.
fn prompt_text(prompt: &str, category: Option<GpCategory>) -> String {
    match category {
        Some(c) => format!("{}\n**{prompt}**", c.display()),
        None => format!("**{prompt}**"),
    }
}

pub fn gp_prompt_embed(w: &GpPromptView) -> Card {
    Card {
        title: Some(round_title(w.round_idx, w.total_rounds)),
        description: Some(prompt_text(&w.prompt, w.category)),
        fields: vec![
            field(GP_PROMPT_HOW_TO_TITLE, GP_PROMPT_HOW_TO, false),
            field(
                GP_PROMPT_CLOSES_TITLE,
                format!("<t:{}:R> {GP_PROMPT_CLOSES_EARLY}", w.closes_at),
                false,
            ),
        ],
        colour: Some(colour::FOOYOO),
        ..Card::default()
    }
}

pub fn gp_prompt_closed_embed(c: &GpClosedView) -> Card {
    let status = if c.count == 0 {
        GP_WINDOW_EMPTY.to_string()
    } else {
        format!("{GP_WINDOW_CLOSED} {} {GP_WINDOW_CLOSED_SONGS}", c.count)
    };
    Card {
        title: Some(round_title(c.round_idx, c.total_rounds)),
        description: Some(clip(
            &format!("{}\n\n{status}", prompt_text(&c.prompt, c.category)),
            DESCRIPTION_MAX,
        )),
        colour: Some(colour::DARKER_GREY),
        ..Card::default()
    }
}

pub fn gp_warning_text(w: &GpWindowWarning) -> String {
    format!(
        "{GP_WINDOW_WARNING} **{}** — {} {GP_WINDOW_WARNING_IN} <t:{}:R>",
        w.prompt, w.count, w.closes_at
    )
}

/// The song message: prompt, title and what to do. Never the submitter.
pub fn gp_track_embed(s: &GpSongView) -> Card {
    let hint = if s.guessable {
        format!("{GP_ROUND_HINT}\n{GP_LIKE_HINT}")
    } else {
        GP_LIKE_HINT.to_string()
    };
    Card {
        title: Some(song_title(
            s.round_idx,
            s.total_rounds,
            s.track_idx,
            s.total_tracks,
        )),
        description: Some(clip(
            &format!(
                "*{}*\n\n{}\n\n{hint}",
                s.prompt,
                song_link(&s.title, &s.url)
            ),
            DESCRIPTION_MAX,
        )),
        colour: Some(colour::BLURPLE),
        ..Card::default()
    }
}

/// The song message once the song has ended. Names the submitter and shows what
/// they scored -- unless the game reveals at the end of the round, in which
/// case it names nobody and shows no scores: only that the song is over, and
/// its likes, which give nothing away.
pub fn gp_reveal_embed(res: &GpRevealView) -> Card {
    let title = song_title(
        res.round_idx,
        res.total_rounds,
        res.track_idx,
        res.total_tracks,
    );
    let link = song_link(&res.title, &res.url);
    if res.held {
        let description = clip(
            &format!("*{}*\n\n{link}\n\n{GP_REVEAL_HELD}", res.prompt),
            DESCRIPTION_MAX,
        );
        let (fields, colour) = if res.failed {
            (
                vec![field(GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE, false)],
                colour::RED,
            )
        } else {
            (
                vec![field(GP_LIKES, res.likes.to_string(), true)],
                colour::DARKER_GREY,
            )
        };
        return Card {
            title: Some(title),
            description: Some(description),
            fields,
            colour: Some(colour),
            ..Card::default()
        };
    }
    let description = clip(
        &format!(
            "*{}*\n\n{link}\n\n{GP_REVEAL} {}",
            res.prompt,
            mention(&res.submitter)
        ),
        DESCRIPTION_MAX,
    );
    let scoreboard = field(
        GP_SCOREBOARD,
        clip(&scores_lines(&res.scores), FIELD_MAX),
        false,
    );
    if res.failed {
        return Card {
            title: Some(title),
            description: Some(description),
            fields: vec![
                field(GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE, false),
                scoreboard,
            ],
            colour: Some(colour::RED),
            ..Card::default()
        };
    }
    let mut fields = Vec::new();
    if res.guessable {
        let correct = if res.correct.is_empty() {
            GP_NOBODY_GUESSED.to_string()
        } else {
            res.correct
                .iter()
                .map(mention)
                .collect::<Vec<_>>()
                .join(", ")
        };
        fields.push(field(GP_GUESSED_RIGHT, clip(&correct, FIELD_MAX), false));
        if res.fooled_everyone {
            fields.push(field(GP_FOOLED_EVERYONE, mention(&res.submitter), false));
        }
    }
    if res.played_full {
        fields.push(field(GP_FULL_SONG, GP_FULL_SONG_NOTE, false));
    }
    fields.push(field(GP_LIKES, res.likes.to_string(), true));
    fields.push(scoreboard);
    Card {
        title: Some(title),
        description: Some(description),
        fields,
        colour: Some(colour::DARK_GREEN),
        ..Card::default()
    }
}

pub fn gp_scoreboard_embed(scores: &[(UserRef, u32)], title: &str) -> Card {
    Card {
        title: Some(title.to_string()),
        description: Some(clip(&scores_lines(scores), DESCRIPTION_MAX)),
        colour: Some(colour::GOLD),
        ..Card::default()
    }
}

fn points_lines(points: &[(UserRef, u32)]) -> String {
    if points.is_empty() {
        return GP_RESULTS_NOBODY_SCORED.to_string();
    }
    points
        .iter()
        .enumerate()
        .map(|(i, (id, pts))| format!("{}. {} — +{pts}", i + 1, mention(id)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line of the results per song. `names` lists who guessed right by
/// mention; otherwise it is a count, for a round too big for the names to fit.
fn song_result_line(i: usize, s: &GpSongRow, guessable: bool, names: bool) -> String {
    let mut line = format!(
        "{}. **{}** · {}",
        i + 1,
        song_name(&s.title),
        mention(&s.submitter)
    );
    if s.failed {
        line.push_str(&format!(" · {GP_TRACK_FAILED}"));
        return line;
    }
    if guessable {
        let guessed = if s.correct.is_empty() {
            GP_NOBODY_GUESSED.to_string()
        } else if names {
            s.correct.iter().map(mention).collect::<Vec<_>>().join(", ")
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
pub fn gp_round_results_embed(r: &GpRoundView) -> Card {
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
    if description.chars().count() > DESCRIPTION_MAX {
        description = lines(false);
    }
    description = clip(&description, DESCRIPTION_MAX);
    Card {
        title: Some(format!(
            "{} {GP_RESULTS_TITLE}",
            round_title(r.round_idx, r.total_rounds)
        )),
        description: Some(description),
        fields: vec![
            field(
                GP_RESULTS_THIS_ROUND,
                clip(&points_lines(&r.points), FIELD_MAX),
                false,
            ),
            field(
                GP_SCOREBOARD,
                clip(&scores_lines(&r.scores), FIELD_MAX),
                false,
            ),
        ],
        colour: Some(colour::DARK_GOLD),
        ..Card::default()
    }
}

pub fn gp_status_embed(status: &GpStatusView) -> Card {
    let list = |names: &[String]| {
        if names.is_empty() {
            GP_NOBODY_YET.to_string()
        } else {
            clip(
                &names
                    .iter()
                    .map(|n| escape(n))
                    .collect::<Vec<_>>()
                    .join(", "),
                FIELD_MAX,
            )
        }
    };
    match status {
        GpStatusView::Submitting {
            host,
            round,
            total,
            prompt,
            closes_at,
            submitted,
            scores,
        } => Card {
            title: Some(format!("{GP_STATUS_SUBMITTING} {round}/{total}")),
            fields: vec![
                field(GP_STATUS_PROMPT, prompt.clone(), false),
                field("Host", mention(host), true),
                field(GP_STATUS_CLOSES, format!("<t:{closes_at}:R>"), true),
                field(GP_STATUS_SUBMITTED, list(submitted), false),
                field(
                    GP_STATUS_SCORES,
                    clip(&scores_lines(scores), FIELD_MAX),
                    false,
                ),
            ],
            colour: Some(colour::FOOYOO),
            ..Card::default()
        },
        GpStatusView::Playing {
            round,
            total,
            track,
            tracks,
            prompt,
            guessed,
            likes,
            scores,
        } => Card {
            title: Some(format!(
                "{GP_STATUS_PLAYING} {round}/{total} · {GP_SONG_TITLE} {track}/{tracks}"
            )),
            fields: vec![
                field(GP_STATUS_PROMPT, prompt.clone(), false),
                field(GP_STATUS_GUESSED, list(guessed), false),
                field(GP_STATUS_LIKES, likes.to_string(), true),
                field(
                    GP_STATUS_SCORES,
                    clip(&scores_lines(scores), FIELD_MAX),
                    false,
                ),
            ],
            colour: Some(colour::BLURPLE),
            ..Card::default()
        },
        GpStatusView::WrongPrompt {
            round,
            total,
            phase,
            closes_at,
            count,
            scores,
        } => {
            let (label, detail) = match phase {
                GpPhase::Submitting => ("Submissions open", format!("{count} songs submitted.")),
                GpPhase::Playing => ("Playing clips", format!("{count} clips in this round.")),
                GpPhase::Discussing => ("Discussion", format!("{count} eligible clips.")),
                GpPhase::Voting => ("Voting open", format!("{count} eligible clips.")),
                GpPhase::Finished => ("Finished", String::new()),
            };
            let description = match closes_at {
                Some(closes_at) => format!("**{label}** · <t:{closes_at}:R>\n{detail}"),
                None => format!("**{label}**\n{detail}"),
            };
            Card {
                title: Some(wrong_prompt_title(round.saturating_sub(1), *total)),
                description: Some(description),
                fields: vec![field(
                    GP_STATUS_SCORES,
                    clip(&scores_lines(scores), FIELD_MAX),
                    false,
                )],
                colour: Some(colour::FOOYOO),
                ..Card::default()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_common::messages::{
        GP_QUEUE_CLEARED, GP_STARTED, GP_STARTED_NO_RESULTS, GP_STARTED_REVEAL_SONG,
    };
    use std::time::Duration;

    #[test]
    fn wrong_prompt_picker_shows_all_themes_and_remembers_picks() {
        let rows = wrong_prompt_pick_components(&[]);
        let [Row::Select(menu), Row::Buttons(buttons)] = rows.as_slice() else {
            panic!("expected a theme menu and its Start/Cancel buttons");
        };
        assert_eq!(menu.id, GP_PICK_MENU_ID);
        assert_eq!(menu.max, Some(12));
        assert_eq!(menu.options.len(), 12);
        assert!(buttons
            .iter()
            .any(|button| button.id == GP_PICK_START_ID && button.disabled));

        let picked = &WrongPromptTheme::ALL[1..3];
        let rows = wrong_prompt_pick_components(picked);
        let [Row::Select(menu), Row::Buttons(buttons)] = rows.as_slice() else {
            panic!("expected a theme menu and its Start/Cancel buttons");
        };
        let defaults: Vec<_> = menu
            .options
            .iter()
            .filter(|option| option.default)
            .map(|option| option.value.as_str())
            .collect();
        assert_eq!(
            defaults,
            picked.iter().map(|theme| theme.key()).collect::<Vec<_>>()
        );
        assert!(buttons
            .iter()
            .any(|button| button.id == GP_PICK_START_ID && !button.disabled));
    }

    #[test]
    fn wrong_prompt_final_names_joint_winners() {
        let scores = [
            (UserId::new(1), 3),
            (UserId::new(2), 3),
            (UserId::new(3), 1),
        ];
        let card = wrong_prompt_final_embed(&scores);
        assert_eq!(
            card.description.as_deref(),
            Some("<@1>, <@2> share the win with **3 points** each."),
        );
    }

    #[test]
    fn gp_started_renders_as_before() {
        let plain = CrackedMessage::Gp(Box::new(GpCard::Started {
            category: "🥹 Nostalgia".into(),
            rounds: 5,
            timer_secs: 180,
            clip: None,
            reveal: GpReveal::Song,
            round_results: true,
            cleared_queue: false,
        }));
        assert_eq!(
            render(&plain, &RenderCx::default()),
            Rendered::card(Card {
                description: Some("🎭 Game on! 5 rounds of 🥹 Nostalgia — 00:03:00 to submit each round. Submitters are revealed after each song.".into()),
                colour: Some(colour::BLUE),
                ..Card::default()
            })
        );
        let clipped = CrackedMessage::Gp(Box::new(GpCard::Started {
            category: "🥹 Nostalgia".into(),
            rounds: 5,
            timer_secs: 180,
            clip: Some(GpClip {
                start: Duration::from_secs(30),
                length: Duration::from_secs(20),
            }),
            reveal: GpReveal::Song,
            round_results: true,
            cleared_queue: false,
        }));
        assert_eq!(
            render(&clipped, &RenderCx::default()),
            Rendered::card(Card {
                description: Some("🎭 Game on! 5 rounds of 🥹 Nostalgia — 00:03:00 to submit each round. Playing 00:00:20 of each song, from 00:00:30 Submitters are revealed after each song.".into()),
                colour: Some(colour::BLUE),
                ..Card::default()
            })
        );
    }

    #[test]
    fn gp_started_messages_display() {
        let msg = CrackedMessage::Gp(Box::new(GpCard::Started {
            category: "🥹 Nostalgia".into(),
            rounds: 5,
            timer_secs: 180,
            clip: None,
            reveal: GpReveal::Song,
            round_results: true,
            cleared_queue: false,
        }));
        let s = msg.to_string();
        assert!(s.starts_with(&format!("{} 5", GP_STARTED)), "{s}");
        assert!(s.contains("🥹 Nostalgia"), "{s}");
        assert!(!s.contains(GP_QUEUE_CLEARED), "{s}");
        let msg = CrackedMessage::Gp(Box::new(GpCard::Started {
            category: "🎲 Random".into(),
            rounds: 3,
            timer_secs: 60,
            clip: None,
            reveal: GpReveal::Round,
            round_results: true,
            cleared_queue: true,
        }));
        assert!(msg.to_string().ends_with(&format!(" {}", GP_QUEUE_CLEARED)));
        let s = msg.to_string();
        assert!(
            !s.contains(GP_STARTED_REVEAL_SONG),
            "the default says nothing: {s}"
        );
        let msg = CrackedMessage::Gp(Box::new(GpCard::Started {
            category: "🎲 Random".into(),
            rounds: 3,
            timer_secs: 60,
            clip: None,
            reveal: GpReveal::Song,
            round_results: true,
            cleared_queue: false,
        }));
        let s = msg.to_string();
        assert!(s.ends_with(GP_STARTED_REVEAL_SONG), "{s}");
        assert!(!s.contains(GP_STARTED_NO_RESULTS), "{s}");
        let msg = CrackedMessage::Gp(Box::new(GpCard::Started {
            category: "🎲 Random".into(),
            rounds: 3,
            timer_secs: 60,
            clip: None,
            reveal: GpReveal::Song,
            round_results: false,
            cleared_queue: false,
        }));
        let s = msg.to_string();
        assert!(s.ends_with(GP_STARTED_NO_RESULTS), "{s}");
    }
}
