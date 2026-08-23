# CrackTunes modernization — handoff

Work done in a Cowork cloud sandbox; the container is gone. Everything needed to
resume is in this file plus `cracktunes-modernize.bundle`.

Repo: `github.com/cycle-five/cracktunes`. Base: `master` @ `7758352`.

## Getting the branch

```sh
git clone https://github.com/cycle-five/cracktunes.git
cd cracktunes
git fetch /path/to/cracktunes-modernize.bundle modernize-2026:modernize-2026
git checkout modernize-2026
```

`cracktunes-modernize.patch` is the same three commits for `git am`, if you prefer.

## State

Three commits on `modernize-2026`, 61 files, +4142/-1989. Each commit builds.
Nothing is pushed — see **GitHub access** below.

| | |
|---|---|
| `42eaa54` | Build on stable Rust 1.98 with upstream serenity/songbird/poise |
| `2614ffd` | Resolve playlists concurrently instead of one track at a time |
| `557b687` | Read YouTube playlists directly; playlists had stopped resolving |

Verified: `cargo build --release -p cracktunes` succeeds (68M binary, runs, exits
cleanly on missing `DISCORD_TOKEN`). `cargo clippy --workspace --all-targets` has
no errors. Tests below.

## The three commits

### 1. Toolchain + dependencies

`rust-toolchain.toml` was pinned to `nightly`. The only nightly feature in the tree
was `fmt_internals` / `formatting_options`, used in exactly one test that
Debug-formatted an embed. Replaced with `format!("{embed:?}")`; now on stable.
Container was on 1.95, `rustup update stable` → **1.98.0**.

**The three `CycleFive/*` forks were all dead weight and are gone:**

- `CycleFive/serenity` `next` — zero custom commits, 29 behind upstream
- `CycleFive/songbird` `serenity-next` — identical commit to upstream
- `CycleFive/poise` `serenity-next` — one commit of delta, a stale `rev` pin

Worse than redundant: songbird 0.6 and poise both resolve serenity from
`serenity-rs/serenity`, so keeping the mirror forced two incompatible copies of
serenity into the graph. All three now point at upstream.

`[patch.crates-io.serenity-voice-model]` removed — that subcrate no longer lives in
the serenity repo and the patch broke resolution outright (`cargo update` refused
to run). `serenity-voice-model` now comes from crates.io at 0.3.

Other bumps: songbird 0.4.5 → 0.6, tokio 1.42 → 1.53, sqlx 0.8.2 → 0.8.6,
extract_map 0.1 → 0.3, vergen-gitcl 1.0 → 10.0 (API changed: `Build::all_build()`
and `Gitcl::all_git()` are infallible now), async-openai 0.26 → 0.41 from crates.io,
dropping the `cycle-five/async-openai` fork and the `backoff` patch with it.
async-openai 0.41 is feature-gated — needs `features = ["chat-completion"]` — and
its types moved to `types::chat::*`.

Also: `.cargo/config.toml` no longer sets `-A warnings`. That flag was hiding every
lint in the workspace, which is a large part of how this much drift went unnoticed.
`--cfg proc_macro_c_str_literals` dropped too; nothing reads it. Dockerfile builder
`rust:1.81.0-alpine3.20` → `rust:1.98.0-alpine3.22` (1.81 cannot build
serenity-next, which requires 1.95), and the exact alpine 3.20 apk pins unpinned
since they don't resolve on 3.22.

**API migration map** (~96 errors, all in `crack-core`):

- `ChannelId` split into `ChannelId` (guild channels) and `GenericChannelId`
  (anything you can send a message to). `ctx.channel_id()` and `Message.channel_id`
  are `GenericChannelId`; `VoiceState.channel_id` and `GuildChannel.id` stay
  `ChannelId`. Convert with `.widen()` / `.expect_channel()`.
