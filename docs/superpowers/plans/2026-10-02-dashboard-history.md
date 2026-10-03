# Dashboard History Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Managers can read a server's queue history at `dash.cracktun.es/g/<id>/history`.
They can filter it and load older entries, and new entries appear while the page is open.

**Architecture:**
- **crack-core:**
  - produces the rows through a new keyset-paged query, `db::queue_audit::audit_page`;
  - produces the wording through plain-text functions split out of `music::audit_view`, which `/auditlog` keeps using, escaped.
- **crack-web:**
  - adds a Manage Server check (`access.rs`);
  - adds a pure query parser and page composer (`history.rs`);
  - adds two `Backend` methods with their routes, and a page with its own script (`assets/history.js`).
- **Live updates:** the script polls the JSON endpoint every 10 s with `after=<newest id>`.

**Tech Stack:** Rust: axum 0.8, serenity `next`, sqlx 0.9 with offline `.sqlx`, chrono, typed serde. Plain browser JS under CSP Trusted Types.

**Spec:** `docs/superpowers/specs/2026-10-02-dashboard-history-design.md`

## Global Constraints

- **Serde:** typed only, never `serde_json::json!` or `serde_json::Value` for data we own. Errors go back as a typed struct.
- **No source-scan tests.** Enforce with types and clippy, not by grepping code.
- **Sabotage every new test:** break the code it guards, watch it fail, then restore. Report the sabotage you did.
- **Commits:**
  - Every commit ends with exactly this trailer and no other `Co-Authored-By`: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
  - Never `git add -A` or `git add .`. Stage named files.
- **CI's lint must pass:** `cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return`.
- **Tests:** run with `SQLX_OFFLINE=true cargo test -q -p <crate>`. DB tests need `--features db-tests` and a `DATABASE_URL`, as Task 2 shows.
- **Discord snowflakes are sent to the browser as strings.** JavaScript numbers are exact only to 2^53. Row ids (`BIGSERIAL`) stay numbers.
- **The browser renders all text with `textContent`.** The CSP (`require-trusted-types-for 'script'; trusted-types 'none'`) makes any HTML sink throw.
- **Do not change `/auditlog`'s output or `recent_audit`.** Existing `audit_view` tests must pass unmodified.
- **Page size is 50.** The poll interval is 10 s. `NAME_LOOKUPS` is 10 per request. Role and name memos last 5 min and 1 h.
- **Local docker:** use `docker --context default` for local containers. Bare `docker` targets a remote host here.

## Review Focus

1. **Ids above 2^53.** User ids are sent as strings, and JS must never parse them as numbers. Task 4's `compose_page` test asserts `who.id` is a string.
2. **A hostile title or server name** (`</script>`, `<img onerror>`) must not break the inlined JSON or the HTML. Task 5's page test is modelled on `page.rs`'s hostile-title test.
3. **More than 50 rows arriving between polls.** The `after` window is the oldest 50 above the cursor, so none are skipped. Task 2's DB test pins it.
4. **A member who isn't in the cache** (large servers). Their roles must come from the role memo or HTTP, not count as "no permission". Task 3's memo test and the `LiveBackend` code in Task 5 cover it.
5. **A filter change during an in-flight poll** must not mix old and new rows. `history.js` guards it with a sequence number, and the TuneTitan manual check covers it.

---

### Task 1: Plain-text wording, game hiding and choice lookup in `audit_view`

**Files:**
- Modify: `crack-core/src/music/audit_view.rs`

**Interfaces:**
- Produces (all `pub`, in `crack_core::music::audit_view`):
  - `fn how_text(row: &AuditRow) -> String`: `/play`, `@skip`, `dashboard`, or the bot reason. This is today's private `how`, renamed and made public.
  - `fn what_text(a: &Action) -> String`: today's wording, titles cut at 40 chars and **not** escaped.
  - `fn hidden_by_game(row: &AuditRow, running_since: Option<DateTime<Utc>>) -> bool`
  - `impl ActionChoice { fn from_name(s: &str) -> Option<ActionChoice> }`
  - `impl SourceChoice { fn from_name(s: &str) -> Option<SourceChoice> }`

- [ ] **Step 1: Write the failing tests.** Add them to the existing `mod test` in `audit_view.rs`, which already has helpers `t(title)` and `row(source, command, user, action)`:

```rust
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
            what_text(&Action::Skip { track: Some(t(&long)) }),
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
            ActionChoice::Leave,
        ] {
            assert_eq!(ActionChoice::from_name(c.name()), Some(c));
        }
        assert_eq!(ActionChoice::from_name("dance"), None);
        assert_eq!(ActionChoice::from_name("Move"), None, "stored names are lowercase");
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
```

Check the existing `row()` helper's `at` field. If it isn't `pub`-settable as written above (`r.at = …`), adapt by building the `AuditRow` literal directly; its fields are all `pub`.

- [ ] **Step 2: Run the tests and see them fail**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-core --lib audit_view`
Expected: compile errors (`what_text`, `how_text`, `hidden_by_game` and `from_name` are not found).

- [ ] **Step 3: Implement.** In `audit_view.rs`:

Replace `fn title(t: &TrackRef) -> String { … }` with an unescaped version:

```rust
/// A track's title for display: cut at `TITLE_MAX` characters with `…`, or
/// `(untitled)`. Not escaped: `/auditlog` escapes the whole line's wording.
fn title_text(t: &TrackRef) -> String {
    let Some(raw) = t.title.as_deref() else {
        return "(untitled)".to_owned();
    };
    let cut: String = raw.chars().take(TITLE_MAX).collect();
    if raw.chars().count() > TITLE_MAX {
        format!("{cut}…")
    } else {
        cut
    }
}
```

Rename `fn how(row: &AuditRow) -> String` to `pub fn how_text(row: &AuditRow) -> String`, with this doc comment:

```rust
/// How the change was asked for: `/play`, `@skip`, `dashboard`, or the bot's
/// reason. Command names are ours, so nothing here needs escaping.
```

Replace `fn what(a: &Action) -> String { … }` with a plain-text `what_text` and an escaping `what`:

```rust
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
        Action::Leave { discarded } => format!("left voice, {discarded} tracks discarded"),
    }
}

/// [`what_text`] escaped for Discord. Escaping the whole text equals escaping
/// each title: the fixed words contain none of the characters `escape` touches.
fn what(a: &Action) -> String {
    escape(&what_text(a))
}
```

In `audit_line`, change `how(row)` to `how_text(row)`.

Replace `hide_running_game`'s filter with a shared predicate:

```rust
/// Whether a running `/gp` game hides this row: a `gp`/`gp …` command recorded
/// at or after the game's start. Titles in those rows are the answers.
#[must_use]
pub fn hidden_by_game(row: &AuditRow, running_since: Option<DateTime<Utc>>) -> bool {
    running_since.is_some_and(|start| is_gp(&row.command) && row.at >= start)
}
```

In `hide_running_game`, change the filter line to `.filter(|r| !hidden_by_game(r, Some(start)))`.

Add the lookups beside the existing `impl`s:

```rust
impl ActionChoice {
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
            "leave" => ActionChoice::Leave,
            _ => return None,
        })
    }
}

