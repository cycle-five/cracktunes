# `/auditlog` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Members with Manage Server can read their server's queue audit log in Discord
with `/auditlog [user] [action] [source] [since]`, as an ephemeral paged embed.

**Architecture:**
- **Pure functions in a new `music::audit_view` module:** parsing `since`, formatting a
  row as a line, hiding a running `/gp` game's rows, and the choice enums.
- **The database:** a typed query plus an `AuditRow` in `db/queue_audit.rs`.
- **A thin command** in `commands/music/auditlog.rs` that wires them to the existing
  paged embed, which gains an `ephemeral` flag.

**Tech Stack:** Rust; poise (slash, `ChoiceParameter`), serenity `next`, sqlx 0.9
(postgres, chrono, json, offline `.sqlx`), and chrono.

**Spec:** `docs/superpowers/specs/2026-10-02-auditlog-command-design.md`

## Global Constraints

- **Typed serde only:** no `serde_json::json!` or `Value` for data we own. `detail` is
  read as `sqlx::types::Json<Action>`.
- **No source-scan tests.** Enforce with types and `clippy.toml`.
- **Sabotage every test.** Each new test must be seen to FAIL against a deliberate break
  of the code it guards. Record the break in the task report.
- **User-facing fixed strings** are `pub const`s in `crack-core/src/messaging/messages.rs`.
  Log lines stay where they are.
- **Ephemeral reply.** Never `ctx.defer()` publicly in `/auditlog`.
- **`AUDITLOG_LIMIT = 200`.** `since` accepts `<n>m|h|d|w`, with `n ≥ 1` and a total of at
  most 52 weeks.
- **Titles:** truncated to 40 characters with `…`; a missing title is `(untitled)`.
  Markdown in titles is escaped.
- **Migrations:** none in this plan.
- **Local Docker:** plain `docker` targets a remote host. Use `docker --context default`
  for the throwaway Postgres.
- **`.sqlx`:** the local sqlx-cli (0.8.6) writes a newer format than the existing files.
  Commit only the new `query-*.json` files, stripped of `origin` keys to match the
  existing format. Revert any rewritten existing files.
- **Never `git add -A`.** Stage named paths. The commit trailer is exactly, and alone:
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- Use `SQLX_OFFLINE=true` for cargo commands when no database is up.

## Rulings made while planning

- **The "newest 200" and "hidden `/gp` entries" notes go as leading lines of the
  content, not the footer.** The footer belongs to the pager's "Page x/y".
- **`create_paged_embed` gains `ephemeral: bool`.** Its four existing callers pass `false`,
  so their behaviour is unchanged.

## Review Focus

1. **A running `/gp` game's rows never reach the reply.** That includes member-issued
   `gp skip` rows, whose detail carries a title. Pinned in Task 1,
   `a_running_games_rows_are_hidden_and_others_kept`.
2. **A malformed `since` is refused, never ignored.** Pinned in Task 1, `since_rejects_bad_input`.
3. **One guild never sees another's rows.** Pinned in Task 2, `filters_and_guild_isolation`.
4. **The choice lists can't drift from what the recorder writes.** Pinned in Task 1,
   `action_choices_match_action_names` and `source_choices_match_source_names`.
5. **Long titles and markdown don't break a line.** Pinned in Task 1,
   `titles_are_truncated_and_escaped`.

---

### Task 1: `music::audit_view`, the pure parts

**Files:**
- Create: `crack-core/src/music/audit_view.rs`
- Modify: `crack-core/src/music/mod.rs` (`pub mod audit_view;`)

