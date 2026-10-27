# Handoff: Lavalink as a playback source for CrackTunes (#398)

**Written:** 2026-10-27, by the session that fixed the YouTube `available_at` 403s
(v0.45.2 / v0.45.3).
**For:** the agent that picks up cycle-five/cracktunes#398.
**State when written:** master `6af3772b` (v0.45.3). Production and TuneTitan (the beta
bot) both run v0.45.3-coin, deployed 2026-10-27.

Read this whole file before touching code. Then read #398 itself, which is the owner's
own write-up and is still the best statement of the problem.

---

## 1. The job

Give CrackTunes a second way to play music: through a **Lavalink node** instead of
songbird's in-process driver plus yt-dlp. The node already exists (`lavalink-ops`), and so
does a working Rust client for it in another of the owner's bots (`bot-template-rs`).
What doesn't exist is the seam in CrackTunes that lets a guild's playback go through
Lavalink while everything built on top keeps working. That covers the queue, the audit
log, queue resume, the dashboard controls, the `/gp` game and the ops layer.

**This is a feature arc, not a bug fix.** Follow the owner's cadence:
brainstorm, then spec, then plan, then subagent-driven execution, then PR (the owner
merges). Start with a spike (§6). Do not start building before the owner has approved a
spec. There are real decisions in §5 that only the owner can make.

## 2. Why the owner wants it

- **The host's IP.** The owner, 2026-09-17, when an agent suggested closing #398: "right
  now this is technically running on a residential connection which is why yt-dlp works
  at all, should we ever need / want to run this on a datacenter ip, we will certainly
  need lavalink." From a datacenter IP, YouTube answers the player endpoint with
  `LOGIN_REQUIRED` ("Sign in to confirm you're not a bot"). #398 has the evidence.
  **#398 must stay open until this ships. Never propose closing it.**
- **The cat-and-mouse game.** On 2026-10-27 every YouTube track failed with 403, because
  YouTube began refusing a stream URL until its pre-roll ad would have ended. yt-dlp already
  handled this, by waiting for `available_at`. songbird re-implements yt-dlp's *download*
  half, so it didn't wait, and every track failed. The owner, that day: "Maybe it's just a
  cat and mouse game, and it's a constant break and fix. There's the whole lavalink-ops
  repo that we built exactly for this issue, and that's why there's still an open issue
  to use lavalink as a source." Lavalink's `youtube-source` plugin is maintained by
  people whose whole job is keeping up with YouTube.