- `EventHandler`'s per-event methods collapsed into one `dispatch(&self, ctx,
  &FullEvent)`. `SerenityHandler`'s handlers kept their shapes as inherent methods
  with a `dispatch` that routes to them.
- poise dropped `FrameworkOptions::event_handler` and `FrameworkError::EventHandler`,
  so `handle_event` is now driven from the serenity handler's `dispatch`.
- Components v2: top-level components are `CreateComponent`, action rows are one
  variant. `create_nav_btns` returns `Vec<CreateComponent>`.
- Collectors take `&Context`, not a `ShardMessenger`; `Context::shard` is private.
- `MessageUpdateEvent` is now `{ message: Message }`. `GuildChannel` gained a
  flattened `base` (`guild_id`, `name`, `kind`).
- `FullEvent::snake_case_name()` gone; the enum derives `strum::IntoStaticStr` with
  SCREAMING_SNAKE_CASE. Helper is `event_log::full_event_name`.
- `CreateEmbed::thumbnail` takes an alt-text second arg. `CreateAttachment::path` is
  no longer async. `GuildId::ban` takes `u32` days.
- poise no longer accepts `usize` command arguments — `remove`, `movesong`, `skip`
  take `u32` and convert at the boundary.
- songbird 0.6: `AudioStream` lost its `hint` field; `DecodeMode::Decode` now carries
  a `DecodeConfig`.
- `Client::builder` takes erased handles: `.voice_manager(Arc<dyn ...>)`,
  `.event_handler(Arc::new(h))`, `.framework(Box::new(f))`. `client.shard_manager` is
  a plain `ShardManager`, not an `Arc` — use `get_shutdown_trigger()` for the
  Ctrl-C path.

### 2. Playlist resolution was serial

The play path threw away the metadata the playlist fetch had already returned and
re-resolved every entry from scratch, one at a time, spawning a `yt-dlp` subprocess
per track (`queue_vec_query_type` → `ready_query` →
`get_track_source_and_metadata`). Load time scaled linearly with playlist length and
a single `?` on a bad entry aborted the whole load.

Now: entries carry metadata straight from the listing. Anything that genuinely needs
resolving (keyword lists, Spotify tracks) goes through `resolve_track_many`, which
keeps `RESOLVE_CONCURRENCY` (8) lookups in flight via `buffered` — `buffered` not
`buffer_unordered`, because playlist order is user-visible. Keyword resolution no
longer follows its search hit with a redundant `get_info` round trip; that was
doubling the cost of every Spotify playlist track for metadata the search already
returned. The first track is queued alone so playback starts immediately, progress
edits are throttled to 3 s (Discord rate-limits edits per channel), unresolvable
entries are skipped and logged, and batch enqueues take the call lock once.

### 3. Playlists had stopped resolving entirely

**This is the finding that matters most.** `rusty_ytdl` 0.7.4 locates playlist
entries by looking for `playlistVideoRenderer` in `ytInitialData`. YouTube migrated
playlist listings to their view-model shape, so that lookup finds nothing and every
playlist fails with `PlaylistBodyCannotParsed`. Not slow — broken.

Reproduce:

```sh
curl -sS -A 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36' \
  'https://www.youtube.com/playlist?list=PLc1HPXyC5ookjUsyLkdfek0WUIGuGXRcP' -o pl.html