impl SourceChoice {
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
```

Put those methods inside the existing `impl ActionChoice` and `impl SourceChoice` blocks; there should be one block per type.

- [ ] **Step 4: Run all of `audit_view`'s tests**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-core --lib audit_view`
Expected: all pass. The old tests (`each_action_reads_as_one_line`, `titles_are_truncated_and_escaped`, and so on) are unmodified.

- [ ] **Step 5: Sabotage.** Do each, see the named test fail, then restore:
  - make `what` return `what_text(a)` without `escape`. Expected failures: `web_wording_is_the_discord_wording_unescaped` and `titles_are_truncated_and_escaped`;
  - drop `row.at >= start` from `hidden_by_game`. Expected failure: the `hidden_by_game` test;
  - map `"leave"` to `Pause` in `from_name`. Expected failure: the round-trip test.

- [ ] **Step 6: Run clippy, then commit**

```bash
cargo clippy -q -p crack-core --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-core/src/music/audit_view.rs
git commit -m "audit_view: plain-text wording, hidden_by_game and choice lookups for the dashboard

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: `audit_page`, the id-ordered keyset query

**Files:**
- Create: `migrations/20261002120000_queue_audit_guild_id.sql`
- Create: `crack-core/test_migrations/20261002120000_queue_audit_guild_id.sql`, identical to the migration. `test_migrations` mirrors `migrations` file for file.
- Modify: `crack-core/src/db/queue_audit.rs`
- Create: two `.sqlx/query-*.json` files (generated)

**Interfaces:**
- Consumes: `AuditRow` and `AuditFilter`, both existing.
- Produces (in `crack_core::db::queue_audit`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditCursor { #[default] Newest, Before(i64), After(i64) }

#[derive(Debug, Clone)]
pub struct AuditPageRow { pub id: i64, pub row: AuditRow }

pub async fn audit_page(
    pool: &PgPool, guild_id: GuildId, f: &AuditFilter<'_>, cursor: AuditCursor, limit: i64,
) -> sqlx::Result<Vec<AuditPageRow>>
```

What `audit_page` returns, always newest first:
- **`Newest`:** the newest `limit` rows.
- **`Before(id)`:** the newest `limit` rows with id below `id`.
- **`After(id)`:** the **oldest** `limit` rows with id above `id`, so a burst is never skipped.

- [ ] **Step 1: Add the migration in both places.**

```sql
-- Keyset paging for the dashboard's history page: newest first by insertion
-- order (id), per guild. Spec: docs/superpowers/specs/2026-10-02-dashboard-history-design.md
CREATE INDEX IF NOT EXISTS queue_audit_guild_id ON queue_audit (guild_id, id DESC);
```

- [ ] **Step 2: Write the failing DB test.** Add it to `queue_audit.rs`'s `mod test`, after `filters_and_guild_isolation`:

```rust
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn audit_page_pages_by_id_both_ways(pool: PgPool) {
        let g = GuildId::new(1);
        let other = GuildId::new(2);
        let now = chrono::Utc::now();
        let actor = Actor::web(UserId::new(20));
        // Ten rows in guild 1, inserted in order; one in guild 2 between them.
        for i in 0..10 {
            let e = AuditEvent {
                at: now,
                guild_id: g,
                voice_channel: None,
                actor: actor.clone(),
                action: Action::Shuffle { count: i },
            };
            insert_audit_event(&pool, &e).await.unwrap();
            if i == 4 {
                let e = AuditEvent {
                    guild_id: other,
                    ..e
                };
                insert_audit_event(&pool, &e).await.unwrap();
            }
        }
        let count_of = |rows: &[AuditPageRow]| -> Vec<usize> {
            rows.iter()
                .map(|r| match r.row.detail {
                    Action::Shuffle { count } => count,
                    _ => unreachable!(),
                })
                .collect()
        };
        let all = AuditFilter::default();

        let newest = audit_page(&pool, g, &all, AuditCursor::Newest, 3).await.unwrap();
        assert_eq!(count_of(&newest), vec![9, 8, 7], "newest first, guild 1 only");
        assert!(newest.windows(2).all(|w| w[0].id > w[1].id));

        let before = audit_page(&pool, g, &all, AuditCursor::Before(newest[2].id), 3)
            .await
            .unwrap();
        assert_eq!(count_of(&before), vec![6, 5, 4]);

        // After the 3rd-oldest row: the OLDEST three above it, newest first.
        let oldest = audit_page(&pool, g, &all, AuditCursor::Newest, 10).await.unwrap();
        let third_oldest = oldest[7].id;
        let after = audit_page(&pool, g, &all, AuditCursor::After(third_oldest), 3)
            .await
            .unwrap();
        assert_eq!(count_of(&after), vec![5, 4, 3], "the window right above the cursor");

        let nothing_newer = audit_page(&pool, g, &all, AuditCursor::After(newest[0].id), 3)
            .await
            .unwrap();
        assert!(nothing_newer.is_empty());

        // Filters apply with a cursor too.
        let by_action = AuditFilter {
            action: Some("pause"),
            ..AuditFilter::default()
        };
        assert!(audit_page(&pool, g, &by_action, AuditCursor::Newest, 10)
            .await
            .unwrap()
            .is_empty());
    }
```

If `AuditEvent` isn't `Clone` or doesn't allow struct-update syntax, build the guild-2 event as a full literal instead. `Action::Shuffle`'s `count` field may not be `usize`. If it isn't, make `count_of` return that type's `Vec`, so the comparisons stay exact.

- [ ] **Step 3: Implement `audit_page`.** Add it after `recent_audit` in `queue_audit.rs`:

```rust
/// Where a history page starts. Ids are insertion order: the audit writer is
/// one FIFO task, so "after id X" can never miss a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditCursor {
    /// The newest rows.
    #[default]
    Newest,
    /// Rows older than this id.
    Before(i64),
    /// Rows newer than this id: the oldest of them first in line, so a burst
    /// bigger than one page is read across polls, never skipped.
    After(i64),
}

/// A row with its id, for paging.
#[derive(Debug, Clone)]
pub struct AuditPageRow {
    pub id: i64,
    pub row: AuditRow,
}

/// A page of a guild's audit rows, newest first: see [`AuditCursor`] for
/// which `limit` rows.
pub async fn audit_page(
    pool: &PgPool,
    guild_id: GuildId,
    f: &AuditFilter<'_>,
    cursor: AuditCursor,
    limit: i64,
) -> sqlx::Result<Vec<AuditPageRow>> {
    let guild = guild_id.get() as i64;
    let user = f.user.map(|u| u.get() as i64);
    if let AuditCursor::After(after) = cursor {
        let rows = sqlx::query!(
            r#"SELECT id, at, actor_user_id, source, command, action,
                      detail AS "detail!: Json<Action>"
               FROM queue_audit
               WHERE guild_id = $1
                 AND ($2::bigint      IS NULL OR actor_user_id = $2)
                 AND ($3::text        IS NULL OR action = $3)
                 AND ($4::text        IS NULL OR source = $4)
                 AND ($5::timestamptz IS NULL OR at >= $5)
                 AND id > $6
               ORDER BY id ASC
               LIMIT $7"#,
            guild,
            user,
            f.action,
            f.source,
            f.since,
            after,
            limit,
        )
        .fetch_all(pool)
        .await?;
        let mut out: Vec<AuditPageRow> = rows
            .into_iter()
            .map(|r| AuditPageRow {
                id: r.id,
                row: AuditRow {
                    at: r.at,
                    actor_user_id: r.actor_user_id,
                    source: r.source,
                    command: r.command,
                    action: r.action,
                    detail: r.detail.0,
                },
            })
            .collect();
        out.reverse();
        return Ok(out);
    }
    let before = match cursor {
        AuditCursor::Before(id) => Some(id),
        _ => None,
    };
    let rows = sqlx::query!(
        r#"SELECT id, at, actor_user_id, source, command, action,
                  detail AS "detail!: Json<Action>"
           FROM queue_audit
           WHERE guild_id = $1
             AND ($2::bigint      IS NULL OR actor_user_id = $2)
             AND ($3::text        IS NULL OR action = $3)
             AND ($4::text        IS NULL OR source = $4)
             AND ($5::timestamptz IS NULL OR at >= $5)
             AND ($6::bigint      IS NULL OR id < $6)
           ORDER BY id DESC
           LIMIT $7"#,
        guild,
        user,
        f.action,
        f.source,
        f.since,
        before,
        limit,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AuditPageRow {
            id: r.id,
            row: AuditRow {
                at: r.at,
                actor_user_id: r.actor_user_id,
                source: r.source,
                command: r.command,
                action: r.action,
                detail: r.detail.0,
            },
        })
        .collect())
}
```

- [ ] **Step 4: Generate the offline query cache.** Start a throwaway Postgres, migrate it and prepare:

```bash
docker --context default run -d --rm --name ct-history-pg -e POSTGRES_PASSWORD=pw -p 55432:5432 postgres:16-alpine
sleep 4
export DATABASE_URL=postgres://postgres:pw@localhost:55432/postgres
sqlx migrate run --source migrations
cargo sqlx prepare --workspace
git status --short .sqlx
```

🪤 **The local sqlx-cli is 0.8.6, but the crate is sqlx 0.9.** `prepare` rewrites *every* `.sqlx` file, adding `"origin"` keys. Keep only the **two new** `query-*.json` files:
1. Restore the rest with `git checkout -- .sqlx`, then confirm `git status --short .sqlx` lists only the two untracked files.
2. Open an existing `.sqlx` file. Remove from the two new files any key that the existing files do not have (for example `"origin"`), so they match the existing format exactly.

- [ ] **Step 5: Run the DB tests and the offline build**

```bash
DATABASE_URL=postgres://postgres:pw@localhost:55432/postgres cargo test -q -p crack-core --features db-tests --lib queue_audit
SQLX_OFFLINE=true cargo check -q -p crack-core --tests
```
Expected: `audit_page_pages_by_id_both_ways` and the existing queue_audit DB tests pass. The offline check compiles.

- [ ] **Step 6: Sabotage.** Do each, see the test fail, then restore:
  - change the `After` query's `ORDER BY id ASC` to `DESC`. Expected: the after-window assertion fails, `[9, 8, 7]` instead of `[5, 4, 3]`;
  - drop `out.reverse()`. Expected: the after window comes back oldest first.