- **An alternative, not a replacement** (#398). Direct resolution has lower latency, has
  no external dependency, and works fine from a residential IP. Which path is right
  depends on where the bot is deployed, so #398 asks for a *runtime* choice rather than a
  compile-time or fork-time one.

## 3. What already exists: reuse it, don't rebuild it

### 3.1 The node: `cycle-five/lavalink-ops` (public)

- **Stack:** Lavalink v4 (`ghcr.io/lavalink-devs/lavalink:4`), yt-cipher (remote signature
  deciphering), bgutil-pot (PoToken), an admin panel (FastAPI + htmx), and a
  docker-socket-proxy. Terraform modules exist for AWS, UpCloud and Vultr. Start with
  `README.md` and `docs/DESIGN.md`.
- **Plugins** (`config/application.yml`):
  - `dev.lavalink.youtube:youtube-plugin:1.18.0`
  - `com.github.topi314.lavasrc:lavasrc-plugin:4.8.1`
  - Built-in YouTube is off. bandcamp, soundcloud, twitch, vimeo and http are on.
- **Ports:** Lavalink listens on **2333**. In the stock layout the admin panel and the
  helpers are published to loopback only, so reaching the node from elsewhere is an infra
  question (§5.4).
- **Health:** `GET /healthz` (aggregate), `/healthz/lavalink`, `/healthz/cipher`,
  `/healthz/pot`.
- **A live instance:** an EC2 instance named `lavalink-ops` is referenced in homelab's
  snapcam design docs (2026-10-09), which give its instance id. Ask the owner whether it is
  running and how to reach it. Don't assume.
- **Hard-won facts.** These come from the lavalink-ops project's own notes, and their memory
  dir is readable at
  `~/.claude/projects/-home-lothrop-code-MusicBot-lavalink-ops2-lavalink-stack/memory/`:
  - **On EC2, OAuth is load-bearing.** Turn OAuth off and *every* YouTube interaction
    fails, search included. Use a burner Google account, never a primary one.
  - **PoToken applies only to the WEB and WEBEMBEDDED clients; OAuth applies only to TV.**
    They are parallel channels, not redundant ones.
  - **Search and direct URL loads hit different YouTube endpoints.** `ytsearch:` skips
    the playability check that a direct watch URL runs. So an age-gated video can load by
    search and fail by URL (compare CrackTunes #637, age-gated YouTube).
  - **youtube-source config trap:** per-client options live under
    `plugins.youtube.clientOptions.<CLIENT>`. The wrong path is silently ignored. Check the
    plugin README (`lavalink-devs/youtube-source`) for anything client-related. It changes
    between minor versions.

### 3.2 The client: `cycle-five/bot-template-rs`, a working reference

Local clone: `~/code/bot-template-rs`. Read `docs/ARCHITECTURE.md` (Lavalink backend
section), `docs/FORK_STACK.md`, `src/music_backend.rs` and `src/lavalink.rs`.

- **`MusicBackend` trait** (`src/music_backend.rs:102`) has two implementations:
  `LavalinkBackend` (`src/lavalink.rs`) and `NativeBackend` (songbird + yt-dlp,
  `src/native_backend.rs`). It has 26 methods: `on_ready`, `ensure_joined`, `leave`,
  `kind`, `play`, `play_url`, `skip`, `stop`, `clear`, `pause`, `resume`, `now_playing`,
  `queue_snapshot`, `shuffle`, `jump`, `move_track`, `remove_at`, `remove_duplicates`,
  `leave_cleanup`, `seek`, `set_volume`, `set_loop`, `get_loop`, `previous`, `set_filters`
  and `get_filters`. That list is a good checklist of what a backend has to cover.
- **Choosing a backend:** `MUSIC_BACKEND=native|lavalink`, global, at startup.
- **songbird and lavalink-rs in one process** (this answers #398's first open question).
  1. `songbird.join_gateway(guild, channel)` does only the voice-gateway handshake and
     returns `ConnectionInfo`.
  2. Those three fields are copied by hand into lavalink-rs's own `ConnectionInfo`, and
     `create_player_context` is called (`src/lavalink.rs` around line 330).
  3. From then on the node owns the voice UDP, and songbird is idle on that call.
- **Why the copy is by hand:** lavalink-rs's `songbird` feature pulls songbird 0.5, which
  would put a second songbird in the tree. Turn it off.
- **lavalink-rs fork:** `gitlab.com/cycle.five/lavalink-rs`, branch
  `bot-template-experiment` (0.15.0). Its `serenity-dep` points at
  `github.com/serenity-rs/serenity` branch `next`, **the same source CrackTunes uses**, so
  serenity should unify to one copy. Verify that with `cargo tree -d`.
- **Fallback for failed direct loads:** if a YouTube URL fails, get the title via oEmbed
  and retry as `ytsearch:` (see `LavalinkBackend::play` / `play_url`). Read why in
  `~/.claude/projects/-home-lothrop-code-bot-template-rs/memory/youtube_fallback_architecture.md`.
- **The requester survives the round trip** via the track's `user_data`.
- **TTS on Lavalink punches an inbound hole into the bot.** Discord allows one voice UDP
  session per guild, and Lavalink owns it, so the bot can't push bytes it generated
  itself. Instead it serves them over HTTP and hands Lavalink a URL to fetch
  (`docs/ARCHITECTURE.md`, "TTS routing"; `src/audio_http.rs`; `src/tts.rs`).
  - **How it works:** `AudioStore` keeps the bytes for 60 s. An embedded axum server
    serves `GET /audio/{id}` and `/health`. The node is given
    `${BOT_PUBLIC_URL}/audio/{id}`, where `BOT_PUBLIC_URL` is a tunnel (cloudflared) or a
    reverse-proxy hostname that faces the internet.
  - **The server is open.** It binds `BOT_HTTP_BIND_ADDR`, default `0.0.0.0:8080`, and
    the compose file publishes `8080:8080` on every host interface. `/audio/{id}` has no
    auth at all.
  - **The ids are guessable.** They come from a **sequential hex counter**
    (`format!("{:x}", counter.fetch_add(1))`): `0`, `1`, `2`, and so on. So anyone who
    knows the public URL can walk the ids and download whatever the bot spoke in the last
    60 s.
  - homelab's `docs/bot-ingress-contract.md` classes `GET /audio/{id}` as a machine route
    with "none [auth] for a capability URL". A counter is not a capability. That contract
    assumes ids nobody can guess, and this code doesn't provide them.
  - This is a bug in bot-template-rs. It isn't filed. Mention it to the owner; don't
    copy it.
- **One thing *not* to copy:** bot-template-rs uses **lavalink-rs's own queue** for the
  Lavalink backend. CrackTunes almost certainly can't (§5.2).

### 3.3 CrackTunes today: where things live

- **Dependencies** (workspace `Cargo.toml`):
  - serenity: `serenity-rs/serenity` branch `next` (lock `37b9f433`).
  - poise: `serenity-next`.
  - songbird: **our fork**, `CycleFive/songbird` branch `serenity-next-cracktunes`. The
    comment above it says why. Keep it: the native path needs those fixes.
- **Query → source:**
  - `QueryType` is at `crack-types/src/lib.rs:265`. Its variants: Keywords, KeywordList,
    VideoLink, SpotifyTracks, PlaylistLink, SpotifyLink, File, NewYoutubeDl, YoutubeSearch.
  - `get_track_source_and_metadata` (`crack-music/src/music/query.rs:459`) turns one into
    a songbird `Input` plus metadata.
  - `CrackTrackClient` (`crack-testing/src/lib.rs:98`, used by crack-music and crack-core)
    is the metadata resolver: `resolve_track`, `resolve_track_many`,
    `resolve_playlist_limit`, `resolve_search`. That is #398's "seam 1".
  - Spotify goes through sleevenote (`crack-sleevenote`).
  - `RetryRefused` (`crack-music/src/sources/retry.rs`) retries a refused stream once.
- **The queue is songbird's `TrackQueue`.** This is the central fact. Its primitives are
  in `crack-music/src/music/queue.rs`.
- **The ops layer** (`crack-music/src/music/ops/`) is the *only* place a user-initiated
  playback or queue change is orchestrated: lease, preconditions, mutation, audit row.
  Clippy enforces this: `clippy.toml` disallows the queue primitives everywhere else. Read
  the spec, `docs/superpowers/specs/2026-10-04-ops-layer-and-dashboard-controls-design.md`.
  A backend seam belongs **under** this layer, not at the command layer.
- **Built on the queue** (each has a spec in `docs/superpowers/specs/`):
  - the audit log (2026-09-30);
  - the dashboard queue and controls (2026-09-30, 2026-10-04);
  - queue resume (2026-10-09);
  - autoplay (`crack-music/src/music/autoplay.rs`);
  - the floating status message;
  - `/gp` playback (`crack-core/src/commands/music/gp/`);
  - economy walk-on clips (`crack-core/src/economy/walk_on.rs`, which "reads songbird,
    the queue and `Data`").
- **Voice join:** `crack-core/src/commands/music_utils.rs`, around lines 130–170. It
  mirrors `Songbird::join_inner`. Read the 🪤 comment at line 96 about a connectionless
  `Call` first.
- **Track events:**
  - `crack-core/src/handlers/track_end.rs` and `music_utils.rs`
    (`set_global_handlers_with`);
  - `handlers/guarded.rs`;
  - `commands/music/queue.rs` and `stop.rs`;
  - `gp/playback.rs`.
- **Coupling, measured 2026-10-27:** lines mentioning songbird types (`songbird::`,
  `TrackQueue`, `TrackHandle`, `Call`, `Songbird`):
  - crack-core: about 270 lines in 34 files;
  - crack-music: about 180 lines in 27 files;
  - crack-rooms: 149 lines in 4 files. Check how much of that is really songbird.
  - The owner's 2026-10-22 Matrix investigation reached the same conclusion: "songbird
    owns the queue". See the project memory `matrix-platform-seam.md`.

## 4. Decisions already made (by the owner)

- #398 stays open. Lavalink is the planned answer for a datacenter host, not a
  speculative one.
- It is an **alternative** to the direct path, chosen at **runtime** (#398). The direct
  path stays, and stays the default while the bot runs on the residential bots VM.
- Production runs on the homelab bots VM (residential ISP). Nothing about Lavalink changes
  that by itself.

## 5. Decisions *not* made: take these to the owner

Bring each of these to the owner through brainstorming, with the spike's evidence, and
put the answers in the spec.

1. **How granular is the switch?**
   - Global at startup is what bot-template-rs does, and is the simplest.
   - Per guild, chosen at connect time, is what #398 sketches.
   - Automatic fallback is a third option, for example when yt-dlp is refused.
   - The backend can't change mid-track, and probably not while connected. My read: a
     seam that *allows* per-guild, shipped first as a global setting, if the owner agrees.
2. **Who owns the queue? This is the crux.**
   - Today the queue *is* songbird's `TrackQueue`. On a Lavalink guild songbird's driver
     is idle, so `TrackQueue` can't hold that guild's queue.
   - Option (a): a queue abstraction with a songbird implementation and a Lavalink
     implementation, both under `ops`.
   - Option (b): one queue model of our own, which drives either songbird or Lavalink one
     track at a time. #398 leans this way: "keep ours and drive Lavalink one track at a
     time".
   - Option (c): adopt lavalink-rs's queue, as bot-template-rs does. That gives two
     sources of truth, which #398 calls "a bug factory".
   - Everything in §3.3's "built on the queue" list depends on the answer.
3. **Feature parity.** List each feature against each backend for the owner.
   - Lavalink supports seek, volume and filters natively.
   - Check: queue resume (it seeks), autopause, autoplay, `/gp`, walk-on clips (songbird
     can't play into a Lavalink-owned call: relay or skip?), the status message,
     now-playing, voice receive (v0.30.1 joins self-deafened and decodes nothing).
   - Decide which differences are acceptable as documented behaviour.
4. **Where does the node run, and how does the bot reach it?**
   - Port 2333 on an EC2 host isn't reachable from the homelab as deployed. Options
     include a tunnel, or a node inside the homelab (which is residential again, so only
     the maintenance benefit remains).
   - Read homelab `docs/bot-ingress-contract.md` around line 81. Putting SSO in front of a
     node breaks it.
   - The node's password is a secret: it goes in the gitignored env files, never in a
     committed file or in output.
5. **Which sources go through Lavalink?** YouTube only, or SoundCloud and Spotify too
   (LavaSrc could overlap with sleevenote)? Keep the scope small unless the owner widens it.
6. **Does CrackTunes need an inbound audio relay at all?**
   - The relay is only needed for audio the bot produces itself, which today means
     walk-on clips. Attachments and links are URLs the node can fetch directly.
   - Option: don't build one, and document that walk-ons don't play on Lavalink.
   - If the owner wants one, it must not repeat bot-template-rs's version (§3.2):
     - ids carry at least 128 random bits, or the URL is HMAC-signed with an expiry;
     - entries are short-lived and single-use where possible;
     - the server binds only where the tunnel reaches it, not `0.0.0.0` on a published
       port;
     - the hostname is a machine-class one (homelab's bot-ingress-contract names
       it), **never behind Cloudflare Access**. A node can't complete an SSO redirect, and it fails silently
       (bot-ingress-contract; `edge/verify.sh` assertion 4 enforces this).

## 6. Suggested path

**Phase 0: spike.** Throwaway, on a branch, not merged. Answer these with evidence:

- Does lavalink-rs (the fork) build inside CrackTunes' dependency graph?
  - Features: `default-features = false`, plus `serenity`,
    `tungstenite-rustls-webpki-roots` and `macros`. **Not** `songbird`.
  - `cargo tree -d` should show one serenity and one songbird.
  - Never use `cargo update -p songbird`. It drags serenity forward and fails on a missing
    `chrono` feature. Edit the lock, or let `cargo metadata` re-resolve minimally.
- Can TuneTitan join a channel via `join_gateway`, hand the session to the node, and play
  one track?
- **DAVE.** bot-template-rs's `Cargo.toml` says Discord is making DAVE (voice E2EE)
  mandatory, and that songbird 0.6 is needed for it. With Lavalink the *node* terminates
  the voice connection, so the node's Lavalink version has to support DAVE. The image tag
  is a floating `:4`. Check the running version, and confirm that audio actually plays in
  a DAVE-enforced channel.
- Run `loadtracks` for a search, a direct URL and an age-gated video against the node, and
  record what comes back.

**Phase 1: brainstorm and write the spec** with the owner (superpowers:brainstorming),
using the spike's findings and §5. The spec goes in
`docs/superpowers/specs/YYYY-MM-DD-lavalink-backend-design.md`.

**Phase 2: plan and execute** (superpowers:writing-plans, then
superpowers:subagent-driven-development). Likely shape, for the spec to confirm or reject:

- a backend trait **under `music::ops`**, modelled on bot-template-rs's `MusicBackend` but
  keeping CrackTunes' queue semantics;
- #398's seam 1, a `TrackResolver` trait over `CrackTrackClient`'s four methods, with a
  Lavalink implementation. This half is mechanical;
- a config and guild setting for the backend, plus a startup health check that falls back
  to direct with a warning instead of failing to start (#398);
- the title-search fallback for failed direct YouTube loads.

This is a feature, so it's a **minor** version bump.

**Phase 3: TuneTitan, then production.**
- TuneTitan first, from a branch image (`docker.yml` workflow_dispatch builds
  `cracktunes:<branch>-coin` and `cracktunes-migrate:<branch>`).
- The owner plays tracks and confirms before merging.
- Production only on the owner's explicit go, after a fresh backup.

## 7. Traps

- **lavalink-rs's `songbird` feature** puts a second songbird in the tree. Turn it off and
  convert `ConnectionInfo` by hand.
- **`cargo update -p songbird`** moves serenity too (see Phase 0).
- **One Discord token, two processes:** slash commands register per *application*, so two
  processes on one token clobber each other's commands. Never run CrackTunes elsewhere with
  production's token. See the memory `cracktunes-bots-vm-deployment.md`.
- **A Lavalink-owned call can't take bytes from songbird.** Anything that plays a clip
  directly (walk-on themes) needs a relay, which is an inbound hole (§5.6), or must be
  documented as unavailable on Lavalink.
- **The node fetches whatever URL we give it, from its own network.** lavalink-ops enables
  Lavalink's `http` source. CrackTunes refuses links to loopback, private, link-local,
  CGNAT and metadata addresses (CHANGELOG, Security). That check has to run **before** a
  URL is handed to the node, because the node's network is not ours: on EC2,
  `169.254.169.254` is the instance metadata service. Keep the guard on the bot side, and
  confirm the instance requires IMDSv2.
- **On EC2, YouTube needs OAuth**, and direct URLs fail where search succeeds (§3.1).
- **Don't hammer YouTube from the home IP.** The owner: "Don't hammer it too hard with the
  tests." Make single probes, never loops.
- **No live network tests in the default suite.** Anything that reaches YouTube, Lavalink
  or Discord gets `#[ignore = "<what it hits>"]`. On 2026-10-27 the owner had a test
  deleted because "it tests nothing other than that youtube is up". Test the seam with
  fakes, the way `crack-music/src/sources/ytdl.rs` tests the songbird fork with a stand-in
  yt-dlp.
- **Typed serde only:** no `serde_json::json!` or `Value` for data we own. lavalink-rs
  has typed models. If you call the REST API directly, define structs.
- **Explicit imports**, enforced by clippy `wildcard_imports` and `enum_glob_use` since
  v0.45.1. The workspace denies clippy `all`.
- **Logging:** never `{:?}` something that holds a signed stream URL or a node password.
  v0.45.3 had to give songbird's kept resolve a hand-written `Debug`, because `/play` logs
  its query with `{:?}`.
- **The workspace builds with `--locked` in CI.** Commit the lock.

## 8. Working rules

Most of these are in the owner's global `~/.claude/CLAUDE.md` and this project's memory
(`MEMORY.md`, `cracktunes-standing-rules.md`), which load automatically. The ones that
cost the most when missed:

- **Commits:** end with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`,
  and no other Co-Authored-By line. Never `git add -A`.
- **Worktrees** go outside the repo (`~/projects/cracktunes-<name>`).
- **Merging and releasing:** the owner merges PRs. Never `gh pr merge --delete-branch`.
  Never `gh release create`: cargo-dist creates the release when a tag is pushed.
- **Cargo:** run every local `cargo` / `nextest` / `clippy` under
  `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`.
  Never put that prefix in a zsh variable. Never `cargo clean` the shared
  `~/.cache/cargo-target`.
- **Docker:** never build images locally, because container DNS is broken here. Bare
  `docker` targets a remote host; use `--context default` for local containers.
- **Test DB:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres` with
  `SQLX_OFFLINE=true`. The `pg` container is `--rm`; how to restart it is in
  `cracktunes-standing-rules.md`.
- **Secrets:** `.env.tunetitan` and `.env.bots` hold secrets. Edit their image pins in
  place, never print them, and never `bash -n` them. The crack-coin signing key is never
  printed, anywhere.
- **Subagents:** Sonnet for implementers and task reviews, Opus only for the final
  whole-branch review. Name the model on every dispatch.
- **Upstream PRs:** before opening a PR or issue on someone else's project, read its
  contribution rules *and its org's* for an AI policy, and tell the owner first. The
  serenity-rs org forbids AI-written code and prose. lavalink-rs, Lavalink and
  youtube-source may have rules too.

## 9. Reading order

1. cycle-five/cracktunes#398.
2. This file, §3.3, then the ops-layer spec
   (`docs/superpowers/specs/2026-10-04-ops-layer-and-dashboard-controls-design.md`).
3. bot-template-rs: `docs/ARCHITECTURE.md`, `docs/FORK_STACK.md`, `src/music_backend.rs`,
   `src/lavalink.rs`.
4. lavalink-ops: `README.md`, `docs/DESIGN.md`, `config/application.yml`, and the
   lavalink-ops project memory dir (§3.1).
5. CrackTunes memory: `youtube-stream-available-at.md` (why songbird + yt-dlp keeps
   breaking), `lavalink-still-planned.md`, `matrix-platform-seam.md`,
   `tunetitan-beta-tenant.md`.
6. The queue-resume and audit-log specs, before §5.2's queue discussion.
