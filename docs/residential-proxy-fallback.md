# Residential-proxy fallback for gated YouTube

Optional, off by default. Lets the bot play age- or login-gated YouTube videos
that a datacenter-hosted bot cannot otherwise resolve.

## Why

YouTube applies its login/age gate based on IP reputation at resolve time, and a
datacenter IP fails it. Egress from a residential IP clears it. Stream URLs are
bound to the IP that resolved them, so the resolve and the stream must leave
through the same exit IP. The fallback therefore uses one sticky proxy session
for both legs.

## What it does

- A normal track resolves directly. It never touches the proxy.
- If a direct resolve fails with a recognised gate error, the track is retried
  through the residential proxy (resolve and stream via one sticky exit IP).
- Only gated failures trigger the retry; other failures are not retried.

## Activation

Set all of these in the bot's environment (see `.env.example`):

| Variable | Meaning |
| --- | --- |
| `PROXY_FALLBACK_HOST` | Proxy gateway host (required) |
| `PROXY_FALLBACK_PORT` | Gateway port (default `823`, DataImpulse) |
| `PROXY_FALLBACK_USER` | Proxy account login (required) |
| `PROXY_FALLBACK_PASSWORD` | Proxy account password (required) |

If HOST, USER or PASSWORD is unset or empty, the feature is fully inert and
behaviour is identical to before. Never commit real credentials.

## Cost

Spend is capped by the provider's prepaid balance. The bot does no metering of
its own; when the balance runs out, gated videos simply fail as they did before.

## Why in-process

The youtube-source plugin / Lavalink's own proxy configuration does not work
for this, which is why the fallback lives in the bot process.
