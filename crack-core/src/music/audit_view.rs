//! Reading the queue audit log back: what `/auditlog` shows, and what it hides.
//! Spec: docs/superpowers/specs/2026-10-02-auditlog-command-design.md

use crate::messaging::messages::{
    AUDITLOG_GP_HIDDEN, AUDITLOG_TRUNCATED, PATREON_URL, PREMIUM_HISTORY,
};
use crate::music::audit::{Action, Source, TrackRef};
use chrono::{DateTime, Duration, Utc};

/// The most entries one `/auditlog` reply shows.
pub const AUDITLOG_LIMIT: usize = 200;
/// The longest window `since` accepts.
const MAX_SINCE_WEEKS: i64 = 52;
/// Titles longer than this are cut, with `…`.
pub(crate) const TITLE_MAX: usize = 40;
/// An `add` names at most this many tracks, then a count.
const ADD_NAMES: usize = 3;

/// One `queue_audit` row, as `/auditlog` reads it.
#[derive(Debug, Clone)]
pub struct AuditRow {
    pub at: DateTime<Utc>,
    pub actor_user_id: Option<i64>,
    pub source: String,
    pub command: String,
    pub action: String,
    pub detail: Action,
}

/// `90m`, `6h`, `2d`, `1w` (any case): a positive whole number and one unit,
/// at most 52 weeks. A leading `+` is rejected.
#[must_use]
pub fn parse_since(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.starts_with('+') {
        return None;
    }
    let unit = s.chars().last()?;
    let n: i64 = s[..s.len() - unit.len_utf8()].parse().ok()?;
    if n < 1 {
        return None;
    }
    let d = match unit.to_ascii_lowercase() {
        'm' => Duration::try_minutes(n)?,
        'h' => Duration::try_hours(n)?,
        'd' => Duration::try_days(n)?,
        'w' => Duration::try_weeks(n)?,
        _ => return None,
    };
    (d <= Duration::weeks(MAX_SINCE_WEEKS)).then_some(d)
}

/// Escape Discord markdown, links and mentions in text we did not write, and
/// flatten line breaks.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // A newline would split the one-line entry, and could split a page.
            '\n' | '\r' => out.push(' '),
            // `[`/`]` make masked links; `<` starts mentions and timestamps.
            '*' | '_' | '`' | '~' | '|' | '>' | '<' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(c);
            },
            _ => out.push(c),
        }
    }
    out
}

/// A track's title for display: cut at `TITLE_MAX` characters with `…`, or
/// `(untitled)`. Not escaped: `/auditlog` escapes the whole line's wording.
fn title_text(t: &TrackRef) -> String {
    match t.title.as_deref() {
        Some(raw) => cap(raw, TITLE_MAX),
        None => "(untitled)".to_owned(),
    }
}

/// `raw` cut at `max` characters with `…`, or whole if it fits. Not escaped.
pub(crate) fn cap(raw: &str, max: usize) -> String {
    let cut: String = raw.chars().take(max).collect();
    if raw.chars().count() > max {
        format!("{cut}…")
    } else {
        cut
    }
}

fn who(row: &AuditRow) -> String {
    match row.actor_user_id {
        Some(id) => format!("<@{id}>"),
        None => "bot".to_owned(),
    }
}

/// How the change was asked for: `/play`, `@skip`, `dashboard`, or the bot's
/// reason. Command names are ours, so nothing here needs escaping.
#[must_use]
pub fn how_text(row: &AuditRow) -> String {
    match row.source.as_str() {
        "slash" => format!("/{}", row.command),
        "prefix" => format!("@{}", row.command),
        "web" => "dashboard".to_owned(),
        _ => row.command.clone(),
    }
}

