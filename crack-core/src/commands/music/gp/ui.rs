use super::state::*;
use crate::commands::music::gp_prompts::GpCategory;
use crate::messaging::messages::{
    GP_FOOLED_EVERYONE, GP_FULL_SONG, GP_FULL_SONG_NOTE, GP_GUESSED_RIGHT, GP_HOW_TO,
    GP_HOW_TO_TITLE, GP_LIKES, GP_LIKE_HINT, GP_LIKE_LABEL, GP_NOBODY_GUESSED, GP_NOBODY_YET,
    GP_PICK_CANCEL, GP_PICK_PLACEHOLDER, GP_PICK_START, GP_PICK_TITLE, GP_PROMPT_CLOSES_EARLY,
    GP_PROMPT_CLOSES_TITLE, GP_PROMPT_HOW_TO, GP_PROMPT_HOW_TO_TITLE, GP_RESULTS_GUESSED_BY,
    GP_RESULTS_GUESSED_COUNT, GP_RESULTS_NOBODY_SCORED, GP_RESULTS_THIS_ROUND, GP_RESULTS_TITLE,
    GP_REVEAL, GP_REVEAL_HELD, GP_ROUND_HINT, GP_ROUND_TITLE, GP_RULES_TEXT, GP_SCOREBOARD,
    GP_SELECT_PLACEHOLDER, GP_SONG_TITLE, GP_STATUS_CLOSES, GP_STATUS_GUESSED, GP_STATUS_LIKES,
    GP_STATUS_PLAYING, GP_STATUS_PROMPT, GP_STATUS_SCORES, GP_STATUS_SUBMITTED,
    GP_STATUS_SUBMITTING, GP_TITLE, GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE, GP_WINDOW_CLOSED,
    GP_WINDOW_CLOSED_SONGS, GP_WINDOW_EMPTY, GP_WINDOW_WARNING, GP_WINDOW_WARNING_IN,
};
use crate::messaging::{
    discord,
    format::{clip, escape, TrackLabel, DESCRIPTION_MAX, FIELD_MAX},
    message::CrackedMessage,
    render::{render, RenderCx, Rendered},
};
use ::serenity::all::{GuildId, UserId};
use crack_model::{Button, ButtonStyle, Card, Field, Mention, Row, Select, SelectOption};
use crack_types::discord::ToRef;

/// The colours `/gp` cards wear, as 0xRRGGBB.
const FOOYOO: u32 = 0x11_CA_80;
const BLURPLE: u32 = 0x72_89_DA;
const DARKER_GREY: u32 = 0x54_6E_7A;
const DARK_GREEN: u32 = 0x1F_8B_4C;
const DARK_GOLD: u32 = 0xC2_7C_0E;
const GOLD: u32 = 0xF1_C4_0F;
const RED: u32 = 0xE7_4C_3C;

/// A submitted song's title is third-party text; YouTube caps titles at 100,
/// so a real one is never cut and only a pathological one is.
pub const GP_TITLE_MAX: usize = 100;

/// Every message `/gp` sends, as data. [`render_card`] turns one into Discord
/// output, and `messaging::render` reaches it through `CrackedMessage::Gp`, so
/// `/gp` has no way to Discord but the one renderer.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "Song carries a ResolvedTrack (~2.4KB); a card is built once per message and travels boxed inside CrackedMessage::Gp"
)]
pub enum GpCard {
    Rules,
    Status(GpStatus),
    /// The category picker; its menu and Start/Cancel only while `open`.
    Picker {
        text: String,
        picked: Vec<GpCategory>,
        open: bool,
    },
    Prompt(GpWindowOpened),
    PromptClosed(GpWindowClosed),
    /// The song message and its guess/👍 controls. `GpTrackStart` has no guild.
    Song {
        start: GpTrackStart,
        guild: GuildId,
    },
    Reveal(GpTrackResult),
    RoundResults(GpRoundResult),
    /// `lead` rides as content above the embed (a lost game's `GP_LOST`).
    Scoreboard {
        scores: Vec<(UserId, u32)>,
        title: &'static str,
        lead: Option<&'static str>,
    },
    /// Plain text, as `/gp` has always sent its one-liners.
    Line(String),
}

impl From<GpCard> for CrackedMessage {
    fn from(card: GpCard) -> Self {
        CrackedMessage::Gp(Box::new(card))
    }
}

/// For logs: a line is its text, everything else its kind.
impl std::fmt::Display for GpCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Line(text) => return f.write_str(text),
            Self::Rules => "rules",
            Self::Status(_) => "status",
            Self::Picker { .. } => "picker",
            Self::Prompt(_) => "prompt",
            Self::PromptClosed(_) => "prompt closed",
            Self::Song { .. } => "song",
            Self::Reveal(_) => "reveal",
            Self::RoundResults(_) => "round results",
            Self::Scoreboard { .. } => "scoreboard",
        };
        write!(f, "gp: {kind}")
    }
}

