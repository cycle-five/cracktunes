# Premium History Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Free servers see the last 24 hours of queue history on the dashboard and in
`/auditlog`, and premium servers see all of it. Where the limit hides something, the
reader is told and pointed to Patreon. The Patreon plugs are reworded to match.

**Architecture:**
- **crack-core:**
  - A new `guild::plan::Plan { Free, Premium }` turns `guild_settings.premium` into a
    history floor and a plug message.
  - `music::audit_view` gains `older_than_floor` and `trim_to_floor`. The trim runs in
    Rust after the query, so "nothing older" and "older is premium" can be told apart
    without another query.
  - `compose_auditlog` takes the floor.
- **crack-web:** `compose_page` takes the floor and sets a new `HistoryPage::capped`.
  The page shows a premium note while `capped` is true.

**Tech Stack:** Rust: poise/serenity `next`, axum 0.8, chrono, typed serde, sqlx 0.9
offline. Plain browser JS under CSP Trusted Types.

**Spec:** `docs/superpowers/specs/2026-10-03-premium-history-window-design.md`

## Global Constraints

- **Serde:** typed only, never `serde_json::json!` or `serde_json::Value` for data we own.
- **No source-scan tests.** Enforce with types and clippy, not by grepping code.
- **Sabotage every new test:** break the code it guards, watch the test fail, then
  restore the code. Report each sabotage you did.
- **Commits:**
  - Every commit ends with exactly this trailer and no other `Co-Authored-By`:
    `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
  - Never `git add -A` or `git add .`. Stage named files.
- **CI's lint must pass:**
  `cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return`.
- **Tests:** run with `SQLX_OFFLINE=true cargo test -q -p <crate>`. No SQL changes in
  this plan, so `.sqlx` must not change.
- **The free window is exactly 24 hours.** A row whose `at` is exactly the floor is kept.
- **A server whose premium setting is unknown (`None`) is Free.**
- **The Patreon link is `https://patreon.com/CrackTunes`.**
- **User-facing strings are constants in `crack-core/src/messaging/messages.rs`.**
  Logs stay inline.
- **The browser renders server text with `textContent` only.** The CSP forbids HTML sinks.
- **Local docker:** use `docker --context default`. Bare `docker` targets a remote host.

## Review Focus

1. **The 51st row (the one fetched to learn `older`) is older than 24 hours.** The
   page must say `capped` now, not show "Load older" for a click that returns nothing.
   Task 3's `the_extra_row_past_the_floor_caps_the_page` test pins it.
2. **A free server whose only changes are older than 24 hours.** `/auditlog` must not
   say "nothing matches". Task 2's `only_older_rows_are_only_older` test pins it.
3. **A `before` page entirely past the floor.** It returns no rows, `older` false and
   `capped` true, so the page hides the button and shows the note. Task 3's
   `a_before_page_past_the_floor_is_empty_and_capped` test pins it.
4. **An `after` poll never reports `capped`.** Otherwise a poll could flip the note.
   Task 3's `after_polls_never_report_capped` test pins it.
5. **Trimming before the 200-row cut.** A truncation notice must count only rows the
   server may see. Task 2's `the_trim_runs_before_the_200_row_cut` test pins it.

---

### Task 1: `Plan`, the Patreon messages, and the idle decision

**Files:**
- Create: `crack-core/src/guild/plan.rs`
- Modify: `crack-core/src/guild/mod.rs` (add `pub mod plan;`)
- Modify: `crack-core/src/messaging/messages.rs` (`IDLE_ALERT` near line 98,
  `PREMIUM_PLUG` near line 144; new constants beside the `AUDITLOG_*` ones near line 432)
- Modify: `crack-core/src/messaging/message.rs` (the `PremiumPlug` variant near
  line 84 and its `Display` arm near line 383)
- Modify: `crack-core/src/handlers/idle.rs`