  Then stop the database: `docker --context default stop ct-history-pg`.

- [ ] **Step 7: Run clippy, then commit**

```bash
SQLX_OFFLINE=true cargo clippy -q -p crack-core --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add migrations/20261002120000_queue_audit_guild_id.sql crack-core/test_migrations/20261002120000_queue_audit_guild_id.sql crack-core/src/db/queue_audit.rs .sqlx/query-<the two new hashes>.json
git commit -m "queue_audit: audit_page, id-ordered keyset paging both ways, and its index

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: The Manage Server check and the role memo (`crack-web/src/access.rs`)

**Files:**
- Modify: `crack-web/src/access.rs`

**Interfaces:**
- Consumes: the existing `Membership`.
- Produces (in `crate::access`):

```rust
pub enum HistoryAccess { Hidden, Unavailable, Forbidden, Allowed }   // Debug, Clone, Copy, PartialEq, Eq
pub fn has_manage_guild(owner: UserId, user: UserId, everyone: Permissions,
    roles: &[RoleId], role_permissions: impl Fn(RoleId) -> Option<Permissions>) -> bool
pub fn decide_history(membership: Membership, manages: Option<bool>) -> HistoryAccess
pub struct RoleMemo   // new(ttl), get(g, u, now) -> Option<Vec<RoleId>>, record(g, u, roles, now)
```

- [ ] **Step 1: Write the failing tests.** Add them to `access.rs`'s `mod test`:

```rust
    use serenity::all::{Permissions, RoleId};

    #[test]
    fn manage_guild_comes_from_owner_admin_or_the_permission() {
        let owner = UserId::new(1);
        let me = UserId::new(2);
        let admin = RoleId::new(10);
        let manager = RoleId::new(11);
        let dj = RoleId::new(12);
        let perms = |r: RoleId| match r.get() {
            10 => Some(Permissions::ADMINISTRATOR),
            11 => Some(Permissions::MANAGE_GUILD),
            12 => Some(Permissions::CONNECT | Permissions::SPEAK),
            _ => None,
        };
        let none = Permissions::empty();
        assert!(has_manage_guild(owner, owner, none, &[], perms), "the owner");
        assert!(has_manage_guild(owner, me, none, &[admin], perms), "Administrator");
        assert!(has_manage_guild(owner, me, none, &[dj, manager], perms), "Manage Server on a role");
        assert!(
            has_manage_guild(owner, me, Permissions::MANAGE_GUILD, &[], perms),
            "Manage Server on @everyone"
        );
        assert!(!has_manage_guild(owner, me, none, &[dj], perms), "no permission");
        assert!(
            !has_manage_guild(owner, me, none, &[RoleId::new(99)], perms),
            "a role the guild does not have counts for nothing"
        );
    }

    #[test]
    fn the_history_decision_table() {
        use Membership::*;
        assert_eq!(decide_history(NotMember, Some(true)), HistoryAccess::Hidden);
        assert_eq!(decide_history(Unknown, Some(true)), HistoryAccess::Unavailable);
        assert_eq!(decide_history(Member, None), HistoryAccess::Unavailable);
        assert_eq!(decide_history(Member, Some(false)), HistoryAccess::Forbidden);
        assert_eq!(decide_history(Member, Some(true)), HistoryAccess::Allowed);
    }