**Interfaces:**
- Consumes: `crate::music::audit::{Action, Source, TrackRef, AddAt}`.
- Produces:
  - `pub fn parse_since(s: &str) -> Option<chrono::Duration>`;
  - `pub struct AuditRow { pub at: DateTime<Utc>, pub actor_user_id: Option<i64>, pub source: String, pub command: String, pub action: String, pub detail: Action }`;
  - `pub fn audit_line(row: &AuditRow) -> String`;
  - `pub fn hide_running_game(rows: Vec<AuditRow>, running_since: Option<DateTime<Utc>>) -> (Vec<AuditRow>, bool /*hid any*/)`;
  - `#[derive(poise::ChoiceParameter)] pub enum ActionChoice { Add, Remove, Move, Skip, Clear, Shuffle, Stop, Pause, Resume, Leave }`
    with `pub fn name(self) -> &'static str`;
  - `#[derive(poise::ChoiceParameter)] pub enum SourceChoice { Slash, Prefix, Web, Bot }`
    with `pub fn source(self) -> Source`;
  - `pub const AUDITLOG_LIMIT: usize = 200;`.

- [ ] **Step 1: Write the failing tests** (`#[cfg(test)] mod test` in `audit_view.rs`)

```rust
#[cfg(test)]
mod test {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn t(title: &str) -> TrackRef {
        TrackRef { title: Some(title.into()), url: None }
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
        assert_eq!(parse_since("90m"), Some(chrono::Duration::minutes(90)));
        assert_eq!(parse_since("6h"), Some(chrono::Duration::hours(6)));
        assert_eq!(parse_since("2d"), Some(chrono::Duration::days(2)));
        assert_eq!(parse_since("52w"), Some(chrono::Duration::weeks(52)));
        assert_eq!(parse_since(" 1w "), Some(chrono::Duration::weeks(1)));
    }

    #[test]
    fn since_rejects_bad_input() {
        for bad in ["", "0h", "-1h", "5", "h", "5y", "1.5h", "53w", "9999999999999999999m", "two days"] {
            assert_eq!(parse_since(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn each_action_reads_as_one_line() {
        let cases = [
            (row("slash", "play", Some(7), Action::Add { tracks: vec![t("A")], at: AddAt::Back }), "<@7> · /play — added A"),
            (row("slash", "play", Some(7), Action::Add { tracks: vec![t("A"), t("B"), t("C"), t("D"), t("E")], at: AddAt::Back }),
             "<@7> · /play — added 5 tracks: A, B, C (+2)"),
            (row("slash", "remove", Some(7), Action::Remove { track: t("A"), index: 3 }), "<@7> · /remove — removed A from #3"),
            (row("web", "dashboard move", Some(7), Action::Move { track: t("A"), from: 8, to: 0 }), "<@7> · dashboard — moved A 8 → 0"),
            (row("prefix", "skip", Some(7), Action::Skip { track: Some(t("A")) }), "<@7> · @skip — skipped A"),
            (row("slash", "skip", Some(7), Action::Skip { track: None }), "<@7> · /skip — skipped"),
            (row("slash", "clear", Some(7), Action::Clear { removed: 4 }), "<@7> · /clear — cleared 4 tracks"),
            (row("slash", "shuffle", Some(7), Action::Shuffle { count: 9 }), "<@7> · /shuffle — shuffled 9 tracks"),
            (row("slash", "gp start", Some(7), Action::Stop { removed: 2 }), "<@7> · /gp start — stopped, 2 tracks dropped"),
            (row("bot", "autopause", None, Action::Pause), "bot · autopause — paused"),
            (row("slash", "resume", Some(7), Action::Resume), "<@7> · /resume — resumed"),
            (row("bot", "idle timeout", None, Action::Leave { discarded: 4 }), "bot · idle timeout — left voice, 4 tracks discarded"),
        ];
        for (r, want) in cases {
            assert_eq!(audit_line(&r), format!("<t:1790843254:R> {want}"));
        }
    }

    #[test]
    fn titles_are_truncated_and_escaped() {
        let long = "x".repeat(60);
        let line = audit_line(&row("slash", "remove", Some(1), Action::Remove { track: t(&long), index: 1 }));
        assert!(line.contains(&format!("{}…", "x".repeat(40))), "{line}");
        let line = audit_line(&row("slash", "remove", Some(1), Action::Remove { track: t("**bold** _it_ `c` |s| ~t~ >q"), index: 1 }));
        assert!(line.contains(r"\*\*bold\*\* \_it\_ \`c\` \|s\| \~t\~ \>q"), "{line}");
        let line = audit_line(&row("slash", "remove", Some(1), Action::Remove { track: TrackRef { title: None, url: None }, index: 1 }));
        assert!(line.contains("removed (untitled) from #1"), "{line}");
    }

    #[test]
    fn a_running_games_rows_are_hidden_and_others_kept() {
        let start = Utc.timestamp_opt(1_790_843_000, 0).unwrap();
        let mut before = row("bot", "gp", None, Action::Add { tracks: vec![t("old answer")], at: AddAt::Back });
        before.at = start - chrono::Duration::hours(1);
        let during_gp = row("bot", "gp", None, Action::Add { tracks: vec![t("answer")], at: AddAt::Back });
        let during_member_gp = row("slash", "gp skip", Some(7), Action::Skip { track: Some(t("answer")) });
        let during_other = row("slash", "pause", Some(7), Action::Pause);
        let rows = vec![during_gp, during_member_gp, during_other.clone(), before.clone()];

        let (kept, hid) = hide_running_game(rows.clone(), Some(start));
        assert!(hid);
        assert_eq!(kept.iter().map(|r| r.command.as_str()).collect::<Vec<_>>(), vec!["pause", "gp"]);
        assert_eq!(kept[1].at, before.at);

        let (kept, hid) = hide_running_game(rows, None);
        assert!(!hid);
        assert_eq!(kept.len(), 4);
    }

    #[test]
    fn action_choices_match_action_names() {
        let tr = t("a");
        let pairs = [
            (ActionChoice::Add, Action::Add { tracks: vec![], at: AddAt::Back }),
            (ActionChoice::Remove, Action::Remove { track: tr.clone(), index: 0 }),
            (ActionChoice::Move, Action::Move { track: tr.clone(), from: 0, to: 1 }),
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
        for (c, s) in [(SourceChoice::Slash, Source::Slash), (SourceChoice::Prefix, Source::Prefix),
                       (SourceChoice::Web, Source::Web), (SourceChoice::Bot, Source::Bot)] {
            assert_eq!(c.source(), s);
        }
    }
}
```