grep -c playlistVideoRenderer pl.html   # 0
grep -c lockupViewModel        pl.html  # >0
```

Continuations moved too: `continuationItemRenderer` → `continuationItemViewModel`,
with the token one `innertubeCommand` deeper.

`crack-testing/src/yt_playlist.rs` reads the playlist page directly and handles both
shapes, following continuations via `/youtubei/v1/browse` for playlists longer than
one page. `rusty_ytdl` stays as the fallback so we pick its richer results back up if
it's ever fixed upstream. There is no newer release to upgrade to — 0.7.4 is current
on crates.io.

Measured live: 8 entries 520 ms, 100 entries 730 ms (1 request), 300 entries 1.8 s
(3 requests). `rusty_ytdl` failed all three. `cargo run -p crack-testing --example
playlist_bench -- <url> <limit>` reproduces this.

## Verification

```sh
cargo build --release -p cracktunes
cargo clippy --workspace --all-targets
cargo test --workspace
```

Tests need Postgres. Without it, ~20 crack-core tests fail with `PoolTimedOut`.
With it, **120/120 crack-core pass**:

```sh
# any Postgres will do; docker-compose-postgres.yml also works
createdb cracktunes
export DATABASE_URL="postgres://postgres@127.0.0.1:5432/cracktunes"
cargo install sqlx-cli --no-default-features --features rustls,postgres
sqlx migrate run
cargo test --workspace
```

**Two tests fail on any bot-checked IP and this is not a code problem:**
`crack_testing::tests::test_resolve_track` and `test_enqueue_query`. Both hit the
innertube `player` endpoint, which from a datacenter IP returns:

```
playabilityStatus.status = LOGIN_REQUIRED
reason = "Sign in to confirm you're not a bot"
adaptiveFormats = 0
```

Confirmed against `WEB` (current and older client versions), `ANDROID` and `IOS`
contexts — all refused, so no client-string bump fixes it. They may well pass from a
residential IP. `test_cli2` (playlist resolution) **now passes** where it previously
failed; that one was the real bug.

Note the enqueue path no longer touches the player endpoint at all — only actual
playback does. So queueing a playlist works and displays correctly even under
bot-check conditions.

Remaining clippy noise is `mismatched_lifetime_syntaxes` (a newer rustc lint,
`ResolvedTrack` → `ResolvedTrack<'_>` in return position). Purely stylistic, left
alone deliberately to keep the diff attributable.

## GitHub access

Nothing is pushed. The sandbox had a working GitHub credential authenticated as
`cycle-five` — `GET /user` returned 200 — but a git proxy scoped it per-repo:

```
GET /repos/cycle-five/cracktunes  → 403
git push origin modernize-2026    → 403
remote: access denied by the git proxy: cycle-five/cracktunes is not in this
session's authorized repository set, so the proxy will not inject a credential
for it. To fix, add the repository to the session's sources.
```

Not a connector problem — there is no GitHub MCP connector installed, and GitHub
isn't in the connector registry. In Claude Code on your own machine with your own
credentials this is a non-issue.

## Pending issues to file

Two drafts, deliberately **not** in this branch — the PR stays scoped to the library
upgrade:

- `issue-playlist-limit.md` — make `PLAYLIST_PLAY_LIMIT` (currently 100) a guild
  setting and raise the default. Now ~600 ms per 100 tracks, so the limit is close
  to free. Lists what to sort out first: queue memory at scale, queue-embed
  pagination beyond four nav buttons, `MAX_CONTINUATIONS`, and the fact that Spotify
  playlists take a different (per-track) path and need separate measurement.
- `issue-lavalink-resolver.md` — add Lavalink as an alternative resolver, switchable
  at runtime, not a replacement. Key point: there are two seams. Metadata resolution
  is easy (`CrackTrackClient` is already the single chokepoint — a `TrackResolver`
  trait over its four methods is mechanical). Stream production is hard, because
  Lavalink replaces songbird's driver and terminates the voice connection itself, so
  "switch at runtime" realistically means per-guild at connect time. There's a
  tempting metadata-only split that probably doesn't work — the stream URL still has
  to be fetched from the bot-checked IP — worth confirming before designing around it.

## Deliberately out of scope

- Raising the playlist limit (issue above)
- Lavalink (issue above)
- `mismatched_lifetime_syntaxes` cleanup
- CI workflows still list a `nightly` matrix entry. Harmless —
  `rust-toolchain.toml` overrides it — but it's now pointless.
- The workspace-root `build.rs` is dead code (the workspace root is not a package)
  and still uses the vergen 8 `EmitBuilder` API. Left untouched.
