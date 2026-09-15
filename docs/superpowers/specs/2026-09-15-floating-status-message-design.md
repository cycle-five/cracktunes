# Floating status message — design

**Date:** 2026-09-15
**Target version:** 0.13.0
**Status:** approved in conversation (sections 1–4 by the owner); ready for planning

## Problem

On production v0.12.1 the bot plays a playlist and says nothing when the queue
moves on. `TrackEndHandler::act` returns at `track_end.rs:166`
(`if !autoplay { return None; }`) before the only code that posts "Now
playing" for the next track. That early return dates to v0.3.8 (2024-07-17), so
"Now playing" updates have only ever appeared with autoplay on — and autoplay is
session-only, so every restart turns it off.

Posting a fresh "Now playing" per track would fix the silence and flood the
channel on a long playlist. The owner wants the opposite feel: **one status
message that follows the bottom of the chat while the bot plays**, which is also
where playback controls will eventually live.

A second, unrelated defect rides along: a playlist `/play` replies with the
literal text `PlaylistQueued`, because `doplay.rs:359` formats the message with
`{:?}` instead of `Display` (since 2024-12-09).

## Behaviour (owner decisions)

1. **Edit or move.** When the status must change: if no other message has been
   posted in its channel since the status message, edit it in place; otherwise
   delete it and send a new one at the bottom.
2. **Every now-playing moment goes through it:** a track change (autoplay or
   not), `/nowplaying`, a `/play` that starts a song, and a `/skip` that leaves a
   track playing.
3. **Channel:** the guild's music channel if one is set; otherwise the channel of
   the most recent music command. A now-playing moment in a *different* channel
   moves the status there (old one deleted) — one status message per guild.
4. **Finished state.** When playback ends — queue runs out, `/stop`, `/leave`,
   idle disconnect, kicked/disconnected — the status is edited (or moved) into a
   "Finished" state and **stays tracked**; a later now-playing moment continues
   with the same message under rule 1.
5. **Command replies follow a new guild setting**, `ephemeral_replies` (saved in
   the database, default off):
   - **on:** the command's reply is ephemeral (seen only by its author), is not a
     channel message, and so does not by itself force the status to move;
   - **off:** the command keeps a visible reply, and the status then moves below
     it.
   Prefix commands cannot be ephemeral and always behave as "off".
6. **`/nowplaying`'s reply is a one-line pointer** in both modes: the title plus a
   jump link to the status message.
7. **Guilty Pleasure guard.** While a `/gp` game owns playback, no status
   updates happen — the status would reveal the song being guessed.

## Detection: "has anything been posted since?"

