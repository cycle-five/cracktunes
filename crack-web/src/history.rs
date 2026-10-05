//! The history page's data: the query it accepts, the rows it shows, and how
//! one becomes the other. Pure, apart from the name memo's clock.
//! Spec: docs/superpowers/specs/2026-10-02-dashboard-history-design.md

use crack_core::db::queue_audit::{AuditCursor, AuditPageRow};
use crack_core::music::audit_view::{
    hidden_by_game, how_text, older_than_floor, parse_since, what_text, ActionChoice, SourceChoice,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serenity::all::UserId;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

/// Rows per page and per poll.
pub const PAGE_SIZE: usize = 50;
/// Discord user lookups one request may make for names it cannot find.
pub const NAME_LOOKUPS: usize = 10;
/// How long a looked-up name is kept.
pub const NAME_TTL: Duration = Duration::from_secs(3600);
/// How long an HTTP-fetched role list is kept.
pub const ROLE_TTL: Duration = Duration::from_secs(300);

pub const BAD_USER: &str = "user must be a Discord user id";
pub const BAD_ACTION: &str =
    "action must be one of add, remove, move, skip, clear, shuffle, stop, pause, resume, repeat, leave";
pub const BAD_SOURCE: &str = "source must be one of slash, prefix, web, bot";
pub const BAD_SINCE: &str = "since must look like 90m, 6h, 2d or 1w, at most 52 weeks";
pub const BAD_CURSOR: &str = "before and after must be positive row ids";
pub const BOTH_CURSORS: &str = "use before or after, not both";
pub const NEEDS_MANAGE: &str =
    "Only members with Manage Server can see this server's queue history.";
pub const NO_DATABASE: &str = "Queue history needs the bot's database, which isn't set up.";
pub const HISTORY_FAILED: &str = "The history could not be read. Try again in a moment.";

/// Who made a change. Ids are strings: snowflakes exceed what a JavaScript
/// number holds exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Who {
    Member { id: String, name: Option<String> },
    Bot,
}

/// One entry as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRow {
    /// The row id: the paging cursor. Insertion order.
    pub id: i64,
    /// RFC 3339; the page shows it relative to now.
    pub at: String,
    pub who: Who,
    pub how: String,
    pub what: String,
}

/// A page of history, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HistoryPage {
    pub rows: Vec<HistoryRow>,
    /// More rows exist below the last one. Always false for an `after` poll.
    pub older: bool,
    /// A `/gp` game is running, so its rows are being left out.
    pub game_hidden: bool,
    /// The plan's history floor hid older rows: the page says older history is
    /// premium, instead of offering "Load older". Always false for an `after` poll.
    pub capped: bool,
}

/// What the page asked for, checked.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryQuery {
    pub user: Option<UserId>,
    pub action: Option<ActionChoice>,
    pub source: Option<SourceChoice>,
    pub since: Option<chrono::Duration>,
    pub cursor: AuditCursor,
}

/// A refused query, and what is accepted instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadQuery(pub &'static str);

/// The JSON body of every error answer.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: &'static str,
}

/// The query string as sent. Empty values mean "any".
#[derive(Debug, Default, Deserialize)]
struct RawQuery {
    user: Option<String>,
    action: Option<String>,
    source: Option<String>,
    since: Option<String>,
    before: Option<String>,
    after: Option<String>,
}