**Interfaces:**
- Produces (in `crack_core::guild::plan`):
  - `pub const FREE_HISTORY_WINDOW: chrono::Duration` (24 hours)
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Plan { Free, Premium }`
  - `Plan::of(premium: Option<bool>) -> Plan`
  - `Plan::history_floor(self, now: DateTime<Utc>) -> Option<DateTime<Utc>>`
  - `Plan::plug(self) -> CrackedMessage`
- Produces (in `crack_core::messaging::messages`):
  - `pub const PATREON_URL: &str = "https://patreon.com/CrackTunes";`
  - `pub const PREMIUM_HISTORY: &str = "Older history is a premium feature.";`
  - `pub const AUDITLOG_ONLY_OLDER: &str = "📜 Nothing in the queue history matches that in the last 24 hours.";`
  - `pub const PREMIUM_THANKS: &str = "👑 Thanks for supporting CrackTunes! Your server has premium.";`
- Produces: `CrackedMessage::PremiumThanks`, and `pub fn times_out(no_timeout: bool, limit: usize, count: usize) -> bool`
  in `crack_core::handlers::idle`.

- [ ] **Step 1: Write the failing tests for `Plan`**

Create `crack-core/src/guild/plan.rs` with this content:

```rust
//! What a server's plan allows. Premium is `guild_settings.premium`.
//! Spec: docs/superpowers/specs/2026-10-03-premium-history-window-design.md

use crate::messaging::message::CrackedMessage;
use chrono::{DateTime, Duration, Utc};

/// How far back a free server's queue history goes.
pub const FREE_HISTORY_WINDOW: Duration = Duration::hours(24);

/// A server's plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Free,
    Premium,
}

impl Plan {
    /// The plan for a server's premium setting. Unknown settings (`None`, such
    /// as before they have loaded) count as Free: the limit fails closed.
    #[must_use]
    pub fn of(premium: Option<bool>) -> Plan {
        todo!()
    }

    /// The oldest moment of queue history this plan shows, or `None` for all of it.
    #[must_use]
    pub fn history_floor(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        todo!()
    }

    /// The Patreon message for this plan: the plug for Free, thanks for Premium.
    #[must_use]
    pub fn plug(self) -> CrackedMessage {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn only_a_known_true_setting_is_premium() {
        assert_eq!(Plan::of(Some(true)), Plan::Premium);
        assert_eq!(Plan::of(Some(false)), Plan::Free);
        assert_eq!(Plan::of(None), Plan::Free);
    }

    #[test]
    fn free_sees_24_hours_and_premium_sees_everything() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        assert_eq!(
            Plan::Free.history_floor(now),
            Some(now - Duration::hours(24))
        );
        assert_eq!(Plan::Premium.history_floor(now), None);
    }

    #[test]
    fn free_gets_the_plug_and_premium_gets_thanks() {
        assert!(matches!(Plan::Free.plug(), CrackedMessage::PremiumPlug));
        assert!(matches!(Plan::Premium.plug(), CrackedMessage::PremiumThanks));
    }
}
```

Add `pub mod plan;` to `crack-core/src/guild/mod.rs`, keeping alphabetical order:

```rust
pub mod cache;
pub mod operations;
pub mod permissions;
pub mod plan;
pub mod settings;
```

Add the variant `PremiumThanks,` directly after `PremiumPlug,` in the `CrackedMessage`
enum in `crack-core/src/messaging/message.rs`. Add its `Display` arm after the
`PremiumPlug` arm:

```rust
            Self::PremiumPlug => f.write_str(PREMIUM_PLUG),
            Self::PremiumThanks => f.write_str(PREMIUM_THANKS),
```

If `message.rs` imports message constants by name rather than by glob, add
`PREMIUM_THANKS` to that import. If the compiler reports other exhaustive `match`es on
`CrackedMessage`, add a `PremiumThanks` arm beside each `PremiumPlug` arm, doing the same thing.

Add to `crack-core/src/messaging/messages.rs`, after `AUDITLOG_GP_HIDDEN`:

```rust
pub const AUDITLOG_ONLY_OLDER: &str =
    "📜 Nothing in the queue history matches that in the last 24 hours.";