/// What changed, in plain text. The dashboard inserts it as text; `/auditlog`
/// escapes it (see [`what`]). One source of words for both.
#[must_use]
pub fn what_text(a: &Action) -> String {
    match a {
        Action::Add { tracks, .. } if tracks.len() == 1 => {
            format!("added {}", title_text(&tracks[0]))
        },
        Action::Add { tracks, .. } => {
            let names: Vec<String> = tracks.iter().take(ADD_NAMES).map(title_text).collect();
            let more = tracks.len().saturating_sub(ADD_NAMES);
            let tail = if more > 0 {
                format!(" (+{more})")
            } else {
                String::new()
            };
            format!("added {} tracks: {}{tail}", tracks.len(), names.join(", "))
        },
        Action::Remove { track, index } => {
            format!("removed {} from #{index}", title_text(track))
        },
        Action::Move { track, from, to } => format!("moved {} {from} → {to}", title_text(track)),
        Action::Skip { track: Some(t) } => format!("skipped {}", title_text(t)),
        Action::Skip { track: None } => "skipped".to_owned(),
        Action::Clear { removed } => format!("cleared {removed} tracks"),
        Action::Shuffle { count } => format!("shuffled {count} tracks"),
        Action::Stop { removed } => format!("stopped, {removed} tracks dropped"),
        Action::Pause => "paused".to_owned(),
        Action::Resume => "resumed".to_owned(),
        Action::Repeat { on: true } => "repeat on".to_owned(),
        Action::Repeat { on: false } => "repeat off".to_owned(),
        Action::Leave { discarded } => format!("left voice, {discarded} tracks discarded"),
    }
}

/// [`what_text`] escaped for Discord. Escaping the whole text equals escaping
/// each title: the fixed words contain none of the characters `escape` touches.
fn what(a: &Action) -> String {
    escape(&what_text(a))
}

/// `<t:UNIX:R> WHO · HOW — WHAT`.
#[must_use]
pub fn audit_line(row: &AuditRow) -> String {
    format!(
        "<t:{}:R> {} · {} — {}",
        row.at.timestamp(),
        who(row),
        how_text(row),
        what(&row.detail)
    )
}

/// `gp` itself or one of its subcommands (`gp skip`), not a command that
/// merely starts with those letters.
fn is_gp(command: &str) -> bool {
    command == "gp" || command.starts_with("gp ")
}

/// Whether a running `/gp` game hides this row: a `gp`/`gp …` command recorded
/// at or after the game's start. Titles in those rows are the answers.
#[must_use]
pub fn hidden_by_game(row: &AuditRow, running_since: Option<DateTime<Utc>>) -> bool {
    running_since.is_some_and(|start| is_gp(&row.command) && row.at >= start)
}

/// Drop a running `/gp` game's rows: their titles are the answers. Rows from
/// before the game started, and rows of other commands, are kept. Returns
/// whether anything was dropped, so the reply can say so.
#[must_use]
pub fn hide_running_game(
    rows: Vec<AuditRow>,
    running_since: Option<DateTime<Utc>>,
) -> (Vec<AuditRow>, bool) {
    let Some(start) = running_since else {
        return (rows, false);
    };
    let before = rows.len();
    let kept: Vec<AuditRow> = rows
        .into_iter()
        .filter(|r| !hidden_by_game(r, Some(start)))
        .collect();
    let hid = kept.len() != before;
    (kept, hid)
}

/// Whether the plan's history floor hides this row: it is older than `floor`.
/// A row exactly at the floor is shown. No floor hides nothing.
#[must_use]
pub fn older_than_floor(row: &AuditRow, floor: Option<DateTime<Utc>>) -> bool {
    floor.is_some_and(|f| row.at < f)
}

/// Drop the rows older than the plan's history floor. Returns whether anything
/// was dropped: older history exists, and the reply says it's premium.
///
/// This runs on what the query returned, not in SQL, so "nothing older" and
/// "older is premium" can be told apart without another query. Rows come
/// newest first, so this drops a tail.
#[must_use]
pub fn trim_to_floor(rows: Vec<AuditRow>, floor: Option<DateTime<Utc>>) -> (Vec<AuditRow>, bool) {
    if floor.is_none() {
        return (rows, false);
    }
    let before = rows.len();
    let kept: Vec<AuditRow> = rows
        .into_iter()
        .filter(|r| !older_than_floor(r, floor))
        .collect();
    let capped = kept.len() != before;
    (kept, capped)
}

/// The notice for history the free window hid, with the Patreon link.
#[must_use]
pub fn premium_history_line() -> String {
    format!("_{PREMIUM_HISTORY}_ [CrackTunes Patreon]({PATREON_URL})")
}