    #[test]
    fn the_role_memo_remembers_roles_and_forgets_in_time() {
        let memo = RoleMemo::new(Duration::from_secs(300));
        let (g, u) = (GuildId::new(1), UserId::new(2));
        let t0 = Instant::now();
        assert_eq!(memo.get(g, u, t0), None);
        memo.record(g, u, vec![RoleId::new(5)], t0);
        assert_eq!(memo.get(g, u, t0 + Duration::from_secs(299)), Some(vec![RoleId::new(5)]));
        assert_eq!(memo.get(g, u, t0 + Duration::from_secs(300)), None);
    }
```

The test module already imports `GuildId`, `UserId`, `Duration` and `Instant` for the membership memo tests. Add any that are missing.

- [ ] **Step 2: Run the tests and see them fail**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web --lib access`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement.** Add these to `access.rs`, after `decide`. Extend the file's `serenity::all` import with `Permissions` and `RoleId`:

```rust
/// Who may see a guild's queue history (spec: dashboard history design §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryAccess {
    /// Not a member: 404, as for the queue.
    Hidden,
    /// Discord did not answer: 503.
    Unavailable,
    /// A member without Manage Server: 403.
    Forbidden,
    Allowed,
}

/// Manage Server, as Discord computes it at the guild level: the owner always;
/// otherwise `@everyone`'s permissions plus each of the member's roles',
/// with Administrator implying everything. Roles the guild does not have
/// count for nothing. Channel overrides do not apply to a guild permission.
pub fn has_manage_guild(
    owner: UserId,
    user: UserId,
    everyone: Permissions,
    roles: &[RoleId],
    role_permissions: impl Fn(RoleId) -> Option<Permissions>,
) -> bool {
    if user == owner {
        return true;
    }
    let perms = roles
        .iter()
        .filter_map(|r| role_permissions(*r))
        .fold(everyone, |acc, p| acc | p);
    perms.intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD)
}

/// The history rule. `manages` is `None` when the member's roles could not be
/// read.
pub fn decide_history(membership: Membership, manages: Option<bool>) -> HistoryAccess {
    match (membership, manages) {
        (Membership::NotMember, _) => HistoryAccess::Hidden,
        (Membership::Unknown, _) | (Membership::Member, None) => HistoryAccess::Unavailable,
        (Membership::Member, Some(false)) => HistoryAccess::Forbidden,
        (Membership::Member, Some(true)) => HistoryAccess::Allowed,
    }
}

/// Remembered role lists of members fetched over HTTP, so a history page
/// polling every 10 s asks Discord at most once per TTL.
pub struct RoleMemo {
    ttl: Duration,
    entries: DashMap<(GuildId, UserId), (Vec<RoleId>, Instant)>,
}

impl RoleMemo {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: DashMap::new(),
        }
    }

    pub fn get(&self, g: GuildId, u: UserId, now: Instant) -> Option<Vec<RoleId>> {
        let entry = self.entries.get(&(g, u))?;
        let (roles, at) = &*entry;
        (now.saturating_duration_since(*at) < self.ttl).then(|| roles.clone())
    }

    pub fn record(&self, g: GuildId, u: UserId, roles: Vec<RoleId>, now: Instant) {
        self.entries.insert((g, u), (roles, now));
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web --lib access`
Expected: all pass.

- [ ] **Step 5: Sabotage.** Do each, see it fail, then restore:
  - remove the owner short-circuit;
  - change `intersects(ADMINISTRATOR | MANAGE_GUILD)` to `contains(MANAGE_GUILD)`. Expected: the Administrator case fails;
  - make `(Member, None)` return `Forbidden`.

- [ ] **Step 6: Run clippy, then commit**

```bash
SQLX_OFFLINE=true cargo clippy -q -p crack-web --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-web/src/access.rs
git commit -m "crack-web: Manage Server check and role memo for the history page

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: `history.rs`, with the wire types, the query parser, the page composer and the name memo

**Files:**
- Create: `crack-web/src/history.rs`
- Modify: `crack-web/src/lib.rs`: add `pub mod history;`
- Modify: `crack-web/Cargo.toml`: add `chrono = { version = "0.4", features = ["serde"] }` under `[dependencies]`, the same spec as crack-core's.

**Interfaces:**
- Consumes: Task 1's `how_text`, `what_text`, `hidden_by_game`, `ActionChoice::from_name`, `SourceChoice::from_name` and `parse_since`. Task 2's `AuditCursor` and `AuditPageRow`.
- Produces (in `crate::history`):

```rust
pub const PAGE_SIZE: usize = 50;
pub const NAME_LOOKUPS: usize = 10;
pub const NAME_TTL: Duration = Duration::from_secs(3600);
pub const ROLE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Who { Member { id: String, name: Option<String> }, Bot }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRow { pub id: i64, pub at: String, pub who: Who, pub how: String, pub what: String }

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HistoryPage { pub rows: Vec<HistoryRow>, pub older: bool, pub game_hidden: bool }

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryQuery {
    pub user: Option<UserId>, pub action: Option<ActionChoice>, pub source: Option<SourceChoice>,
    pub since: Option<chrono::Duration>, pub cursor: AuditCursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadQuery(pub &'static str);

#[derive(Debug, Serialize)]
pub struct ErrorBody { pub error: &'static str }

pub fn parse_history_query(raw: Option<&str>) -> Result<HistoryQuery, BadQuery>
pub fn fetch_limit(cursor: AuditCursor) -> i64
pub fn compose_page(rows: Vec<AuditPageRow>, cursor: AuditCursor,
    running_since: Option<chrono::DateTime<chrono::Utc>>,
    name_of: impl Fn(UserId) -> Option<String>) -> HistoryPage
pub struct NameMemo  // new(ttl), get(u, now) -> Option<String>, record(u, name, now)
```

- [ ] **Step 1: Write the module with its tests first.** Create `crack-web/src/history.rs` with the test module below. Fill in the implementation in Step 3.

```rust
#[cfg(test)]
mod test {
    use super::*;
    use crack_core::music::audit::{Action, TrackRef};
    use crack_core::music::audit_view::AuditRow;
    use chrono::{TimeZone, Utc};

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
            vec![page_row(2, "skip", Some(big), 100), page_row(1, "skip", None, 50)],
            AuditCursor::Newest,
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
        let newest = compose_page(rows.clone(), AuditCursor::Newest, None, |_| None);
        assert!(newest.older);
        assert_eq!(newest.rows.len(), PAGE_SIZE);
        assert_eq!(newest.rows[0].id, PAGE_SIZE as i64 + 1, "kept the newest");
        let after = compose_page(rows[..PAGE_SIZE].to_vec(), AuditCursor::After(0), None, |_| None);
        assert!(!after.older);
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
            |_| None,
        );
        assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2, 1]);
        assert!(page.game_hidden);
        let idle = compose_page(vec![page_row(1, "gp", None, 1_500)], AuditCursor::Newest, None, |_| None);
        assert!(!idle.game_hidden);
        assert_eq!(idle.rows.len(), 1);
    }

    #[test]
    fn a_stored_user_id_that_is_not_a_snowflake_gets_no_name_lookup() {
        let page = compose_page(vec![page_row(1, "skip", Some(0), 1)], AuditCursor::Newest, None, |_| {
            panic!("UserId::new(0) would panic; never look it up")
        });
        assert_eq!(
            page.rows[0].who,
            Who::Member {
                id: "0".into(),
                name: None
            }
        );
    }

    #[test]
    fn the_name_memo_remembers_and_forgets_in_time() {
        let memo = NameMemo::new(NAME_TTL);
        let u = UserId::new(4);
        let t0 = Instant::now();
        assert_eq!(memo.get(u, t0), None);
        memo.record(u, "Bo".into(), t0);
        assert_eq!(memo.get(u, t0 + NAME_TTL - Duration::from_secs(1)), Some("Bo".into()));
        assert_eq!(memo.get(u, t0 + NAME_TTL), None);
    }
}
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web --lib history`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement.** Put this above the test module in `history.rs`:

```rust
//! The history page's data: the query it accepts, the rows it shows, and how
//! one becomes the other. Pure, apart from the name memo's clock.
//! Spec: docs/superpowers/specs/2026-10-02-dashboard-history-design.md

