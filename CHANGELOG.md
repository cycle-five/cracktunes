# Change Log

## TODO:

- [ ] /changenicks command. Renames all users in the guild
      to a random nick name from a themed list of names. Use your
      own custom list, or choose from one of the many I've
      pre-curated and use in my own server.
- [ ] Codebase architecture documentation.
- [ ] Support discordbotlist.com (voting service).
- [ ] Decide on whether to use ephemeral for admin messages.

## Unreleased

### Added

- **`/gp` — "What's your song?" party game** (alias `/guiltypleasure`, category Games).
  The host runs `/gp start <category> [rounds] [timer]` in a voice channel, picking one of
  seventeen prompt categories (🥹 Nostalgia, 🔥 Slightly More Dangerous, 🎶 The Really Good
  Game Prompts, 🚗 Car / Driving, 🌿 Altered-State / Chill, 😭 Emotional, 🤢 Bad Music,
  🎧 Hyper-Specific, 🖤 Weirdly Revealing, 😂 Game Chaos, ⚡ One-Worders, 🎤 Social / Go-To,
  😈 Guilty Pleasures / Secret Taste, 💋 Sex / Romance / Attraction, 🥀 Emotional Damage,
  🕺 Chaotic / Funny, 🧠 Personality Reveals) or 🎲 Mixed.
  Each round the bot posts a prompt ("What song do you cry to?") with a live countdown;
  everyone in the voice channel secretly submits one song with the ephemeral, slash-only
  `/gp submit <song>` (resubmitting replaces it). The window closes on the timer (default
  3 minutes, 30-second warning), as soon as every non-bot member of the voice channel has
  submitted, or when the host runs `/gp close`. The round's songs then play back-to-back
  with the requester hidden ("(auto)" in the now-playing/queue embeds); under each song a
  dropdown asks who submitted it and a 👍 button lets people like it. When the song ends
  the message is edited with the reveal, likes and scores. `/gp skip` (host) ends a song
  early, `/gp status` shows the prompt, who has submitted/guessed, likes and scores,
  `/gp end` (host or Manage Server) aborts. Scoring: +100 per correct guess, +100 to the
  submitter if nobody guessed them, +10 per 👍. A one-song round is likes-only; an empty
  round is skipped. Scores are in memory for the game only. While a game runs,
  music commands that would take playback out of the game's hands (`play`, `skip`,
  `voteskip`, `stop`, `pause`, `seek`, `repeat`, `leave`, `summon`, ...) are refused with
  a pointer to `/gp skip` / `/gp close` / `/gp end`, autoplay and autopause are
  suspended, and the game is discarded if the bot is disconnected from voice. `/resume`
  is deliberately left open as an escape hatch. Prompts are compiled in from
  `crack-core/src/commands/music/gp_prompts.json`.
- `/gp voteskip` ends the current song once a strict majority has voted, so a song can be
  moved past without the host. The song's own submitter does not vote on it: running the
  command on your own song pulls it outright, and the submitter is left out of the pool
  the majority is measured against, so a two-person channel still only needs the one
  eligible voter. Votes belong to the song and are cleared with it.
- The game is closed to spectators: acting on a running game requires being in its voice
  channel *and* having submitted a song. Submitting is how you join, and it sticks for the
  rest of the game, so sitting a round out does not put you back outside it. This applies
  to the guess dropdown and 👍 as well as to the subcommands, so someone who never submits
  can no longer guess their way to winning a game. `/gp end` is exempt from both checks so
  an admin can always stop a game from outside it, and the host can read `/gp status`
  before submitting.

### Changed

- `TrackEndHandler` now forgets skip votes before applying autopause, rather than after.
  The two are independent, and the order changed only so the `/gp` early-return could sit
  between them; guilds that never use `/gp` see no behaviour change.
- `IdleHandler` no longer counts a guild as idle while a `/gp` game is running. A
  submission window is silent by design, and at the default ten-minute idle timeout a long
  window would disconnect the bot mid-round and take the game with it.

### Fixed

- Ending a game no longer starts autoplay. `/gp end` removed the game before stopping
  playback, so the resulting `TrackEvent::End` reached the global handler with no game to
  find and was treated as an ordinary track ending -- in an autoplay guild, dropping an
  unrelated recommended track in on top of the game ending. Because `TrackHandle::stop()`
  only queues the `End` for the driver, the game now stays in the map until that event
  actually lands and the global handler collects it; `/gp end` no longer removes it itself,
  with a short backstop for the case where the event never arrives.
- A song whose stream dies part-way through is scored normally again. Any `Errored` state
  counted as "never played", so a stream that failed two minutes in discarded everyone's
  guesses and 👍 and told the room it never played. A failure now only skips scoring if
  less than 30 seconds played -- long enough that "the room heard it" is true, rather than
  paying the submitter the fooled-everyone bonus for a song that died in the first
  fraction of a second and nobody could have guessed.