/// Shown where the free 24-hour window hid older history: in `/auditlog`
/// and on the dashboard's history page.
pub const PREMIUM_HISTORY: &str = "Older history is a premium feature.";
pub const PATREON_URL: &str = "https://patreon.com/CrackTunes";
```

Replace `PREMIUM_PLUG` and add `PREMIUM_THANKS` after it:

```rust
pub const PREMIUM_PLUG: &str = "👑 Like the bot? Support my development and unlock premium features by subscribing to my Patreon!\n[CrackTunes Patreon](https://patreon.com/CrackTunes)";
pub const PREMIUM_THANKS: &str = "👑 Thanks for supporting CrackTunes! Your server has premium.";
```

Replace `IDLE_ALERT`:

```rust
pub const IDLE_ALERT: &str = "⚠️ I've been idle for a while so I'm going to hop off, set the idle timeout to change this! Also support my development and keep the bot idle in vc as long as you like!\n[CrackTunes Patreon](https://patreon.com/CrackTunes)";
```

Add a `Display` test beside the existing `CrackedMessage` display tests in
`message.rs` (find them with `grep -n "fn test_\|#\[test\]" crack-core/src/messaging/message.rs`):

```rust
    #[test]
    fn premium_thanks_displays_the_thanks() {
        assert_eq!(CrackedMessage::PremiumThanks.to_string(), PREMIUM_THANKS);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-core plan::`
Expected: the three `plan` tests panic at `not yet implemented`.

- [ ] **Step 3: Implement `Plan`**

```rust
    pub fn of(premium: Option<bool>) -> Plan {
        if premium == Some(true) {
            Plan::Premium
        } else {
            Plan::Free
        }
    }

    pub fn history_floor(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Plan::Free => Some(now - FREE_HISTORY_WINDOW),
            Plan::Premium => None,
        }
    }

    pub fn plug(self) -> CrackedMessage {
        match self {
            Plan::Free => CrackedMessage::PremiumPlug,
            Plan::Premium => CrackedMessage::PremiumThanks,
        }
    }
```

If `Duration::hours` is not `const` in this chrono version, the compiler will say so. In
that case use `Duration::seconds(24 * 60 * 60)` if that is `const`, or else make
`FREE_HISTORY_WINDOW` a `fn free_history_window() -> Duration` and update the test.

- [ ] **Step 4: Write the failing test for the idle decision**

In `crack-core/src/handlers/idle.rs`, add this function after the `IdleHandler` struct:

```rust
/// Whether an idle bot leaves now. `count` is the idle seconds counted before
/// this tick. A premium server (`no_timeout`) never times out, so it never sees
/// `IDLE_ALERT`; a `limit` of 0 means the timeout is off.
#[must_use]
pub fn times_out(no_timeout: bool, limit: usize, count: usize) -> bool {
    todo!()
}
```

Add at the end of the file:

```rust
#[cfg(test)]
mod test {
    use super::times_out;

    #[test]
    fn premium_never_times_out() {
        assert!(!times_out(true, 600, 0));
        assert!(!times_out(true, 600, 600));
        assert!(!times_out(true, 600, usize::MAX));
    }

    #[test]
    fn free_times_out_once_idle_reaches_the_limit() {
        assert!(!times_out(false, 600, 540));
        assert!(times_out(false, 600, 600));
        assert!(times_out(false, 600, 660));
    }