`Action` and `TrackRef` need `Clone` (they have it from v0.16.0). `AuditRow` needs `Clone`
and `Debug`.

- [ ] **Step 2: Run them to watch them fail**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib music::audit_view`
Expected: a compile FAIL (the module is empty).

- [ ] **Step 3: Implement `audit_view.rs`**

```rust
//! Reading the queue audit log back: what `/auditlog` shows, and what it hides.
//! Spec: docs/superpowers/specs/2026-10-02-auditlog-command-design.md

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

/// `90m`, `6h`, `2d`, `1w`: a positive whole number and one unit, at most 52 weeks.
#[must_use]
pub fn parse_since(s: &str) -> Option<Duration> {
    let s = s.trim();
    let unit = s.chars().last()?;
    let n: i64 = s[..s.len() - unit.len_utf8()].parse().ok()?;
    if n < 1 {
        return None;
    }
    let d = match unit {
        'm' => Duration::try_minutes(n)?,
        'h' => Duration::try_hours(n)?,
        'd' => Duration::try_days(n)?,
        'w' => Duration::try_weeks(n)?,
        _ => return None,
    };
    (d <= Duration::weeks(MAX_SINCE_WEEKS)).then_some(d)
}

/// Escape Discord markdown in text we did not write.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '_' | '`' | '~' | '|' | '>' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn title(t: &TrackRef) -> String {
    let Some(raw) = t.title.as_deref() else {
        return "(untitled)".to_owned();
    };
    let cut: String = raw.chars().take(TITLE_MAX).collect();
    let cut = if raw.chars().count() > TITLE_MAX { format!("{cut}…") } else { cut };
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
            let tail = if more > 0 { format!(" (+{more})") } else { String::new() };
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
    format!("<t:{}:R> {} · {} — {}", row.at.timestamp(), who(row), how(row), what(&row.detail))
}

