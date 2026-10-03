# Premium history window — design

**Date:** 2026-10-03
**Status:** approved in conversation; this document is the written record.
**Sub-project 4 of 4** of the queue audit log. Sub-project 1 is the record
(`2026-09-30-queue-audit-log-design.md`, v0.16.0). Sub-project 2 is `/auditlog`
(`2026-10-02-auditlog-command-design.md`, v0.17.0). Sub-project 3 is the dashboard's
history page (`2026-10-02-dashboard-history-design.md`, v0.18.0).

## Goal

Free servers see the last 24 hours of queue history, and premium servers see all of it.
The limit is the same on the dashboard and in `/auditlog`. Where it hides something, the
reader is told so and pointed to Patreon.

This also introduces the `Plan` type, which the dashboard's premium controls will use
next (add, skip, like, pause…). Free servers keep viewing and re-ordering the queue.

## Decisions (owner, 2026-10-03)

| Question | Answer |
|---|---|
| What premium gates here | **How far back history goes:** 24 hours for free, everything for premium. |
| Does `/auditlog` get the limit too | **Yes.** One rule everywhere. Otherwise `/auditlog since: 4w` would go around it. |
| Where the note points | **patreon.com/CrackTunes.** Premium is granted by hand until Patreon is linked. |
| The two Patreon plugs | **Reworded**, with a thank-you for premium servers in place of the plug. The idle alert never shows on premium. The plug stays unsent for now. |

## 1. The rule (crack-core)

- **The plan type:** `Plan { Free, Premium }`.
  - `Plan::of(premium: Option<bool>)` maps `Some(true)` to `Premium`, and anything else
    to `Free`. The input is what `data.get_premium(guild)` returns.
  - A server whose settings aren't loaded counts as Free. That fails closed.
  - The plan is read on every request, so granting or removing premium takes effect
    at once. Nothing caches it.
- **The window:**
  - `FREE_HISTORY_WINDOW` is 24 hours.
  - `Plan::history_floor(self, now) -> Option<DateTime<Utc>>` returns `now - 24h`
    for Free and `None` for Premium.
- **The trim:**
  - `trim_to_floor(rows, floor) -> (rows, capped: bool)` keeps the rows whose `at`
    is at or after the floor.
  - `capped` is true when it dropped at least one row.
  - With no floor it changes nothing, and `capped` is false.
- **Why the trim runs after the query, not in SQL:**
  - If SQL stopped at the floor, the reader couldn't tell "nothing older" from
    "older is premium".
  - Trimming what the query returned answers that with no extra query.
  - The queries already order by `id`, newest first, so the trim drops a tail.
    The page and `older` logic stay correct.
- **Where it lives:** beside `hide_running_game` in `crack-core/src/music/audit_view.rs`,
  with `Plan` in its own small module under `crack-core/src/guild/`.
- **For later:** the controls will add their own methods to `Plan`
  (`can_control()` or similar) when they are built, not now.

## 2. Dashboard (crack-web)

- **The backend:** `history()` reads the plan, and the trim runs after `audit_page` and
  before names are filled in.
  - On a first-page or `before` request where the trim dropped rows, `older` becomes
    false and `capped` becomes true.
  - On an `after` poll, `capped` is always false. New rows are inside the window by
    definition, and the client ignores the field on polls.
- **The wire:** `HistoryPage` gains `capped: bool`. It is typed serde, like the rest.
- **The page:**
  - `history.js` keeps `capped` from the last first-page or "Load older" response.
  - While `capped` is true, the "Load older" button stays hidden. In its place a line
    reads "Older history is a premium feature." with a link to
    `https://patreon.com/CrackTunes`.
  - The line and link are built with DOM text nodes, under the existing CSP and
    Trusted Types. The server-rendered first page sends the same flag inline.
  - If the trim leaves no rows, the empty state shows the premium line instead of
    "No history yet".
  - Rows that drift past 24 hours while the page is open stay on screen. They go on
    the next reload; this is a convenience feature.
- **The filters:**
  - `since` still works. On a free server, `since=1w` returns the same rows as
    no `since`.
  - The `user`, `action` and `source` filters are unaffected.