/// What `/auditlog` has to say, before paging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditlogReply {
    /// No row matched the filter.
    Empty,
    /// Rows matched, but all belong to the running `/gp` game.
    AllHidden,
    /// Rows matched, but all are older than the plan's history floor.
    OnlyOlder,
    /// The lines to page, leading notice lines included.
    Lines(Vec<String>),
}

/// Decide the reply from the rows fetched (newest first, at most
/// `AUDITLOG_LIMIT + 1`): cut to the plan's history floor, then to the limit,
/// hide a running game's rows, and prepend the notices. Pure; the notice texts come from `messages.rs`.
#[must_use]
pub fn compose_auditlog(
    rows: Vec<AuditRow>,
    running_since: Option<DateTime<Utc>>,
    floor: Option<DateTime<Utc>>,
) -> AuditlogReply {
    if rows.is_empty() {
        return AuditlogReply::Empty;
    }
    // The floor first: the 200-row cut and its notice speak only of rows this
    // server's plan shows.
    let (mut rows, capped) = trim_to_floor(rows, floor);
    if rows.is_empty() {
        return AuditlogReply::OnlyOlder;
    }
    let truncated = rows.len() > AUDITLOG_LIMIT;
    rows.truncate(AUDITLOG_LIMIT);
    let (rows, hid) = hide_running_game(rows, running_since);
    if rows.is_empty() {
        return AuditlogReply::AllHidden;
    }
    let mut lines = Vec::with_capacity(rows.len() + 3);
    if hid {
        lines.push(AUDITLOG_GP_HIDDEN.to_owned());
    }
    if truncated {
        lines.push(AUDITLOG_TRUNCATED.to_owned());
    }
    if capped {
        lines.push(premium_history_line());
    }
    lines.extend(rows.iter().map(audit_line));
    AuditlogReply::Lines(lines)
}

/// The `action` option's choices: exactly the names the recorder writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum ActionChoice {
    Add,
    Remove,
    Move,
    Skip,
    Clear,
    Shuffle,
    Stop,
    Pause,
    Resume,
    Repeat,
    Leave,
}

impl ActionChoice {
    /// The stored `action` value this choice filters on.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ActionChoice::Add => "add",
            ActionChoice::Remove => "remove",
            ActionChoice::Move => "move",
            ActionChoice::Skip => "skip",
            ActionChoice::Clear => "clear",
            ActionChoice::Shuffle => "shuffle",
            ActionChoice::Stop => "stop",
            ActionChoice::Pause => "pause",
            ActionChoice::Resume => "resume",
            ActionChoice::Repeat => "repeat",
            ActionChoice::Leave => "leave",
        }
    }

    /// The choice whose stored name is `s` (lowercase, as recorded).
    #[must_use]
    pub fn from_name(s: &str) -> Option<ActionChoice> {
        Some(match s {
            "add" => ActionChoice::Add,
            "remove" => ActionChoice::Remove,
            "move" => ActionChoice::Move,
            "skip" => ActionChoice::Skip,
            "clear" => ActionChoice::Clear,
            "shuffle" => ActionChoice::Shuffle,
            "stop" => ActionChoice::Stop,
            "pause" => ActionChoice::Pause,
            "resume" => ActionChoice::Resume,
            "repeat" => ActionChoice::Repeat,
            "leave" => ActionChoice::Leave,
            _ => return None,
        })
    }
}

/// The `source` option's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum SourceChoice {
    Slash,
    Prefix,
    Web,
    Bot,
}

impl SourceChoice {
    #[must_use]
    pub fn source(self) -> Source {
        match self {
            SourceChoice::Slash => Source::Slash,
            SourceChoice::Prefix => Source::Prefix,
            SourceChoice::Web => Source::Web,
            SourceChoice::Bot => Source::Bot,
        }
    }

