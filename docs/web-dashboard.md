# Web dashboard

Arc 1: view a guild's queue live; reorder it from the bot's voice channel.
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

## What it holds

**In the browser:** a user id and username in a signed, HttpOnly cookie
(24 h).

**In the bot's memory:** for each user who has signed in since the bot
started, what Discord returned at sign-in (scope `identify` only): the user
id, username, global (display) name, avatar URL, and the Discord OAuth
refresh token with its expiry. catacombs' `MemoryStorage` keeps these in
plaintext: it ignores the encryption key it is handed. Logging out clears the
refresh token; the profile stays. All of it goes when the process restarts.

**In the log:** each move is logged with the mover's user id; catacombs logs
the username and user id at sign-in and log-out.

Nothing is written to the database. Restarting the bot invalidates no session: sessions are JWTs signed with `WEB_JWT_SECRET`; rotate that to sign
everyone out.
