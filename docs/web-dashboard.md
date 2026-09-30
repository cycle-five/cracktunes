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

## What it holds

A user id and username in a signed, HttpOnly cookie (24 h). Nothing is written
to the database. Restarting the bot invalidates nothing: sessions are JWTs
signed with `WEB_JWT_SECRET`; rotate that to sign everyone out.