    #[test]
    fn a_zero_limit_never_times_out() {
        assert!(!times_out(false, 0, 10_000));
    }
}
```

Run: `SQLX_OFFLINE=true cargo test -q -p crack-core handlers::idle`
Expected: the three tests panic at `not yet implemented`.

- [ ] **Step 5: Implement `times_out` and use it**

```rust
pub fn times_out(no_timeout: bool, limit: usize, count: usize) -> bool {
    !no_timeout && limit > 0 && count >= limit
}
```

In `act`, replace this block:

```rust
        if !self.no_timeout.load(Ordering::Relaxed)
            && self.limit > 0
            && self.count.fetch_add(60, Ordering::Relaxed) >= self.limit
        {
```

with:

```rust
        let no_timeout = self.no_timeout.load(Ordering::Relaxed);
        // Count only while a timeout can happen, as before: premium and a zero
        // limit leave the counter alone.
        let count = if no_timeout || self.limit == 0 {
            0
        } else {
            self.count.fetch_add(60, Ordering::Relaxed)
        };
        if times_out(no_timeout, self.limit, count) {
```

The body of the `if` is unchanged.

- [ ] **Step 6: Run the tests and lint**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-core` — all pass.
Run the CI clippy command from Global Constraints — clean.

- [ ] **Step 7: Sabotage each new test**
  - In `Plan::of`, return `Plan::Premium` for `None`: `only_a_known_true_setting_is_premium` fails.
  - Change `FREE_HISTORY_WINDOW` to 25 hours: `free_sees_24_hours_and_premium_sees_everything` fails.
  - Swap the two `plug` arms: `free_gets_the_plug_and_premium_gets_thanks` fails.
  - Make the `PremiumThanks` display arm write `PREMIUM_PLUG`: `premium_thanks_displays_the_thanks` fails.
  - Drop `!no_timeout &&` from `times_out`: `premium_never_times_out` fails.
  - Change `>=` to `>` in `times_out`: `free_times_out_once_idle_reaches_the_limit` fails.
  - Drop `limit > 0 &&`: `a_zero_limit_never_times_out` fails.

  Restore the code after each one.

- [ ] **Step 8: Commit**

```bash
git add crack-core/src/guild/plan.rs crack-core/src/guild/mod.rs \
  crack-core/src/messaging/messages.rs crack-core/src/messaging/message.rs \
  crack-core/src/handlers/idle.rs
git commit -m "feat: Plan, the reworded Patreon plugs, and a thank-you for premium

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: `/auditlog` gets the 24-hour window

**Files:**
- Modify: `crack-core/src/music/audit_view.rs` (beside `hidden_by_game` and
  `hide_running_game` near line 155, `AuditlogReply` and `compose_auditlog` near
  line 189, and the test module)
- Modify: `crack-core/src/commands/music/auditlog.rs`

**Interfaces:**
- Consumes (Task 1): `crack_core::guild::plan::Plan`, and
  `messages::{PREMIUM_HISTORY, PATREON_URL, AUDITLOG_ONLY_OLDER}`.
- Produces (in `crack_core::music::audit_view`, all `pub`):
  - `fn older_than_floor(row: &AuditRow, floor: Option<DateTime<Utc>>) -> bool`
  - `fn trim_to_floor(rows: Vec<AuditRow>, floor: Option<DateTime<Utc>>) -> (Vec<AuditRow>, bool)`
  - `fn premium_history_line() -> String`: `_Older history is a premium feature._ [CrackTunes Patreon](https://patreon.com/CrackTunes)`
  - `AuditlogReply::OnlyOlder`, a new variant
  - `compose_auditlog(rows: Vec<AuditRow>, running_since: Option<DateTime<Utc>>, floor: Option<DateTime<Utc>>) -> AuditlogReply`,
    with a new third parameter

- [ ] **Step 1: Write the failing tests**

Add these functions after `hide_running_game`, with `todo!()` bodies for now:

```rust
/// Whether the plan's history floor hides this row: it is older than `floor`.
/// A row exactly at the floor is shown. No floor hides nothing.
#[must_use]
pub fn older_than_floor(row: &AuditRow, floor: Option<DateTime<Utc>>) -> bool {
    todo!()
}

/// Drop the rows older than the plan's history floor. Returns whether anything
/// was dropped: older history exists, and the reply says it's premium.
///
/// This runs on what the query returned, not in SQL, so "nothing older" and
/// "older is premium" can be told apart without another query. Rows come
/// newest first, so this drops a tail.
#[must_use]
pub fn trim_to_floor(
    rows: Vec<AuditRow>,
    floor: Option<DateTime<Utc>>,
) -> (Vec<AuditRow>, bool) {
    todo!()
}

/// The notice for history the free window hid, with the Patreon link.
#[must_use]
pub fn premium_history_line() -> String {
    todo!()
}
```

Extend the `use crate::messaging::messages::{...}` line at the top of the file with
`PATREON_URL, PREMIUM_HISTORY`.

Add the variant to `AuditlogReply`, after `AllHidden`:

```rust
    /// Rows matched, but all are older than the plan's history floor.
    OnlyOlder,
```

Change `compose_auditlog`'s signature to add `floor: Option<DateTime<Utc>>` as the
third parameter, leaving its body as it is for now. Update its doc comment to say:
"cut to the plan's history floor, then to the limit, hide a running game's rows, and
prepend the notices." Add `None` as the third argument to every existing
`compose_auditlog(...)` call in the test module. Those tests must otherwise stay unchanged.

Add these tests to the test module. They build rows with the module's existing
`row(source, command, user, action)` builder (near line 322), setting `at`:

```rust
    /// A plain `pause` by a member, recorded at `at`.
    fn row_at(at: DateTime<Utc>) -> AuditRow {
        AuditRow {
            at,
            ..row("slash", "pause", Some(7), Action::Pause)
        }
    }
```

```rust
    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn a_row_exactly_at_the_floor_is_kept() {
        let floor = Some(t(0));
        assert!(!older_than_floor(&row_at(t(0)), floor));
        assert!(older_than_floor(&row_at(t(-1)), floor));
        assert!(!older_than_floor(&row_at(t(-1_000_000)), None));
    }

    #[test]
    fn the_trim_drops_older_rows_and_says_so() {
        let rows = vec![row_at(t(10)), row_at(t(0)), row_at(t(-1))];
        let (kept, capped) = trim_to_floor(rows.clone(), Some(t(0)));
        assert_eq!(kept.len(), 2);
        assert!(capped);
        let (kept, capped) = trim_to_floor(rows[..2].to_vec(), Some(t(0)));
        assert_eq!(kept.len(), 2);
        assert!(!capped);
        let (kept, capped) = trim_to_floor(rows, None);
        assert_eq!(kept.len(), 3);
        assert!(!capped);
    }

    #[test]
    fn the_premium_notice_follows_the_others_only_when_rows_were_dropped() {
        let AuditlogReply::Lines(lines) =
            compose_auditlog(vec![row_at(t(10)), row_at(t(-10))], None, Some(t(0)))
        else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], premium_history_line());
        assert!(premium_history_line().contains(PREMIUM_HISTORY));
        assert!(premium_history_line().contains(PATREON_URL));

        let AuditlogReply::Lines(lines) =
            compose_auditlog(vec![row_at(t(10))], None, Some(t(0)))
        else {
            panic!("expected lines");
        };
        assert_eq!(lines.len(), 1);
        assert_ne!(lines[0], premium_history_line());
    }

    #[test]
    fn only_older_rows_are_only_older() {
        assert_eq!(
            compose_auditlog(vec![row_at(t(-10))], None, Some(t(0))),
            AuditlogReply::OnlyOlder
        );
        assert_eq!(compose_auditlog(vec![], None, Some(t(0))), AuditlogReply::Empty);
    }

    #[test]
    fn the_trim_runs_before_the_200_row_cut() {
        // 150 visible rows and 100 too old: 250 fetched, but only 150 may be
        // seen, so nothing was cut at 200.
        let mut rows: Vec<AuditRow> = (0..150).map(|i| row_at(t(1000 - i))).collect();
        rows.extend((0..100).map(|i| row_at(t(-1 - i))));
        let AuditlogReply::Lines(lines) = compose_auditlog(rows, None, Some(t(0))) else {
            panic!("expected lines");
        };
        assert!(!lines.contains(&AUDITLOG_TRUNCATED.to_owned()));
        assert_eq!(lines.len(), 150 + 1);
        assert_eq!(lines[0], premium_history_line());
    }
```

Also add a test that pins the notice order when a game hides rows as well:

```rust
    #[test]
    fn the_premium_notice_comes_after_the_game_notice() {
        let mut gp = row_at(t(20));
        gp.command = "gp".to_owned();
        let rows = vec![gp, row_at(t(10)), row_at(t(-10))];
        let AuditlogReply::Lines(lines) = compose_auditlog(rows, Some(t(5)), Some(t(0))) else {
            panic!("expected lines");
        };
        assert_eq!(lines[0], AUDITLOG_GP_HIDDEN);
        assert_eq!(lines[1], premium_history_line());
        assert_eq!(lines.len(), 3);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-core audit_view`
Expected: the new tests fail (`not yet implemented`, or the assertions on the
unchanged body). The existing tests still pass.

- [ ] **Step 3: Implement**

```rust
pub fn older_than_floor(row: &AuditRow, floor: Option<DateTime<Utc>>) -> bool {
    floor.is_some_and(|f| row.at < f)
}

pub fn trim_to_floor(
    rows: Vec<AuditRow>,
    floor: Option<DateTime<Utc>>,
) -> (Vec<AuditRow>, bool) {
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

pub fn premium_history_line() -> String {
    format!("_{PREMIUM_HISTORY}_ [CrackTunes Patreon]({PATREON_URL})")
}
```

`compose_auditlog`'s body becomes:

```rust
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
```

- [ ] **Step 4: Wire `/auditlog`**

In `crack-core/src/commands/music/auditlog.rs`:
- Import `crate::guild::operations::GuildSettingsOperations` (the trait that gives
  `Data::get_premium`), `crate::guild::plan::Plan`, `AUDITLOG_ONLY_OLDER`, and
  `premium_history_line`.
- Before `compose_auditlog`, read the floor:

```rust
    let floor = Plan::of(ctx.data().get_premium(guild_id).await)
        .history_floor(chrono::Utc::now());
```

- Pass it: `compose_auditlog(rows, running_since, floor)`.
- Add the new arm. `say` takes `&'static str`, so send this one directly:

```rust
        AuditlogReply::OnlyOlder => {
            let text = format!("{AUDITLOG_ONLY_OLDER}\n{}", premium_history_line());
            ctx.send(CreateReply::default().content(text).ephemeral(ephemeral))
                .await?;
            return Ok(());
        },
```

- [ ] **Step 5: Run the tests and lint**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-core` — all pass.
Run the CI clippy command — clean.

- [ ] **Step 6: Sabotage each new test**
  - Change `<` to `<=` in `older_than_floor`: `a_row_exactly_at_the_floor_is_kept` fails.
  - Make `trim_to_floor` return `false` for `capped` always:
    `the_trim_drops_older_rows_and_says_so` and the notice test fail.
  - Remove the `OnlyOlder` early return: `only_older_rows_are_only_older` fails.
  - Move the trim after `rows.truncate(AUDITLOG_LIMIT)`, computing `truncated` first:
    `the_trim_runs_before_the_200_row_cut` fails.
  - Push the premium line before the game notice:
    `the_premium_notice_comes_after_the_game_notice` fails.

  Restore the code after each one.

- [ ] **Step 7: Commit**

```bash
git add crack-core/src/music/audit_view.rs crack-core/src/commands/music/auditlog.rs
git commit -m "feat: /auditlog shows free servers 24 hours of history

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: The dashboard gets the 24-hour window

**Files:**
- Modify: `crack-web/src/history.rs` (`HistoryPage` near line 59, `shown_members`
  near line 164, `compose_page` near line 185, tests)
- Modify: `crack-web/src/lib.rs` (`LiveBackend::history` near line 220)
- Modify: `crack-web/src/page.rs` (`history_page` near line 113, tests)
- Modify: `crack-web/assets/history.js`
- Modify: `crack-web/src/routes.rs` (tests only, near line 1270)

**Interfaces:**
- Consumes (Tasks 1–2): `Plan`, `older_than_floor`, `PREMIUM_HISTORY`, `PATREON_URL`.
- Produces:
  - `HistoryPage { rows, older, game_hidden, capped: bool }`. The wire field is `capped`.
  - `shown_members(rows: &[AuditPageRow], running_since: Option<DateTime<Utc>>, floor: Option<DateTime<Utc>>) -> Vec<UserId>`
  - `compose_page(rows: Vec<AuditPageRow>, cursor: AuditCursor, running_since: Option<DateTime<Utc>>, floor: Option<DateTime<Utc>>, name_of: impl Fn(UserId) -> Option<String>) -> HistoryPage`

- [ ] **Step 1: Write the failing tests for `compose_page`**

Add to `HistoryPage`:

```rust
    /// The plan's history floor hid older rows: the page says older history is
    /// premium, instead of offering "Load older". Always false for an `after` poll.
    pub capped: bool,
```

Give `compose_page` and `shown_members` the new `floor` parameter (fourth for
`compose_page`, third for `shown_members`), leaving their bodies unchanged, and set
`capped: false` in `compose_page`'s result for now. Update every existing call (tests,
and `lib.rs`, where `None` is passed for now) and every `HistoryPage { .. }` literal
(add `capped: false`). Find them with `grep -rn "compose_page\|shown_members\|HistoryPage {" crack-web/src`.

Add these tests to `history.rs`'s test module, using its existing
`page_row(id, command, user, at_secs)` builder:

```rust
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
        assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![3, 2]);
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
        let rows = vec![page_row(2, "skip", Some(7), 500), page_row(1, "skip", Some(7), 400)];
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
        let rows = vec![page_row(2, "skip", Some(7), 5), page_row(1, "skip", Some(7), 4)];
        let page = compose_page(rows, AuditCursor::Newest, None, None, |_| None);
        assert_eq!(page.rows.len(), 2);
        assert!(!page.capped);
    }

    #[test]
    fn names_are_not_looked_up_for_rows_past_the_floor() {
        let rows = vec![page_row(2, "skip", Some(7), 1001), page_row(1, "skip", Some(8), 999)];
        assert_eq!(shown_members(&rows, None, floor()), vec![UserId::new(7)]);
    }
```

If `page_row`'s `at_secs` is not seconds since the epoch, adjust `floor()` to match,
keeping each test's meaning.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web history::`
Expected: the new tests fail on their assertions. The existing tests pass.

- [ ] **Step 3: Implement the trim in `compose_page` and `shown_members`**

Import `older_than_floor` from `crack_core::music::audit_view`. In `shown_members`, add
`.filter(|r| !older_than_floor(&r.row, floor))` after the `hidden_by_game` filter.
`compose_page` becomes:

```rust
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
            // unchanged
        })
        .collect();
    HistoryPage {
        rows,
        older,
        game_hidden: running_since.is_some(),
        capped,
    }
}
```

Keep the existing `HistoryRow` mapping exactly as it is. Update the doc comment to
mention the floor.

- [ ] **Step 4: Wire `LiveBackend::history`**

In `crack-web/src/lib.rs`, import `crack_core::guild::{operations::GuildSettingsOperations, plan::Plan}`.
After `running_since` is computed in `history`:

```rust
        // Read on every request: granting or removing premium applies at once.
        let floor = Plan::of(self.deps.data.get_premium(g).await)
            .history_floor(chrono::Utc::now());
```

Pass `floor` to `history::shown_members(&rows, running_since, floor)` and to
`history::compose_page(rows, q.cursor, running_since, floor, |u| ...)`.

- [ ] **Step 5: Write the failing page test**

In `page.rs`'s test module, add (build the `HistoryPage` the way the module's existing
history-page test does):

```rust
    #[test]
    fn the_premium_note_is_shown_only_when_capped() {
        let note = format!(
            "<p id=\"premium-note\">{PREMIUM_HISTORY} <a href=\"{PATREON_URL}\">CrackTunes Patreon</a></p>"
        );
        let capped = HistoryPage { capped: true, ..HistoryPage::default() };
        let html = history_page("S", GuildId::new(5), &capped);
        assert!(html.contains(&note), "{html}");

        let open = HistoryPage::default();
        let html = history_page("S", GuildId::new(5), &open);
        assert!(html.contains("<p id=\"premium-note\" hidden>"), "{html}");
    }
```

Import `crack_core::messaging::messages::{PATREON_URL, PREMIUM_HISTORY}` in `page.rs`.
Run: `SQLX_OFFLINE=true cargo test -q -p crack-web page::` — the new test fails.

- [ ] **Step 6: Render the note**

In `history_page`, right after the `Load older` button and before `</section>`, add the
note. Its `hidden` attribute is set when the first page isn't capped:

```rust
<button type=\"button\" id=\"older\" hidden>Load older</button>\
<p id=\"premium-note\"{premium_hidden}>{premium} <a href=\"{patreon}\">CrackTunes Patreon</a></p></section>\
```

with these format arguments:

```rust
            premium_hidden = if page.capped { "" } else { " hidden" },
            premium = esc(PREMIUM_HISTORY),
            patreon = esc(PATREON_URL),
```

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web` — all pass.

- [ ] **Step 7: The page script**

In `crack-web/assets/history.js`:
- After `const gpNote = ...`: `const premiumNote = document.getElementById("premium-note");`
- After `let gameHidden = first.game_hidden;`: `let capped = first.capped;`
- In `render()`:
  - Replace the empty-state line with
    `rowsEl.replaceChildren(el("li", "empty", capped ? "Nothing matches in the last 24 hours." : "Nothing matches."));`
  - After `gpNote.hidden = !gameHidden;`, add `premiumNote.hidden = !capped;`.
- In `reload()`, after `gameHidden = data.game_hidden;`: `capped = data.capped;`
- In `loadOlder()`, after `older = data.older;`: `capped = data.capped;`
- `pollOnce()` does **not** touch `capped`. Add this comment beside its
  `gameHidden = data.game_hidden;` line:
  `// \`capped\` is left alone: a poll only brings new rows, which the floor never hides.`

- [ ] **Step 8: Route test for the JSON**

In `routes.rs`, extend `history_json_carries_the_backends_page`, or add a sibling, so the
canned page has `capped: true` and the JSON body contains `"capped":true`. Do this by
deserializing the body into `HistoryPage` and comparing it with the canned page, as the
existing test does.

- [ ] **Step 9: Run everything and lint**

Run: `SQLX_OFFLINE=true cargo test -q -p crack-web` and `SQLX_OFFLINE=true cargo test -q -p crack-core` — all pass.
Run the CI clippy command — clean.

- [ ] **Step 10: Sabotage each new test**
  - Compute `capped` over the rows after `truncate`:
    `the_extra_row_past_the_floor_caps_the_page` fails.
  - Drop `!poll &&` from `capped`: `after_polls_never_report_capped` fails.
  - Drop the floor filter from the row mapping:
    `rows_past_the_floor_are_dropped_and_the_page_is_capped` and
    `a_before_page_past_the_floor_is_empty_and_capped` fail.
  - Drop `!capped &&` from `older`: `rows_past_the_floor_...` fails on `!page.older`.
  - Drop the floor filter from `shown_members`:
    `names_are_not_looked_up_for_rows_past_the_floor` fails.
  - Invert `premium_hidden`: `the_premium_note_is_shown_only_when_capped` fails.

  Restore the code after each one.

- [ ] **Step 11: Commit**

```bash
git add crack-web/src/history.rs crack-web/src/lib.rs crack-web/src/page.rs \
  crack-web/assets/history.js crack-web/src/routes.rs
git commit -m "feat: the dashboard's history shows free servers 24 hours, and says older is premium

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: Docs, changelog, version

**Files:**
- Modify: `docs/web-dashboard.md` (the "Queue history" section near line 26)
- Modify: `CHANGELOG.md` (under `## Unreleased`: `### Added` at line 43 and
  `### Changed` at line 107)
