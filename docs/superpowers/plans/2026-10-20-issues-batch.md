# Issues batch (#554, #397, #207, #535) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four open issues fixed in one PR, shipping as v0.31.0:
- startup no longer depends on httpbin.org (#554);
- the playlist fetch limit is a bot-config knob, default 300 (#397);
- `/downvote` is registered and records its dislike against the right row (#207);
- `/ephemeral` works and is re-enabled (#535).

**Architecture:** Four independent fixes in different files, plus a release-prep task. No migration.

**Tech Stack:** Rust, poise/serenity `next`, songbird, sqlx 0.9 (Postgres, offline `.sqlx` cache), once_cell.

**Spec:** the issues themselves (`gh issue view 554|397|207|535`) and the owner's rulings of 2026-10-20, restated here because an executor sees only this file:

- **#554.** `main_async` (`crack-cli/src/main.rs:79`) awaits `http_utils::init_http_client()`, which sends two GETs to `https://httpbin.org/ip` through the same `CLIENT` and returns `Err` if either fails. The bot then never reaches the gateway, `main` logs and exits 0, and Docker restart-loops it. Building a reqwest client does no I/O, so nothing needs a request.
- **#397.** `PLAYLIST_PLAY_LIMIT = 100` (`crack-core/src/music/query.rs:48`) was a latency guard from per-track resolution. Pages are now read ~100 entries per request (~600 ms each). **Ruling:**
  - a bot-config knob `playlist_limit` in `cracktunes.toml`, **default 300**;
  - `crack-testing/src/yt_playlist.rs`'s `MAX_CONTINUATIONS` (20) becomes a function of the limit;
  - no per-guild setting and no migration;
  - the per-guild override and queue jump-to-page nav stay open, so the PR does **not** close #397.
- **#207.** `/downvote` (`crack-core/src/commands/music/skip.rs:59-102`) is unregistered. `Data::downvote_track` (`crack-core/src/lib.rs:615-627`) has three bugs:
  - it ignores its `_track` argument;
  - it takes ids from `PlayLog::get_last_played_by_guild_metadata`, which returns **metadata** ids (`play_log.rs:239`), and passes one as a **play_log** id to `TrackReaction::insert` (FK `track_reaction.play_log_id → play_log.id`);
  - it unwraps twice.

  Separately, `TrackReaction::insert` is `ON CONFLICT DO NOTHING RETURNING … fetch_one`, which errors with RowNotFound on a second downvote of the same play.

  **Ruling:**
  - register it, and add it to `GP_BLOCKED_COMMANDS`;
  - record the dislike against the newest play of the track actually playing;
  - stop it joining voice: it uses `get_call_or_join_author`;
  - no unwraps;
  - its description becomes "skip and hide from /playlog". Dislikes feed only `/playlog`'s filter (`PlayLog::get_last_played_by_guild_filter_limit`), not autoplay. Wiring them into autoplay is a follow-up.
- **#535.**
  - `/ephemeral` toggles `guild_settings.ephemeral_replies`, but `do_join` (`crack-core/src/commands/music_utils.rs`, the `ctx.defer()` after the permission gate) defers **publicly** before `/play` replies. Discord ignores `ephemeral` on an edit, so the reply stays public.
  - `play_internal` (`crack-core/src/commands/music/doplay.rs:587`, entered by every `/play` variant) computes `private` only *after* `get_call_or_join_author` (`:627`).
  - **Ruling:** when the setting is on, `/play` defers privately *before* joining. poise's `defer` is idempotent, so `do_join`'s later `defer()` is then a no-op, and the "Joining 🔊" notice goes private along with the reply. The channel still sees the public floating status message.
  - Re-enable `/ephemeral` in `music_commands()`.
  - `/skip` and `/nowplaying` never join, so they already reply privately and are untouched.

## Global Constraints

- **No migration.** Don't touch `migrations/`, `crack-core/test_migrations/` or the schema.
- New or changed `sqlx::query!`/`query_as!` macros need the offline cache regenerated: `cargo sqlx prepare --workspace -- --tests --all` against a local postgres. Commit the changed `.sqlx/*.json` files. CI's Build job checks the cache.
- **Local postgres for db-tests:** a ctor forces `DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres`, whatever you export.
  - Start it with `docker --context default run -d --rm --name pg -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine`.
  - 🪤 Bare `docker` targets a remote host, so always pass `--context default`.
  - Run with `cargo test --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests`.
  - `docker --context default stop pg` when done.
- User-facing strings that are new or changed go in `crack-core/src/messaging/messages.rs` as constants, the repo's localization convention (#537). Log lines stay inline.
- CI gates: `cargo fmt --all -- --check`; `cargo clippy --all --locked -- -D clippy::all -D warnings --allow clippy::needless_return`; `cargo test --workspace --locked`.
- Every test must be shown to fail against broken code. Record a mutation → caught-by row for each in the task report, and report any mutation nothing catches rather than hiding it.
- Never `git add -A`; name paths. Commit trailer: exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`, and no other Co-Authored-By line.
- The Bash tool runs zsh: no `for f in $files` word-splitting, and no `echo ====`. Use `bash <<'EOF'` with `set -euo pipefail` for multi-step scripts.

## Review Focus

1. **A `/downvote` with nothing playing, with no play_log row for the playing URL (the DB writer is async and may lag), or with no database** must reply with an error, never panic. Pinned by Task 3's tests on `PlayLog::last_play_of_url` returning `None`, and by reading the command.
2. **Downvoting the same play twice** must count two dislikes, not error. Pinned by `downvoting_twice_counts_two_dislikes` (Task 3).
3. **A `cracktunes.toml` without `playlist_limit`** must load with 300. One with `playlist_limit = 0` must not make every playlist empty: treat 0 as unset. Pinned by Task 2's config tests.
4. **`/play` with `ephemeral_replies` on, when the bot is ALREADY in the channel** (no join, so no `do_join` defer) must still reply privately. The early `defer_ephemeral` covers it because it runs before both branches. Reviewed by reading Task 4, then checked live.
5. **A prefix command** (`~play`) must never be deferred privately. `reply_privately(setting, is_prefix)` is already false for prefix, and poise's defer is a no-op for prefix. Reviewed by reading Task 4.

---

### Task 1: Startup does no network I/O (#554)

**Files:**
- Modify: `crack-core/src/http_utils.rs:205-224` (`init_http_client`)
- Modify: `crack-cli/src/main.rs:78-79` (its call)

**Interfaces:**
- Produces: `pub fn init_http_client()` (sync, returns `()`) in `http_utils.rs`. It calls `once_cell::sync::Lazy::force(&CLIENT)` and nothing else.

- [ ] **Step 1: Implement.**
  - Replace `init_http_client`'s body and signature as above. Its doc says it builds the shared client eagerly, so a TLS or builder failure shows at boot, and that it makes no request.
  - In `main.rs`, replace the comment and the `.await?` call with a plain call.
  - Remove the stale "both the new and old version of the library" comment.
  - Leave `get_client_old` alone: eight call sites use it, and they're out of scope.
- [ ] **Step 2: Verify no request is made.** `rg -n "httpbin" crack-*/src` returns nothing outside tests and comments. If any test still names httpbin, report it.
- [ ] **Step 3: Optional live check.** If feasible without secrets, run the built binary under `unshare -rn` (no network) with a dummy `DISCORD_TOKEN`. Confirm the log reaches the gateway connection attempt instead of failing on httpbin. Report the result, or why it wasn't feasible. Don't print env values.
- [ ] **Step 4: Gates.** fmt, clippy, and `cargo test -p crack-core -p cracktunes`.
- [ ] **Step 5: Commit.** `git add crack-core/src/http_utils.rs crack-cli/src/main.rs`, message `fix(startup): no request to httpbin.org before connecting (#554)`. The PR body carries `Closes #554`.

### Task 2: Configurable playlist limit, default 300 (#397)

**Files:**
- Modify: `crack-core/src/lib.rs` (`BotConfig` field, `Default`, `Display`, getter)
- Modify: `crack-core/src/music/query.rs:44-48,441,547`
- Modify: `crack-testing/src/yt_playlist.rs:28-30,95`
- Test: the tests module of `crack-core/src/lib.rs`, `crack-testing/src/yt_playlist.rs`, and the `BotConfig` TOML test next to `bot_config_ignores_a_leftover_self_deafen_key` in `crack-cli/src/main.rs`

**Interfaces:**
- Produces:
  - `pub const DEFAULT_PLAYLIST_LIMIT: u64 = 300;` in `crack-core/src/music/query.rs`. It replaces `PLAYLIST_PLAY_LIMIT`; update its doc to give the issue's reason (one request per ~100 entries).
  - `BotConfig.playlist_limit: Option<u64>`.
  - `pub fn get_playlist_limit(&self) -> u64` on `BotConfig`. It returns the configured value, or `DEFAULT_PLAYLIST_LIMIT` when unset **or 0**.
  - `pub fn max_continuations(limit: usize) -> usize` in `yt_playlist.rs`: `limit.div_ceil(100).saturating_mul(2).max(20)`. It replaces the `MAX_CONTINUATIONS` const at its one use (`:95`).
- Consumes: `ctx.data().bot_settings` at both `query.rs` sites. `mode_next` passes the limit to rusty_ytdl's `PlaylistSearchOptions { limit, .. }`; `mode_end` passes it to `resolve_playlist_limit`.

- [ ] **Step 1: Write failing tests.**
  - `bot_config_defaults_to_a_300_track_playlist_limit`: `BotConfig::default().get_playlist_limit() == 300`.
  - `a_zero_playlist_limit_means_the_default`: `playlist_limit: Some(0)` → 300.
  - `a_configured_playlist_limit_is_used`: `Some(1000)` → 1000.
  - In crack-cli: `bot_config_reads_playlist_limit_from_toml`. It writes a temp `.toml` with `playlist_limit = 750` and loads it through the real `BotConfig::from_config_file`, the same way the self_deafen test does, then asserts 750.
  - `max_continuations_has_a_floor_of_20`: limits 1, 100 and 300 → 20.
  - `max_continuations_grows_with_the_limit`: 5000 → 100.
- [ ] **Step 2: Run them and confirm they fail** (compile errors count).
- [ ] **Step 3: Implement** per Interfaces. `BotConfig`'s `Display` prints the field the way neighbouring fields do. `crack-testing`'s `DEFAULT_PLAYLIST_LIMIT = 50` (`crack-testing/src/lib.rs:40`) is a different path (`resolve_playlist`, which `/playytplaylist` uses): leave it, and say so in the report.
- [ ] **Step 4: Run the tests until they pass, then sabotage each one** (e.g. return 100 from the getter; drop the 0 guard; return a constant 20 from `max_continuations`). Record the rows. Also record one glue mutation that no test can catch offline: `query.rs` using the constant instead of the getter. Mark it "not caught: needs a ctx; reviewed by reading".
- [ ] **Step 5: Gates** (fmt, clippy, `cargo test --workspace --locked`).
- [ ] **Step 6: Commit.** Name the paths. Message `feat(playlists): playlist_limit in cracktunes.toml, default 300 (#397)`.

### Task 3: `/downvote` works and is registered (#207)

**Files:**
- Modify: `crack-core/src/db/play_log.rs` (new query)
- Modify: `crack-core/src/db/track_reaction.rs` (the `insert` conflict behaviour)
- Modify: `crack-core/src/lib.rs:614-627` (`Data::downvote_track`)
- Modify: `crack-core/src/commands/music/skip.rs:59-102` (the command)
- Modify: `crack-core/src/commands/music/mod.rs` (`music_commands()`)
- Modify: `crack-core/src/commands/music/gp/state.rs:127` (`GP_BLOCKED_COMMANDS`) and `crack-core/src/commands/music/gp/test.rs:~2436-2447` (the guard that fires when downvote is registered)
- Modify: `crack-core/src/messaging/messages.rs` (any new user-facing copy)
- Modify: `.sqlx/` (regenerated)

**Interfaces:**
- Produces:
  - `PlayLog::last_play_of_url(conn: &PgPool, guild_id: i64, source_url: &str) -> Result<Option<i64>, Error>`: the `play_log.id` of the newest play in the guild whose `metadata.source_url` equals `source_url`.
  - `Data::downvote_track(&self, guild_id: GuildId, source_url: &str) -> Result<TrackReaction, CrackedError>`: it looks up that id, and when there is none returns an error that names the case (add a `CrackedError` variant if none fits; its message goes in messages.rs). It then records one dislike and returns the updated row.
- `TrackReaction::insert` must not error when the row exists. Make it an upsert that returns the row, or make it return `Option`, at whichever call sites exist; keep `add_dislike` incrementing by one. `rg -n "TrackReaction::insert"` first, and keep every caller correct.

- [ ] **Step 1: Write failing db-tests** (`#[sqlx::test]`, feature `db-tests`, in the files' existing test modules):
  - `last_play_of_url_finds_the_newest_play_of_that_url`: two plays of URL A and one of B in a guild. It returns the newer A play's `play_log.id`, which is a **play_log** id, not the metadata id. Arrange the fixtures so the two ids differ.
  - `last_play_of_url_is_none_for_an_unplayed_url`.
  - `downvoting_twice_counts_two_dislikes`: two calls of `Data::downvote_track`, or of the DB pair it uses, on the same play give `dislikes == 2` and no error.
  - Use the existing fixture helpers in `db/` tests for metadata and play_log rows; read them first.
- [ ] **Step 2: Run them with the local postgres and confirm they fail.**
- [ ] **Step 3: Implement** the query, the insert fix and `downvote_track` (no unwraps, `source_url` used). Then the command:
  - `/downvote` uses `music_utils::connected_call(&manager, guild_id, None)` and replies `NotConnected` instead of joining;
  - its doc and `description` become "Skip the current track and hide it from /playlog" (copy in messages.rs if poise needs a literal, otherwise the doc comment);
  - it is registered in `music_commands()`;
  - `"downvote"` is added to `GP_BLOCKED_COMMANDS`;
  - the gp/test.rs block that asserts downvote is unregistered is replaced with an assertion that it is blocked, plus a one-line comment on why.
- [ ] **Step 4: Regenerate `.sqlx`**, then run the db-tests and the full workspace suite with db-tests.
- [ ] **Step 5: Sabotage.**
  - Pass the metadata id again: the first test fails.
  - Revert the insert to `fetch_one` on conflict: the twice-test fails.
  - Remove `"downvote"` from `GP_BLOCKED_COMMANDS`: the gp test fails.

  Record the rows.
- [ ] **Step 6: Gates and commit.** Name the paths, including `.sqlx/` files. Message `fix(downvote): dislike the playing track's own play, register /downvote (#207)`.

### Task 4: `/ephemeral` takes effect on `/play` (#535)

**Files:**
- Modify: `crack-core/src/commands/music/doplay.rs:587-640` (`play_internal`)
- Modify: `crack-core/src/commands/music/mod.rs:76-78` (re-register `ephemeral()`)
- Modify: `crack-core/src/commands/music_utils.rs` (`do_join`'s doc only: say that a caller may have deferred already, privately)

**Interfaces:**
- Consumes: `crate::messaging::status::reply_privately(setting: bool, is_prefix: bool) -> bool` (existing and tested), and `ctx.data().get_ephemeral_replies(guild_id)`.

- [ ] **Step 1: Implement.**
  - In `play_internal`, compute `private` once, at the top, as soon as `guild_id` and `is_prefix` are known. If it is true, `ctx.defer_ephemeral().await?` there, before the paused/`NoQuery` early returns and before `get_call_or_join_author`.
  - Delete the later computation and pass the same `private` to `send_search_message_as`. Comment why with a 🪤: Discord fixes a response's visibility when it is created, and `do_join` defers publicly.
  - Remove the "deliberately absent" comment and add `ephemeral::ephemeral()` back to `music_commands()` in alphabetical position.
- [ ] **Step 2: Check `/ephemeral`'s own reply copy** still matches what now happens ("/play, /skip and /nowplaying now reply privately"). If it overclaims or underclaims, fix the constant in messages.rs.
- [ ] **Step 3: Tests.** The ordering is poise glue with no offline seam, so no unit test can pin it. Record the mutation "defer after the join" as **not caught offline, checked live on TuneTitan** in the report. If an existing test lists registered music commands (gp/test.rs builds `music` from `music_commands()`), make sure it still passes with `ephemeral` registered, and check whether `ephemeral` belongs in `GP_BLOCKED_COMMANDS`: it changes a guild setting, not the queue. Rule on that in the report.
- [ ] **Step 4: Gates and commit.** Message `fix(ephemeral): /play defers privately before joining, /ephemeral is back (#535)`.

### Task 5: Release prep (v0.31.0)

**Files:** `CHANGELOG.md` (under `## Unreleased`), `Cargo.toml:24`, `Cargo.lock`, and `docs/` if any page lists commands or config keys (`rg -n "downvote|ephemeral|playlist" docs/ README.md`).

- [ ] **Step 1: CHANGELOG.** Match the style of the neighbouring entries: a bold lead, then the plain explanation.
  - **Fixed:** #554, #207, #535.
  - **Added:** `/downvote` registered, `/ephemeral` re-enabled, `playlist_limit`.
  - **Changed:** the default playlist limit goes from 100 to 300.
- [ ] **Step 2: Document `playlist_limit`** wherever `cracktunes.toml` keys are documented (find it). If nowhere, add it to the CHANGELOG entry only.
- [ ] **Step 3: Version.** Set `[workspace.package] version` to `0.31.0` (the branch will have been rebased onto v0.30.2), then `cargo build --workspace` so `Cargo.lock` follows. Only the workspace crates' versions change.
- [ ] **Step 4: Commit.** `v0.31.0: /downvote, /ephemeral, playlist_limit, no httpbin at startup`.
