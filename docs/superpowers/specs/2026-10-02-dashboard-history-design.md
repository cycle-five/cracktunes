# Dashboard history page — design

**Date:** 2026-10-02
**Status:** approved in conversation, section by section; this document is the written record.
**Sub-project 3 of 4** of the queue audit log. Sub-project 1 is the record
(`2026-09-30-queue-audit-log-design.md`, v0.16.0). Sub-project 2 is `/auditlog`
(`2026-10-02-auditlog-command-design.md`, v0.17.0). The dashboard itself is
`2026-09-30-web-dashboard-queue-design.md`.

## Goal

A server's managers can read its queue history at `dash.cracktun.es`: who changed the queue,
how, when, and what changed. They can filter it, load older entries, and see new
entries appear while the page is open. It's a convenience feature, so it's kept lean.

## Decisions (owner, 2026-10-02)

| Question | Answer |
|---|---|
| Who may see it | **Manage Server**, as `/auditlog`'s default. `/auditlog`'s per-server Integrations overrides are not mirrored. |
| What it is for | **Both:** a history page with filters and paging, which also updates live while it's open. |
| How it stays live | **The page polls a JSON endpoint** every 10 s. There's no stream and no push from the audit writer. |

## 1. Access

- **The rule:** history needs membership and Manage Server. A member has Manage Server
  when they own the server, or when the permissions of `@everyone` plus their roles
  include **Administrator** or **Manage Server**. Channel overrides don't apply,
  because Manage Server is a server-wide permission.
- **The decision is a pure function** of the owner id, `@everyone`'s permissions,
  each role's permissions, and the member's role ids. It lives in `crack-web/src/access.rs`
  beside `decide`.
- **Inputs:**
  - The server's roles and owner come from the cache.
  - The member's roles come from the cached member, or else from the `get_member` call that membership already makes.
  - `MemberMemo` (5 min) keeps the member's role ids with the membership answer, so polling costs at
    most one Discord call per user per server per 5 minutes.
- **Answers:**

| Viewer | `/g/<id>/history` | `/g/<id>/history.json` |
|---|---|---|
| Not a member | 404, like the queue page | 404 |
| Member without Manage Server | 403, "Needs Manage Server" | 403 |
| Discord did not answer | 503 | 503 |
| Manager | the page | rows |

- **Revocation** takes up to the memo's 5 minutes. A session doesn't extend it.
- **The queue page** shows a "History" link only to managers.

## 2. Endpoint and query

`GET /g/<id>/history.json`. Every parameter is optional:

| Parameter | Accepts |
|---|---|
| `user` | a user id |
| `action` | one of `Action::name()`'s ten names (`ActionChoice`) |
| `source` | `slash`, `prefix`, `web`, `bot` |
| `since` | `parse_since`'s form: `90m`, `6h`, `2d`, `1w` |
| `before` | a row id: rows older than it |
| `after` | a row id: rows newer than it |

- **A bad value gets a 400** naming the accepted form. `before` and `after` together get a 400.
- **The response is typed serde:**
  - `rows`: up to 50, newest first. Each row has:
    - `id`
    - `at`: RFC 3339
    - `who`: `{ "kind": "member", "id", "name" }` or `{ "kind": "bot" }`
    - `how`: e.g. `/play`, `@skip`, `dashboard`, `idle timeout`
    - `what`: e.g. `moved Song A 5 → 2`
  - `older`: bool
  - `game_hidden`: bool
- **Ordering is by `id` (insertion order), not `at`.** The audit writer is one FIFO task,
  so `id > X` misses nothing when polling. A cursor on `at` could miss a row stamped a
  few milliseconds before the previous newest. A migration adds
  `queue_audit (guild_id, id DESC)`.
- **The query** is a new `db::queue_audit::audit_page`. It takes the four filters plus the
  cursor, and fetches 51 rows to set `older`. With `after`, it returns the 50 *oldest* rows
  above the cursor, sorted newest first, so a poll that finds more than 50 new rows never
  skips any: the client asks again. `recent_audit` is unchanged, so `/auditlog`
  doesn't change.
- **Shared wording:** `audit_view`'s "how" and "what" become plain-text functions.
  `/auditlog` escapes their output for Discord; the web sends it as is, and the
  browser inserts it as text. One source of words, so the two can't drift.
- **A running `/gp` game's rows are hidden** with `hide_running_game`, using the start
  time from `Data::gp_games`, as in `/auditlog`. `game_hidden` is true whenever a game is
  running, whether or not this page had rows to drop. The page shows the note while it
  is true, and reloads when it turns false, so the game's rows appear once it ends.
- **Names:**
  - A member's name comes from the cache, then from a 1-hour name memo, then from at most 10
    `get_user` calls per request.
  - A name still unknown is sent as `null`, and the page shows the id. A later poll fills it in.

## 3. The page

- **`/g/<id>/history`** is server-rendered like the queue page (`page.rs`), with the first page
  of rows inlined as JSON. A new `assets/history.js` renders and updates it using
  DOM text nodes only; the dashboard's CSP and Trusted Types forbid string-to-HTML
  sinks.
- **Top:** "History · <server>", a link back to the queue, and a note when `game_hidden`.
- **Filters:**
  - selects for action (all + 10), source (all, slash, prefix, dashboard, bot) and since
    (all, 1h, 6h, 1d, 1w);
  - clicking a person's name filters to that person, shown as a chip with ×;
  - any change refetches the first page.
- **Rows:** `3 min ago · Alice · /play — added Song A`, with the exact time as a tooltip.
  Relative times refresh on each poll.
- **Live:** every 10 s, while `document.visibilityState` is `visible`, it fetches
  `after=<newest id>` with the current filters and adds the rows on top.
- **Older:** a "Load older" button fetches `before=<oldest id>` until `older` is false.
- **Failures:**
  - A 403 or 404 stops polling and shows the message.
  - A network error or 5xx shows "reconnecting…" and retries on the next tick.

## Testing

- **The Manage Server function:**
  - the owner;
  - Administrator on a role;
  - Manage Server on a role;
  - Manage Server on `@everyone`;
  - no permission;
  - roles the guild doesn't have (ignored).
- **Routes, against the fake backend:**
  - 404, 403, 503 and 200 for the page and the JSON;
  - 400 for each bad parameter and for `before`+`after`;
  - the History link for managers only;
  - `game_hidden` set when a game is running.
- **`audit_page`** (a `db-tests` test needing `DATABASE_URL`):
  - `before` and `after`;
  - each filter;
  - guild isolation;
  - the 51-row `older` flag;
  - the oldest-first window for `after`.
- **Shared wording:** `/auditlog`'s existing line tests pass unchanged on the factored
  text, and one test pins the web text and the Discord line to the same words.
- **Every new test is seen to fail under a deliberate sabotage** of the code it guards.
- **On TuneTitan before production:** the owner opens the page as a manager and as a plain member.

## Out of scope

- Filters kept in the URL; search by title; export; the voice channel per row.
- Mirroring `/auditlog`'s per-server permission overrides.
- Premium gating (sub-project 4) and decay retention.