    /// The choice whose stored spelling is `s`: `slash`, `prefix`, `web`, `bot`.
    #[must_use]
    pub fn from_name(s: &str) -> Option<SourceChoice> {
        Some(match s {
            "slash" => SourceChoice::Slash,
            "prefix" => SourceChoice::Prefix,
            "web" => SourceChoice::Web,
            "bot" => SourceChoice::Bot,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn t(title: &str) -> TrackRef {
        TrackRef {
            title: Some(title.into()),
            url: None,
        }
    }
    fn row(source: &str, command: &str, user: Option<i64>, action: Action) -> AuditRow {
        AuditRow {
            at: Utc.timestamp_opt(1_790_843_254, 0).unwrap(),
            actor_user_id: user,
            source: source.into(),
            command: command.into(),
            action: action.name().into(),
            detail: action,
        }
    }

    /// A plain `pause` by a member, recorded at `at`.
    fn row_at(at: DateTime<Utc>) -> AuditRow {
        AuditRow {
            at,
            ..row("slash", "pause", Some(7), Action::Pause)
        }
    }

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn a_row_exactly_at_the_floor_is_kept() {
        let floor = Some(ts(0));
        assert!(!older_than_floor(&row_at(ts(0)), floor));
        assert!(older_than_floor(&row_at(ts(-1)), floor));
        assert!(!older_than_floor(&row_at(ts(-1_000_000)), None));
    }

    #[test]
    fn the_trim_drops_older_rows_and_says_so() {
        let rows = vec![row_at(ts(10)), row_at(ts(0)), row_at(ts(-1))];
        let (kept, capped) = trim_to_floor(rows.clone(), Some(ts(0)));
        assert_eq!(kept.len(), 2);
        assert!(capped);
        let (kept, capped) = trim_to_floor(rows[..2].to_vec(), Some(ts(0)));
        assert_eq!(kept.len(), 2);
        assert!(!capped);
        let (kept, capped) = trim_to_floor(rows, None);
        assert_eq!(kept.len(), 3);
        assert!(!capped);
    }

    #[test]
    fn the_premium_notice_follows_the_others_only_when_rows_were_dropped() {
        let AuditlogReply::Lines(lines) =
            compose_auditlog(vec![row_at(ts(10)), row_at(ts(-10))], None, Some(ts(0)))
        else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], premium_history_line());
        assert!(premium_history_line().contains(PREMIUM_HISTORY));
        assert!(premium_history_line().contains(PATREON_URL));

        let AuditlogReply::Lines(lines) = compose_auditlog(vec![row_at(ts(10))], None, Some(ts(0)))
        else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), 1);
        assert_ne!(lines[0], premium_history_line());
    }

    #[test]
    fn only_older_rows_are_only_older() {
        assert_eq!(
            compose_auditlog(vec![row_at(ts(-10))], None, Some(ts(0))),
            AuditlogReply::OnlyOlder
        );
        assert_eq!(
            compose_auditlog(vec![], None, Some(ts(0))),
            AuditlogReply::Empty
        );
    }

    #[test]
    fn the_trim_runs_before_the_200_row_cut() {
        // 150 visible rows and 100 too old: 250 fetched, but only 150 may be
        // seen, so nothing was cut at 200.
        let mut rows: Vec<AuditRow> = (0..150).map(|i| row_at(ts(1000 - i))).collect();
        rows.extend((0..100).map(|i| row_at(ts(-1 - i))));
        let AuditlogReply::Lines(lines) = compose_auditlog(rows, None, Some(ts(0))) else {
            panic!("expected lines");
        };
        assert!(!lines.contains(&AUDITLOG_TRUNCATED.to_owned()));
        assert_eq!(lines.len(), 150 + 1);
        assert_eq!(lines[0], premium_history_line());
    }

    #[test]
    fn the_premium_notice_comes_after_the_game_notice() {
        let mut gp = row_at(ts(20));
        gp.command = "gp".to_owned();
        let rows = vec![gp, row_at(ts(10)), row_at(ts(-10))];
        let AuditlogReply::Lines(lines) = compose_auditlog(rows, Some(ts(5)), Some(ts(0))) else {
            panic!("expected lines");
        };
        assert_eq!(lines[0], AUDITLOG_GP_HIDDEN);
        assert_eq!(lines[1], premium_history_line());
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn since_accepts_minutes_hours_days_weeks() {
        assert_eq!(parse_since("90m"), Some(Duration::minutes(90)));
        assert_eq!(parse_since("6h"), Some(Duration::hours(6)));
        assert_eq!(parse_since("2d"), Some(Duration::days(2)));
        assert_eq!(parse_since("52w"), Some(Duration::weeks(52)));
        assert_eq!(parse_since(" 1w "), Some(Duration::weeks(1)));
        assert_eq!(parse_since("6H"), Some(Duration::hours(6)));
        assert_eq!(parse_since("90M"), Some(Duration::minutes(90)));
        assert_eq!(parse_since("2D"), Some(Duration::days(2)));
        assert_eq!(parse_since("1W"), Some(Duration::weeks(1)));
    }

    #[test]
    fn since_rejects_bad_input() {
        for bad in [
            "",
            "0h",
            "-1h",
            "+5h",
            "5",
            "h",
            "5y",
            "1.5h",
            "53w",
            "9999999999999999999m",
            "two days",
        ] {
            assert_eq!(parse_since(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn each_action_reads_as_one_line() {
        let cases = [
            (
                row(
                    "slash",
                    "play",
                    Some(7),
                    Action::Add {
                        tracks: vec![t("A")],
                        at: crate::music::audit::AddAt::Back,
                    },
                ),
                "<@7> · /play — added A",
            ),
            (
                row(
                    "slash",
                    "play",
                    Some(7),
                    Action::Add {
                        tracks: vec![t("A"), t("B"), t("C"), t("D"), t("E")],
                        at: crate::music::audit::AddAt::Back,
                    },
                ),
                "<@7> · /play — added 5 tracks: A, B, C (+2)",
            ),
            (
                row(
                    "slash",
                    "remove",
                    Some(7),
                    Action::Remove {
                        track: t("A"),
                        index: 3,
                    },
                ),
                "<@7> · /remove — removed A from #3",
            ),
            (
                row(
                    "web",
                    "dashboard move",
                    Some(7),
                    Action::Move {
                        track: t("A"),
                        from: 8,
                        to: 0,
                    },
                ),
                "<@7> · dashboard — moved A 8 → 0",
            ),
            (
                row(
                    "prefix",
                    "skip",
                    Some(7),
                    Action::Skip {
                        track: Some(t("A")),
                    },
                ),
                "<@7> · @skip — skipped A",
            ),
            (
                row("slash", "skip", Some(7), Action::Skip { track: None }),
                "<@7> · /skip — skipped",
            ),
            (
                row("slash", "clear", Some(7), Action::Clear { removed: 4 }),
                "<@7> · /clear — cleared 4 tracks",
            ),
            (
                row("slash", "shuffle", Some(7), Action::Shuffle { count: 9 }),
                "<@7> · /shuffle — shuffled 9 tracks",
            ),
            (
                row("slash", "gp start", Some(7), Action::Stop { removed: 2 }),
                "<@7> · /gp start — stopped, 2 tracks dropped",
            ),
            (
                row("bot", "autopause", None, Action::Pause),
                "bot · autopause — paused",
            ),
            (
                row("slash", "resume", Some(7), Action::Resume),
                "<@7> · /resume — resumed",
            ),
            (
                row("bot", "idle timeout", None, Action::Leave { discarded: 4 }),
                "bot · idle timeout — left voice, 4 tracks discarded",
            ),
        ];
        for (r, want) in cases {
            assert_eq!(audit_line(&r), format!("<t:1790843254:R> {want}"));
        }
    }

    #[test]
    fn titles_are_truncated_and_escaped() {
        let long = "x".repeat(60);
        let line = audit_line(&row(
            "slash",
            "remove",
            Some(1),
            Action::Remove {
                track: t(&long),
                index: 1,
            },
        ));
        assert!(line.contains(&format!("{}…", "x".repeat(40))), "{line}");
        let line = audit_line(&row(
            "slash",
            "remove",
            Some(1),
            Action::Remove {
                track: t("**bold** _it_ `c` |s| ~t~ >q"),
                index: 1,
            },
        ));
        assert!(
            line.contains(r"\*\*bold\*\* \_it\_ \`c\` \|s\| \~t\~ \>q"),
            "{line}"
        );
        let line = audit_line(&row(
            "slash",
            "remove",
            Some(1),
            Action::Remove {
                track: t("[x](u) <@5> <t:1:R>\r\nnext\nline"),
                index: 1,
            },
        ));
        assert!(
            line.contains(r"\[x\](u) \<@5\> \<t:1:R\>  next line"),
            "{line}"
        );
        assert_eq!(line.lines().count(), 1, "{line}");
        let line = audit_line(&row(
            "slash",
            "remove",
            Some(1),
            Action::Remove {
                track: TrackRef {
                    title: None,
                    url: None,
                },
                index: 1,
            },
        ));
        assert!(line.contains("removed (untitled) from #1"), "{line}");
    }

    #[test]
    fn a_running_games_rows_are_hidden_and_others_kept() {
        let start = Utc.timestamp_opt(1_790_843_000, 0).unwrap();
        let mut before = row(
            "bot",
            "gp",
            None,
            Action::Add {
                tracks: vec![t("old answer")],
                at: crate::music::audit::AddAt::Back,
            },
        );
        before.at = start - Duration::hours(1);
        let during_gp = row(
            "bot",
            "gp",
            None,
            Action::Add {
                tracks: vec![t("answer")],
                at: crate::music::audit::AddAt::Back,
            },
        );
        let during_member_gp = row(
            "slash",
            "gp skip",
            Some(7),
            Action::Skip {
                track: Some(t("answer")),
            },
        );
        let during_other = row("slash", "pause", Some(7), Action::Pause);
        let rows = vec![
            during_gp,
            during_member_gp,
            during_other.clone(),
            before.clone(),
        ];

        let (kept, hid) = hide_running_game(rows.clone(), Some(start));
        assert!(hid);
        assert_eq!(
            kept.iter().map(|r| r.command.as_str()).collect::<Vec<_>>(),
            vec!["pause", "gp"]
        );
        assert_eq!(kept[1].at, before.at);

        let (kept, hid) = hide_running_game(rows, None);
        assert!(!hid);
        assert_eq!(kept.len(), 4);
    }

    #[test]
    fn only_gp_and_its_subcommands_are_hidden() {
        let start = Utc.timestamp_opt(1_790_843_000, 0).unwrap();
        let rows = vec![
            row("slash", "gpx", Some(7), Action::Pause),
            row("slash", "gp", Some(7), Action::Pause),
            row("slash", "gp skip", Some(7), Action::Pause),
        ];
        let (kept, hid) = hide_running_game(rows, Some(start));
        assert!(hid);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].command, "gpx");
    }

    fn many(n: usize) -> Vec<AuditRow> {
        (0..n)
            .map(|_| row("slash", "pause", Some(7), Action::Pause))
            .collect()
    }

    #[test]
    fn compose_no_rows_is_empty() {
        assert_eq!(compose_auditlog(vec![], None, None), AuditlogReply::Empty);
    }

    #[test]
    fn compose_all_rows_hidden_is_not_empty() {
        let start = Utc.timestamp_opt(1_790_843_000, 0).unwrap();
        let rows = vec![
            row("bot", "gp", None, Action::Pause),
            row("slash", "gp skip", Some(7), Action::Pause),
        ];
        assert_eq!(
            compose_auditlog(rows, Some(start), None),
            AuditlogReply::AllHidden
        );
    }

    #[test]
    fn compose_over_the_limit_cuts_and_says_so() {
        let AuditlogReply::Lines(lines) = compose_auditlog(many(AUDITLOG_LIMIT + 1), None, None)
        else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), AUDITLOG_LIMIT + 1);
        assert_eq!(lines[0], AUDITLOG_TRUNCATED);
        let AuditlogReply::Lines(lines) = compose_auditlog(many(AUDITLOG_LIMIT), None, None) else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), AUDITLOG_LIMIT);
        assert!(!lines.contains(&AUDITLOG_TRUNCATED.to_owned()));
    }

    #[test]
    fn compose_a_hidden_row_adds_the_notice_first() {
        let start = Utc.timestamp_opt(1_790_843_000, 0).unwrap();
        let rows = vec![
            row("bot", "gp", None, Action::Pause),
            row("slash", "pause", Some(7), Action::Pause),
        ];
        let AuditlogReply::Lines(lines) = compose_auditlog(rows, Some(start), None) else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], AUDITLOG_GP_HIDDEN);
        assert!(lines[1].contains("/pause"), "{lines:?}");
    }

    #[test]
    fn action_choices_match_action_names() {
        let tr = t("a");
        let pairs = [
            (
                ActionChoice::Add,
                Action::Add {
                    tracks: vec![],
                    at: crate::music::audit::AddAt::Back,
                },
            ),
            (
                ActionChoice::Remove,
                Action::Remove {
                    track: tr.clone(),
                    index: 0,
                },
            ),
            (
                ActionChoice::Move,
                Action::Move {
                    track: tr.clone(),
                    from: 0,
                    to: 1,
                },
            ),
            (ActionChoice::Skip, Action::Skip { track: None }),
            (ActionChoice::Clear, Action::Clear { removed: 0 }),
            (ActionChoice::Shuffle, Action::Shuffle { count: 0 }),
            (ActionChoice::Stop, Action::Stop { removed: 0 }),
            (ActionChoice::Pause, Action::Pause),
            (ActionChoice::Resume, Action::Resume),
            (ActionChoice::Repeat, Action::Repeat { on: true }),
            (ActionChoice::Leave, Action::Leave { discarded: 0 }),
        ];
        for (c, a) in pairs {
            assert_eq!(c.name(), a.name());
        }
    }

    #[test]
    fn source_choices_match_source_names() {
        for (c, s) in [
            (SourceChoice::Slash, Source::Slash),
            (SourceChoice::Prefix, Source::Prefix),
            (SourceChoice::Web, Source::Web),
            (SourceChoice::Bot, Source::Bot),
        ] {
            assert_eq!(c.source(), s);
        }
    }

    #[test]
    fn web_wording_is_the_discord_wording_unescaped() {
        let a = Action::Move {
            track: t("*Bold* [x](y) <@1>"),
            from: 5,
            to: 2,
        };
        assert_eq!(what_text(&a), "moved *Bold* [x](y) <@1> 5 → 2");
        let r = row("slash", "move", Some(7), a);
        assert!(
            audit_line(&r).ends_with(r"<@7> · /move — moved \*Bold\* \[x\](y) \<@1\> 5 → 2"),
            "{}",
            audit_line(&r)
        );
        assert_eq!(how_text(&r), "/move");
    }

    #[test]
    fn web_wording_still_cuts_long_titles() {
        let long = "x".repeat(50);
        assert_eq!(
            what_text(&Action::Skip {
                track: Some(t(&long))
            }),
            format!("skipped {}…", "x".repeat(40))
        );
    }

    #[test]
    fn hidden_by_game_needs_a_running_game_a_gp_command_and_a_late_enough_row() {
        let start = Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
        let mut r = row("bot", "gp", None, Action::Pause);
        r.at = start;
        assert!(hidden_by_game(&r, Some(start)));
        assert!(!hidden_by_game(&r, None));
        r.at = start - chrono::Duration::seconds(1);
        assert!(!hidden_by_game(&r, Some(start)));
        let mut other = row("slash", "gpx", Some(1), Action::Pause);
        other.at = start;
        assert!(!hidden_by_game(&other, Some(start)));
    }

    #[test]
    fn repeat_is_described_and_filterable() {
        assert_eq!(what_text(&Action::Repeat { on: true }), "repeat on");
        assert_eq!(what_text(&Action::Repeat { on: false }), "repeat off");
        assert_eq!(
            ActionChoice::from_name("repeat"),
            Some(ActionChoice::Repeat)
        );
        assert_eq!(ActionChoice::Repeat.name(), "repeat");
    }

    #[test]
    fn choices_round_trip_through_their_names() {
        for c in [
            ActionChoice::Add,
            ActionChoice::Remove,
            ActionChoice::Move,
            ActionChoice::Skip,
            ActionChoice::Clear,
            ActionChoice::Shuffle,
            ActionChoice::Stop,
            ActionChoice::Pause,
            ActionChoice::Resume,
            ActionChoice::Repeat,
            ActionChoice::Leave,
        ] {
            assert_eq!(ActionChoice::from_name(c.name()), Some(c));
        }
        assert_eq!(ActionChoice::from_name("dance"), None);
        assert_eq!(
            ActionChoice::from_name("Move"),
            None,
            "stored names are lowercase"
        );
        for c in [
            SourceChoice::Slash,
            SourceChoice::Prefix,
            SourceChoice::Web,
            SourceChoice::Bot,
        ] {
            assert_eq!(SourceChoice::from_name(c.source().as_str()), Some(c));
        }
        assert_eq!(SourceChoice::from_name("email"), None);
    }
}