## 3. `/auditlog` (crack-core)

- **The trim:** `/auditlog` reads the plan, and `compose_auditlog` takes the floor.
  - The trim runs **first**, before the 200-row cut and before the running `/gp` game is
    hidden. That way `AUDITLOG_TRUNCATED` speaks only of rows the server may see.
  - When the trim dropped rows, the reply gets the premium notice, after the other
    notices.
- **If nothing is left:** when the trim leaves nothing, the reply is a new
  `AuditlogReply::OnlyOlder`. It says there were no changes in the last 24 hours, and
  that older history is a premium feature, with the link. "No changes" would be
  wrong, because there were changes; they're just older.
- **The wording:** the notice and its link are constants in
  `crack-core/src/messaging/messages.rs`. The
  dashboard reuses the sentence from there, so the two can't drift.

## 4. The Patreon plugs (crack-core)

Both say premium doesn't gate anything, which stops being true with this release.

- **`PREMIUM_PLUG`**, for free servers: "👑 Like the bot? Support my development and
  unlock premium features by subscribing to my Patreon!" plus the
  `[CrackTunes Patreon](https://patreon.com/CrackTunes)` link.
- **`PREMIUM_THANKS`**, new, for premium servers: "👑 Thanks for supporting
  CrackTunes! Your server has premium."
  - It's a new `CrackedMessage::PremiumThanks` beside `PremiumPlug`.
  - `Plan::plug(self) -> CrackedMessage` returns `PremiumPlug` for Free and
    `PremiumThanks` for Premium.
  - **Nothing sends either one yet.** `PremiumPlug` has never been sent; this
    release keeps that. The dashboard controls arc is where it's expected to appear.
- **`IDLE_ALERT`:** "⚠️ I've been idle for a while so I'm going to hop off, set the
  idle timeout to change this! Also support my development and keep the bot idle in
  vc as long as you like!" plus the same link.
  - It already never shows on a premium server. `IdleHandler` gets
    `no_timeout = premium` when the bot joins (`commands/music_utils.rs`), so a
    premium server is never timed out. This release keeps that.
  - The decision moves into a pure function,
    `IdleHandler`'s `times_out(no_timeout, limit, count) -> bool`, so a test can pin it.
  - Known and accepted: premium is read when the bot joins voice, so granting it
    mid-session takes effect at the next join.

## Testing

- **The rule, in pure functions:**
  - `Plan::of` for `Some(true)`, `Some(false)` and `None`.
  - The floor for each plan.
  - The trim:
    - a row exactly at the floor is kept;
    - a row one second older is dropped, and `capped` is set;
    - nothing to drop leaves `capped` false;
    - with no floor, nothing is touched.
- **`compose_auditlog`:**
  - the notice appears only when rows were dropped;
  - `OnlyOlder` when every row is older than the floor;
  - the trim runs before the 200-row cut. A test with 250 rows, 100 of them too old,
    gets no truncation notice.
- **Routes, against the fake backend** with premium on and off:
  - `capped` and the trimmed rows in the JSON;
  - the premium line and link in the first page, only when capped;
  - `after` polls never report `capped`.
- **The plugs:**
  - `Plan::plug` picks the plug for Free and the thanks for Premium;
  - `times_out` is never true with `no_timeout` set, whatever the count, and is
    true for a free server once the count reaches the limit.
- **Every new test is seen to fail under a deliberate sabotage** of the code it guards.
- **On TuneTitan before production:** the owner checks history on a free server and on a
  server set premium by hand. A free server needs rows older than 24 hours; TuneTitan has
  them from earlier testing.

## Release

- Minor version: **v0.19.0**. It changes what free servers see.
- The CHANGELOG says plainly that free servers now see 24 hours of history.

## Out of scope

- **Linking Patreon to premium.** The org's fork https://github.com/CycleFive/patreon-service
  ("A HTTP microservice to link Discord and Patreon with tier support", Rust) is the
  candidate. It's its own project.
- **The premium dashboard controls** (add, skip, like, pause…), the next arc.
- **Decay retention.** Premium's "everything" means everything the retention keeps.
- **Sending the plug anywhere.** That waits for the dashboard controls arc.