/// Drop a running `/gp` game's rows: their titles are the answers. Rows from
/// before the game started, and rows of other commands, are kept. Returns
/// whether anything was dropped, so the reply can say so.
#[must_use]
pub fn hide_running_game(rows: Vec<AuditRow>, running_since: Option<DateTime<Utc>>) -> (Vec<AuditRow>, bool) {
    let Some(start) = running_since else {
        return (rows, false);
    };
    let before = rows.len();
    let kept: Vec<AuditRow> = rows
        .into_iter()
        .filter(|r| !(r.command.starts_with("gp") && r.at >= start))
        .collect();
    let hid = kept.len() != before;
    (kept, hid)
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
```

If `chrono::Duration::try_minutes`/`try_weeks` etc. are absent in this chrono version, use
`Duration::minutes(n)` guarded by checking `n` against a precomputed max per unit, and note
it in the report. `parse_since("9999999999999999999m")` must still return `None`; the
`parse()` into `i64` overflow already does that.

- [ ] **Step 4: Run the tests**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib music::audit_view`
Expected: 7 passed.

- [ ] **Step 5: Sabotage, one at a time, restoring each**

  1. In `hide_running_game`, drop the `&& r.at >= start` clause.
     `a_running_games_rows_are_hidden_and_others_kept` must FAIL, because the older game's row vanishes.
  2. In `parse_since`, remove the `n < 1` check. `since_rejects_bad_input` must FAIL on `"0h"`.
  3. In `escape`, remove `'|'`. `titles_are_truncated_and_escaped` must FAIL.
  4. In `ActionChoice::name`, return `"moved"` for `Move`. `action_choices_match_action_names` must FAIL.
  5. In `what`, change `ADD_NAMES` usage to `take(2)`. `each_action_reads_as_one_line` must FAIL.

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/music/audit_view.rs crack-core/src/music/mod.rs
git commit -m "feat(auditlog): reading the audit log back: lines, since, the /gp filter

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: The query

**Files:**
- Modify: `crack-core/src/db/queue_audit.rs` (add `recent_audit` and its gated test)
- Modify: `.sqlx/` (one new query file)

**Interfaces:**
- Consumes: `music::audit_view::AuditRow`, and Task 1's `ActionChoice::name` /
  `SourceChoice::source().as_str()` for the caller's filter strings.
- Produces: `pub struct AuditFilter<'a> { pub user: Option<UserId>, pub action: Option<&'a str>, pub source: Option<&'a str>, pub since: Option<DateTime<Utc>> }`
  (derives `Default`), and
  `pub async fn recent_audit(pool: &PgPool, guild_id: GuildId, f: &AuditFilter<'_>, limit: i64) -> sqlx::Result<Vec<AuditRow>>`.

- [ ] **Step 1: Write the failing gated test** in `db/queue_audit.rs`'s test module

```rust
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn filters_and_guild_isolation(pool: PgPool) {
        use crate::music::audit::{BotReason};
        let g = GuildId::new(1);
        let other = GuildId::new(2);
        let now = chrono::Utc::now();
        let ev = |guild, mins_ago: i64, actor: Actor, action: Action| AuditEvent {
            at: now - chrono::Duration::minutes(mins_ago),
            guild_id: guild,
            voice_channel: None,
            actor,
            action,
        };
        let alice = Actor::for_command(UserId::new(10), false, "pause", None);
        let bob = Actor::web(UserId::new(20));
        for e in [
            ev(g, 1, alice.clone(), Action::Pause),
            ev(g, 30, bob, Action::Move { track: TrackRef { title: Some("m".into()), url: None }, from: 2, to: 0 }),
            ev(g, 600, Actor::bot(BotReason::IdleTimeout), Action::Leave { discarded: 3 }),
            ev(other, 1, alice, Action::Pause),
        ] {
            insert_audit_event(&pool, &e).await.unwrap();
        }

        let all = recent_audit(&pool, g, &AuditFilter::default(), 10).await.unwrap();
        assert_eq!(all.iter().map(|r| r.action.as_str()).collect::<Vec<_>>(), vec!["pause", "move", "leave"], "newest first, guild 1 only");

        let by_user = recent_audit(&pool, g, &AuditFilter { user: Some(UserId::new(20)), ..Default::default() }, 10).await.unwrap();
        assert_eq!(by_user.len(), 1);
        assert_eq!(by_user[0].source, "web");

        let by_action = recent_audit(&pool, g, &AuditFilter { action: Some("leave"), ..Default::default() }, 10).await.unwrap();
        assert_eq!((by_action.len(), by_action[0].actor_user_id), (1, None));

        let by_source = recent_audit(&pool, g, &AuditFilter { source: Some("slash"), ..Default::default() }, 10).await.unwrap();
        assert_eq!(by_source.len(), 1);

        let recent = recent_audit(&pool, g, &AuditFilter { since: Some(now - chrono::Duration::hours(1)), ..Default::default() }, 10).await.unwrap();
        assert_eq!(recent.len(), 2);

        let capped = recent_audit(&pool, g, &AuditFilter::default(), 2).await.unwrap();
        assert_eq!(capped.len(), 2);
        assert_eq!(capped[0].detail, Action::Pause);
    }
```

- [ ] **Step 2: Implement `recent_audit`**

```rust
/// What `/auditlog` asked for. Every field is optional.
#[derive(Debug, Default, Clone)]
pub struct AuditFilter<'a> {
    pub user: Option<UserId>,
    pub action: Option<&'a str>,
    pub source: Option<&'a str>,
    pub since: Option<DateTime<Utc>>,
}

/// A guild's audit rows, newest first, at most `limit`.
pub async fn recent_audit(
    pool: &PgPool,
    guild_id: GuildId,
    f: &AuditFilter<'_>,
    limit: i64,
) -> sqlx::Result<Vec<AuditRow>> {
    let rows = sqlx::query!(
        r#"SELECT at, actor_user_id, source, command, action,
                  detail AS "detail!: Json<Action>"
           FROM queue_audit
           WHERE guild_id = $1
             AND ($2::bigint      IS NULL OR actor_user_id = $2)
             AND ($3::text        IS NULL OR action = $3)
             AND ($4::text        IS NULL OR source = $4)
             AND ($5::timestamptz IS NULL OR at >= $5)
           ORDER BY at DESC
           LIMIT $6"#,
        guild_id.get() as i64,
        f.user.map(|u| u.get() as i64),
        f.action,
        f.source,
        f.since,
        limit,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AuditRow {
            at: r.at,
            actor_user_id: r.actor_user_id,
            source: r.source,
            command: r.command,
            action: r.action,
            detail: r.detail.0,
        })
        .collect())
}
```

Import `AuditRow` from `crate::music::audit_view`, plus `UserId`/`GuildId` and
`chrono::{DateTime, Utc}` as needed.

- [ ] **Step 3: Regenerate `.sqlx` and run the test red, then green**

Use the throwaway-Postgres script from v0.16.0's plan, with
`docker --context default`, port 55434 and a container named `ct-auditlog-prepare`. Then:
- `cargo sqlx migrate run --source migrations/`;
- `cargo sqlx prepare --workspace -- --tests --all`;
- `cargo test -p crack-core --lib --features db-tests queue_audit`.

Commit only the new `.sqlx/query-*.json`, with `origin` keys stripped to match the
existing files. `git checkout` any rewritten existing ones. Stop the container at the end.

- [ ] **Step 4: Sabotage** (with the container up)

  1. Change `ORDER BY at DESC` to `ORDER BY at ASC`. `filters_and_guild_isolation` must FAIL.
  2. Drop `WHERE guild_id = $1` (keep the rest valid). It must FAIL.

  Restore both, re-run green, and stop the container.

- [ ] **Step 5: Confirm the offline build:** `SQLX_OFFLINE=true cargo check --workspace --all-targets`

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/db/queue_audit.rs .sqlx
git commit -m "feat(auditlog): read a guild's audit rows, filtered, newest first

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: The `/auditlog` command

**Files:**
- Create: `crack-core/src/commands/music/auditlog.rs`
- Modify: `crack-core/src/commands/music/mod.rs` (declare the module, re-export it as the
  others are, and add `auditlog()` to `music_commands()` next to `playlog()`)
- Modify: `crack-core/src/utils.rs` (`create_paged_embed` gains `ephemeral: bool` as its
  last parameter, applied with `CreateReply::default().ephemeral(ephemeral)`). The
  existing callers in `commands/help.rs` (×2), `commands/music/playlog.rs` and
  `messaging/interface.rs` pass `false`.
- Modify: `crack-core/src/messaging/messages.rs` (the constants below)
- Modify: `docs/queue-audit.md` (a short "Reading it in Discord" section)
- Modify: `Cargo.toml` (`0.16.0` → `0.17.0`) and `Cargo.lock` (via `cargo update -w`)

**Interfaces:**
- Consumes: Task 1 (`parse_since`, `audit_line`, `hide_running_game`, `ActionChoice`,
  `SourceChoice`, `AUDITLOG_LIMIT`) and Task 2 (`recent_audit`, `AuditFilter`).

The messages constants, verbatim:

```rust
pub const AUDITLOG_TITLE: &str = "📜 Queue history";
pub const AUDITLOG_NO_DATABASE: &str = "📜 The queue history needs a database, and this bot is running without one.";
pub const AUDITLOG_BAD_SINCE: &str = "⚠️ `since` takes a number and a unit: `90m`, `6h`, `2d` or `1w` (at most 52w).";
pub const AUDITLOG_EMPTY: &str = "📜 Nothing in the queue history matches that.";
pub const AUDITLOG_TRUNCATED: &str = "_Showing the newest 200. Narrow it with `since` or `user`._";
pub const AUDITLOG_GP_HIDDEN: &str = "_Entries from the `/gp` game in progress are hidden until it ends._";
```

- [ ] **Step 1: Write the command**

```rust
//! `/auditlog`: the queue history, for members with Manage Server.
//! Spec: docs/superpowers/specs/2026-10-02-auditlog-command-design.md

use crate::db::queue_audit::{recent_audit, AuditFilter};
use crate::messaging::messages::{
    AUDITLOG_BAD_SINCE, AUDITLOG_EMPTY, AUDITLOG_GP_HIDDEN, AUDITLOG_NO_DATABASE, AUDITLOG_TITLE,
    AUDITLOG_TRUNCATED,
};
use crate::music::audit_view::{
    audit_line, hide_running_game, parse_since, ActionChoice, SourceChoice, AUDITLOG_LIMIT,
};
use crate::utils::create_paged_embed;
use crate::{Context, Error};
use poise::serenity_prelude as serenity;
use poise::CreateReply;

/// Show who changed this server's queue, and how.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    guild_only,
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn auditlog(
    ctx: Context<'_>,
    #[description = "Only this member's changes"] user: Option<serenity::User>,
    #[description = "Only this kind of change"] action: Option<ActionChoice>,
    #[description = "Only changes made this way"] source: Option<SourceChoice>,
    #[description = "Only changes this recent: 90m, 6h, 2d, 1w"] since: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(crate::CrackedError::NoGuildId)?;
    let say = |text: &'static str| ctx.send(CreateReply::default().content(text).ephemeral(true));

    let Some(pool) = ctx.data().database_pool.clone() else {
        say(AUDITLOG_NO_DATABASE).await?;
        return Ok(());
    };
    let since = match since.as_deref().map(parse_since) {
        None => None,
        Some(Some(d)) => Some(chrono::Utc::now() - d),
        Some(None) => {
            say(AUDITLOG_BAD_SINCE).await?;
            return Ok(());
        },
    };
    let source_str = source.map(|s| s.source().as_str());
    let filter = AuditFilter {
        user: user.map(|u| u.id),
        action: action.map(ActionChoice::name),
        source: source_str,
        since,
    };
    let mut rows = recent_audit(&pool, guild_id, &filter, AUDITLOG_LIMIT as i64 + 1).await?;
    let truncated = rows.len() > AUDITLOG_LIMIT;
    rows.truncate(AUDITLOG_LIMIT);

    let running_since = ctx
        .data()
        .gp_games
        .get(&guild_id)
        .and_then(|g| chrono::DateTime::from_timestamp(g.started_at, 0));
    let (rows, hid) = hide_running_game(rows, running_since);

    if rows.is_empty() {
        say(AUDITLOG_EMPTY).await?;
        return Ok(());
    }
    let mut lines = Vec::new();
    if hid {
        lines.push(AUDITLOG_GP_HIDDEN.to_owned());
    }
    if truncated {
        lines.push(AUDITLOG_TRUNCATED.to_owned());
    }
    lines.extend(rows.iter().map(audit_line));

    create_paged_embed(
        ctx,
        ctx.author().name.clone(),
        AUDITLOG_TITLE.to_owned(),
        lines.join("\n"),
        900,
        true,
    )
    .await?;
    Ok(())
}
```

Adjust to the codebase's real names where the compiler disagrees: the
`CrackedError::NoGuildId` path, `CreateReply`'s import, and how `.get()` on the
`gp_games` DashMap returns a ref (drop the ref before awaiting). Note each in the
report.

- [ ] **Step 2: Add the `ephemeral` parameter to `create_paged_embed`** and update its
  four callers with `false`. Check that `/playlog` and help still compile and behave
  the same.

- [ ] **Step 3: Docs.** Append to `docs/queue-audit.md`:

```markdown
## Reading it in Discord

`/auditlog [user] [action] [source] [since]` shows a server's queue history, newest
first, as a paged reply only the caller sees. By default only members with
**Manage Server** can run it; server admins can grant or remove it per role or
member in Server Settings → Integrations. `since` takes `90m`, `6h`, `2d` or `1w`
(at most 52w). It shows at most 200 entries; a running `/gp` game's entries are
hidden until the game ends.
```

- [ ] **Step 4: Version.** `sed -i 's/^version = "0.16.0"/version = "0.17.0"/' Cargo.toml && cargo update -w`,
  then check that `git diff Cargo.lock` changes only workspace version lines.

- [ ] **Step 5: Full checks**

Run: `cargo fmt --all -- --check && SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings && SQLX_OFFLINE=true cargo test --workspace`
Expected: clean. In particular, the existing
`commands::test::every_registered_command_is_reachable` passes with `/auditlog`
registered. Paste the per-binary `test result` lines in the report.

- [ ] **Step 6: Sabotage.** Remove `auditlog()` from `music_commands()`. If any test or
  registration check notices, record which. If none does, say so plainly in the report:
  that is a known gap, covered by the TuneTitan check. Restore it.

- [ ] **Step 7: Commit**

```bash
git add crack-core/src/commands/music/auditlog.rs crack-core/src/commands/music/mod.rs \
  crack-core/src/utils.rs crack-core/src/commands/help.rs crack-core/src/commands/music/playlog.rs \
  crack-core/src/messaging/interface.rs crack-core/src/messaging/messages.rs docs/queue-audit.md \
  Cargo.toml Cargo.lock
git commit -m "v0.17.0: /auditlog, the queue history for server managers

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
