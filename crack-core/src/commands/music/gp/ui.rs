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
    format::{clip, escape, TrackLabel, DESCRIPTION_MAX, FIELD_MAX},
    message::CrackedMessage,
    render::{render, RenderCx, Rendered},
};
use ::serenity::{
    all::{ButtonStyle, Colour, GuildId, Mentionable, UserId},
    builder::{
        CreateActionRow, CreateButton, CreateComponent, CreateEmbed, CreateSelectMenu,
        CreateSelectMenuKind, CreateSelectMenuOption,
    },
};
use std::borrow::Cow;

/// A submitted song's title is third-party text; YouTube caps titles at 100,
/// so a real one is never cut and only a pathological one is.
pub const GP_TITLE_MAX: usize = 100;

/// Every message `/gp` sends, as data. [`render_card`] turns one into Discord
/// output, and `messaging::render` reaches it through `CrackedMessage::Gp`, so
/// `/gp` has no way to Discord but the one renderer.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "a card is built once and boxed inside CrackedMessage; boxing Song's payload would change every call site for nothing"
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
    match card {
        GpCard::Rules => Rendered::embed(gp_rules_embed()),
        GpCard::Status(status) => Rendered::embed(gp_status_embed(status)),
        GpCard::Picker { text, picked, open } => {
            let out = Rendered::embed(gp_pick_embed(text));
            if *open {
                out.with_components(gp_pick_components(picked))
            } else {
                out
            }
        },
        GpCard::Prompt(w) => Rendered::embed(gp_prompt_embed(w)),
        GpCard::PromptClosed(c) => Rendered::embed(gp_prompt_closed_embed(c)),
        GpCard::Song { start, guild } => {
            Rendered::embed(gp_track_embed(start)).with_components(gp_components(
                *guild,
                start.round_idx,
                start.track_idx,
                &start.players,
                start.guessable,
            ))
        },
        GpCard::Reveal(res) => Rendered::embed(gp_reveal_embed(res)),
        GpCard::RoundResults(r) => Rendered::embed(gp_round_results_embed(r)),
        GpCard::Scoreboard {
            scores,
            title,
            lead,
        } => {
            let out = Rendered::embed(gp_scoreboard_embed(scores, title));
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

/// `/gp start`'s category picker: a menu of every category with `picked`
/// ticked, then Start -- greyed out until something is -- and Cancel.
pub fn gp_pick_components(picked: &[GpCategory]) -> Vec<CreateComponent<'static>> {
    let options: Vec<CreateSelectMenuOption<'static>> = GpCategory::CATEGORIES
        .iter()
        .filter_map(|c| {
            let key = c.key()?;
            Some(
                CreateSelectMenuOption::new(c.display(), key).default_selection(picked.contains(c)),
            )
        })
        .collect();
    let menu = CreateSelectMenu::new(
        GP_PICK_MENU_ID,
        CreateSelectMenuKind::String {
            options: Cow::Owned(options),
        },
    )
    .placeholder(GP_PICK_PLACEHOLDER)
    .min_values(1)
    .max_values(GpCategory::CATEGORIES.len() as u8);
    let start = CreateButton::new(GP_PICK_START_ID)
        .label(GP_PICK_START)
        .style(ButtonStyle::Success)
        .disabled(picked.is_empty());
    let cancel = CreateButton::new(GP_PICK_CANCEL_ID)
        .label(GP_PICK_CANCEL)
        .style(ButtonStyle::Secondary);
    vec![
        CreateComponent::ActionRow(CreateActionRow::SelectMenu(menu)),
        CreateComponent::ActionRow(CreateActionRow::Buttons(Cow::Owned(vec![start, cancel]))),
    ]
}

/// The categories ticked in the picker's menu, from its option values.
pub fn gp_picked<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<GpCategory> {
    values
        .into_iter()
        .filter_map(GpCategory::from_key)
        .collect()
}

pub(in crate::commands::music::gp) fn gp_pick_embed(text: &str) -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(GP_PICK_TITLE)
        .description(text.to_string())
        .colour(Colour::FOOYOO)
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

/// The prompt in bold, under its category when there is one to show.
fn prompt_text(prompt: &str, category: Option<GpCategory>) -> String {
    match category {
        Some(c) => format!("{}\n**{prompt}**", c.display()),
        None => format!("**{prompt}**"),
    }
}

pub fn gp_prompt_embed(w: &GpWindowOpened) -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(round_title(w.round_idx, w.total_rounds))
        .description(prompt_text(&w.prompt, w.category))
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
        .description(clip(
            &format!("{}\n\n{status}", prompt_text(&c.prompt, c.category)),
            DESCRIPTION_MAX,
        ))
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
        .description(clip(
            &format!(
                "*{}*\n\n{}\n\n{hint}",
                s.prompt,
                song_link(&s.track.get_title(), &s.track.get_url())
            ),
            DESCRIPTION_MAX,
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
            .description(clip(
                &format!(
                    "*{}*\n\n{}\n\n{GP_REVEAL_HELD}",
                    res.prompt,
                    song_link(&res.title, &res.url)
                ),
                DESCRIPTION_MAX,
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
            .description(clip(
                &format!(
                    "*{}*\n\n{}\n\n{GP_REVEAL} {}",
                    res.prompt,
                    song_link(&res.title, &res.url),
                    res.submitter.mention()
                ),
                DESCRIPTION_MAX,
            ))
            .field(GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE, false)
            .field(
                GP_SCOREBOARD,
                clip(&scores_lines(&res.scores), FIELD_MAX),
                false,
            )
            .colour(Colour::RED);
    }
    let mut e = CreateEmbed::new()
        .title(song_title(
            res.round_idx,
            res.total_rounds,
            res.track_idx,
            res.total_tracks,
        ))
        .description(clip(
            &format!(
                "*{}*\n\n{}\n\n{GP_REVEAL} {}",
                res.prompt,
                song_link(&res.title, &res.url),
                res.submitter.mention()
            ),
            DESCRIPTION_MAX,
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
        e = e.field(GP_GUESSED_RIGHT, clip(&correct, FIELD_MAX), false);
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
        clip(&scores_lines(&res.scores), FIELD_MAX),
        false,
    )
}

pub fn gp_scoreboard_embed(scores: &[(UserId, u32)], title: &str) -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(title.to_string())
        .description(clip(&scores_lines(scores), DESCRIPTION_MAX))
        .colour(Colour::GOLD)
}

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
    let mut line = format!(
        "{}. **{}** · {}",
        i + 1,
        song_name(&s.title),
        s.submitter.mention()
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
    if description.chars().count() > DESCRIPTION_MAX {
        description = lines(false);
    }
    description = clip(&description, DESCRIPTION_MAX);
    CreateEmbed::new()
        .title(format!(
            "{} {GP_RESULTS_TITLE}",
            round_title(r.round_idx, r.total_rounds)
        ))
        .description(description)
        .field(
            GP_RESULTS_THIS_ROUND,
            clip(&points_lines(&r.points), FIELD_MAX),
            false,
        )
        .field(
            GP_SCOREBOARD,
            clip(&scores_lines(&r.scores), FIELD_MAX),
            false,
        )
        .colour(Colour::DARK_GOLD)
}

pub fn gp_status_embed(status: &GpStatus) -> CreateEmbed<'static> {
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
        } => CreateEmbed::new()
            .title(format!("{GP_STATUS_SUBMITTING} {round}/{total}"))
            .field(GP_STATUS_PROMPT, prompt.clone(), false)
            .field("Host", host.mention().to_string(), true)
            .field(GP_STATUS_CLOSES, format!("<t:{closes_at}:R>"), true)
            .field(GP_STATUS_SUBMITTED, list(submitted), false)
            .field(
                GP_STATUS_SCORES,
                clip(&scores_lines(scores), FIELD_MAX),
                false,
            )
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
            .field(
                GP_STATUS_SCORES,
                clip(&scores_lines(scores), FIELD_MAX),
                false,
            )
            .colour(Colour::BLURPLE),
    }
}