- `/gp voteskip` could not reach a majority in a channel where anyone had not submitted.
  Only players may vote, but the majority was measured against the whole voice channel, so
  the bar counted people who could not help clear it: one non-submitter in a channel of
  three made the song unskippable by vote. The pool is now the players in the channel other
  than the song's submitter.

## v0.4.1 (2026/08/23)

### Security

- Cleared the four Dependabot alerts v0.4.0 left behind. protobuf 2.28 -> 3.7.2
  (via prometheus 0.13 -> 0.14), idna down to 1.1 only (via whois-rust 1.6 -> 3.1),
  and lru 0.12.5 -> 0.16.4 by dropping the `cycle-five/ipinfo-rs` fork for upstream
  ipinfo 3.5 -- upstream had since removed the openssl dependency the fork existed
  to avoid. The fourth, a libcrux panic, is unreachable: davey supports only the
  AES-128-GCM ciphersuite, so ChaCha20Poly1305 is never selected.
- Every workflow now declares an explicit `permissions:` block.

### Dead features repaired

Three optional features had rotted into a state where turning them on would not
compile. `-A warnings` and a lint workflow that had been auto-disabled for
inactivity meant nothing complained.

- **`crack-metrics`** was `[]`, so it never enabled `dep:prometheus`;
  `pub mod metrics;` was commented out of lib.rs, so `crate::metrics` did not exist
  for `utils.rs` to import; and the module held a private, unreferenced
  `metrics_handler` returning a `warp::Reply`, a crate crack-core does not depend on.
- **`crack-telemetry`** was `[]` as well, while `init_telemetry` referenced
  `JsonStorageLayer`, an undefined `formatting_layer`, and an OTLP
  `set_text_map_propagator` -- all left dangling when the opentelemetry imports were
  commented out. It now means structured JSON logs: `tracing-bunyan-formatter`
  behind the feature, with `SERVICE_NAME` finally used as the bunyan service name.
  The propagator call is gone rather than pulling in opentelemetry to feed a tracer
  nothing reads.
- **`crack-metrics` in crack-cli** was an empty feature that gated one dead const;
  it now forwards to `crack-core/crack-metrics`.

`cargo clippy --all-features` is clean for the first time in a long while, and
`rust-clippy.yml` -- disabled by GitHub for inactivity since 2025-11-30 -- is
enabled again now that it has something green to report.

## v0.4.0 (2026/08/22)

### Toolchain

- Builds on **stable Rust 1.98** (`rust-toolchain.toml` was pinned to `nightly`).
  The only nightly feature in use was `fmt_internals` / `formatting_options`,
  and only inside one test that Debug-formatted an embed.
- `.cargo/config.toml` no longer sets `-A warnings` (it was hiding every lint in
  the workspace) or `--cfg proc_macro_c_str_literals` (nothing reads it now).
- Dockerfile builder moved to `rust:1.98.0-alpine3.22`.
- **The workspace is clippy-clean again**, which it had not been in a long time.
  With `-A warnings` gone, `cargo clippy --all -- -D warnings` -- what the lint
  workflow actually runs -- surfaced roughly 120 warnings, and `cargo fmt --check`
  another 33 hunks. Notable substance behind the noise: six `unnecessary_unwrap`
  sites that unwrapped a value right after testing it (the voice-state handler's
  channel-change branch is now a `match` on the pair, and no longer panics when a
  member is missing from both the old and new state), stale `#[allow]`s for a lint
  clippy has since removed, and dead imports. The three `large_enum_variant`
  offenders (`QueryType`, `MessageOrReplyHandle`, `CommandOrMessageInteraction`)
  carry a documented `#[allow]`: boxing them is worth doing, but it touches every
  construction and match site and belongs in its own change.

### Dependencies

- **Dropped the `CycleFive/*` forks of serenity, songbird and poise** in favour
  of upstream. The serenity mirror had no custom commits and was ~30 behind;
  the songbird mirror pointed at the identical upstream commit; the poise fork
  differed only by a stale `rev` pin. Since songbird 0.6 and poise both resolve
  serenity from `serenity-rs/serenity`, keeping the mirror forced two
  incompatible copies of serenity into the dependency graph.
- Removed the `[patch.crates-io.serenity-voice-model]` entry: that subcrate no
  longer lives in the serenity repo, and the patch broke resolution outright.
- serenity 0.12.5-next, songbird 0.4.5 -> 0.6, serenity-voice-model 0.2 -> 0.3,
  tokio 1.42 -> 1.53, sqlx 0.8.2 -> 0.8.6, async-openai 0.26 -> 0.41 (dropping
  the fork and the `backoff` patch), vergen-gitcl 1.0 -> 10.0, extract_map
  0.1 -> 0.3.