- Modify: `Cargo.toml` (`version = "0.18.0"` at line 24 → `"0.19.0"`) and `Cargo.lock`

- [ ] **Step 1: Docs**

In `docs/web-dashboard.md`'s "Queue history" section, add one paragraph:

```markdown
Free servers see the last 24 hours of history, and premium servers
(`guild_settings.premium`) see all of it. When older rows exist, the page says older
history is a premium feature, with the Patreon link, in place of "Load older".
`/auditlog` applies the same window. Design:
`docs/superpowers/specs/2026-10-03-premium-history-window-design.md`.
```

- [ ] **Step 2: Changelog**

At the top of `### Changed` under `## Unreleased`:

```markdown
- **Free servers now see 24 hours of queue history.** The dashboard's history page and
  `/auditlog` show free servers the last 24 hours, and premium servers everything. When
  older entries exist, both say older history is a premium feature and link the
  Patreon. A server whose settings haven't loaded counts as free.
- **The Patreon plugs are reworded.** They no longer say premium gates nothing. The idle
  alert, which premium servers never see, now says premium keeps the bot in the voice
  channel as long as you like.
```

At the top of `### Added` under `## Unreleased`:

```markdown
- **A thank-you message for premium servers** (`CrackedMessage::PremiumThanks`), which
  `Plan::plug` picks in place of the Patreon plug. Nothing sends either yet.
```

- [ ] **Step 3: Version**

Set the workspace `version` in `Cargo.toml` to `0.19.0`, then run
`SQLX_OFFLINE=true cargo build -q` so `Cargo.lock` updates. `git diff Cargo.lock` must
show only the workspace crates' version lines.

- [ ] **Step 4: Full check**

Run: `SQLX_OFFLINE=true cargo test -q --workspace`. Every test passes; report the
passed, failed and ignored counts. Run the CI clippy command, which must be clean. Run
`cargo fmt --all -- --check`, which must be clean.

- [ ] **Step 5: Commit**

```bash
git add docs/web-dashboard.md CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "v0.19.0: premium history window

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