/// What a card looks like in Discord. Reached through `messaging::render`.
pub fn render_card(card: &GpCard, _cx: &RenderCx) -> Rendered {
    let embed = |card: &Card| Rendered::embed(discord::embed(card));
    match card {
        GpCard::Rules => embed(&gp_rules_embed()),
        GpCard::Status(status) => embed(&gp_status_embed(status)),
        GpCard::Picker { text, picked, open } => {
            let out = embed(&gp_pick_embed(text));
            if *open {
                out.with_components(discord::action_rows(&gp_pick_components(picked)))
            } else {
                out
            }
        },
        GpCard::Prompt(w) => embed(&gp_prompt_embed(w)),
        GpCard::PromptClosed(c) => embed(&gp_prompt_closed_embed(c)),
        GpCard::Song { start, guild } => {
            embed(&gp_track_embed(start)).with_components(discord::action_rows(&gp_components(
                *guild,
                start.round_idx,
                start.track_idx,
                &start.players,
                start.guessable,
            )))
        },
        GpCard::Reveal(res) => embed(&gp_reveal_embed(res)),
        GpCard::RoundResults(r) => embed(&gp_round_results_embed(r)),
        GpCard::Scoreboard {
            scores,
            title,
            lead,
        } => {
            let out = embed(&gp_scoreboard_embed(scores, title));
            match lead {
                Some(lead) => out.with_content(*lead),
                None => out,
            }
        },
        GpCard::Line(text) => Rendered::text(text.clone()),
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
) -> Vec<Row> {
    let mut rows = Vec::with_capacity(2);
    if guessable {
        let options = players
            .iter()
            .take(GP_MAX_PLAYERS)
            .map(|(id, name)| SelectOption {
                label: name.clone(),
                value: id.to_string(),
                default: false,
            })
            .collect();
        rows.push(Row::Select(Select {
            id: gp_custom_id(GpComponent::Guess, guild_id, round_idx, track_idx),
            placeholder: Some(GP_SELECT_PLACEHOLDER.to_string()),
            options,
            min: Some(1),
            max: Some(1),
        }));
    }
    rows.push(Row::Buttons(vec![Button {
        id: gp_custom_id(GpComponent::Like, guild_id, round_idx, track_idx),
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
    let menu = Select {
        id: GP_PICK_MENU_ID.to_string(),
        placeholder: Some(GP_PICK_PLACEHOLDER.to_string()),
        options,
        min: Some(1),
        max: Some(GpCategory::CATEGORIES.len() as u8),
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
                picked.is_empty(),
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
fn mention(id: UserId) -> String {
    Mention::User(id.to_ref()).to_string()
}

fn field(name: impl Into<String>, value: impl Into<String>, inline: bool) -> Field {
    Field {
        name: name.into(),
        value: value.into(),
        inline,
    }
}

pub(in crate::commands::music::gp) fn gp_pick_embed(text: &str) -> Card {
    Card {
        title: Some(GP_PICK_TITLE.to_string()),
        description: Some(text.to_string()),
        colour: Some(FOOYOO),
        ..Card::default()
    }
}

fn scores_lines(scores: &[(UserId, u32)]) -> String {
    if scores.is_empty() {
        return "-".to_string();
    }
    scores
        .iter()
        .enumerate()
        .map(|(i, (id, pts))| format!("{}. {} — {pts}", i + 1, mention(*id)))
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
        colour: Some(FOOYOO),
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

pub fn gp_prompt_embed(w: &GpWindowOpened) -> Card {
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
        colour: Some(FOOYOO),
        ..Card::default()
    }
}

pub fn gp_prompt_closed_embed(c: &GpWindowClosed) -> Card {
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
        colour: Some(DARKER_GREY),
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
pub fn gp_track_embed(s: &GpTrackStart) -> Card {
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
                song_link(&s.track.get_title(), &s.track.get_url())
            ),
            DESCRIPTION_MAX,
        )),
        colour: Some(BLURPLE),
        ..Card::default()
    }
}

/// The song message once the song has ended. Names the submitter and shows what
/// they scored -- unless the game reveals at the end of the round, in which
/// case it names nobody and shows no scores: only that the song is over, and
/// its likes, which give nothing away.
pub fn gp_reveal_embed(res: &GpTrackResult) -> Card {
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
                RED,
            )
        } else {
            (
                vec![field(GP_LIKES, res.likes.to_string(), true)],
                DARKER_GREY,
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
            mention(res.submitter)
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
            colour: Some(RED),
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
                .map(|id| mention(*id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        fields.push(field(GP_GUESSED_RIGHT, clip(&correct, FIELD_MAX), false));
        if res.fooled_everyone {
            fields.push(field(GP_FOOLED_EVERYONE, mention(res.submitter), false));
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
        colour: Some(DARK_GREEN),
        ..Card::default()
    }
}

pub fn gp_scoreboard_embed(scores: &[(UserId, u32)], title: &str) -> Card {
    Card {
        title: Some(title.to_string()),
        description: Some(clip(&scores_lines(scores), DESCRIPTION_MAX)),
        colour: Some(GOLD),
        ..Card::default()
    }
}

fn points_lines(points: &[(UserId, u32)]) -> String {
    if points.is_empty() {
        return GP_RESULTS_NOBODY_SCORED.to_string();
    }
    points
        .iter()
        .enumerate()
        .map(|(i, (id, pts))| format!("{}. {} — +{pts}", i + 1, mention(*id)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line of the results per song. `names` lists who guessed right by
/// mention; otherwise it is a count, for a round too big for the names to fit.
fn song_result_line(i: usize, s: &GpSongResult, guessable: bool, names: bool) -> String {
    let mut line = format!(
        "{}. **{}** · {}",
        i + 1,
        song_name(&s.title),
        mention(s.submitter)
    );
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
                .map(|id| mention(*id))
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
pub fn gp_round_results_embed(r: &GpRoundResult) -> Card {
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
        colour: Some(DARK_GOLD),
        ..Card::default()
    }
}

pub fn gp_status_embed(status: &GpStatus) -> Card {
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
        GpStatus::Submitting {
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
                field("Host", mention(*host), true),
                field(GP_STATUS_CLOSES, format!("<t:{closes_at}:R>"), true),
                field(GP_STATUS_SUBMITTED, list(submitted), false),
                field(
                    GP_STATUS_SCORES,
                    clip(&scores_lines(scores), FIELD_MAX),
                    false,
                ),
            ],
            colour: Some(FOOYOO),
            ..Card::default()
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
            colour: Some(BLURPLE),
            ..Card::default()
        },
    }
}