### API migration (serenity `next`, songbird 0.6, poise)

- `ChannelId` split into `ChannelId` (guild channels) and `GenericChannelId`
  (anything you can send a message to).
- `EventHandler`'s per-event methods collapsed into a single `dispatch`;
  poise dropped `FrameworkOptions::event_handler`, so the event log/router is
  driven from the serenity handler now.
- Components v2: top-level components are `CreateComponent`, with action rows
  as one variant.
- `MessageUpdateEvent` is now `{ message }`; `GuildChannel` gained a flattened
  `base`; `FullEvent::snake_case_name()` gave way to `strum::IntoStaticStr`.
- poise no longer accepts `usize` command arguments; affected commands take
  `u32` and convert at the boundary.

### Fixed: playlists

- **Playlists had stopped resolving entirely.** `rusty_ytdl` 0.7.4 looks for
  `playlistVideoRenderer` entries in `ytInitialData`; YouTube has since moved
  playlist listings to `lockupViewModel`, so every playlist failed with
  `PlaylistBodyCannotParsed`. Added `crack-testing`'s `yt_playlist` module,
  which reads the playlist page directly, understands both shapes, and follows
  continuations (also moved, to `continuationItemViewModel`) for playlists
  longer than one page. rusty_ytdl remains the fallback.
- **Playlist loading is no longer serial.** The play path used to discard the
  metadata the playlist fetch had already returned and re-resolve every entry
  one at a time, spawning a `yt-dlp` subprocess per track. Entries now carry
  their metadata straight from the listing, and anything that does need
  resolving (keyword lists, Spotify tracks) goes through `resolve_track_many`,
  which runs `RESOLVE_CONCURRENCY` lookups at a time and preserves order.
- Keyword resolution no longer follows its search hit with a redundant
  `get_info` round trip -- that doubled the cost of every Spotify playlist
  track for metadata the search already returned.
- The first track of a playlist is queued on its own so playback starts
  immediately, and progress edits are throttled instead of one-per-batch
  (Discord rate-limits edits per channel).
- A single unresolvable entry (deleted, private, region-locked) is skipped and
  logged instead of aborting the whole playlist load.
- Batch enqueues take the call lock once rather than once per track.

## v0.3.16 (2024/12/12)
- Commands each show up and work only where they are supposed to (guilds, dms, etc).

## v0.3.16-alpha.3 (2024/12/09)
- re-enable the commands that were disabled in the last release
  for the serenity-next branch.
- Got the rusty_ytdl library with the compose to an Input working.
  The result is the bot starts up and responds and queues songs much faster.
- Youtube suggestions are now working again.

## v0.3.16-alpha.2 (2024/12/01)
- [x] update to serenity-next branch

## v0.3.15-alpha.1 (2024/11/23)
- [x] bug fix patch 

## v0.3.14 (2024/11/05)
- [x] Big refactor, moving a lot of the code into modules.
- [x] crack-testing module for testing and developing new features without
  affecting the main bot.
- [x] crack-types module for shared types. New modules can depend on this
  to avoid circular dependencies.
- [x] Auto complete for `/play` brings up actual youtube search results.

## v0.3.13 (2024/09/19)
- Dependency updates

## v0.3.12 (2024/09/12)

- [x] `/movesong` command
- [x] `muteall` command to server mute all other people in a call (Admin only)
- [x] `@bot` mention works like a prefix.
- [x] default to playing the album version of songs where possible.
- ~~[ ] Add setting for whether or not to look for album version of song.~~ (reverted moved to next release)
- [x] Large refactoring of code into more modules
- [x] Test Coverage > 24%.

## v0.3.11 (???)

- ???

## v0.3.10 (2024/07/28)

- [x] performance improvements.
- [x] All milestones recorded as GitHub issues.
- [x] Add help option to all commands.
- [x] Added back in internal playlist support. 
- [x] `/playlist create <playlistname>` Creates a playlist with the given name
- [x] `/playlist delete <playlistname>` Deletes a playlist with the given name
- [x] `/playlist addto <playlistname>` Adds the currently playing song to <playlistname>
- [x] `/playlist list` List your playlists
- [x] `/playlist get <playlistname>` displays the contents of <playlistname>
- [x] `/playlist pplay <playlistname>` queues the given playlist on the bot
- [x] `/playlist loadspotify <spotifyurl> <playlistname>` loads a spotify playlist into a Crack Tunes playlist.

## ~~v0.3.9~~

- internal testing version, publicly skipped
- i.e. git branches got fucked and this was easier

