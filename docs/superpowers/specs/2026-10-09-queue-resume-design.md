# Resume the music queue after a restart (ct#595)

**Date:** 2026-10-09 · **Release:** v0.26.0 · **Issue:** #595
**Owner rulings (2026-10-09):** snapshot **at shutdown only**; resume within **5 minutes**;
the current track resumes at its **saved position** (a failed seek falls back to the top);
an **empty voice channel** means don't rejoin, drop it; restore **paused, repeat, autoplay**;
the old status message **loses its dead buttons**; **free** (reliability, not a perk).
"Work straight through": no review stops.

## Why

A restart (a deploy, a clean host reboot) drops every guild's voice connection
and queue. `/queue` then says "I'm not connected", `/summon` finds an empty
queue, and the pre-restart now-playing message keeps a stale "ends in…" and four
live-looking buttons. `/gp` games already survive this (`gp_persist`); ordinary
playback should too.

## Shape

One table, written at shutdown, **claimed** on the way back up.

```sql
CREATE TABLE IF NOT EXISTS queue_snapshot (
    guild_id          BIGINT PRIMARY KEY,
    saved_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    voice_channel_id  BIGINT NOT NULL,
    text_channel_id   BIGINT,          -- where the bot talks: music channel, else last command channel
    status_channel_id BIGINT,          -- the now-playing status message on screen, if any
    status_message_id BIGINT,
    position_ms       BIGINT NOT NULL, -- of the current (first) track
    paused            BOOLEAN NOT NULL,
    looping           BOOLEAN NOT NULL,
    autoplay          BOOLEAN NOT NULL,
    tracks            JSONB NOT NULL   -- typed: Vec<SnapshotTrack>, current first
);
```

`SnapshotTrack { url: String, title: Option<String>, artist: Option<String>,
duration_secs: Option<i64>, requester: Option<i64> }`, `serde`-derived and read
and written through `sqlx::types::Json<Vec<SnapshotTrack>>` — never
`serde_json::Value`. It converts to and from `crack_types::SavedTrack` (the
`/gp` track-as-data) plus the requester.

**Why no tombstones (unlike `/gp`):** rows are written only at shutdown, and only
for guilds actually playing then. A `/stop`, `/leave`, idle timeout or kick
before the shutdown leaves an empty queue, so no row.

**Why at most once:** the resume claims its row with
`DELETE FROM queue_snapshot WHERE guild_id = $1 RETURNING …, EXTRACT(EPOCH FROM now() - saved_at)`.
`on_guild_create` runs on every gateway reconnect; the second run finds nothing.
This is #469's class of bug removed by construction.

## Shutdown

In `config.rs`'s shutdown handler, right after `gp_shutdown`, before the pool
closes: `queue_shutdown(data, budget = 3 s)`.

For every call in `data.songbird.iter()` that is connected, whose queue is not
empty, and whose guild has no `/gp` game: build a `QueueSnapshot` from
- the queue, in order: each track's `TrackData` metadata (`source_url`,
  `title`, `artist`, `duration`) and requester (`utils::get_track_handle_metadata`,
  `utils::get_requesting_user`); a track with no source URL is skipped;
- the current track's `get_info` (bounded by `TRACK_INFO_TIMEOUT`): position,
  paused (`PlayMode::Pause`), looping (`LoopState::Infinite`); an unanswered
  `get_info` reads as position 0, not paused, not looping;
- the session's autoplay (`get_autoplay`);
- the status slot: the tracked message (channel, id) if any, and
  `last_command_channel`; `text_channel_id` = the guild's music channel, else
  `last_command_channel`, else the status message's channel;
- the call's current voice channel.

Snapshots are built concurrently, then written in one transaction as upserts.
The whole step is bounded by the budget; on timeout it logs and writes nothing
rather than half. `/gp`'s 5 s and this 3 s fit Docker's 10 s stop grace (both are
almost always instant).

## Resume

`queue_resume_guild(data, ctx, guild)` runs from `on_guild_create` **after**
`gp_resume_guild`:

1. **Claim** the guild's row. None: return.
2. **Retire** the old status message: `Transport::clear_components` on
   (status_channel_id, status_message_id), whatever happens next. Best effort.
3. **Decide** (a pure function over age, listeners, and whether a `/gp` game is
   active): `Resume`, `TooLate` (age > `QUEUE_RESUME_WINDOW_SECS` = 300),
   `Empty` (no non-bot member in the voice channel), `GameRunning`. Anything but
   `Resume` logs why and posts nothing.
4. **Rejoin** exactly as `/gp` does: `perms::ensure_can_join` →
   `music_utils::join_permitted` → `set_global_handlers_with`. A refusal or a
   failed join logs and gives up.
5. **Restore**, in a spawned task so `on_guild_create` is not held, under a
   `QueueGuard` from `lock_queue(guild, PlaybackOwner::Free, Actor::bot(BotReason::Resume))`
   held for the whole restore:
   1. enqueue the **current track only** (`ResolvedTrack::from_saved` +
      `with_user_id(requester)`, through `enqueue_resolved_tracks_back`);
   2. if its saved position is ≥ `QUEUE_RESUME_MIN_SEEK` (5 s), seek it to
      position − `QUEUE_RESUME_REWIND` (3 s) and wait for the confirmation,
      bounded by `SEEK_TIMEOUT`.
      - 🔑 **Seek failed:** songbird documents a failed seek as fatal and
        *removes the track*. The queue is then empty, so the track is enqueued
        again, fresh, and plays from the top.
      - Timed out: leave it; it plays from wherever the driver lands.
   3. enqueue the **rest** of the tracks, in order, with their requesters;
   4. restore repeat (`repeat_on(.., Some(true))`) and paused (`pause_on`) as saved;
   5. release the guard, then restore autoplay (`set_autoplay`), last, so an
      autoplay refill cannot race the rebuild.
6. **Announce:** note the text channel as the guild's command channel, post
   "♻️ Back after a restart — picking up where we left off." there, then
   `show_now_playing`, which posts a fresh status (with buttons if the server
   has them on).

The seek is a seam (`impl FnOnce(&TrackHandle, Duration) -> Future<Output = SeekResult>`)
so the failure path is testable without a voice driver.

## Out of scope

Crash-safe snapshots (on every change); resuming `/play`'s in-flight playlist
progress line; a queue left more than 5 minutes; dashboard notice of a resume;
the stale "ends in…" embed text on the old message (its buttons go; the text
stays as it was).

## Testing

- **db** (db-tests): save then claim round-trips every field and the tracks in
  order; a second claim returns nothing; a second save for a guild replaces the
  first; claim of an absent guild is `None`; the age comes back.
- **snapshot builder** (test calls via `music::ops::test_support`): tracks in
  order with requesters and metadata; an empty queue gives no snapshot; a guild
  with a `/gp` game gives none; the status slot's message and command channel
  land in the snapshot.
- **decision** (pure): each outcome, and the 300 s boundary.
- **restore** (test call + injected seek): order and requesters after restore;
  a failed seek leaves the current track re-queued first; a timed-out seek
  leaves it; no seek below 5 s; repeat and paused applied; autoplay set after.
- **Discord side** (`FakeTransport`): the old status message gets exactly one
  `ClearComponents`; the announce line goes to the text channel.
- Every test sabotaged; mutation table in the PR. Glue that needs a live gateway
  (`queue_resume_guild`'s claim → join path, the shutdown hook) is named
  untested and covered by a TuneTitan checklist.

## Rollback

One additive migration (a new table). An older bot image ignores it. Keep the
migrate image at v0.26.0 or later.
