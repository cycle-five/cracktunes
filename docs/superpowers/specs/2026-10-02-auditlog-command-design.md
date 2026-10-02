# `/auditlog` — design

**Date:** 2026-10-02
**Status:** approved in conversation; this document is the written record.
**Sub-project 2 of 4** of the queue audit log. Sub-project 1 is the record itself:
`docs/superpowers/specs/2026-09-30-queue-audit-log-design.md` (v0.16.0).

## Goal

Members trusted with **Manage Server** can read their server's queue history in
Discord: who changed the queue, how, when, and what changed. They can filter it by
member, action, source and time.

## Decisions (owner, 2026-10-02)

| Question | Answer |
|---|---|
| Who may run it | **Manage Server by default, adjustable per server.** This uses Discord's `default_member_permissions`, so servers can re-assign it under Server Settings → Integrations. |
| Options | `user`, `action`, `source`, `since` (all four, all optional) |
| Reply visibility | **Only the caller** (ephemeral) |
| Shape | A paged embed of formatted lines, rejected alternatives being a CSV attachment and deferring to the dashboard panel (sub-project 3) |

## Design

### 1. Command and access

- `/auditlog [user] [action] [source] [since]`: `slash_command` only, `guild_only`,
  `default_member_permissions = "MANAGE_GUILD"`, registered in `music_commands()`
  next to `playlog`.
- **No prefix form.** Prefix commands work only by @mention, so a prefix gate would
  have to hard-code the permission with `required_permissions`, which defeats the
  per-server delegation.
- The reply is **ephemeral**. The command does not `defer()` publicly. If it defers
  at all, it defers ephemerally.
- **No database** (`database_pool` is `None`): an ephemeral reply saying the audit log
  needs a database. This is not an error.

### 2. Options

- `user`: a Discord member (`serenity::User`).
- `action`: a choice list with exactly the names `Action::name()` returns: add,
  remove, move, skip, clear, shuffle, stop, pause, resume, leave. Built from a
  poise `ChoiceParameter` enum whose variants map one-to-one onto those names. A
  test pins every variant to `Action::name()` for a sample action of that kind,
  so the two cannot drift.
- `source`: a choice list of slash, prefix, web and bot. It is pinned to
  `Source::as_str` the same way.
- `since`: a duration string, a positive integer followed by `m`, `h`, `d` or `w`
  (`90m`, `6h`, `2d`, `1w`), at most 52 weeks. Anything else gets an ephemeral reply
  naming the accepted form. It is never silently ignored.

### 3. Query (`db/queue_audit.rs`)

```sql
SELECT at, actor_user_id, voice_channel_id, source, command, action,
       detail AS "detail!: Json<Action>"
FROM queue_audit
WHERE guild_id = $1
  AND ($2::bigint      IS NULL OR actor_user_id = $2)
  AND ($3::text        IS NULL OR action = $3)
  AND ($4::text        IS NULL OR source = $4)
  AND ($5::timestamptz IS NULL OR at >= $5)
ORDER BY at DESC
LIMIT $6
```

- `$6` is `AUDITLOG_LIMIT + 1`, with `AUDITLOG_LIMIT = 200`. Fetching one extra
  row says whether more matched without a `COUNT`.
- The query uses the existing `queue_audit_guild_at` index.
- It returns typed rows (`AuditRow`) and `detail` as the typed `Action`. There is
  no JSON handling by hand.

### 4. `/gp` games stay secret

While the guild has a game in `gp_games`, rows with `command` starting `gp` and
`at >= that game's started_at` are dropped before formatting, because titles give
the answers away. Rows from earlier, finished games are shown. When any row is
dropped, the footer says entries from the running game are hidden. The rule is a
pure function `hide_running_game(rows, running_since: Option<DateTime<Utc>>)`.

### 5. One line per entry

`<t:UNIX:R> WHO · HOW — WHAT`

- **WHO:** `<@id>` for a member (an ephemeral embed's mention never pings), or
  `bot` when `actor_user_id` is NULL.
- **HOW:** `/command` for slash, `@command` for prefix, `dashboard` for web, and
  the bot reason (e.g. `idle timeout`, `autoplay`, `gp`) for bot.
- **WHAT:** per action.

| Action | WHAT |
|---|---|
| add | `added N tracks: T1, T2, T3 (+K)`, or `added T1` for one track |
| remove | `removed T from #I` |
| move | `moved T F → T2` |
| skip | `skipped T`, or `skipped` if there is no track |
| clear | `cleared N tracks` |
| shuffle | `shuffled N tracks` |
| stop | `stopped, N tracks dropped` |
| pause / resume | `paused` / `resumed` |
| leave | `left voice, N tracks discarded` |

- Titles are truncated to 40 characters with `…`. A missing title is `(untitled)`.
- Markdown in titles is escaped.
- Formatting is a pure function `audit_line(&AuditRow) -> String`.
- User-facing fixed strings (the title, footers, the no-database and bad-`since`
  replies) are constants in `messaging/messages.rs`, per the localization
  convention.

### 6. Paging

- Lines go into the existing paged embed (`utils::create_paged_embed`), extended
  with an `ephemeral: bool` parameter, `false` at the existing `/playlog` call
  site, so behaviour there is unchanged.
- The footer adds "showing the newest 200 — narrow with `since` or `user`" when
  the extra row came back.
- If nothing matches, it says so in one ephemeral line.

## Testing

- **`audit_line`:** one unit test per action row in the table above, plus truncation,
  missing title and markdown escaping.
- **`since` parser:** the accepted forms, zero, an unknown unit, an over-the-cap
  value and garbage.
- **`hide_running_game`:** no game, a running game (its `gp*` rows hidden, other rows
  and an older game's rows kept), and the footer flag.
- **Choice enums:** pinned to `Action::name()` and `Source::as_str()`.
- **Query:** one `DATABASE_URL`-gated test inserts rows across two guilds and checks
  each filter, the guild isolation, the ordering and the +1 limit.
- **Every test is sabotaged** against the code it guards first, as usual.
- **Not automatable:** the permission gate and the ephemeral reply, which Discord
  enforces. Both get checked on TuneTitan before production.

## Out of scope

- The dashboard history panel (sub-project 3).
- Premium gating (sub-project 4).
- Decay retention.
- Exporting the log.
- Showing the voice channel per line. The data is there, but it isn't worth the
  width yet.