## v0.3.8 (2024/07/17)

- [x] Looked at rolling back to reqwest 2.11 because it was causing problems.
      Decided to stick with 2.12 and keep using the forked and patched version
      of serenity, poise, songbird, etc.
- [x] Pulled in songbird update to support soundcloud and streaming m8u3 files.
- [x] More refactoring.
- [x] Brainf\*\*k interpreter.
- [x] Switched all locks from blocking to non-blocking async.
- [x] Unify messaging module.
- [x] Fixed repeat bug when nothing is playing.
- [-] Change `let _ = send_reply(&ctx, msg, true).await?;`
  to `ctx.send_reply(msg, true).await?;` (half done)
  ...
  For next version...

## v0.3.7 (2024/05/29)

- crackgpt 0.2.0!
  Added back chatgpt support, which I am now self hosting for CrackTunes
  and is backed by GPT 4o.
- Use the rusty_ytdl library as a first try, fallback to yt-dlp if it fails.
- Remove the grafana dashboard.
- Switch to async logging.
- Add an async service to handle the database (accept writes on a channel,
  and write to the database in a separate thread).
  Eventually this could be a seperate service (REST / GRPC).

## v0.3.6 (2024/05/03)

- Music channel setting (can lock music playing command and responses to a specific channel)
- Fixes in logging
- Fixes in admin commands
- Lots of refactoring code cleanup.

## v0.3.5 (2024/04/23)

- Significantly improved loading speed of songs into the queue.
- Fix Youtube Playlists.
- Lots of refactoring.
- Can load spotify playlists very quickly
- Option to vote for Crack Tunes on top.gg for 12 hours of premium access.

## v0.3.4

- playlist loadspotify and playlist play commands
- Invite and voting links
- Updated serenity / poise / songbird to latest versions
- Refactored functions for creating embeds and sending messages to it's own module

## v0.3.3 (2024/04/??)

- `/loadspotify <spotifyurl> <playlistname>` loads a spotify playlist into a Crack Tunes playlist.
- voting tracking

## v0.3.2 (2024/03/27)

- Playlists!
- Here are the available playlist commands
  - `/playlist create <playlistname>` Creates a playlist with the given name
  - `/playlist delete <playlistname>` Deletes a playlist with the given name
  - `/playlist addto <playlistname>` Adds the currently playing song to <playlistname>
  - `/playlist list` List your playlists
  - `/playlist get <playlistname>` displays the contents of <playlistname>
  - `/playlist play <playlistname>` queues the given playlist on the bot
- Added pl alias for playlist
- Added /playlist list
- Fixed Requested by Field
- JSON for grafana dashboards

## v0.3.1 (2024/03/21)

- Fix the requesting user not always displaying
- Reversed order of this Change Log so newest stuff is on top

## ~~0.3.0-rc.6~~

## 0.3.0

- Added more breakdown of features which can be optionally turned on/off
- Telemitry
- Metrics / logging
- Removed a lot of unescesarry dependencies

## 0.1.4 (crack-osint) (2024/03/12)

- osint scan command to check urls for malicious content

## 0.3.0-rc.5 (2024/03/09)

- cargo update
- GuildId checks
- user authorized message
- adding scan command
- add feature for osint
- make admin commands usable by guild members with admin
- add dry run to rename_all

## 0.3.0-rc.4

- fix storing auto role and timeout I think
- download and skip together
- ~~try to finally fix this fucking volume bug~~
- fix loading guild settings
- add pgadmin to docker compose
- ~~fix volume~~ (volume is still broken)

## 0.3.0-rc.2

- [x] Clean command
- [x] Bug fixes
- ~~[ ] Down vote~~ (not working)

## 0.3.0-rc.1

- [x] Dockerized!
- [x] Refactored settings commands.
- [x] Storing and retrieving settings from Postgres.
- [x] Updated dependencies to be in line with current.

## ~~0.2.13~~

- ~~[] Port to next branch of serenity~~
- ~~[] Flesh out admin commands~~

## ~~0.2.12~~

## ~~0.2.6~~

Didn't really track stuff here...

## 0.2.5

- ~~[] Shuttle~~
- ~~[] Reminders~~
- ~~[] Notes~~

## 0.2.4 (2023/07/17)

- [x] Bug fixes.
- [x] Remove reliance on slash commands everywhere.
- [x] Remove shuttle for now

## 0.2.3

- [x] Bug fixes (volume)
- [x] Shuttle support (still broken)

## 0.2.2 (2023/07/09 ish)

- [x] Welcome Actions
- [x] Play on multiple servers at once

## 0.2.1 (2023/07/02)

- [x] Play music from local files

## 0.2.0

- [x] Play music from YouTube
- [x] Play music from Spotify (kind of...)