Approach 1 of three considered (owner's choice): **serenity's cache.**

serenity updates `BaseGuildChannel::last_message_id` on every `MESSAGE_CREATE`
for guild channels (text channels, voice-channel chats) and threads
(`cache/event.rs:340-395`). Cracktunes runs serenity's default cache settings
(`max_messages = 0`), under which the update is unconditional: the channel's
`last_message_id` becomes the new message's id. Guild channels enter the cache on
each `GuildCreate`; production logs settings loads for every guild on boot, so
this is the path that populates them.

The status message is still at the bottom iff the channel's cached
`last_message_id` is **not newer than** the status message id. Snowflakes grow
with time, so this also handles the race where our own send has not yet echoed
back through the gateway (the cached id is older, so we still edit). A channel
missing from the cache yields "unknown", which is treated as "moved".

Rejected: an own per-channel tracker from `FullEvent::Message` (duplicates
serenity's bookkeeping across ~155 guilds) and a REST `GET messages?after=`
per update (an API call per track change, and needs Read Message History).

Known, accepted imprecision: if the only message after the status is deleted,
the status still moves. Harmless.

## Components

### `crack-core/src/messaging/status.rs` (new)

The only code that posts, edits or deletes the status message.

```rust
pub struct StatusSlot {
    pub message: Option<StatusMessage>,
    pub last_command_channel: Option<GenericChannelId>,
}
pub struct StatusMessage { pub channel: GenericChannelId, pub id: MessageId, pub phase: Phase }
pub enum Phase { Playing, Finished }

pub enum Placement { Edit, Replace, Send }

/// Pure. No tracked message → Send; tracked elsewhere → Replace; channel's last
/// message newer than ours, or unknown → Replace; otherwise Edit.
pub fn placement(current: Option<&StatusMessage>, target: GenericChannelId,
                 channel_last: Option<MessageId>) -> Placement;

/// Pure. Music channel, else last command channel, else the tracked message's
/// channel, else None (post nothing).
pub fn target_channel(music: Option<GenericChannelId>, last_command: Option<GenericChannelId>,
                      tracked: Option<GenericChannelId>) -> Option<GenericChannelId>;
```

**State:** `DataInner` gains
`status_slots: dashmap::DashMap<GuildId, Arc<tokio::sync::Mutex<StatusSlot>>>`,
the same shape as `queue_locks`. Every update for a guild holds its slot's lock,
so a `/skip` cannot race a track end.

**Transport seam, for tests.** The executor talks to Discord through a small
trait so every branch is testable without Discord:

```rust
#[async_trait]
pub trait StatusTransport: Send + Sync {
    async fn send(&self, channel: GenericChannelId, embed: CreateEmbed<'static>) -> Result<MessageId, TransportError>;
    async fn edit(&self, channel: GenericChannelId, id: MessageId, embed: CreateEmbed<'static>) -> Result<(), TransportError>;
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError>;
    fn last_message_id(&self, guild: GuildId, channel: GenericChannelId) -> Option<MessageId>;
}
pub enum TransportError { UnknownMessage, Other(String) }
```

The production implementation wraps `Arc<Http>` + `Arc<Cache>`; `Other` carries
the error's text for the log. `UnknownMessage` is Discord's 10008, recognised by
`is_unknown_message`, which moves from `commands/utility/clean.rs` to a shared
helper both use.

**Entry points:**

- `note_command_channel(data, guild, channel)` — records `last_command_channel`.
  Called from `cmd_check_music` when the check passes (50 music and playlist
  commands run it; `grab` and `spotify` do not, and so do not move the status).
- `show_now_playing(data, http, cache, guild, call) -> Option<StatusMessage>` —
  builds the now-playing embed with the existing `create_now_playing_embed`,
  **then** takes the slot lock, resolves the target channel, and applies
  `placement` with phase Playing. Returns what is on screen afterwards (None if
  nothing could be posted).
- `show_finished(data, http, cache, guild) -> Option<StatusMessage>` — same
  rule, a "Finished" embed, phase Finished; needs no voice call.
- `now_playing_pointer(title, link: Option<MessageLink>)` — the `/nowplaying`
  one-liner: "Now playing: *title* ↓" without a link, or with a jump link.

Both `show_*` calls are **idempotent**: repeating one with nothing posted in
between edits the same message again. `/stop` relies on this (its own call and
the track-end event its queue stop fires both land on Finished).

### Setting: `ephemeral_replies`

- `GuildSettings.ephemeral_replies: bool` (serde default false; part of the
  hand-written `PartialEq`), set in `GuildSettings::new`, carried by
  `From<GuildSettingsRead>`.
- `GuildSettingsRead.ephemeral_replies: bool`.
- Migration `migrations/20260915120000_ephemeral_replies.sql`:
  `ALTER TABLE guild_settings ADD COLUMN ephemeral_replies BOOLEAN NOT NULL DEFAULT FALSE;`
- The full-row upsert in `db/guild.rs::write_settings` writes the column. The
  `SELECT *` / `RETURNING *` queries pick it up through the struct.
- `.sqlx` regenerated with `cargo sqlx prepare --workspace` against a
  **throwaway** local Postgres (`docker run --rm` postgres:16-alpine on a free
  port, migrations applied, removed afterwards). Not `docker-compose-postgres.yml`
  (binds 127.0.0.1:5432, held by runecast-staging; `restart: always`; carries a
  Grafana agent with production settings). No other local database is touched.
- `GuildSettingsOperations::get_ephemeral_replies(guild)`.
- `/settings toggle ephemeral` — `settings/toggle/toggle_ephemeral.rs`,
  admin-only, the `toggle_autopause` shape: `ensure_settings_loaded`, flip,
  `save`. Registered in `toggle/mod.rs` (its `subcommands(...)` list and
  `commands()`), the only place toggles are registered.
- The migration goes into **both** `migrations/` and
  `crack-core/test_migrations/` (the latter mirrors the former, plus a test
  seed, and is what `#[sqlx::test(migrator = "MIGRATOR")]` applies).
- `fn reply_privately(setting: bool, is_prefix: bool) -> bool` — pure.

## Triggers

| moment | call |
|---|---|
| track end, next track exists (any autoplay state) | `show_now_playing` |
| track end, nothing next, autoplay off | `show_finished` |
| track end, autoplay queued a pick | `show_now_playing` (after the queue) |
| track end, autoplay found nothing / failed | existing "autoplay off" message, then `show_finished` |
| `/play` that starts a song (queue was empty) | `show_now_playing` after the reply |
| `/skip` leaving a track playing | `show_now_playing` after the reply |
| `/nowplaying` | see ordering below |
| `/stop` | `show_finished` (after the reply) |
| `/leave`, idle disconnect (`idle.rs`), kicked/disconnected (`serenity.rs:333`) | `show_finished` |

**`/nowplaying` ordering.** The pointer can only link to a message that exists.
If the reply is visible **and** the status's target channel is the command's
channel, the reply goes first as "Now playing: *title* ↓" (no link) and
`show_now_playing` follows, moving the status directly below it. In every other
case — ephemeral reply, or the status living in the music channel —
`show_now_playing` runs first and the reply carries a jump link to the message
it returned.

Any `gp_is_active(guild)` → no status call. Failed-join cleanup
(`music_utils.rs:220`) is not a playback end and calls nothing.

`TrackEndHandler::act` is restructured around a pure decision:

```rust
enum TrackEndStatus { NowPlaying, Finished, Autoplay, Nothing }
fn track_end_status(gp_active: bool, next_exists: bool, autoplay: bool) -> TrackEndStatus;
```

The existing autopause, errored-track, and recommendation code keeps its
behaviour; only the now-playing and finished posts move to the status module.
`send_now_playing` remains for `grab` only.

## Failure handling

- **Edit → UnknownMessage** (deleted by hand or `/clean`): send a new one.
- **Delete of the old one fails:** UnknownMessage is ignored; any other error is
  logged at `warn!` and the new one is still sent.
- **Edit fails with anything else** (e.g. missing permissions): `warn!` and
  clear the tracked message; the next update sends a fresh one.
- **Send fails** (e.g. no Send Messages / Embed Links): `warn!` and clear the
  tracked message (the phase lives on the message, so it goes with it). No
  retry loop; playback never depends on it.
- **Channel not cached:** treated as moved.

## Concurrency

- One lock per guild slot; all Discord calls for a status update happen under it.
- **Lock order:** the embed is built first (the Call lock is taken briefly and
  released), then the slot lock. The Call lock is never held while waiting for a
  slot lock.
- Track-end updates run inline on songbird's event task, as `send_now_playing`
  already does; no queue guard is taken.

## Edge cases

- **Restart:** slots start empty; the first update sends a new message. A
  pre-restart status message is left behind, as `/clean` already does for
  pre-restart messages.
- **Status deleted while playing:** handled by the edit's UnknownMessage path.
- **"Autoplay off", "Spotify took too long" and other messages** stay ordinary
  messages; they count as "posted since" and the next update moves the status.

## `PlaylistQueued`

`doplay.rs:359` uses `CrackedMessage::PlaylistQueued.to_string()` (i.e.
`PLAY_PLAYLIST`, "📃 Added playlist to queue!").

## Testing

Every test below is written red first and then sabotaged (break the code it
guards, watch it fail, restore by checksum).

- `placement` — all four branches plus the snowflake-race case (cached id older
  than ours → Edit).
- `target_channel` — precedence music > last command > tracked > none.
- Executor over a fake `StatusTransport`: first send records the slot; edit in
  place; edit UnknownMessage → send; Replace deletes old then sends to the new
  channel; delete `Other` still sends; a send failure, and an edit failure other
  than UnknownMessage, clear the tracked message; Finished sets the phase and
  keeps tracking; a later Playing update continues the same message.
- `track_end_status` — gp guard, next/none × autoplay on/off.
- `reply_privately` — setting × prefix.
- `now_playing_pointer` — title and link.
- Settings — default off; `From<GuildSettingsRead>` carries the column; toggle
  flips.
- `PlaylistQueued` — the playlist arm's embed, extracted as
  `playlist_queued_embed()`, serializes with `PLAY_PLAYLIST` and without
  `PlaylistQueued`. (Not through `build_play_embed` with an offline queue: its
  multi-track branch estimates play time with songbird's `get_info`, which
  never answers without a voice connection.)
- Gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings`, `SQLX_OFFLINE=true cargo test --workspace`.

**Only a live bot can show** the gateway-driven parts: cached `last_message_id`
behaviour, ephemeral replies, and edit/move in a real channel. A branch image on
TuneTitan covers them before the PR: autoplay off across a track change; chat in
between (move) and silence in between (edit); `/nowplaying` in both modes;
`/clean` deleting the status mid-playlist; `/stop` → Finished → `/play` resumes
it; a command from a second channel; a `/gp` round showing no status.

## Rollout

Feature → minor bump, all ten members 0.12.1 → 0.13.0. The migration adds a
column with a default. No new environment variables.

- **Production (`bots`):** the pinned `cracktunes-migrate` one-shot applies it
  before the bot starts, once the homelab pins move to v0.13.0.
- **TuneTitan:** has no migrate step — the bot never runs migrations. The
  migration is applied by hand over the ssh tunnel described in
  `homelab/tunetitan/docker-compose.yml` **before** any image that reads the
  column is deployed; otherwise every guild's settings load fails on the
  missing column. v0.12.1 keeps working against the migrated database (the
  column is last and defaulted), so rolling TuneTitan back is safe.

## Out of scope / follow-ups

- Playback controls (buttons) on the status message — the slot owns the message
  id and phase so they can be added in this module alone.
- Applying `ephemeral_replies` to all commands — part of the larger messaging
  refactor.
- The ~150 s of silence before "Spotify took too long to answer" (sleevenote's
  produce budget).
- `toggle_autopause` replies "Self-deafen is now …" (copy-paste bug).
