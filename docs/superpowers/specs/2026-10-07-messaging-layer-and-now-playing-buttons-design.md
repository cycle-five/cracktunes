# Messaging layer and now-playing buttons

**Status:** approved in brainstorming, 2026-10-07.
**Ships as:** two stacked PRs. PR 1 is the messaging layer (v0.22.0). PR 2 is the now-playing buttons (v0.23.0).
**Follows:** `2026-10-04-ops-layer-and-dashboard-controls-design.md`, which did for queue changes what this does for messages.

## Why

The owner asked for every message to go through one abstraction, the way `music::ops` did for queue changes, "and then I think we'll find the quirks and annoyance are hammered out". Today the bot reaches Discord by five routes:

1. **Command replies.**
   - `CrackedMessage` (113 variants, rendered by `Display`) goes through `PoiseContextExt::send_reply` and `send_message`, at about 146 `send_reply` call sites.
   - About 75 direct `send_message`, `.say`, `ctx.send` and `CreateReply` calls bypass that, across about 30 files.
   - The callers decide `as_embed` each time.
2. **The floating status message** (`messaging::status`). It already sits behind a `StatusTransport` seam with tests against a fake.
3. **Background notices:** `track_failed` (which reuses `StatusTransport`), `send_plain` for the autoplay notices, idle-leave, and voice stats.
4. **Dashboard echoes:** `remote::announce`.
5. **`/gp`:** its own embeds and components, routed by the persistent `gp:` custom-id prefix in `handlers/serenity.rs`.

