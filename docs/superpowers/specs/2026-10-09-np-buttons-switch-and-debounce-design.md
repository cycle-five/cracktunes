# Now-playing buttons: a per-server switch and a press debounce

**Date:** 2026-10-09 · **Release:** v0.25.0 · **Arc:** messaging-refactor follow-up 2a
**Owner rulings (2026-10-09):** 2-second window; a debounced press is dropped silently;
turning the buttons off strips the live ones at once; the debounce covers buttons only.
The general internal permission system (follow-up 2b) is backlog: #209.

## Why

The now-playing buttons (v0.23.0, #587) bypass Discord's per-command
Integrations restrictions: component clicks never see them. A server that
locked `/skip` down needs a way to turn the buttons off. Separately, nobody
should be able to hammer them -- with echoes on, every press posts a line in
the channel.

## What

### 1. The switch

- **Storage:** `guild_settings.now_playing_buttons BOOLEAN NOT NULL DEFAULT TRUE`,
  plumbed exactly as `control_echoes` was in v0.23.0: migration in `migrations/`
  and `crack-core/test_migrations/`; `GuildSettings.now_playing_buttons`
  (`#[serde(default = "default_true")]`, in `PartialEq`, loaded from the row,
  `true` in `new`); `GuildSettingsRead` + the upsert in `db/guild.rs`; `.sqlx`
  regenerated.
- **Operations:** `get_now_playing_buttons(guild) -> bool` (on when the guild has no
  settings loaded) and `toggle_now_playing_buttons(guild) -> Result<bool>` (loads the
  stored row first, like `toggle_control_echoes`).
- **Command:** `/buttons`, category Settings, `guild_only`, admin-only
  (`required_permissions` + `default_member_permissions` = ADMINISTRATOR),
  registered. It flips the setting, replies "on"/"off", then brings the screen
  in line (below).
- **Rendering:** `status::show_now_playing_on` is the only path that draws the
  status card with buttons. When the setting is off it drops the card's
  `controls`.
- **On screen, at once:**
  - *Off:* if the guild's status slot holds a message in `Phase::Playing`,
    `Transport::clear_components` on it (the embed stays). Best effort, logged.
  - *On:* if `music_utils::connected_call` finds a call, `show_now_playing_on`
    re-renders the status (which now carries the buttons). Nothing playing,
    nothing to do.
  - Both live in one testable function over `&dyn Transport`; the poise
    command is glue.

### 2. The debounce

- **`Throttle<K>`** (new, `crack-core/src/utils/throttle.rs` or alongside
  `messaging::buttons`): a `DashMap<K, Instant>` and a window. `allow(key, now)
  -> bool` is one atomic `entry` check-and-set: `true` (and the key's stamp set
  to `now`) when the key has no stamp or `now - stamp >= window`; otherwise
  `false` and the stamp is left alone. **Fixed window from the last accepted
  press**: a dropped press does not extend it, so a masher gets one press
  through every window. Lazy pruning keeps the map bounded: when it holds more
  than a fixed number of keys, `allow` first drops stamps older than the window.
  Time is a parameter, so tests use explicit `Instant`s.
- **Where:** `Data` gains `np_presses: Throttle<(GuildId, UserId)>` with
  `NP_PRESS_WINDOW = 2s`. In-memory only; a restart clears it.
- **`buttons::respond` order:** acknowledge → parse (out-of-date answer, as
  today) → **throttle** (inside the window: return, nothing more) → **switch**
  (off: private "The now-playing buttons are turned off in this server.") →
  `music_access` → run. The throttle comes before the switch so mashing an old
  button on a server that turned them off cannot draw a private reply per press.
  A press counts toward the window once it parses for this guild, whatever
  happens after.
- **Not covered:** the dashboard's controls.

## Strings

In `messaging/messages.rs` ([[localization-strings-in-one-file]]):
`NP_BUTTONS_ON`, `NP_BUTTONS_OFF` (the `/buttons` replies) and
`NP_BUTTONS_DISABLED` (the private refusal), as `CrackedMessage` variants
`NowPlayingButtonsOn` / `NowPlayingButtonsOff` / `NowPlayingButtonsDisabled`.

## Testing

- `Throttle`: first press allowed; inside the window dropped; at exactly the
  window allowed; a dropped press does not extend the window; keys independent;
  pruning drops only stale stamps.
- `respond` (FakePress + stub run): a second press inside the window is
  acknowledged and nothing else (run not called); another user, or another
  guild, is not throttled; after the window it runs again; buttons off → private
  refusal and no run; buttons off + mashing → one refusal per window.
- Settings: default on for a guild with no settings; follows the setting;
  toggle flips and reports; db save/load round trip (db-tests).
- `show_now_playing_on` (FakeTransport, `queue_of`): no components when off.
- The on-screen function: off clears components on a Playing slot and nothing
  else; off with a Finished or empty slot does nothing; on re-renders when a call
  is given and does nothing without one.
- `/buttons` is a registered, guild-only, admin-only command.
- Every test sabotaged; mutation table in the PR.

## Rollback

v0.25.0 adds one migration. A v0.24.x bot image runs on the migrated DB (the
upsert names its columns; the new one keeps its default). Keep the migrate
image at v0.25.0 or later.
