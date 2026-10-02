//! Reading the queue audit log back: what `/auditlog` shows, and what it hides.
//! Spec: docs/superpowers/specs/2026-10-02-auditlog-command-design.md

use crate::messaging::messages::{AUDITLOG_GP_HIDDEN, AUDITLOG_TRUNCATED};
use crate::music::audit::{Action, Source, TrackRef};
use chrono::{DateTime, Duration, Utc};

/// The most entries one `/auditlog` reply shows.
pub const AUDITLOG_LIMIT: usize = 200;
/// The longest window `since` accepts.
const MAX_SINCE_WEEKS: i64 = 52;
/// Titles longer than this are cut, with `…`.
const TITLE_MAX: usize = 40;
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
fn escape(s: &str) -> String {
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

fn title(t: &TrackRef) -> String {
    let Some(raw) = t.title.as_deref() else {
        return "(untitled)".to_owned();
    };
    let cut: String = raw.chars().take(TITLE_MAX).collect();
    let cut = if raw.chars().count() > TITLE_MAX {
        format!("{cut}…")
    } else {
        cut
    };
    escape(&cut)
}

fn who(row: &AuditRow) -> String {
    match row.actor_user_id {
        Some(id) => format!("<@{id}>"),
        None => "bot".to_owned(),
    }
}

fn how(row: &AuditRow) -> String {
    match row.source.as_str() {
        "slash" => format!("/{}", row.command),
        "prefix" => format!("@{}", row.command),
        "web" => "dashboard".to_owned(),
        _ => row.command.clone(),
    }
}

fn what(a: &Action) -> String {
    match a {
        Action::Add { tracks, .. } if tracks.len() == 1 => format!("added {}", title(&tracks[0])),
        Action::Add { tracks, .. } => {
            let names: Vec<String> = tracks.iter().take(ADD_NAMES).map(title).collect();
            let more = tracks.len().saturating_sub(ADD_NAMES);
            let tail = if more > 0 {
                format!(" (+{more})")
            } else {
                String::new()
            };
            format!("added {} tracks: {}{tail}", tracks.len(), names.join(", "))
        },
        Action::Remove { track, index } => format!("removed {} from #{index}", title(track)),
        Action::Move { track, from, to } => format!("moved {} {from} → {to}", title(track)),
        Action::Skip { track: Some(t) } => format!("skipped {}", title(t)),
        Action::Skip { track: None } => "skipped".to_owned(),
        Action::Clear { removed } => format!("cleared {removed} tracks"),
        Action::Shuffle { count } => format!("shuffled {count} tracks"),
        Action::Stop { removed } => format!("stopped, {removed} tracks dropped"),
        Action::Pause => "paused".to_owned(),
        Action::Resume => "resumed".to_owned(),
        Action::Leave { discarded } => format!("left voice, {discarded} tracks discarded"),
    }
}

/// `<t:UNIX:R> WHO · HOW — WHAT`.
#[must_use]
pub fn audit_line(row: &AuditRow) -> String {
    format!(
        "<t:{}:R> {} · {} — {}",
        row.at.timestamp(),
        who(row),
        how(row),
        what(&row.detail)
    )
}

/// `gp` itself or one of its subcommands (`gp skip`), not a command that
/// merely starts with those letters.
fn is_gp(command: &str) -> bool {
    command == "gp" || command.starts_with("gp ")
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
        .filter(|r| !(is_gp(&r.command) && r.at >= start))
        .collect();
    let hid = kept.len() != before;
    (kept, hid)
}

/// What `/auditlog` has to say, before paging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditlogReply {
    /// No row matched the filter.
    Empty,
    /// Rows matched, but all belong to the running `/gp` game.
    AllHidden,
    /// The lines to page, leading notice lines included.
    Lines(Vec<String>),
}

/// Decide the reply from the rows fetched (newest first, at most
/// `AUDITLOG_LIMIT + 1`): cut to the limit, hide a running game's rows, and
/// prepend the notices. Pure; the notice texts come from `messages.rs`.
#[must_use]
pub fn compose_auditlog(
    mut rows: Vec<AuditRow>,
    running_since: Option<DateTime<Utc>>,
) -> AuditlogReply {
    if rows.is_empty() {
        return AuditlogReply::Empty;
    }
    let truncated = rows.len() > AUDITLOG_LIMIT;
    rows.truncate(AUDITLOG_LIMIT);
    let (rows, hid) = hide_running_game(rows, running_since);
    if rows.is_empty() {
        return AuditlogReply::AllHidden;
    }
    let mut lines = Vec::with_capacity(rows.len() + 2);
    if hid {
        lines.push(AUDITLOG_GP_HIDDEN.to_owned());
    }
    if truncated {
        lines.push(AUDITLOG_TRUNCATED.to_owned());
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
            ActionChoice::Leave => "leave",
        }
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
        assert_eq!(compose_auditlog(vec![], None), AuditlogReply::Empty);
    }

    #[test]
    fn compose_all_rows_hidden_is_not_empty() {
        let start = Utc.timestamp_opt(1_790_843_000, 0).unwrap();
        let rows = vec![
            row("bot", "gp", None, Action::Pause),
            row("slash", "gp skip", Some(7), Action::Pause),
        ];
        assert_eq!(
            compose_auditlog(rows, Some(start)),
            AuditlogReply::AllHidden
        );
    }

    #[test]
    fn compose_over_the_limit_cuts_and_says_so() {
        let AuditlogReply::Lines(lines) = compose_auditlog(many(AUDITLOG_LIMIT + 1), None) else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), AUDITLOG_LIMIT + 1);
        assert_eq!(lines[0], AUDITLOG_TRUNCATED);
        let AuditlogReply::Lines(lines) = compose_auditlog(many(AUDITLOG_LIMIT), None) else {
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
        let AuditlogReply::Lines(lines) = compose_auditlog(rows, Some(start)) else {
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
}