**Quirks this design removes:**
- Third-party text is interpolated raw:
  - `SkipTo` and `SongQueued` write `[**{title}**]({url})` unescaped;
  - an empty title gives "⏭ Skipped to **!" (the owner's TuneTitan test, 2026-10-06);
  - `*`, `_` or `@` in a title breaks the markdown.
- "Added to queue! Track duration: 00:00" and "Estimated time until play" are computed from an unknown duration.
- `build_now_playing_embed_metadata` logs `error parsing url: RelativeUrlWithoutBase` for every track without a thumbnail, and shows "Streaming via unknown" with an `unknown` favicon.
- Plain-text replies are coloured with the `colored` crate's ANSI codes (`poise_ext.rs`). That is harmless only because the container's stdout is not a TTY.
- Mention safety depends on the route: embeds never ping, but plain content pings whatever it contains.
- Durations are formatted three ways: `duration_to_string` gives `00:00:00`, `get_human_readable_timestamp` has its own format, and `/gp` has a third.
- Titles are capped in three places: `audit_view::cap`, the duplicate in `commands/status.rs`, and `track_failed`.
- The now-playing progress line ("00:00 / 4:33") is frozen at the moment of sending.

The owner also wants buttons on the now-playing message, so people don't need slash commands. A button press is itself a message exchange, so the buttons are built on the layer, not beside it.

## Rulings

These are the owner's decisions from brainstorming:

| Question | Ruling |
|---|---|
| Scope of "all messages" | **Music-facing first.** Listener-facing music output moves in PR 1. `/gp`, admin, osint, settings, register and utility become named follow-ups. |
| Order | **Layer first, buttons on top.** One spec, two stacked PRs. |
| Approach | **A: typed messages, one renderer, one courier**, enforced by clippy. Rejected alternatives: patching in place (B), and a lifecycle registry for every message (C). |
| Who can press buttons | **Same as the slash commands.** Free, under the `cmd_check_music` rules. |
| What the channel sees on a press | **Status updates and an echo line.** Echoes are a first-class guild setting that can be turned on and off. |
| Progress line | **Live Discord timestamp, re-rendered on events.** True real-time ticking is a follow-up arc. |

## Architecture

All of this lives in `crack-core::messaging`, in four layers.

### 1. Vocabulary: `CrackedMessage`

`CrackedMessage` stays the one list of what the bot says.
- Variants that mention a track stop carrying formatted strings and carry a **`TrackLabel`**:

  ```rust
  pub struct TrackLabel {
      pub title: Option<String>,
      pub url: Option<String>,      // raw; validated at render time
      pub duration: Option<Duration>,
  }
  ```

  It is built from `AuxMetadata` by one constructor, `TrackLabel::from_metadata(&AuxMetadata)`.
- **New variants** cover what is built ad hoc today:
  - `NowPlaying`, carrying the label, the requester, the thumbnail URL, the position and the playback flags;
  - `Finished`;
  - `TrackFailed`, the coalesced list `track_failed` renders today;
  - `Echo`, the control echo, carrying the action, an optional `TrackLabel`, the user, and where the control came from (`Dashboard` or `Button`);
  - `Queued`, for one track or a playlist, carrying the estimated wait.
- `Display` on `CrackedMessage` is kept only as a thin wrapper over `render`'s text, for logs and the existing tests. **No send path uses `Display` directly.**

### 2. `render`

```rust
pub fn render(msg: &CrackedMessage, cx: &RenderCx) -> Rendered;

pub struct Rendered {
    pub content: Option<String>,             // text outside any embed
    pub embed: Option<CreateEmbed<'static>>,
    pub components: Vec<CreateComponent<'static>>,
    pub mentions: Mentions,                  // what may ping; default None
}
```

- `render` is **pure** and **total**: every message renders something, and it never fails or panics.
- `RenderCx` carries what a message depends on but does not own: playback flags for the now-playing buttons, the guild id, and "now" for timestamps. Tests build it by hand.
- Each variant decides **once** whether it is an embed or text. The callers' `as_embed` flag goes away.
- `Rendered` can hold content **and** an embed together. The missing-`EMBED_LINKS` notice rides in the content (`degraded_delivery`'s `NoticeDelivery::Content`), because Discord strips the embed it would otherwise sit in.
- Colours stay as they are per variant: errors red, `Other` gold, everything else blue.

**Rendering rules.** These live only in `render`'s helpers:

- **Titles** (`TrackLabel::title_text(max)`):
  - trimmed, and a blank title becomes `(untitled)`;
  - escaped with `audit_view::escape`, which handles markdown, `<`, brackets and line breaks;
  - cut at a character boundary with `…`: 60 characters inside a sentence (`INLINE_TITLE_MAX`), and 256 for an embed title, Discord's limit.
  - `audit_view::cap` becomes the shared `cap`. `/auditlog` and the dashboard keep their 40. The duplicate `cap` in `commands/status.rs` is deleted.
- **Links:** a title is a link only when its URL parses as http(s). Otherwise it is plain bold text.
- **Durations:** one formatter, `m:ss`, or `h:mm:ss` from an hour up. An unknown or zero duration is **left out**, never shown as `00:00`, and so is anything computed from it, such as "Estimated time until play".
- **Thumbnail and footer:** set only from a valid http(s) URL. With no host, the footer is left out. Neither ever logs a parse error.
- **Mentions:** every `Rendered` states its `Mentions`, and the default allows none. Sends map this to `CreateAllowedMentions`, so plain text cannot ping because of a title.
- **No ANSI:** the `colored` coloring leaves every Discord send path.
- **Length limits:** content is capped at 2000, an embed description at 4096, a field value at 1024, an embed title at 256, and the author line at 256. Each cut is on a character boundary with `…`. An overlong message is trimmed, never rejected by Discord.
- **Progress line** (now-playing):
  - while playing: `"<duration> · ends <t:UNIX:R>"`, a Discord timestamp the client counts down live;
  - while paused: `"Paused at <position>"`;
  - with an unknown duration (live streams): `"Started <t:UNIX:R>"`.
  - It is recomputed on every status render: track change, pause or resume, and a button press. Ticking between events is the follow-up arc.

### 3. `Courier` and `Destination`

```rust
pub enum Destination<'a> {
    Reply(ReplyTo<'a>),          // the command's own reply; ephemeral per `reply_privately`
    Channel(GenericChannelId),
    Status(GuildId),             // the floating status message: placement, edit/replace
    Notice(GuildId),             // the coalescing slot track_failed uses
    Echo(GuildId),               // lands where Status would; nothing when control_echoes is off
    Interaction(&'a ComponentInteraction), // answering a button press
}
```

- `Courier::send(msg, dest) -> Result<Sent, TransportError>` renders and delivers. `Courier::edit(&Sent, msg)` edits something already sent.
- `Sent` carries `(channel, id)` when the result is a visible channel message. For a reply it also keeps what an edit needs (the poise `ReplyHandle`).
- **Transport seam.** Today's `StatusTransport` widens into `Transport`, which covers channel send, edit and delete, `last_message_id`, and interaction acknowledge and follow-up. `DiscordTransport` is the real one; a recording fake serves the tests.
- **Reply seam.** Replies go through a separate `ReplySink` trait with an associated `Handle`:
  - the poise implementation uses `Handle = ReplyHandle<'ctx>`;
  - the fake records `Send`/`EditHandle` operations;
  - the courier's reply logic is generic over it, so the reply rules are tested without poise.
- **Status and Notice** keep their per-guild slots (`status_slots`, `failure_notices`) and their current placement rules. `apply_after` and `report` move behind the courier unchanged in behaviour.
- **Echo** checks the guild's `control_echoes` setting. In PR 1 the setting does not exist yet, so Echo always posts, as today. An echo for a control that changed nothing is not posted (see PR 2).

**Behaviour to preserve.** Each item gets a named test against the fakes:
- **#494:** an edit goes to the handle it was given (`ReplyHandle::edit`), never `@original`.
- **#535 and ephemeral:** a reply's ephemeral flag comes from `reply_privately(setting, is_prefix)` at send time. A reply following a public defer does not pretend to be private.
- **The status floor:** a visible reply's `(channel, id)` is the `after` floor for the next status update. `Sent` provides it, replacing `reply_floor`'s extra GET wherever the courier already has the message.
- **Missing `EMBED_LINKS`:** the content-level notice is delivered when the embed would be stripped.
- **Background destinations** (Status, Notice, Echo, Channel) are best-effort: failures are logged and swallowed. Reply failures return to the caller, and framework `on_error` reports them.

### 4. Enforcement

- `clippy.toml` `disallowed-methods` bans the raw send and edit calls outside `messaging`, with the reason "send through messaging::Courier". The candidates are:
  - serenity's `GenericChannelId::{send_message, say, edit_message, delete_message}` and `ChannelId::{send_message, say}`;
  - `Message::{reply, edit}` and `ComponentInteraction::{create_response, edit_response, create_followup}`;
  - poise's `Context::{send, say}`, `ReplyHandle::edit` and `poise::send_reply`.

  The plan verifies the exact paths that clippy resolves.
- `messaging` itself carries a module-level `#[expect]`: it is where the sends are made.
- Every not-yet-migrated module (`/gp`, admin, osint, settings, register, utility) carries `#![expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]`. Nothing on the music path may carry one.
- The list of these expects is the follow-ups' to-do list. Each follow-up deletes its own, and clippy shows exactly what remains.

## PR 1: the messaging layer (v0.22.0)

Each step leaves the bot working:

1. **Formatting core:** `TrackLabel`, `title_text`, link and URL validation, the duration formatter, thumbnail and footer, `Mentions`, and the length limits, with unit tests.
2. **`Rendered`, `render`, `RenderCx`, `Courier`, `Destination`,** the widened `Transport`, `ReplySink`, and both fakes. Nothing calls them yet.
3. **Shared routes:**
   - the `PoiseContextExt` send helpers, `send_reply` and `send_message`;
   - framework `on_error`;
   - the status message (now-playing and finished);
   - `track_failed`, `send_plain` (the autoplay notices) and `remote::announce` (dashboard echoes);
   - the idle-leave and voice-stats handlers.
4. **Music commands, in clusters:**
   - **transport:** skip, pause, resume, repeat, shuffle, seek, volume, stop, leave, clear and remove;
   - **play:** `/play`, `doplay`, the playlist progress edits in `music::queue`, `music::query`'s sends, and search (`dosearch`, `yt_search_select`);
   - **paging:** queue, nowplaying and lyrics. The collectors stay; their sends and edits move;
   - **rest:** auditlog, playlog, autoplay, autopause, vote and voteskip, grab, summon, diagnose, manage_sources, spotify, gambling, the unregistered `ephemeral` toggle, and `music_utils`.
5. **The ban:** the `clippy.toml` entries, plus the temporary module expects on the non-music modules. A raw send added to a migrated module fails clippy (shown once, then reverted).

Wording is unchanged except for the quirk fixes above. Every variant's rendered output is pinned against **literal** strings: a test comparing a constant to itself passes whatever the constant says.

## PR 2: now-playing buttons (v0.23.0)

**Buttons.** One action row on the status message while something is playing, and none on Finished:

| Button | custom id | Shown when |
|---|---|---|
| `⏸ Pause` | `np:pause:<guild>` | playing |
| `▶ Resume` | `np:resume:<guild>` | paused |
| `⏭ Skip` | `np:skip:<guild>:<track uuid>` | always |
| `🔁 Repeat` | `np:repeat-on:<guild>` (style Secondary) / `np:repeat-off:<guild>` (style Success) | always |
| `🔀 Shuffle` | `np:shuffle:<guild>` | always |

- There is no remove (it needs a queue position, so it stays on the dashboard), and no stop or leave (destructive, and one misclick ends everyone's session).
- **The ids name intent, never a toggle.**
  - `NowPlayingButton` (an enum) parses and formats them, with round-trip and rejection tests.
  - Skip carries the track it was drawn for (`Control::Skip { expect }`), so a stale message or two simultaneous presses cannot double-skip.
  - The ids carry everything, so buttons on an old status message work after a restart.
  - Each id fits Discord's 100-character limit: `np:skip:` + 20 digits + `:` + 36 = 65.
- **Routing:** `handlers/serenity.rs` sends component interactions prefixed `np:` to `messaging::buttons::handle`, next to `GP_CUSTOM_ID_PREFIX`.
- **Access:** `cmd_check_music`'s body is extracted into `music_access(data, guild, member, channel) -> Result<(), CrackedError>`. The slash check and `handle` both call it, so the rules cannot drift:
  - bots are refused;
  - the music channel and role apply;
  - `GameInProgress` applies while `/gp` owns playback.

  The slash check keeps `note_command_channel`.
- **On a press:**
  1. Acknowledge at once with a deferred update (`CreateInteractionResponse::Acknowledge`) via `Destination::Interaction`, inside Discord's 3-second window, before anything slow.
  2. Run the access check. A refusal becomes an ephemeral follow-up to the presser, in the slash command's wording.
  3. Run `remote::control`, generalised from dashboard-only to any control actor:
     - `Actor::button(user, op)` with a new `audit::Source::Button` (stored as the text `"button"`; the `source` column is `TEXT`);
     - `audit_view` shows it as "button" and accepts `button` as a source filter;
     - the dashboard's history page maps it the same way.
  4. **Echo**, if the guild's `control_echoes` is on and the control changed something: `"⏭ Skipped **Title** — @user"`. A control that changed nothing posts no echo, which also fixes the dashboard's parked "a stale tab's redundant pause still echoes".
  5. Settle the status below the echo, as `remote::control` does today. It is re-rendered with the new button states and progress line.

  Refusals from the op (nothing playing, or the track already changed) become ephemeral follow-ups.
- **Unknown or malformed `np:` ids** are logged, and the presser gets the ephemeral "This button is out of date".
- **Playback flags** for rendering come from `remote::read_flags` and `playback_flags`, bounded by `TRACK_INFO_TIMEOUT`. On a timeout the buttons render their defaults (Pause, repeat off) rather than blocking the update. `get_info` never answers on an offline call.

**Echo setting.**
- A new guild setting, `control_echoes: bool`, default **true**.
- Migration `20261007120000_control_echoes.sql`: `ALTER TABLE guild_settings ADD COLUMN IF NOT EXISTS control_echoes BOOLEAN NOT NULL DEFAULT TRUE;`.
- Loaded and saved like `ephemeral_replies`, with `Data::toggle_control_echoes`.
- A new `/echoes` command flips it. It is admin-only like `/ephemeral` and **registered**.
- It covers both button and dashboard echoes.
- The registered command count goes from 43 to 44. A dashboard toggle comes later.

## Errors

- `render` cannot fail.
- Background sends are best-effort: logged, never louder than what they report.
- Reply failures return to the command, and `on_error` reports them through the same courier.
- Buttons acknowledge before anything that can be slow, and every later outcome is an ephemeral follow-up to the presser.

## Testing

- **Formatters:**
  - blank, whitespace and escaped titles;
  - caps on a multibyte character boundary;
  - invalid, relative and non-http URLs;
  - unknown and zero durations, the `m:ss` / `h:mm:ss` boundary;
  - thumbnail and footer omission;
  - every length limit.
- **`render`:** every migrated variant pinned against literal strings; mentions default to none; the progress line in all three states.
- **`Courier`:**
  - each destination against the fake `Transport`;
  - the named preservation tests (#494 via `ReplySink`, ephemeral, the status floor, `EMBED_LINKS` content);
  - Echo off posts nothing; a no-change control posts no echo.
- **Buttons:**
  - id round-trips and rejections;
  - `music_access` parity with the slash check;
  - `handle` against the fakes:
    - acknowledges first;
    - a refusal is ephemeral;
    - a stale skip is refused;
    - echo on and off;
    - the audit source is `button`.
- **The ban:** clippy in CI; shown to bite once.
- **Standing rule:** every new test is sabotaged once to prove it can fail.

## Release

- **Branches:** PR 1 is `feat/messaging-layer` → master. PR 2 is `feat/now-playing-buttons`, stacked on it. PR 2 is retargeted to master only at the moment PR 1 merges (see the stacked-PR memory).
- **Testing:** each goes to TuneTitan for the owner's hand check, with a checklist in its PR body, then production on the owner's go.
- **Verify:** PR 2 makes it report 26/26 migrations and 44 commands.

## Follow-ups (named, not in this arc)

1. **`/gp` onto the courier:** `gp.rs` and `gp_persist.rs`, with their own components and lifecycle.
2. **Admin, osint, settings, register and utility** onto the courier. The temporary module expects reach zero.
3. **Real-time now-playing:** a progress line that ticks between events.
4. **A dashboard toggle for `control_echoes`.**
5. **Queue paging buttons as persistent custom ids** instead of collectors, so they survive restarts.