use crack_core::db::queue_audit::{AuditCursor, AuditPageRow};
use crack_core::music::audit_view::{
    hidden_by_game, how_text, parse_since, what_text, ActionChoice, SourceChoice,
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
    "action must be one of add, remove, move, skip, clear, shuffle, stop, pause, resume, leave";
pub const BAD_SOURCE: &str = "source must be one of slash, prefix, web, bot";
pub const BAD_SINCE: &str = "since must look like 90m, 6h, 2d or 1w, at most 52 weeks";
pub const BAD_CURSOR: &str = "before and after must be positive row ids";
pub const BOTH_CURSORS: &str = "use before or after, not both";
pub const NEEDS_MANAGE: &str = "Only members with Manage Server can see this server's queue history.";
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

/// The page for `rows` (newest first, as `audit_page` returns them, fetched
/// with [`fetch_limit`]): cut to a page, a running game's rows dropped, names
/// filled in where `name_of` knows them.
pub fn compose_page(
    mut rows: Vec<AuditPageRow>,
    cursor: AuditCursor,
    running_since: Option<chrono::DateTime<chrono::Utc>>,
    name_of: impl Fn(UserId) -> Option<String>,
) -> HistoryPage {
    let older = !matches!(cursor, AuditCursor::After(_)) && rows.len() > PAGE_SIZE;
    rows.truncate(PAGE_SIZE);
    let rows = rows
        .into_iter()
        .filter(|r| !hidden_by_game(&r.row, running_since))
        .map(|r| HistoryRow {
            id: r.id,
            at: r.row.at.to_rfc3339(),
            who: match r.row.actor_user_id {
                None => Who::Bot,
                Some(id) => Who::Member {
                    id: id.to_string(),
                    name: u64::try_from(id)
                        .ok()
                        .and_then(NonZeroU64::new)
                        .and_then(|n| name_of(UserId::new(n.get()))),
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
```

Add `pub mod history;` to `crack-web/src/lib.rs`, in alphabetical order after `pub mod config;`. Add the chrono dependency to `crack-web/Cargo.toml`.

- [ ] **Step 4: Run the tests and see them pass**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web --lib history`
Expected: all pass. If the RFC 3339 assertion differs only in format (`+00:00` against `Z`), keep `to_rfc3339()` and fix the expected string to what chrono produces; JS `Date.parse` accepts both.

- [ ] **Step 5: Sabotage.** Do each, see it fail, then restore:
  - send `id` as a number by making `Who::Member.id` an `i64`. Expected: the string assertion fails;
  - compute `older` without the `After` exclusion;
  - drop the `hidden_by_game` filter;
  - remove `present()` from `action`. Expected: `action=` refused.

- [ ] **Step 6: Run clippy, then commit**

```bash
SQLX_OFFLINE=true cargo clippy -q -p crack-web --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-web/src/history.rs crack-web/src/lib.rs crack-web/Cargo.toml Cargo.lock
git commit -m "crack-web: history query parser, page composer and name memo

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: Backend methods, `LiveBackend`, routes and pages

**Files:**
- Modify: `crack-web/src/backend.rs`, `crack-web/src/lib.rs`, `crack-web/src/routes.rs`, `crack-web/src/page.rs`, `crack-web/src/test_support.rs`

**Interfaces:**
- Consumes: Tasks 2, 3 and 4.
- Produces:
  - **`Backend` methods:**
    - `fn history_access(&self, g: GuildId, u: UserId) -> impl Future<Output = HistoryAccess> + Send;`
    - `fn history(&self, g: GuildId, q: &HistoryQuery) -> impl Future<Output = Result<HistoryPage, HistoryError>> + Send;`
  - **`backend::HistoryError`:** `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum HistoryError { NoDatabase, Failed }`
  - **Routes:**
    - `GET /g/{guild}/history` returns HTML;
    - `GET /g/{guild}/history.json` returns JSON. Both are in the `per_user` router, so they get `no_store`.
  - **`page::history_page(guild_name: &str, guild_id: GuildId, page: &HistoryPage) -> String`**
  - **`page::queue_page` signature change:** a fourth parameter, `history_link: bool`.

- [ ] **Step 1: Extend the fake.** In `test_support.rs`:
  - Add fields to `FakeBackend`:

```rust
    pub history_access: Mutex<crate::access::HistoryAccess>,
    pub history_result: Mutex<Result<crate::history::HistoryPage, crate::backend::HistoryError>>,
    /// Every history query the routes made.
    pub history_queries: Mutex<Vec<crate::history::HistoryQuery>>,
```

  - In `FakeBackend::new`, initialise them to `Mutex::new(crate::access::HistoryAccess::Forbidden)`, `Mutex::new(Ok(crate::history::HistoryPage::default()))` and `Mutex::new(Vec::new())`. `Forbidden` by default keeps today's queue-page tests unchanged.
  - Add these to `impl Backend for FakeBackend`:

```rust
    async fn history_access(&self, _g: GuildId, _u: UserId) -> crate::access::HistoryAccess {
        *self.history_access.lock().unwrap()
    }

    async fn history(
        &self,
        _g: GuildId,
        q: &crate::history::HistoryQuery,
    ) -> Result<crate::history::HistoryPage, crate::backend::HistoryError> {
        self.history_queries.lock().unwrap().push(q.clone());
        self.history_result.lock().unwrap().clone()
    }
```

- [ ] **Step 2: Write the failing route tests.** Add them to `routes.rs`'s `mod test`:

```rust
    use crate::{
        access::HistoryAccess,
        backend::HistoryError,
        history::{HistoryPage, HistoryRow, Who},
    };

    fn history_fake(access: HistoryAccess) -> std::sync::Arc<FakeBackend> {
        let fake = member_viewing();
        *fake.history_access.lock().unwrap() = access;
        fake
    }

    #[tokio::test]
    async fn signed_out_history_goes_to_login_and_its_json_is_401() {
        let r = get(member_viewing(), "/g/5/history", None).await;
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            r.headers()[header::LOCATION],
            "/auth/login?return_to=%2Fg%2F5%2Fhistory"
        );
        let r = get(member_viewing(), "/g/5/history.json", None).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn history_access_maps_to_its_answers() {
        for (access, status) in [
            (HistoryAccess::Hidden, StatusCode::NOT_FOUND),
            (HistoryAccess::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
            (HistoryAccess::Forbidden, StatusCode::FORBIDDEN),
            (HistoryAccess::Allowed, StatusCode::OK),
        ] {
            for uri in ["/g/5/history", "/g/5/history.json"] {
                let r = get(history_fake(access), uri, Some(&session(9))).await;
                assert_eq!(r.status(), status, "{access:?} {uri}");
            }
        }
    }

    #[tokio::test]
    async fn bad_history_queries_are_400_and_never_reach_the_backend() {
        for q in [
            "user=abc",
            "action=dance",
            "source=email",
            "since=3y",
            "before=x",
            "before=1&after=2",
        ] {
            let fake = history_fake(HistoryAccess::Allowed);
            let r = get(fake.clone(), &format!("/g/5/history.json?{q}"), Some(&session(9))).await;
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{q}");
            assert!(body(r).await.contains("\"error\""), "{q}");
            assert!(fake.history_queries.lock().unwrap().is_empty(), "{q}");
        }
    }

    #[tokio::test]
    async fn the_backend_gets_exactly_the_query_sent() {
        let fake = history_fake(HistoryAccess::Allowed);
        let r = get(
            fake.clone(),
            "/g/5/history.json?user=7&action=move&source=web&since=6h&after=40",
            Some(&session(9)),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let got = fake.history_queries.lock().unwrap().clone();
        assert_eq!(
            got,
            vec![crate::history::parse_history_query(Some(
                "user=7&action=move&source=web&since=6h&after=40"
            ))
            .unwrap()]
        );
    }

    #[tokio::test]
    async fn history_json_carries_the_backends_page() {
        let fake = history_fake(HistoryAccess::Allowed);
        let page = HistoryPage {
            rows: vec![HistoryRow {
                id: 3,
                at: "2026-10-02T12:00:00+00:00".into(),
                who: Who::Bot,
                how: "idle timeout".into(),
                what: "left voice, 2 tracks discarded".into(),
            }],
            older: true,
            game_hidden: true,
        };
        *fake.history_result.lock().unwrap() = Ok(page.clone());
        let r = get(fake, "/g/5/history.json", Some(&session(9))).await;
        let got: HistoryPage = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(got, page);
    }

    #[tokio::test]
    async fn a_missing_database_is_503_with_its_reason() {
        let fake = history_fake(HistoryAccess::Allowed);
        *fake.history_result.lock().unwrap() = Err(HistoryError::NoDatabase);
        let r = get(fake.clone(), "/g/5/history.json", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(body(r).await.contains(crate::history::NO_DATABASE));
        let r = get(fake, "/g/5/history", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn the_history_page_inlines_the_first_page() {
        let fake = history_fake(HistoryAccess::Allowed);
        let page = HistoryPage {
            rows: vec![HistoryRow {
                id: 1,
                at: "2026-10-02T12:00:00+00:00".into(),
                who: Who::Member {
                    id: "7".into(),
                    name: Some("</script><img src=x>".into()),
                },
                how: "/play".into(),
                what: "added </script>".into(),
            }],
            older: false,
            game_hidden: false,
        };
        *fake.history_result.lock().unwrap() = Ok(page.clone());
        let html = body(get(fake, "/g/5/history", Some(&session(9))).await).await;
        assert!(!html.contains("<img"), "{html}");
        let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
        let end = start + html[start..].find("</script>").unwrap();
        let got: HistoryPage = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(got, page);
        assert!(html.contains("src=\"/assets/history.js\""));
    }

    #[tokio::test]
    async fn the_queue_page_links_history_for_managers_only() {
        let html = body(get(history_fake(HistoryAccess::Allowed), "/g/5", Some(&session(9))).await).await;
        assert!(html.contains("href=\"/g/5/history\""));
        for access in [HistoryAccess::Forbidden, HistoryAccess::Unavailable] {
            let html = body(get(history_fake(access), "/g/5", Some(&session(9))).await).await;
            assert!(!html.contains("/history"), "{access:?}");
        }
    }

    #[tokio::test]
    async fn history_js_is_served() {
        let r = get(member_viewing(), "/assets/history.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/javascript"));
    }
```

`history_js_is_served` needs Task 6's file. Create an empty `crack-web/assets/history.js` in this task so `include_str!` compiles; Task 6 fills it in.

- [ ] **Step 3: Run the tests and see them fail**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web --lib routes`
Expected: compile errors (the trait methods, `HistoryError` and the routes don't exist yet).

- [ ] **Step 4: Implement `backend.rs`.** Add:

```rust
use crate::{access::HistoryAccess, history::{HistoryPage, HistoryQuery}};

/// Why a history page could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryError {
    /// The bot runs without a database: there is no history to read.
    NoDatabase,
    /// The query failed; logged where it happened.
    Failed,
}
```

and add these to `trait Backend`:

```rust
    fn history_access(&self, g: GuildId, u: UserId) -> impl Future<Output = HistoryAccess> + Send;
    fn history(
        &self,
        g: GuildId,
        q: &HistoryQuery,
    ) -> impl Future<Output = Result<HistoryPage, HistoryError>> + Send;
```

- [ ] **Step 5: Implement `page.rs`.** Change `layout` to take the script list, and add the history page:

```rust
/// Scripts every page but the history page loads.
const QUEUE_SCRIPTS: &[&str] = &["sortable.min.js", "app.js"];
/// The history page: `app.js` for the logout button, `history.js` for the rest.
const HISTORY_SCRIPTS: &[&str] = &["app.js", "history.js"];

fn layout(title: &str, body: &str, scripts: &[&str]) -> String {
    let scripts: String = scripts
        .iter()
        .map(|s| format!("<script src=\"/assets/{s}\"></script>"))
        .collect();
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title><link rel=\"stylesheet\" href=\"/assets/app.css\"></head>\
<body><header><a class=\"brand\" href=\"/\">Crack Tunes</a>\
<button type=\"button\" id=\"logout\">Log out</button></header>\
<main>{body}</main>{scripts}</body></html>",
        title = esc(title),
    )
}
```

Pass `QUEUE_SCRIPTS` from `picker_page`, `queue_page` and `message_page`; their HTML stays byte-identical. Change `queue_page`:

```rust
/// `GET /g/{id}`: the queue page. The state is inlined; `app.js` draws it.
/// `history_link` is for managers (Manage Server).
pub fn queue_page(
    guild_name: &str,
    guild_id: GuildId,
    state: &PageState,
    history_link: bool,
) -> String {
    let links = if history_link {
        format!("<p class=\"links\"><a href=\"/g/{guild_id}/history\">History</a></p>")
    } else {
        String::new()
    };
    layout(
        guild_name,
        &format!(
            "<section id=\"dash\" data-guild=\"{guild_id}\">\
<h1>{name}</h1>{links}<p id=\"badge\" hidden>Reconnecting…</p><p id=\"note\" hidden></p>\
<div id=\"now\"></div><h2>Up next</h2><ol id=\"upcoming\"></ol></section>\
<script type=\"application/json\" id=\"initial\">{json}</script>",
            name = esc(guild_name),
            json = inline_json(state),
        ),
        QUEUE_SCRIPTS,
    )
}
```

Update `page.rs`'s existing test call `queue_page("G", GuildId::new(5), &PageState {…})` to pass `false` as the fourth argument. Add the history page:

```rust
/// `GET /g/{id}/history`: the first page inlined; `history.js` draws it,
/// filters it, polls for new rows and loads older ones.
pub fn history_page(guild_name: &str, guild_id: GuildId, page: &HistoryPage) -> String {
    const ACTIONS: [&str; 10] = [
        "add", "remove", "move", "skip", "clear", "shuffle", "stop", "pause", "resume", "leave",
    ];
    let actions: String = ACTIONS
        .iter()
        .map(|a| format!("<option value=\"{a}\">{a}</option>"))
        .collect();
    layout(
        &format!("History · {guild_name}"),
        &format!(
            "<section id=\"history\" data-guild=\"{guild_id}\">\
<h1>History · {name}</h1>\
<p class=\"links\"><a href=\"/g/{guild_id}\">← Queue</a></p>\
<p id=\"badge\" hidden>Reconnecting…</p><p id=\"note\" hidden></p>\
<p id=\"gp-note\" hidden>Entries from the running /gp game are hidden until it ends.</p>\
<div id=\"filters\">\
<label>Action <select id=\"f-action\"><option value=\"\">all</option>{actions}</select></label>\
<label>Source <select id=\"f-source\"><option value=\"\">all</option>\
<option value=\"slash\">slash</option><option value=\"prefix\">prefix</option>\
<option value=\"web\">dashboard</option><option value=\"bot\">bot</option></select></label>\
<label>Since <select id=\"f-since\"><option value=\"\">all</option>\
<option value=\"1h\">1 h</option><option value=\"6h\">6 h</option>\
<option value=\"1d\">1 d</option><option value=\"1w\">1 w</option></select></label>\
<span id=\"f-user\" class=\"chip\" hidden><span id=\"f-user-name\"></span>\
<button type=\"button\" id=\"f-user-clear\" aria-label=\"Clear the member filter\">×</button></span>\
</div><ol id=\"rows\"></ol>\
<button type=\"button\" id=\"older\" hidden>Load older</button></section>\
<script type=\"application/json\" id=\"initial\">{json}</script>",
            name = esc(guild_name),
            json = inline_json(page),
        ),
        HISTORY_SCRIPTS,
    )
}
```

Import `crate::history::HistoryPage` in `page.rs`.

- [ ] **Step 6: Implement the routes.** In `routes.rs`:
  - add imports: `crate::access::HistoryAccess`, `crate::backend::HistoryError`, `crate::history::{parse_history_query, ErrorBody, HistoryQuery, HISTORY_FAILED, NEEDS_MANAGE, NO_DATABASE}` and `axum::extract::RawQuery`;
  - add the handlers:

```rust
fn forbidden_page() -> Response {
    (
        StatusCode::FORBIDDEN,
        Html(page::message_page("Needs Manage Server", NEEDS_MANAGE)),
    )
        .into_response()
}

fn error_json(status: StatusCode, error: &'static str) -> Response {
    (status, Json(ErrorBody { error })).into_response()
}

async fn history_page<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, _)) = user_id(session) else {
        return login_redirect(&format!("/g/{g}/history"));
    };
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return not_found(),
        HistoryAccess::Unavailable => return unavailable(),
        HistoryAccess::Forbidden => return forbidden_page(),
        HistoryAccess::Allowed => {},
    }
    match s.backend.history(g, &HistoryQuery::default()).await {
        Ok(first) => {
            let name = s
                .backend
                .guild_name(g)
                .unwrap_or_else(|| "Server".to_owned());
            Html(page::history_page(&name, g, &first)).into_response()
        },
        Err(HistoryError::NoDatabase) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Html(page::message_page("No history", NO_DATABASE)),
        )
            .into_response(),
        Err(HistoryError::Failed) => unavailable(),
    }
}

async fn history_json<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return StatusCode::NOT_FOUND.into_response(),
        HistoryAccess::Unavailable => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        HistoryAccess::Forbidden => return error_json(StatusCode::FORBIDDEN, NEEDS_MANAGE),
        HistoryAccess::Allowed => {},
    }
    let q = match parse_history_query(query.as_deref()) {
        Ok(q) => q,
        Err(bad) => return error_json(StatusCode::BAD_REQUEST, bad.0),
    };
    match s.backend.history(g, &q).await {
        Ok(page) => Json(page).into_response(),
        Err(HistoryError::NoDatabase) => error_json(StatusCode::SERVICE_UNAVAILABLE, NO_DATABASE),
        Err(HistoryError::Failed) => error_json(StatusCode::SERVICE_UNAVAILABLE, HISTORY_FAILED),
    }
}
```

  - In `guild_page`'s `Access::View | Access::Control` arm, compute the link and pass it:

```rust
            let history_link =
                s.backend.history_access(g, user).await == HistoryAccess::Allowed;
            Html(page::queue_page(&name, g, &state, history_link)).into_response()
```

  - Add to the `per_user` router, after `/g/{guild}/move`:

```rust
        .route("/g/{guild}/history", get(history_page::<B>))
        .route("/g/{guild}/history.json", get(history_json::<B>))
```

  - Add to `asset`'s match:

```rust
        "history.js" => (
            include_str!("../assets/history.js"),
            "text/javascript; charset=utf-8",
        ),
```

- [ ] **Step 7: Implement `LiveBackend`.** In `crack-web/src/lib.rs`:
  - Add fields to `LiveBackend`: `roles: access::RoleMemo` and `names: history::NameMemo`. In `serve`, construct `roles: access::RoleMemo::new(history::ROLE_TTL)` and `names: history::NameMemo::new(history::NAME_TTL)`.
  - Add these methods to `impl LiveBackend`:

```rust
    /// A member's role ids: the cache, then the role memo, then Discord.
    /// `None` when Discord did not answer.
    async fn member_roles(&self, g: GuildId, u: UserId) -> Option<Vec<RoleId>> {
        if let Some(roles) = self.deps.cache.guild(g).and_then(|guild| {
            guild
                .members
                .get(&u)
                .map(|m| m.roles.iter().copied().collect::<Vec<_>>())
        }) {
            return Some(roles);
        }
        let now = std::time::Instant::now();
        if let Some(roles) = self.roles.get(g, u, now) {
            return Some(roles);
        }
        match self.deps.http.get_member(g, u).await {
            Ok(m) => {
                let roles: Vec<RoleId> = m.roles.iter().copied().collect();
                self.roles.record(g, u, roles.clone(), now);
                Some(roles)
            },
            Err(e) => {
                tracing::warn!("history: could not read {u}'s roles in {g}: {e}");
                None
            },
        }
    }

    /// Names for the members in `rows`: the cache, the name memo, then at most
    /// `NAME_LOOKUPS` Discord lookups. Anyone left out shows as their id.
    async fn names_for(&self, g: GuildId, rows: &[AuditPageRow]) -> HashMap<UserId, String> {
        let mut out = HashMap::new();
        let mut lookups = 0;
        let ids = rows
            .iter()
            .filter_map(|r| r.row.actor_user_id)
            .filter_map(|id| u64::try_from(id).ok())
            .filter_map(NonZeroU64::new)
            .map(|n| UserId::new(n.get()));
        for u in ids {
            if out.contains_key(&u) {
                continue;
            }
            if let Some(name) = self.member_name(g, u) {
                out.insert(u, name);
                continue;
            }
            let now = std::time::Instant::now();
            if let Some(name) = self.names.get(u, now) {
                out.insert(u, name);
                continue;
            }
            if lookups >= history::NAME_LOOKUPS {
                continue;
            }
            lookups += 1;
            if let Ok(user) = self.deps.http.get_user(u).await {
                let name = user.display_name().to_owned();
                self.names.record(u, name.clone(), now);
                out.insert(u, name);
            }
        }
        out
    }
```

  - Add to `impl Backend for LiveBackend`:

```rust
    async fn history_access(&self, g: GuildId, u: UserId) -> HistoryAccess {
        let membership = self.presence(g, u).await.membership;
        if membership != Membership::Member {
            return access::decide_history(membership, None);
        }
        let roles = self.member_roles(g, u).await;
        let manages = roles.and_then(|roles| {
            let guild = self.deps.cache.guild(g)?;
            let everyone = guild
                .roles
                .get(&RoleId::new(g.get()))
                .map(|r| r.permissions)
                .unwrap_or_else(Permissions::empty);
            Some(access::has_manage_guild(
                guild.owner_id,
                u,
                everyone,
                &roles,
                |r| guild.roles.get(&r).map(|role| role.permissions),
            ))
        });
        access::decide_history(membership, manages)
    }

    async fn history(&self, g: GuildId, q: &HistoryQuery) -> Result<HistoryPage, HistoryError> {
        let Some(pool) = self.deps.data.database_pool.clone() else {
            return Err(HistoryError::NoDatabase);
        };
        let filter = AuditFilter {
            user: q.user,
            action: q.action.map(|a| a.name()),
            source: q.source.map(|s| s.source().as_str()),
            since: q.since.map(|d| chrono::Utc::now() - d),
        };
        let rows = audit_page(&pool, g, &filter, q.cursor, history::fetch_limit(q.cursor))
            .await
            .map_err(|e| {
                tracing::warn!("history: query failed in {g}: {e}");
                HistoryError::Failed
            })?;
        // Copy the start time out so the DashMap ref is dropped before any await.
        let running_since = self
            .deps
            .data
            .gp_games
            .get(&g)
            .map(|game| game.started_at)
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0));
        let names = self.names_for(g, &rows).await;
        Ok(history::compose_page(rows, q.cursor, running_since, |u| {
            names.get(&u).cloned()
        }))
    }
```

  - Imports for `lib.rs`:
    - `crate::access::HistoryAccess`;
    - `crate::backend::HistoryError`;
    - `crate::history::{HistoryPage, HistoryQuery}`;
    - `crack_core::db::queue_audit::{audit_page, AuditFilter, AuditPageRow}`;
    - `serenity::all::{Permissions, RoleId}`;
    - `std::collections::HashMap`;
    - `std::num::NonZeroU64`.

    If a `serenity::all` name isn't exported under that path, find its re-export with `grep -rn "pub use" ~/.cargo/git/checkouts/serenity-*/37b9f43/src/model/mod.rs`.

- [ ] **Step 8: Run all crack-web tests**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web`
Expected: all pass, including every test from before this task.

- [ ] **Step 9: Sabotage.** Do each, see the named test fail, then restore:
  - swap the `Forbidden` and `Hidden` arms in `history_json`. Expected: `history_access_maps_to_its_answers`;
  - move `parse_history_query` *after* `s.backend.history(…)` using a default query. Expected: `bad_history_queries_are_400…` or `the_backend_gets_exactly_the_query_sent`;
  - render the link unconditionally. Expected: `the_queue_page_links_history_for_managers_only`.

- [ ] **Step 10: Run clippy, then commit**

```bash
SQLX_OFFLINE=true cargo clippy -q --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-web/src/backend.rs crack-web/src/lib.rs crack-web/src/routes.rs crack-web/src/page.rs crack-web/src/test_support.rs crack-web/assets/history.js
git commit -m "crack-web: history page and JSON routes, Manage Server gate, live backend

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 6: `history.js` and its styles

**Files:**
- Modify: `crack-web/assets/history.js`, which Task 5 created empty
- Modify: `crack-web/assets/app.css`

**Interfaces:**
- Consumes: the page markup from Task 5 (ids `history`, `rows`, `older`, `badge`, `note`, `gp-note`, `f-action`, `f-source`, `f-since`, `f-user`, `f-user-name`, `f-user-clear`, `initial`) and the JSON shape from Task 4.

- [ ] **Step 1: Write `history.js`.** Rows are built with `createElement` and `textContent` only. There's no `innerHTML`; the CSP's Trusted Types would throw on it.

```js
// The history page: draws the inlined first page, then polls for new rows
// every 10 s while visible, loads older rows on demand, and refetches when a
// filter changes. Every string from the server goes in with textContent.
(() => {
  "use strict";

  const root = document.getElementById("history");
  if (!root) return;
  const guild = root.dataset.guild;
  const PAGE_SIZE = 50;
  const POLL_MS = 10000;

  const rowsEl = document.getElementById("rows");
  const olderBtn = document.getElementById("older");
  const badge = document.getElementById("badge");
  const note = document.getElementById("note");
  const gpNote = document.getElementById("gp-note");
  const fAction = document.getElementById("f-action");
  const fSource = document.getElementById("f-source");
  const fSince = document.getElementById("f-since");
  const chip = document.getElementById("f-user");
  const chipName = document.getElementById("f-user-name");
  const chipClear = document.getElementById("f-user-clear");

  const first = JSON.parse(document.getElementById("initial").textContent);
  let rows = first.rows;
  let older = first.older;
  let gameHidden = first.game_hidden;
  let user = null; // { id, label }
  let stopped = false;
  // Bumped on every refetch, so a poll or "load older" that began before a
  // filter changed can never mix its rows into the new list.
  let seq = 0;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }

  function say(text) {
    note.textContent = text || "";
    note.hidden = !text;
  }

  function ago(iso) {
    const secs = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 1000));
    if (secs < 60) return "just now";
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins} min ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours} h ago`;
    return `${Math.floor(hours / 24)} d ago`;
  }

  function rowEl(r) {
    const li = el("li", "row");
    const time = el("span", "when", ago(r.at));
    time.title = new Date(r.at).toLocaleString();
    li.appendChild(time);
    if (r.who.kind === "bot") {
      li.appendChild(el("span", "who bot", "bot"));
    } else {
      const label = r.who.name || r.who.id;
      const who = el("button", "who", label);
      who.type = "button";
      who.title = "Show only this member's changes";
      who.addEventListener("click", () => setUser({ id: r.who.id, label }));
      li.appendChild(who);
    }
    li.appendChild(el("span", "how", r.how));
    li.appendChild(el("span", "what", r.what));
    return li;
  }

  function render() {
    if (rows.length === 0) {
      rowsEl.replaceChildren(el("li", "empty", "Nothing matches."));
    } else {
      rowsEl.replaceChildren(...rows.map(rowEl));
    }
    olderBtn.hidden = !older;
    gpNote.hidden = !gameHidden;
    chip.hidden = !user;
    chipName.textContent = user ? user.label : "";
  }

  function query(extra) {
    const p = new URLSearchParams();
    if (fAction.value) p.set("action", fAction.value);
    if (fSource.value) p.set("source", fSource.value);
    if (fSince.value) p.set("since", fSince.value);
    if (user) p.set("user", user.id);
    for (const [k, v] of Object.entries(extra)) p.set(k, String(v));
    return p.toString();
  }

  // Resolves to the page, or null when the answer means "stop".
  async function fetchPage(extra) {
    const res = await fetch(`/g/${guild}/history.json?${query(extra)}`, {
      credentials: "same-origin",
      headers: { Accept: "application/json" },
    });
    if (res.status === 401) {
      window.location.assign(`/auth/login?return_to=${encodeURIComponent(window.location.pathname)}`);
      stopped = true;
      return null;
    }
    if (res.status === 403 || res.status === 404) {
      stopped = true;
      const body = await res.json().catch(() => null);
      say((body && body.error) || "This history is no longer available to you.");
      return null;
    }
    if (!res.ok) throw new Error(String(res.status));
    return res.json();
  }

  async function reload() {
    const mine = ++seq;
    try {
      const data = await fetchPage({});
      if (!data || mine !== seq) return;
      rows = data.rows;
      older = data.older;
      gameHidden = data.game_hidden;
      badge.hidden = true;
      render();
    } catch (_) {
      if (mine === seq) badge.hidden = false;
    }
  }

  async function poll() {
    if (stopped || document.visibilityState !== "visible") return;
    if (rows.length === 0) return reload();
    const mine = seq;
    try {
      const data = await fetchPage({ after: rows[0].id });
      if (!data || mine !== seq) return;
      badge.hidden = true;
      if (gameHidden && !data.game_hidden) {
        // The game ended: its rows can be shown now.
        return reload();
      }
      gameHidden = data.game_hidden;
      if (data.rows.length > 0) rows = data.rows.concat(rows);
      render();
      if (data.rows.length >= PAGE_SIZE) poll();
    } catch (_) {
      if (mine === seq) badge.hidden = false;
    }
  }

  async function loadOlder() {
    if (rows.length === 0) return;
    const mine = seq;
    olderBtn.disabled = true;
    try {
      const data = await fetchPage({ before: rows[rows.length - 1].id });
      if (!data || mine !== seq) return;
      rows = rows.concat(data.rows);
      older = data.older;
      render();
    } catch (_) {
      say("Could not load older entries. Try again in a moment.");
    } finally {
      olderBtn.disabled = false;
    }
  }

  function setUser(u) {
    user = u;
    reload();
  }

  for (const f of [fAction, fSource, fSince]) f.addEventListener("change", reload);
  chipClear.addEventListener("click", () => setUser(null));
  olderBtn.addEventListener("click", loadOlder);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") poll();
  });
  setInterval(poll, POLL_MS);
  render();
})();
```

- [ ] **Step 2: Add the styles.** Append to `crack-web/assets/app.css`, reusing the existing CSS variables:

```css
.links { margin: 0 0 1rem; }
.links a { color: var(--accent); }
#filters { display: flex; flex-wrap: wrap; gap: 0.5rem 1rem; align-items: center; margin: 0 0 1rem; }
#filters select { margin-left: 0.25rem; }
.chip { display: inline-flex; gap: 0.25rem; align-items: center; padding: 0.1rem 0.5rem;
        border: 1px solid var(--line); border-radius: 999px; background: var(--card); }
.chip button { border: 0; background: none; color: var(--muted); cursor: pointer; font: inherit; }
#rows { list-style: none; padding: 0; margin: 0; }
#rows .row { display: flex; flex-wrap: wrap; gap: 0.25rem 0.6rem; padding: 0.5rem 0;
             border-bottom: 1px solid var(--line); }
#rows .when { color: var(--muted); min-width: 6.5rem; }
#rows .who { border: 0; background: none; padding: 0; color: var(--accent); cursor: pointer; font: inherit; }
#rows .who.bot { color: var(--muted); cursor: default; }
#rows .how { color: var(--muted); }
#rows .what { flex-basis: 100%; overflow-wrap: anywhere; }
#rows .empty { color: var(--muted); padding: 1rem 0; }
#older { margin: 1rem 0; }
```

- [ ] **Step 3: Check syntax and run the tests**

```bash
node --check crack-web/assets/history.js
SQLX_OFFLINE=true cargo test -q -p crack-web
```
Expected: no syntax errors, and all tests pass (`history_js_is_served` now serves real content).

- [ ] **Step 4: Run the page in a browser if you can.** Otherwise say you didn't. There's no browser harness in CI. The owner checks the page on TuneTitan before production: as a manager (rows, filters, the name chip, Load older, a new row within 10 s) and as a plain member (403).

- [ ] **Step 5: Commit**

```bash
git add crack-web/assets/history.js crack-web/assets/app.css
git commit -m "crack-web: history.js draws, filters, polls and pages the history

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 7: Docs, version and changelog

**Files:**
- Modify: `docs/web-dashboard.md`, `CHANGELOG.md`, `Cargo.toml` (workspace version), `Cargo.lock`

- [ ] **Step 1: Update `docs/web-dashboard.md`.**
  - Add this section after "Turning it on":

```markdown
## Queue history

`/g/<id>/history` shows the server's queue history (the `queue_audit` table):
who changed the queue, how, when and what. It is for members with **Manage
Server** (owner, Administrator, or the permission on a role or `@everyone`);
others get 403, and the queue page shows them no link. It needs the database.

The page polls `/g/<id>/history.json` every 10 s for new rows and pages older
ones with `before=<id>`. Rows are ordered by id, which is insertion order.
While a `/gp` game runs, its rows are hidden: their titles are the answers.
Design: `docs/superpowers/specs/2026-10-02-dashboard-history-design.md`.
```

  - In "What it holds", add these bullets under "In the bot's memory":
    - each viewer's role ids, read from Discord when the member isn't cached, kept for 5 minutes;
    - the display names of members shown in a history page, kept for 1 hour.

  - Change "Nothing is written to the database." to: "Nothing is written to the database; the history page reads `queue_audit`."

- [ ] **Step 2: Bump the version.** In the root `Cargo.toml`, change the workspace `version = "0.17.3"` to `"0.18.0"`, then run `cargo check -q --workspace` so `Cargo.lock` follows.

- [ ] **Step 3: Update `CHANGELOG.md`.** Under `## Unreleased` → `### Added`, which is the first `### Added` before `## TODO:`, add at the top:

```markdown
- **The dashboard has a queue history page.** Members with Manage Server see a
  "History" link on a server's queue page. It lists who changed the queue, how
  and when, filters by member, action, source and time, loads older entries, and
  shows new ones within 10 seconds. A running `/gp` game's entries stay hidden
  until it ends.
```

- [ ] **Step 4: Run the full verification**

```bash
cargo fmt --all
SQLX_OFFLINE=true cargo clippy -q --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo test -q --workspace
```
Expected: clean, with all tests passing. Report the total count.

- [ ] **Step 5: Commit**

```bash
git add docs/web-dashboard.md CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "v0.18.0: the dashboard's queue history page

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

(If `cargo fmt` changed files from earlier tasks, add them by name to this commit.)