fn present(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

fn row_id(s: &str) -> Result<i64, BadQuery> {
    s.parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or(BadQuery(BAD_CURSOR))
}

/// Check `/g/<id>/history.json`'s query string.
pub fn parse_history_query(raw: Option<&str>) -> Result<HistoryQuery, BadQuery> {
    let raw: RawQuery = serde_urlencoded::from_str(raw.unwrap_or(""))
        .map_err(|_| BadQuery("the query string could not be read"))?;
    let user = present(raw.user)
        .map(|s| {
            s.parse::<NonZeroU64>()
                .map(|n| UserId::new(n.get()))
                .map_err(|_| BadQuery(BAD_USER))
        })
        .transpose()?;
    let action = present(raw.action)
        .map(|s| ActionChoice::from_name(&s).ok_or(BadQuery(BAD_ACTION)))
        .transpose()?;
    let source = present(raw.source)
        .map(|s| SourceChoice::from_name(&s).ok_or(BadQuery(BAD_SOURCE)))
        .transpose()?;
    let since = present(raw.since)
        .map(|s| parse_since(&s).ok_or(BadQuery(BAD_SINCE)))
        .transpose()?;
    let cursor = match (present(raw.before), present(raw.after)) {
        (Some(_), Some(_)) => return Err(BadQuery(BOTH_CURSORS)),
        (Some(b), None) => AuditCursor::Before(row_id(&b)?),
        (None, Some(a)) => AuditCursor::After(row_id(&a)?),
        (None, None) => AuditCursor::Newest,
    };
    Ok(HistoryQuery {
        user,
        action,
        source,
        since,
        cursor,
    })
}

/// How many rows to fetch: one extra going backwards, to learn whether more
/// exist; exactly a page going forwards (the client polls again on a full one).
pub fn fetch_limit(cursor: AuditCursor) -> i64 {
    match cursor {
        AuditCursor::After(_) => PAGE_SIZE as i64,
        AuditCursor::Newest | AuditCursor::Before(_) => PAGE_SIZE as i64 + 1,
    }
}

/// A stored actor id as a user, if it is a snowflake. `UserId::new(0)` panics.
fn snowflake(id: i64) -> Option<UserId> {
    u64::try_from(id)
        .ok()
        .and_then(NonZeroU64::new)
        .map(|n| UserId::new(n.get()))
}

/// The members `compose_page` will name, each once, in page order. Only the
/// rows it keeps count, so the name lookups are never spent on the extra row
/// fetched to learn `older`, nor on a row a running game hides.
pub fn shown_members(
    rows: &[AuditPageRow],
    running_since: Option<chrono::DateTime<chrono::Utc>>,
    floor: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<UserId> {
    let mut out = Vec::new();
    for u in rows
        .iter()
        .take(PAGE_SIZE)
        .filter(|r| !hidden_by_game(&r.row, running_since))
        .filter(|r| !older_than_floor(&r.row, floor))
        .filter_map(|r| r.row.actor_user_id.and_then(snowflake))
    {
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// The page for `rows` (newest first, as `audit_page` returns them, fetched
/// with [`fetch_limit`]): cut to a page, a running game's rows dropped, names
/// filled in where `name_of` knows them.
pub fn compose_page(
    mut rows: Vec<AuditPageRow>,
    cursor: AuditCursor,
    running_since: Option<chrono::DateTime<chrono::Utc>>,
    floor: Option<chrono::DateTime<chrono::Utc>>,
    name_of: impl Fn(UserId) -> Option<String>,
) -> HistoryPage {
    let poll = matches!(cursor, AuditCursor::After(_));
    // The extra row counts: if even it is past the floor, there is older
    // history, and "Load older" would only find rows the plan hides.
    let capped = !poll && rows.iter().any(|r| older_than_floor(&r.row, floor));
    let older = !poll && !capped && rows.len() > PAGE_SIZE;
    rows.truncate(PAGE_SIZE);
    let rows = rows
        .into_iter()
        .filter(|r| !hidden_by_game(&r.row, running_since))
        .filter(|r| !older_than_floor(&r.row, floor))
        .map(|r| HistoryRow {
            id: r.id,
            at: r.row.at.to_rfc3339(),
            who: match r.row.actor_user_id {
                None => Who::Bot,
                Some(id) => Who::Member {
                    id: id.to_string(),
                    name: snowflake(id).and_then(&name_of),
                },
            },
            how: how_text(&r.row),
            what: what_text(&r.row.detail),
        })
        .collect();
    HistoryPage {
        rows,
        older,
        game_hidden: running_since.is_some(),
        capped,
    }
}

/// Names looked up over HTTP, kept for [`NAME_TTL`]. The bot never fetches
/// member lists, so most of a large guild's members are not in the cache.
pub struct NameMemo {
    ttl: Duration,
    entries: DashMap<UserId, (String, Instant)>,
}

impl NameMemo {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: DashMap::new(),
        }
    }

    pub fn get(&self, u: UserId, now: Instant) -> Option<String> {
        let entry = self.entries.get(&u)?;
        let (name, at) = &*entry;
        (now.saturating_duration_since(*at) < self.ttl).then(|| name.clone())
    }

    pub fn record(&self, u: UserId, name: String, now: Instant) {
        self.entries.insert(u, (name, now));
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{TimeZone, Utc};
    use crack_core::music::audit::{Action, TrackRef};
    use crack_core::music::audit_view::AuditRow;

    fn q(raw: &str) -> Result<HistoryQuery, BadQuery> {
        parse_history_query(Some(raw))
    }

    #[test]
    fn no_query_is_the_newest_unfiltered_page() {
        assert_eq!(parse_history_query(None), Ok(HistoryQuery::default()));
        assert_eq!(q(""), Ok(HistoryQuery::default()));
        assert_eq!(
            q("action=&source=&since=&user="),
            Ok(HistoryQuery::default()),
            "the page's 'all' options send empty values"
        );
    }

    #[test]
    fn every_parameter_parses() {
        let got = q("user=123456789012345678&action=move&source=web&since=6h&after=40").unwrap();
        assert_eq!(got.user, Some(UserId::new(123_456_789_012_345_678)));
        assert_eq!(got.action, Some(ActionChoice::Move));
        assert_eq!(got.source, Some(SourceChoice::Web));
        assert_eq!(got.since, Some(chrono::Duration::hours(6)));
        assert_eq!(got.cursor, AuditCursor::After(40));
        assert_eq!(q("before=7").unwrap().cursor, AuditCursor::Before(7));
    }

    #[test]
    fn every_recorded_action_is_accepted_and_named_in_the_refusal() {
        for name in [
            "add", "remove", "move", "skip", "clear", "shuffle", "stop", "pause", "resume",
            "repeat", "leave",
        ] {
            let got = q(&format!("action={name}")).unwrap().action;
            assert_eq!(got.map(ActionChoice::name), Some(name));
            assert!(
                BAD_ACTION.split([' ', ',']).any(|w| w == name),
                "BAD_ACTION does not name {name}"
            );
        }
    }

    #[test]
    fn bad_values_are_refused_with_the_accepted_form() {
        for bad in [
            "user=abc",
            "user=0",
            "action=dance",
            "source=email",
            "since=3y",
            "since=0h",
            "before=x",
            "before=0",
            "after=-1",
            "before=5&after=6",
        ] {
            assert!(q(bad).is_err(), "{bad} should be refused");
        }
        assert_eq!(q("since=3y"), Err(BadQuery(BAD_SINCE)));
    }

    fn page_row(id: i64, command: &str, user: Option<i64>, at_secs: i64) -> AuditPageRow {
        AuditPageRow {
            id,
            row: AuditRow {
                at: Utc.timestamp_opt(at_secs, 0).unwrap(),
                actor_user_id: user,
                source: "slash".into(),
                command: command.into(),
                action: "skip".into(),
                detail: Action::Skip {
                    track: Some(TrackRef {
                        title: Some("<b>Song</b>".into()),
                        url: None,
                    }),
                },
            },
        }
    }

    #[test]
    fn a_page_maps_rows_and_sends_ids_as_strings() {
        let big = 1_155_000_000_000_000_001_i64; // above 2^53
        let page = compose_page(
            vec![
                page_row(2, "skip", Some(big), 100),
                page_row(1, "skip", None, 50),
            ],
            AuditCursor::Newest,
            None,
            None,
            |u| (u.get() == big as u64).then(|| "Alice".to_owned()),
        );
        assert_eq!(
            page.rows[0],
            HistoryRow {
                id: 2,
                at: "1970-01-01T00:01:40+00:00".into(),
                who: Who::Member {
                    id: big.to_string(),
                    name: Some("Alice".into())
                },
                how: "/skip".into(),
                what: "skipped <b>Song</b>".into(),
            }
        );
        assert_eq!(page.rows[1].who, Who::Bot);
        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains(&format!("\"id\":\"{big}\"")), "{json}");
        assert!(!page.older && !page.game_hidden);
    }

    #[test]
    fn older_is_set_by_the_extra_row_and_never_for_after() {
        let rows: Vec<AuditPageRow> = (0..=PAGE_SIZE as i64)
            .rev()
            .map(|i| page_row(i + 1, "skip", None, i))
            .collect();
        assert_eq!(rows.len(), PAGE_SIZE + 1);
        let newest = compose_page(rows.clone(), AuditCursor::Newest, None, None, |_| None);
        assert!(newest.older);
        assert_eq!(newest.rows.len(), PAGE_SIZE);
        assert_eq!(newest.rows[0].id, PAGE_SIZE as i64 + 1, "kept the newest");
        let after = compose_page(rows.clone(), AuditCursor::After(0), None, None, |_| None);
        assert!(
            !after.older,
            "an after poll never claims older rows, even handed an extra"
        );
        assert_eq!(fetch_limit(AuditCursor::Newest), PAGE_SIZE as i64 + 1);
        assert_eq!(fetch_limit(AuditCursor::Before(9)), PAGE_SIZE as i64 + 1);
        assert_eq!(fetch_limit(AuditCursor::After(9)), PAGE_SIZE as i64);
    }

    #[test]
    fn a_running_game_hides_its_rows_and_says_so() {
        let start = Utc.timestamp_opt(1_000, 0).unwrap();
        let page = compose_page(
            vec![
                page_row(3, "gp", None, 1_500),
                page_row(2, "play", Some(5), 1_400),
                page_row(1, "gp", None, 500),
            ],
            AuditCursor::Newest,
            Some(start),
            None,
            |_| None,
        );
        assert_eq!(
            page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(page.game_hidden);
        let idle = compose_page(
            vec![page_row(1, "gp", None, 1_500)],
            AuditCursor::Newest,
            None,
            None,
            |_| None,
        );
        assert!(!idle.game_hidden);
        assert_eq!(idle.rows.len(), 1);
    }

    #[test]
    fn a_stored_user_id_that_is_not_a_snowflake_gets_no_name_lookup() {
        let page = compose_page(
            vec![page_row(1, "skip", Some(0), 1)],
            AuditCursor::Newest,
            None,
            None,
            |_| panic!("UserId::new(0) would panic; never look it up"),
        );
        assert_eq!(
            page.rows[0].who,
            Who::Member {
                id: "0".into(),
                name: None
            }
        );
    }

    #[test]
    fn names_are_looked_up_only_for_the_rows_shown() {
        let start = Utc.timestamp_opt(1_000, 0).unwrap();
        // A full page plus the extra row; the extra row's member is no one else.
        let mut rows: Vec<AuditPageRow> = (0..PAGE_SIZE as i64)
            .rev()
            .map(|i| page_row(i + 10, "skip", Some(7 + i % 2), 500))
            .collect();
        rows[1] = page_row(58, "gp", Some(9), 1_500); // hidden by the game
        rows[2] = page_row(57, "skip", Some(0), 500); // not a snowflake
        rows.push(page_row(1, "skip", Some(99), 400)); // the 51st
        assert_eq!(rows.len(), PAGE_SIZE + 1);
        let got = shown_members(&rows, Some(start), None);
        assert_eq!(
            got,
            vec![UserId::new(8), UserId::new(7)],
            "each once, in page order; not the hidden row's, not the 51st's"
        );
        assert_eq!(
            shown_members(&rows, None, None),
            vec![UserId::new(8), UserId::new(9), UserId::new(7)],
            "no game, nothing hidden"
        );
    }

    /// The floor for these tests: `page_row`'s `at_secs` below 1000 are older.
    fn floor() -> Option<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::from_timestamp(1000, 0)
    }

    #[test]
    fn rows_past_the_floor_are_dropped_and_the_page_is_capped() {
        let rows = vec![
            page_row(3, "skip", Some(7), 1002),
            page_row(2, "skip", Some(7), 1000),
            page_row(1, "skip", Some(7), 999),
        ];
        let page = compose_page(rows, AuditCursor::Newest, None, floor(), |_| None);
        assert_eq!(
            page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert!(page.capped);
        assert!(!page.older);
    }

    #[test]
    fn the_extra_row_past_the_floor_caps_the_page() {
        // 50 rows inside the window plus the 51st fetched to learn `older`,
        // which is too old: no "Load older", the note instead.
        let mut rows: Vec<AuditPageRow> = (0..50)
            .map(|i| page_row(100 - i, "skip", Some(7), 2000 - i))
            .collect();
        rows.push(page_row(1, "skip", Some(7), 10));
        let page = compose_page(rows, AuditCursor::Newest, None, floor(), |_| None);
        assert_eq!(page.rows.len(), 50);
        assert!(page.capped);
        assert!(!page.older);
    }

    #[test]
    fn a_before_page_past_the_floor_is_empty_and_capped() {
        let rows = vec![
            page_row(2, "skip", Some(7), 500),
            page_row(1, "skip", Some(7), 400),
        ];
        let page = compose_page(rows, AuditCursor::Before(3), None, floor(), |_| None);
        assert!(page.rows.is_empty());
        assert!(page.capped);
        assert!(!page.older);
    }

    #[test]
    fn after_polls_never_report_capped() {
        let rows = vec![page_row(5, "skip", Some(7), 999)];
        let page = compose_page(rows, AuditCursor::After(4), None, floor(), |_| None);
        assert!(!page.capped);
    }

    #[test]
    fn premium_has_no_floor() {
        let rows = vec![
            page_row(2, "skip", Some(7), 5),
            page_row(1, "skip", Some(7), 4),
        ];
        let page = compose_page(rows, AuditCursor::Newest, None, None, |_| None);
        assert_eq!(page.rows.len(), 2);
        assert!(!page.capped);
    }

    #[test]
    fn names_are_not_looked_up_for_rows_past_the_floor() {
        let rows = vec![
            page_row(2, "skip", Some(7), 1001),
            page_row(1, "skip", Some(8), 999),
        ];
        assert_eq!(shown_members(&rows, None, floor()), vec![UserId::new(7)]);
    }

    #[test]
    fn the_name_memo_remembers_and_forgets_in_time() {
        let memo = NameMemo::new(NAME_TTL);
        let u = UserId::new(4);
        let t0 = Instant::now();
        assert_eq!(memo.get(u, t0), None);
        memo.record(u, "Bo".into(), t0);
        assert_eq!(
            memo.get(u, t0 + NAME_TTL - Duration::from_secs(1)),
            Some("Bo".into())
        );
        assert_eq!(memo.get(u, t0 + NAME_TTL), None);
    }
}
