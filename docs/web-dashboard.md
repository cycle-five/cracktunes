# Web dashboard

View a guild's queue live; reorder it from the bot's voice channel; on premium
servers, pause, skip, shuffle, repeat and remove from there too.
Design: `docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md`.

## Turning it on

Built in by default (crack-cli feature `web`); dormant until all of these are set:

| Variable | Value |
|---|---|
| `DISCORD_CLIENT_ID` | the application id (falls back to `DISCORD_APP_ID`) |
| `DISCORD_CLIENT_SECRET` | Developer Portal → OAuth2 → Client Secret |
| `WEB_PUBLIC_ORIGIN` | `https://dash.cracktun.es` (or `http://localhost:8090` for a tunnel) |
| `WEB_JWT_SECRET` | 32+ random characters: `openssl rand -hex 32` |
| `WEB_BIND` | optional, default `0.0.0.0:8090` |

Register `<WEB_PUBLIC_ORIGIN>/auth/callback` under the application's OAuth2
redirects. A missing variable logs `web dashboard off; missing or unusable: …`
and the bot runs on.

`GET /health` answers `cracktunes dashboard ok <version>` with no sign-in. It
reads nothing, so it proves the web server is answering, not that the bot is
on the gateway. The bot's log reports that.

## Controls (premium)

Above the queue, a member in the bot's voice channel gets Pause/Resume, Skip,
Shuffle and Repeat, and a ✕ on each upcoming track to remove it. They post to
`POST /g/<id>/control`. Premium is read on every request, so a `/premium grant`
or `revoke` applies to the next click; an open page picks the new plan up
within 5 s.

Free servers see the controls disabled, with "Dashboard controls are a premium
feature" and the Patreon link under them. Reordering stays free.

The same rule as reordering decides who may use them: members in the bot's voice
channel. Anyone else sees them disabled and gets 403 (`not_allowed`). Each
member may send 5 controls per 10 s, across every server; the sixth gets 429
(`too_many`). Moves are not limited.

Each control posts one line in Discord, where the now-playing message goes,
such as "⏸ Paused from the dashboard — @member", and the now-playing message
lands below it. It is an embed, so the mention never pings. A refused control posts
nothing, and a move stays silent, as it always has.

Each one is recorded in `queue_audit` with source `web` and command
`dashboard skip`, `dashboard pause`, `dashboard resume`, `dashboard repeat`,
`dashboard remove` or `dashboard shuffle`; a reorder is `dashboard move`.

Design: `docs/superpowers/specs/2026-10-04-ops-layer-and-dashboard-controls-design.md`.

## Queue history

`/g/<id>/history` shows the server's queue history (the `queue_audit` table):
who changed the queue, how, when and what. It is for members with **Manage
Server** (owner, Administrator, or the permission on a role or `@everyone`);
others get 403, and the queue page shows them no link. Bot owners (the same set
`owners_only` commands use: the configured owners, or the default, plus the
application's owner and team) see every server's history, member or not, for
debugging and support; in a server they're not in, they open
`/g/<id>/history` directly. It needs the database.

The page polls `/g/<id>/history.json` every 10 s for new rows and pages older
ones with `before=<id>`. Rows are ordered by id, which is insertion order.
While a `/gp` game runs, its rows are hidden: their titles are the answers.

Free servers see the last 24 hours of history, and premium servers
(`guild_settings.premium`) see all of it. When older rows exist, the page says older
history is a premium feature, with the Patreon link, in place of "Load older".
`/auditlog` applies the same window.

A bot owner grants premium with `/premium grant <server_id>` and removes it with
`/premium revoke <server_id>`, run from any server the bot shares with them; only bot
owners can run those two. Members with Manage Server can run `/premium status` in their
server to see its plan. It takes effect at once. Don't edit `guild_settings.premium` in the database while the bot runs: the bot
reads premium from its in-memory settings, so the edit is ignored, and the bot writes
its settings back on shutdown, which replaces the edit. (`/set premium` is not
registered: the whole `/set` command is switched off.)

Design: `docs/superpowers/specs/2026-10-02-dashboard-history-design.md` and `docs/superpowers/specs/2026-10-03-premium-history-window-design.md`.

## What it holds

**In the browser:** a user id and username in a signed, HttpOnly cookie
(24 h).

**In the bot's memory:** for each user who has signed in since the bot
started, what Discord returned at sign-in (scope `identify` only): the user
id, username, global (display) name, avatar URL, and the Discord OAuth
refresh token with its expiry. catacombs' `MemoryStorage` keeps these in
plaintext: it ignores the encryption key it is handed. Logging out clears the
refresh token; the profile stays. All of it goes when the process restarts.
Additionally:
- each viewer's role ids, read from Discord when the member isn't cached, kept for 5 minutes;
- the display names of members shown in a history page, kept for 1 hour.

**In the log:** each move and control is logged with the member's user id; catacombs logs
the username and user id at sign-in and log-out.

The dashboard keeps nothing of its own in the database: moves and controls are
recorded in `queue_audit` like any other queue change, and the history page reads it. Restarting the bot invalidates no session: sessions are JWTs signed with `WEB_JWT_SECRET`; rotate that to sign
everyone out.
