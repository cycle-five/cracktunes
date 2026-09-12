# Music Permission Diagnosis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make cracktunes refuse a voice join it knows Discord will silently drop, say exactly which permission is missing and where, degrade gracefully when it cannot post in a text channel, and answer `/diagnose` with the full picture.

**Architecture:** One pure function (`compute`) turns raw `Permissions` bitsets into a `MusicPermissions` value; a cache-reading `resolve` feeds it, and `ensure_can_join` is a narrow gate placed immediately before every `songbird.join` call. Voice permissions block; text permissions only degrade, surfacing as a field on the reply the command was already sending. `/diagnose` renders the same `MusicPermissions` value the gate uses.

**Tech Stack:** Rust 2021, poise (git, branch `serenity-next`), serenity-next, songbird, sqlx 0.8 (offline), tokio.

**Spec:** `docs/superpowers/specs/2026-09-12-music-permission-diagnosis-design.md`

## Global Constraints

- **Commit trailer.** Every commit MUST end with exactly this line and **no other** `Co-Authored-By` line:
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- **The gate, before every commit**, all four, in this order:
  - `cargo fmt --all -- --check`
  - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
  - `SQLX_OFFLINE=true cargo check`
  - `cargo test --workspace`
- **`SQLX_OFFLINE=true cargo check` is mandatory, not optional.** CI's `Docker` job is skipped on `pull_request` (#424) and the `Build` job stands up a real Postgres, so a `query!` macro with no `.sqlx` entry passes CI and breaks the image build. This plan adds no queries, but run it anyway.
- **One pre-existing test failure is expected:** `crack-testing::tests::test_enqueue_query` (a deleted YouTube video id). It fails on `master` too. It is **not** a regression and **not** something to fix here; open PR ct#471 covers it.
- **Never use `serde_json::json!` or `serde_json::Value`** for data we own. Typed structs/enums with derives only.
- **Nine workspace members each hold a literal `version`.** There is no `[workspace.package] version`, so a partial bump compiles clean and is invisible. All nine end this plan at `0.9.7`.
- **Do not run `gh release create`.** For this repo the tag push IS the release (cargo-dist owns it).
- Fail-open on a cache miss, everywhere. A permission check that cannot read the cache returns "fine", never "refused". A false refusal is worse than the status quo it replaces.

---

## File Structure

| file | responsibility |
|---|---|
| `crack-core/src/music/perms.rs` **(new)** | The whole permission model: `MusicPermissions`, `TextPerms`, `VoicePerms`, the two required-permission constants, pure `compute`, cache-reading `resolve`, and the `ensure_can_join` gate. Self-contained; no other module needs to know how a permission is computed. |
| `crack-core/src/commands/music/diagnose.rs` **(new)** | The `/diagnose` command and its three render states. Presentation only — it calls `resolve` and formats. |
| `crack-core/src/music/mod.rs` | Register the module. |
| `crack-core/src/errors.rs` | `CrackedError::MissingBotPermissions`, `PermScope`, and the `Display` arm. |
| `crack-core/src/messaging/messages.rs` | Copy constants, following the existing `pub const NAME: &str = "..."` pattern. |
| `crack-core/src/commands/music_utils.rs` | Gate `do_join`. |
| `crack-core/src/commands/music/gp_persist.rs` | Gate the restart-resume join; log-channel fallback. |
| `crack-core/src/poise_ext.rs` | Gate `join_vc`. |
| `crack-core/src/commands/music/mod.rs` | Register `diagnose()` in `music_commands()`. |
| `crack-core/src/commands/music/doplay.rs` | The degraded-permissions field on the play reply. |

---

## Task 1: The permission model

**Files:**
- Create: `crack-core/src/music/perms.rs`
- Modify: `crack-core/src/music/mod.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `MusicPermissions`, `TextPerms`, `VoicePerms`, `TEXT_REQUIRED`, `VOICE_REQUIRED`, and `pub fn compute(text_channel: GenericChannelId, text_granted: Permissions, voice: Option<(ChannelId, Permissions)>) -> MusicPermissions`. Tasks 3, 5 and 6 all depend on these exact names.

**Note on the spec.** The spec sketches `TextPerms { view: bool, send: bool, embed: bool }`. Store the raw `granted: Permissions` bitset instead and derive the booleans through accessors. This is a refinement, not a departure: it makes `missing()` a single subtraction, and `serenity::Permissions` already implements `Display` as comma-separated names, so the error copy and the `/diagnose` output get their wording for free instead of re-deriving it from three booleans.

- [ ] **Step 1: Write the failing tests**

Create `crack-core/src/music/perms.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::{ChannelId, GenericChannelId};

    fn text_ch() -> GenericChannelId {
        GenericChannelId::new(1)
    }
    fn voice_ch() -> ChannelId {
        ChannelId::new(2)
    }

    #[test]
    fn everything_granted_is_whole_and_joinable() {
        let p = compute(text_ch(), TEXT_REQUIRED, Some((voice_ch(), VOICE_REQUIRED)));
        assert!(p.text.is_whole());
        assert!(p.text.missing().is_empty());
        let v = p.voice.expect("voice was supplied");
        assert!(v.can_join());
        assert!(v.missing().is_empty());
    }

    #[test]
    fn a_missing_speak_blocks_the_join_and_names_only_speak() {
        let granted = VOICE_REQUIRED - Permissions::SPEAK;
        let p = compute(text_ch(), TEXT_REQUIRED, Some((voice_ch(), granted)));
        let v = p.voice.expect("voice was supplied");
        assert!(!v.can_join());
        assert_eq!(v.missing(), Permissions::SPEAK);
        assert!(v.connect(), "CONNECT was granted and must not be reported missing");
        assert!(!v.speak());
    }

    #[test]
    fn both_voice_perms_missing_are_reported_together() {
        let p = compute(text_ch(), TEXT_REQUIRED, Some((voice_ch(), Permissions::empty())));
        let v = p.voice.expect("voice was supplied");
        assert_eq!(v.missing(), VOICE_REQUIRED);
        // The copy is built from Display on the whole set, so both names must
        // appear. This is the property the refusal message depends on.
        let rendered = format!("{}", v.missing());
        assert!(rendered.contains("Connect"), "got {rendered}");
        assert!(rendered.contains("Speak"), "got {rendered}");
    }

    #[test]
    fn a_missing_embed_links_degrades_text_but_leaves_voice_joinable() {
        let granted = TEXT_REQUIRED - Permissions::EMBED_LINKS;
        let p = compute(text_ch(), granted, Some((voice_ch(), VOICE_REQUIRED)));
        assert!(!p.text.is_whole());
        assert_eq!(p.text.missing(), Permissions::EMBED_LINKS);
        assert!(p.text.view() && p.text.send() && !p.text.embed());
        assert!(p.voice.expect("voice was supplied").can_join());
    }

    #[test]
    fn no_voice_channel_is_none_not_a_denial() {
        let p = compute(text_ch(), TEXT_REQUIRED, None);
        // 🪤 `None` means "the author is in no voice channel", which is a
        // different thing from "in a channel we cannot join". Rendering it as
        // a denial would tell someone to grant a permission that is already
        // granted.
        assert!(p.voice.is_none());
        assert!(p.text.is_whole());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p crack-core --lib music::perms`
Expected: FAIL to compile — `compute`, `TEXT_REQUIRED`, `VOICE_REQUIRED` are not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crack-core/src/music/perms.rs`, above the test module:

```rust
//! What the music commands need from Discord, and whether we have it.
//!
//! Permissions here fall into two tiers that behave differently:
//!
//! - **Blocking** ([`VOICE_REQUIRED`]) — without these a join cannot work.
//!   Discord accepts the voice state update and silently does nothing with
//!   it, so songbird waits out its timeout and reports `JoinError::TimedOut`,
//!   which names nothing useful. [`ensure_can_join`] refuses first instead.
//! - **Degrading** ([`TEXT_REQUIRED`]) — without these the bot still plays
//!   perfectly; it just cannot announce. Playback needs no text permission at
//!   all, so refusing to play because we cannot post the now-playing message
//!   would be strictly worse for the user than playing silently.

use poise::serenity_prelude as serenity;
use serenity::all::{ChannelId, GenericChannelId, Permissions};

/// Text permissions the now-playing posts need. Missing any of these degrades
/// the bot; it never blocks it.
pub const TEXT_REQUIRED: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::SEND_MESSAGES)
    .union(Permissions::EMBED_LINKS);

/// Voice permissions a join needs. Missing either blocks playback outright.
pub const VOICE_REQUIRED: Permissions = Permissions::CONNECT.union(Permissions::SPEAK);

/// The bot's permissions as they bear on playing music in one guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicPermissions {
    pub text: TextPerms,
    /// `None` when the author is in no voice channel at all — deliberately a
    /// distinct state from "in a channel we cannot join", because the two
    /// need different messages.
    pub voice: Option<VoicePerms>,
}

/// What the bot may do in the channel a command was invoked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPerms {
    pub channel: GenericChannelId,
    pub granted: Permissions,
}

/// What the bot may do in the voice channel it would join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePerms {
    pub channel: ChannelId,
    pub granted: Permissions,
}

impl TextPerms {
    /// The subset of [`TEXT_REQUIRED`] we do not have. Renders itself as
    /// comma-separated permission names via serenity's `Display`.
    pub fn missing(&self) -> Permissions {
        TEXT_REQUIRED - self.granted
    }
    pub fn is_whole(&self) -> bool {
        self.missing().is_empty()
    }
    pub fn view(&self) -> bool {
        self.granted.contains(Permissions::VIEW_CHANNEL)
    }
    pub fn send(&self) -> bool {
        self.granted.contains(Permissions::SEND_MESSAGES)
    }
    pub fn embed(&self) -> bool {
        self.granted.contains(Permissions::EMBED_LINKS)
    }
}

impl VoicePerms {
    /// The subset of [`VOICE_REQUIRED`] we do not have.
    pub fn missing(&self) -> Permissions {
        VOICE_REQUIRED - self.granted
    }
    pub fn can_join(&self) -> bool {
        self.missing().is_empty()
    }
    pub fn connect(&self) -> bool {
        self.granted.contains(Permissions::CONNECT)
    }
    pub fn speak(&self) -> bool {
        self.granted.contains(Permissions::SPEAK)
    }
}

/// Build the permission picture from already-resolved bitsets.
///
/// Pure on purpose: no cache, no `await`, no poise `Context`. Every case worth
/// testing is a synthetic [`Permissions`] value, so the model is covered
/// without Discord, a network, or an async runtime. [`resolve`] is the thin
/// layer that reads the cache and calls this.
pub fn compute(
    text_channel: GenericChannelId,
    text_granted: Permissions,
    voice: Option<(ChannelId, Permissions)>,
) -> MusicPermissions {
    MusicPermissions {
        text: TextPerms {
            channel: text_channel,
            granted: text_granted,
        },
        voice: voice.map(|(channel, granted)| VoicePerms { channel, granted }),
    }
}
```

If `Permissions::union` turns out not to be `const` in this serenity revision, write the constants as
`Permissions::from_bits_truncate(Permissions::CONNECT.bits() | Permissions::SPEAK.bits())`
rather than making them non-`const` statics or runtime functions.

- [ ] **Step 4: Register the module**

In `crack-core/src/music/mod.rs`, add `pub mod perms;` alongside the existing `pub mod context;` / `pub mod lease;` lines, keeping alphabetical order:

```rust
pub mod context;
pub mod lease;
pub mod perms;
pub(crate) mod query;
pub(crate) mod queue;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p crack-core --lib music::perms`
Expected: PASS, 5 tests.

- [ ] **Step 6: Run the full gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo check
cargo test --workspace
git add crack-core/src/music/perms.rs crack-core/src/music/mod.rs
git commit -F - <<'EOF'
feat(perms): model the two permission tiers as one pure function

Voice perms block, text perms degrade -- playback needs no text
permission at all, so refusing to play because we cannot announce the
song would be strictly worse than playing silently.

`compute` takes resolved bitsets rather than a Guild, so every case
worth testing is a synthetic Permissions value with no Discord, no
network and no async runtime. Storing the granted set rather than three
booleans means `missing()` is one subtraction and serenity's Display
supplies the permission names for free.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

## Task 2: The typed refusal

**Files:**
- Modify: `crack-core/src/errors.rs`
- Modify: `crack-core/src/messaging/messages.rs`

**Interfaces:**
- Consumes: nothing from Task 1 (kept independent so it can be reviewed alone).
- Produces: `CrackedError::MissingBotPermissions { scope: PermScope, channel: GenericChannelId, missing: Permissions }` and `pub enum PermScope { Text, Voice }`, both exported from `crack_core::errors`. Tasks 3, 4 and 6 depend on these.

**Note.** The variant carries `GenericChannelId` for both scopes. A voice `ChannelId` widens with `.widen()` (the same call `music_utils.rs` already uses), so one variant covers both tiers without a second type.

- [ ] **Step 1: Write the failing test**

Append to the test module at the bottom of `crack-core/src/errors.rs` (create `#[cfg(test)] mod tests { use super::*; ... }` if the file has none):

```rust
#[test]
fn a_voice_refusal_names_every_missing_permission_not_just_the_first() {
    let err = CrackedError::MissingBotPermissions {
        scope: PermScope::Voice,
        channel: GenericChannelId::new(42),
        missing: Permissions::CONNECT | Permissions::SPEAK,
    };
    let rendered = format!("{err}");
    // 🪤 The bug worth guarding is reporting only the first missing
    // permission: someone grants Connect, tries again, and is refused for
    // Speak with no warning it was also missing.
    assert!(rendered.contains("Connect"), "got {rendered}");
    assert!(rendered.contains("Speak"), "got {rendered}");
    assert!(rendered.contains("<#42>"), "must name the channel: {rendered}");
}

#[test]
fn a_text_refusal_reads_differently_from_a_voice_one() {
    let voice = format!(
        "{}",
        CrackedError::MissingBotPermissions {
            scope: PermScope::Voice,
            channel: GenericChannelId::new(1),
            missing: Permissions::SPEAK,
        }
    );
    let text = format!(
        "{}",
        CrackedError::MissingBotPermissions {
            scope: PermScope::Text,
            channel: GenericChannelId::new(1),
            missing: Permissions::EMBED_LINKS,
        }
    );
    assert_ne!(
        voice, text,
        "a blocking refusal and a degradation notice must not read alike"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p crack-core --lib errors`
Expected: FAIL to compile — no variant `MissingBotPermissions`, no `PermScope`.

- [ ] **Step 3: Add the copy constants**

In `crack-core/src/messaging/messages.rs`, add these beside the other `FAIL_*` and play constants (the file is roughly alphabetical; put them near `FAIL_INVALID_PERMS`):

```rust
pub const FAIL_MISSING_VOICE_PERMS: &str = "❌ I can't play in";
pub const FAIL_MISSING_TEXT_PERMS: &str = "⚠️ I can't post in";
pub const MISSING_PERMS_FIX: &str = "Ask an admin to grant it, then try again.";
```

- [ ] **Step 4: Add the variant, the scope enum, and the Display arm**

In `crack-core/src/errors.rs`:

```rust
/// Which tier a missing permission belongs to. The two are reported
/// differently because they mean different things: a `Voice` refusal stopped
/// the command, a `Text` one only limited what it could say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermScope {
    Text,
    Voice,
}
```

Add to the `CrackedError` enum, keeping the existing rough alphabetical placement (next to `InvalidPermissions`):

```rust
    MissingBotPermissions {
        scope: PermScope,
        channel: GenericChannelId,
        missing: Permissions,
    },
```

Add to the `Display` impl (`errors.rs:138`), matching the surrounding style:

```rust
            Self::MissingBotPermissions {
                scope,
                channel,
                missing,
            } => {
                let lead = match scope {
                    PermScope::Voice => FAIL_MISSING_VOICE_PERMS,
                    PermScope::Text => FAIL_MISSING_TEXT_PERMS,
                };
                // `missing` renders as comma-separated names via serenity's
                // own Display, so a two-permission refusal names both without
                // any joining logic here.
                f.write_fmt(format_args!(
                    "{} {} — I'm missing **{}** there.\n\n{}",
                    lead,
                    channel.mention(),
                    missing,
                    MISSING_PERMS_FIX
                ))
            },
```

Ensure the needed imports are present at the top of `errors.rs`: `serenity::all::{GenericChannelId, Permissions}`, the `Mentionable` trait for `.mention()`, and the three new constants from `crate::messaging::messages`.

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p crack-core --lib errors`
Expected: PASS.

- [ ] **Step 6: Run the full gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo check
cargo test --workspace
git add crack-core/src/errors.rs crack-core/src/messaging/messages.rs
git commit -F - <<'EOF'
feat(perms): a typed refusal that names every missing permission

One variant covers both tiers by widening a voice ChannelId, and
PermScope keeps the two readings apart: a Voice refusal stopped the
command, a Text one only limited what it could say.

🪤 The guarded bug is naming only the first missing permission -- grant
Connect, try again, get refused for Speak with no warning it was also
missing. Deferring to serenity's Display on the whole set makes that
impossible to reintroduce.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

## Task 3: Cache resolution and the gate

**Files:**
- Modify: `crack-core/src/music/perms.rs`

**Interfaces:**
- Consumes: Task 1's `compute`, `MusicPermissions`, `VOICE_REQUIRED`; Task 2's `CrackedError::MissingBotPermissions` and `PermScope`.
- Produces:
  - `pub fn resolve(cache: &Cache, guild_id: GuildId, text_channel: GenericChannelId, author: UserId) -> Option<MusicPermissions>`
  - `pub fn ensure_can_join(cache: &Cache, guild_id: GuildId, channel_id: ChannelId) -> Result<(), CrackedError>`

  Task 4 calls `ensure_can_join`; Tasks 5 and 6 call `resolve`.

**Serenity APIs this uses** (verified against the pinned revision — do not guess at these):
- `Cache::guild(GuildId) -> Option<GuildRef<'_>>`
- `Cache::current_user() -> CurrentUserRef<'_>` (`.id` for the bot's `UserId`)
- `Guild::members: ExtractMap<UserId, Member>`
- `Guild::channels: ExtractMap<ChannelId, GuildChannel>`
- `Guild::voice_states: ExtractMap<UserId, VoiceState>`, and `VoiceState::channel_id: Option<ChannelId>`
- `Guild::user_permissions_in(&self, channel: &GuildChannel, member: &Member) -> Permissions` — note it takes a **`&GuildChannel`, not an id**, so the channel must be looked up in `guild.channels` first
- `GenericChannelId::expect_channel() -> ChannelId` (already used at `poise_ext.rs:580`)
- `ChannelId::widen() -> GenericChannelId`

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `crack-core/src/music/perms.rs`:

```rust
    // `resolve` and `ensure_can_join` read a live serenity Cache, which cannot
    // be constructed meaningfully offline, so their *logic* is tested through
    // `compute` above and their *placement* by the source-scan guard in
    // Task 4. What is tested here is the one decision that is neither:
    // what they do when the cache cannot answer.

    #[test]
    fn the_gate_fails_open_when_the_cache_is_empty() {
        let cache = serenity::all::Cache::new();
        // No guild cached, so nothing can be known about permissions.
        let res = ensure_can_join(&cache, serenity::all::GuildId::new(1), voice_ch());
        // 🪤 Fail OPEN, not closed. A cache miss means "we don't know", and
        // refusing a join we could have made would be a worse bug than the
        // timeout this gate exists to replace -- it would break working
        // guilds during the cache warm-up after every restart.
        assert!(
            res.is_ok(),
            "a cache miss must not refuse a join: {:?}",
            res.err().map(|e| e.to_string())
        );
    }

    #[test]
    fn resolving_an_uncached_guild_yields_none() {
        let cache = serenity::all::Cache::new();
        let got = resolve(
            &cache,
            serenity::all::GuildId::new(1),
            text_ch(),
            serenity::all::UserId::new(7),
        );
        assert!(got.is_none(), "an uncached guild has no answer to give");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p crack-core --lib music::perms`
Expected: FAIL to compile — `resolve` and `ensure_can_join` are not defined.

- [ ] **Step 3: Write the implementation**

Add to `crack-core/src/music/perms.rs`, after `compute`. Extend the imports at the top of the file to
`use serenity::all::{Cache, ChannelId, GenericChannelId, GuildId, Permissions, UserId};`
and add `use crate::errors::{CrackedError, PermScope};`.

```rust
/// Read the bot's permissions out of the cache.
///
/// Returns `None` when the guild, the bot's own member, or the text channel is
/// not cached — callers treat that as "assume fine" rather than refusing.
///
/// No HTTP on this path. `GatewayIntents::GUILD_MEMBERS` is enabled
/// (`config.rs:321`), so the bot's own `Member` is cached and permissions
/// resolve locally. That matters because [`ensure_can_join`] runs on every
/// join, and a per-join round trip would be a latency and rate-limit
/// regression.
pub fn resolve(
    cache: &Cache,
    guild_id: GuildId,
    text_channel: GenericChannelId,
    author: UserId,
) -> Option<MusicPermissions> {
    let bot_id = cache.current_user().id;
    let guild = cache.guild(guild_id)?;
    let bot = guild.members.get(&bot_id)?;

    let text_chan = guild.channels.get(&text_channel.expect_channel())?;
    let text_granted = guild.user_permissions_in(text_chan, bot);

    // The author's voice channel, if they are in one. Absent is not a denial.
    let voice = guild
        .voice_states
        .get(&author)
        .and_then(|vs| vs.channel_id)
        .and_then(|cid| {
            let chan = guild.channels.get(&cid)?;
            Some((cid, guild.user_permissions_in(chan, bot)))
        });

    Some(compute(text_channel, text_granted, voice))
}

/// The blocking gate: refuse a join Discord would silently drop.
///
/// Call this immediately before every `songbird.join`. It takes cache handles
/// rather than a poise `Context` on purpose — one of its call sites is a
/// restart-resume with no invoking user and therefore no `Context`.
///
/// Fails **open** on a cache miss: an unknown permission state must not refuse
/// a join that would have worked.
pub fn ensure_can_join(
    cache: &Cache,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<(), CrackedError> {
    let Some(guild) = cache.guild(guild_id) else {
        return Ok(());
    };
    let bot_id = cache.current_user().id;
    let Some(bot) = guild.members.get(&bot_id) else {
        return Ok(());
    };
    let Some(chan) = guild.channels.get(&channel_id) else {
        return Ok(());
    };

    let missing = VOICE_REQUIRED - guild.user_permissions_in(chan, bot);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CrackedError::MissingBotPermissions {
            scope: PermScope::Voice,
            channel: channel_id.widen(),
            missing,
        })
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p crack-core --lib music::perms`
Expected: PASS, 7 tests.

- [ ] **Step 5: Run the full gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo check
cargo test --workspace
git add crack-core/src/music/perms.rs
git commit -F - <<'EOF'
feat(perms): resolve from cache, and gate the join

ensure_can_join takes cache handles rather than a poise Context because
one of its three call sites is gp_persist's restart-resume, which has no
invoking user.

🪤 Both fail OPEN on a cache miss. Refusing a join we could have made
would be a worse bug than the timeout this replaces -- it would break
working guilds during cache warm-up after every restart.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

## Task 4: Gate every join site, and pin that they stay gated

**Files:**
- Modify: `crack-core/src/commands/music_utils.rs` (around line 144)
- Modify: `crack-core/src/commands/music/gp_persist.rs` (around line 615)
- Modify: `crack-core/src/poise_ext.rs` (around line 699)
- Test: new `#[cfg(test)] mod join_site_guard_tests` in `crack-core/src/music/perms.rs`

**Interfaces:**
- Consumes: Task 3's `ensure_can_join`.
- Produces: no new API. All three join sites are gated.

**The three sites** (`grep -rn '\.join(' --include='*.rs' crack-core/src`, excluding string joins):

| site | shape |
|---|---|
| `commands/music_utils.rs:144` — `do_join` | returns `Result<_, Error>`; the error propagates out of the command body and `FrameworkError::Command` (`config.rs:48`) replies |
| `commands/music/gp_persist.rs:615` | restart-resume; returns `()`; nobody to reply to |
| `poise_ext.rs:699` — `SongbirdManagerExt::join_vc` | **zero callers** (ct#481) |

- [ ] **Step 1: Write the failing guard test**

Add a second test module to `crack-core/src/music/perms.rs`:

```rust
/// 🪤 This module exists because the obvious test — "assert the three join
/// sites are guarded" — is a survey of what was found on 2026-09-12, not a
/// property. It would pass while a fourth site was live.
///
/// That is not hypothetical. In v0.9.5 a guard test for play-history writes
/// asserted a hard-coded count of the three call sites then known, passed,
/// and the bug it was written to prevent was live the entire time, because
/// playlists routed through a fourth path. Pinning the whole surface instead
/// immediately turned up two more entry points.
///
/// So this DERIVES the set of join sites by scanning source, and asserts each
/// one it finds is gated. Adding a fourth join site fails this test until it
/// is gated too.
#[cfg(test)]
mod join_site_guard_tests {
    /// Every file that may contain a `songbird` join. Adding a file here is
    /// cheap; forgetting one is the failure mode, so the assertion below also
    /// requires the total to be non-zero — a scan that silently finds nothing
    /// and passes is worse than no test at all (ct#448, #449, #471).
    const SOURCES: &[(&str, &str)] = &[
        (
            "commands/music_utils.rs",
            include_str!("../commands/music_utils.rs"),
        ),
        (
            "commands/music/gp_persist.rs",
            include_str!("../commands/music/gp_persist.rs"),
        ),
        ("poise_ext.rs", include_str!("../poise_ext.rs")),
    ];

    /// The call that must precede every join.
    const GATE: &str = "ensure_can_join(";

    /// How far back from a join to look for its gate. Generous enough to span
    /// a `let Some(..) = ... else` or a log line in between, tight enough that
    /// a gate on an unrelated earlier join cannot satisfy a later one.
    const WINDOW: usize = 1200;

    /// Finds `.join(` calls that are songbird joins. String `.join(" ")` and
    /// friends are excluded by requiring the first argument to not be a
    /// literal.
    fn songbird_join_offsets(src: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(rel) = src[from..].find(".join(") {
            let at = from + rel;
            let arg = src[at + ".join(".len()..].trim_start();
            // `.join(" ")`, `.join("\n")`, `.join(", ")` are slice joins.
            if !arg.starts_with('"') {
                out.push(at);
            }
            from = at + ".join(".len();
        }
        out
    }

    #[test]
    fn every_songbird_join_is_preceded_by_the_gate() {
        let mut checked = 0usize;
        for (name, src) in SOURCES {
            for at in songbird_join_offsets(src) {
                checked += 1;
                let start = at.saturating_sub(WINDOW);
                let before = &src[start..at];
                assert!(
                    before.contains(GATE),
                    "{name}: a songbird join at byte {at} is not preceded by \
                     `{GATE}` within {WINDOW} bytes.\n\n\
                     Every join must be gated, or a guild missing CONNECT or \
                     SPEAK gets songbird's ~10s JoinError::TimedOut, which \
                     names nothing. Add the gate rather than widening this \
                     test.\n\n\
                     Context:\n{}",
                    &src[start.max(at.saturating_sub(300))..at]
                );
            }
        }
        assert!(
            checked >= 3,
            "expected at least the 3 known songbird join sites, scanned \
             {checked} -- the scan found less than it should, which means it \
             has stopped checking rather than that the joins are gone. Fix \
             the scan."
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p crack-core --lib join_site_guard`
Expected: FAIL — the first unguarded join site is reported by name and byte offset.

- [ ] **Step 3: Gate `do_join`**

In `crack-core/src/commands/music_utils.rs`, immediately before `let call = match manager.join(guild_id, channel_id).await {` (line ~144):

```rust
    // Refuse a join Discord would silently drop. Without this the voice state
    // update is accepted, nothing happens, and songbird reports TimedOut ~10s
    // later with no mention of a permission.
    crate::music::perms::ensure_can_join(ctx.cache(), guild_id, channel_id)
        .map_err(|e| -> Error { Box::new(e) })?;
```

- [ ] **Step 4: Gate the restart-resume join**

In `crack-core/src/commands/music/gp_persist.rs`, immediately before `let call = match data.songbird.join(guild_id, voice_channel).await {` (line ~615). This function returns `()`, and there is no invoking user to reply to, so the refusal goes to the log channel and the tracing log:

```rust
    // No invoking user on this path -- the guild is being restored after a
    // restart -- so a refusal is reported to the log channel, not a reply.
    if let Err(e) = crate::music::perms::ensure_can_join(&cache, guild_id, voice_channel) {
        tracing::warn!("gp: cannot rejoin {voice_channel} in {guild_id}: {e}");
        if let Some(log_channel) = data
            .get_guild_settings(guild_id)
            .await
            .and_then(|s| s.get_all_log_channel())
        {
            let _ = log_channel
                .say(&http, format!("{e}"))
                .await
                .inspect_err(|e| tracing::warn!("gp: could not reach the log channel: {e}"));
        }
        data.gp_remove(guild_id);
        return;
    }
```

Adapt the identifiers to what is actually in scope in that function — read the surrounding 60 lines first. In particular: the cache and http handles may be reached through a serenity context rather than as bare locals, and the existing `Err(e)` arm just below already performs the `gp_remove` + `gp_mark_finished` bookkeeping. **Match that existing failure path rather than inventing a second one** — if the existing arm does more than `gp_remove` (it calls `gp_mark_finished(pool, game_id(guild_id), started_at, GpOutcome::Lost)`), factor the shared tail out or fall through to it. Do not leave the guild owned by a game that no longer exists; `music/lease.rs` documents why that leaves `/play` refused forever.

- [ ] **Step 5: Gate `join_vc`**

In `crack-core/src/poise_ext.rs`, `SongbirdManagerExt::join_vc` currently returns `Result<Arc<Mutex<Call>>, songbird::error::JoinError>`, which cannot carry a `CrackedError`. It has **zero callers**, so changing its signature breaks nothing. Change the trait and its impl to take a cache and return `CrackedError`:

```rust
/// Extension trait for Songbird.
pub trait SongbirdManagerExt {
    fn join_vc(
        &self,
        cache: &serenity::Cache,
        guild_id: JoinVCToken,
        channel_id: serenity::ChannelId,
    ) -> impl Future<Output = Result<Arc<tokio::sync::Mutex<songbird::Call>>, CrackedError>>;
}

impl SongbirdManagerExt for songbird::Songbird {
    async fn join_vc(
        &self,
        cache: &serenity::Cache,
        JoinVCToken(guild_id, lock): JoinVCToken,
        channel_id: serenity::ChannelId,
    ) -> Result<Arc<tokio::sync::Mutex<songbird::Call>>, CrackedError> {
        let _guard = lock.lock().await;
        crate::music::perms::ensure_can_join(cache, guild_id, channel_id)?;
        match self.join(guild_id, channel_id).await {
            Ok(call) => Ok(call),
            Err(err) => {
                // On error, the Call is left in a semi-connected state.
                // We need to correct this by removing the call from the manager.
                drop(self.leave(guild_id).await);
                Err(CrackedError::JoinChannelError(err))
            },
        }
    }
}
```

This is deliberate churn on dead code. The alternative — exempting `join_vc` from the guard test — would reintroduce the exception list the test exists to avoid. ct#481 decides whether this function lives at all; until then it stays gated like the others.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p crack-core --lib join_site_guard`
Expected: PASS, and `checked` >= 3.

- [ ] **Step 7: Run the full gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo check
cargo test --workspace
git add crack-core/src/commands/music_utils.rs crack-core/src/commands/music/gp_persist.rs crack-core/src/poise_ext.rs crack-core/src/music/perms.rs
git commit -F - <<'EOF'
feat(perms): gate all three join sites, and derive that set by scanning

A missing CONNECT is accepted by Discord and then silently dropped, so
songbird waits out ~10s and reports TimedOut, naming nothing. The gate
refuses first and says which permission is missing and where.

join_vc has zero callers (ct#481) and its signature could not carry a
CrackedError; changing it breaks nothing and is preferable to exempting
it, which would reintroduce the exception list the guard test exists to
avoid.

🪤 The test derives the join-site set by scanning source rather than
asserting a count. v0.9.5's play-history guard hard-coded the three
sites it knew about, passed, and the bug was live through a fourth the
whole time.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

## Task 5: The degraded-permissions notice on the play reply

**Files:**
- Modify: `crack-core/src/commands/music/doplay.rs` (`build_play_embed` at line ~268, and its call site at line ~440)

**Interfaces:**
- Consumes: Task 1's `TextPerms`; Task 3's `resolve`.
- Produces: `build_play_embed` gains a fourth parameter `text: Option<&TextPerms>`. No other caller exists; confirm with `grep -rn build_play_embed --include='*.rs'`.

**Why a field and not a footer.** `build_play_embed` delegates to `build_queued_embed`, which may already set a footer; `.footer()` replaces rather than appends, so using it risks silently destroying existing text. Embed fields append. Check `build_queued_embed` before choosing, and prefer a field unless it demonstrably sets none.

- [ ] **Step 1: Write the failing test**

Add to the test module at the bottom of `crack-core/src/commands/music/doplay.rs` (create one if absent):

```rust
#[cfg(test)]
mod degraded_perms_notice_tests {
    use super::*;
    use crate::music::perms::{TextPerms, TEXT_REQUIRED};
    use poise::serenity_prelude::all::{GenericChannelId, Permissions};

    fn perms(granted: Permissions) -> TextPerms {
        TextPerms {
            channel: GenericChannelId::new(1),
            granted,
        }
    }

    #[test]
    fn whole_text_perms_add_no_notice() {
        let note = degraded_notice(Some(&perms(TEXT_REQUIRED)));
        assert!(
            note.is_none(),
            "a guild with every permission must see nothing: {note:?}"
        );
    }

    #[test]
    fn absent_perms_add_no_notice() {
        // `resolve` returned None (cache miss). Fail open: say nothing.
        assert!(degraded_notice(None).is_none());
    }

    #[test]
    fn a_missing_embed_links_is_named_in_the_notice() {
        let note = degraded_notice(Some(&perms(TEXT_REQUIRED - Permissions::EMBED_LINKS)))
            .expect("degraded perms must produce a notice");
        assert!(note.contains("Embed Links"), "got {note}");
        assert!(
            note.contains("/diagnose"),
            "the notice must point at the diagnostic: {note}"
        );
    }

    #[test]
    fn several_missing_permissions_are_all_named() {
        let note = degraded_notice(Some(&perms(Permissions::VIEW_CHANNEL)))
            .expect("degraded perms must produce a notice");
        assert!(note.contains("Send Messages"), "got {note}");
        assert!(note.contains("Embed Links"), "got {note}");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p crack-core --lib degraded_perms_notice`
Expected: FAIL to compile — `degraded_notice` is not defined.

- [ ] **Step 3: Write the helper and thread the perms through**

Add to `crack-core/src/commands/music/doplay.rs`:

```rust
use crate::music::perms::TextPerms;

/// The note appended to a play reply when the bot cannot fully use the text
/// channel. `None` means say nothing — either everything is granted, or the
/// cache could not tell us, and a false warning is worse than silence.
///
/// This is a note on a reply that was being sent anyway, never a message of
/// its own: it costs nothing extra, stays next to the action that prompted
/// it, and disappears the moment the permission is granted, with no state to
/// keep and nothing to expire.
fn degraded_notice(text: Option<&TextPerms>) -> Option<String> {
    let text = text?;
    if text.is_whole() {
        return None;
    }
    Some(format!(
        "⚠️ Missing **{}** here — I won't post now-playing. `/diagnose` for detail.",
        text.missing()
    ))
}
```

Change the signature at line ~268 and append the field at the end, just before the `Ok(embed)`:

```rust
pub async fn build_play_embed<'a>(
    queue: &'a [TrackHandle],
    mode: Mode,
    query_type: NewQueryType,
    text: Option<&TextPerms>,
) -> Result<CreateEmbed<'a>, Error> {
```

```rust
    let embed = match degraded_notice(text) {
        Some(note) => embed.field("⚠️ Limited permissions", note, false),
        None => embed,
    };
    Ok(embed)
```

At the call site (line ~440), resolve the permissions and pass them:

```rust
    let text_perms = ctx.guild_id().and_then(|gid| {
        crate::music::perms::resolve(ctx.cache(), gid, ctx.channel_id(), ctx.author().id)
            .map(|p| p.text)
    });
    let embed = build_play_embed(&queue, mode, query_type, text_perms.as_ref()).await?;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p crack-core --lib degraded_perms_notice`
Expected: PASS, 4 tests.

- [ ] **Step 5: Run the full gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo check
cargo test --workspace
git add crack-core/src/commands/music/doplay.rs
git commit -F - <<'EOF'
feat(perms): note degraded text permissions on the reply already going out

Never a message of its own, at any cadence: it costs nothing extra,
stays next to the action that prompted it, and disappears the moment the
permission is granted -- no state to keep, nothing to expire.

A field rather than a footer because build_queued_embed may already set
a footer and `.footer()` replaces rather than appends. Silent when perms
are whole and silent on a cache miss, since a false warning is worse
than saying nothing.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

## Task 6: `/diagnose`

**Files:**
- Create: `crack-core/src/commands/music/diagnose.rs`
- Modify: `crack-core/src/commands/music/mod.rs`

**Interfaces:**
- Consumes: Task 1's `MusicPermissions`/`TextPerms`/`VoicePerms`; Task 3's `resolve`.
- Produces: `pub fn diagnose() -> crate::Command`, registered in `music_commands()`.

**It deliberately carries no `check`.** Every other music command has `check = "cmd_check_music"`, which refuses invocation outside the configured music channel (`CrackedError::NotInMusicChannel`). A diagnostic that can itself be refused for a configuration reason is the worst possible diagnostic — the reason to run it is that the bot is being silent. It stays `guild_only`.

This gives it a property worth having on purpose: slash replies go out over the interaction webhook, which is not subject to channel send permissions, so **`/diagnose` still answers in a channel where the bot has no `SEND_MESSAGES` at all.**

- [ ] **Step 1: Write the failing test**

Create `crack-core/src/commands/music/diagnose.rs` with only the render function's tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::perms::{compute, TEXT_REQUIRED, VOICE_REQUIRED};
    use poise::serenity_prelude::all::{ChannelId, GenericChannelId, Permissions};

    fn text_ch() -> GenericChannelId {
        GenericChannelId::new(1)
    }
    fn voice_ch() -> ChannelId {
        ChannelId::new(2)
    }

    #[test]
    fn all_clear_says_so_and_lists_no_problems() {
        let p = compute(text_ch(), TEXT_REQUIRED, Some((voice_ch(), VOICE_REQUIRED)));
        let out = render(&p);
        assert!(out.contains("All clear"), "got {out}");
        assert!(!out.contains("problem"), "nothing is wrong, so say nothing: {out}");
    }

    #[test]
    fn problems_are_counted_and_each_names_its_consequence() {
        let p = compute(
            text_ch(),
            TEXT_REQUIRED - Permissions::EMBED_LINKS,
            Some((voice_ch(), VOICE_REQUIRED - Permissions::SPEAK)),
        );
        let out = render(&p);
        assert!(out.contains("2 problems"), "got {out}");
        assert!(out.contains("Embed Links"), "got {out}");
        assert!(out.contains("Speak"), "got {out}");
        // A gap without its consequence is a riddle, not a diagnosis.
        assert!(out.contains("now-playing"), "got {out}");
        assert!(out.contains("refuse"), "got {out}");
    }

    #[test]
    fn no_voice_channel_asks_the_user_to_join_one_rather_than_reporting_a_denial() {
        let p = compute(text_ch(), TEXT_REQUIRED, None);
        let out = render(&p);
        // 🪤 Absent is not denied. Rendering this as a missing permission
        // would send someone to grant Connect when Connect is already
        // granted. Nor may this collapse into the all-clear line: we did not
        // check voice, so we must not claim it is fine.
        assert!(out.contains("not in a voice channel"), "got {out}");
        assert!(!out.contains("All clear"), "voice was never checked: {out}");
        assert!(!out.contains("\u{274c}"), "nothing is denied here: {out}");
        assert!(!out.contains("problem"), "nothing is wrong here: {out}");
    }

    #[test]
    fn one_problem_is_singular() {
        let p = compute(
            text_ch(),
            TEXT_REQUIRED - Permissions::EMBED_LINKS,
            Some((voice_ch(), VOICE_REQUIRED)),
        );
        let out = render(&p);
        assert!(out.contains("1 problem"), "got {out}");
        assert!(!out.contains("1 problems"), "got {out}");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p crack-core --lib diagnose`
Expected: FAIL to compile — `render` is not defined.

- [ ] **Step 3: Write the render function and the command**

Prepend to `crack-core/src/commands/music/diagnose.rs`:

```rust
//! `/diagnose` — report what the bot can and cannot do here.
//!
//! Deliberately carries no `check`. Every other music command has
//! `check = "cmd_check_music"`, which refuses invocation outside the
//! configured music channel; a diagnostic that can itself be refused for a
//! configuration reason is the worst possible diagnostic, because the reason
//! to run it is that the bot is being silent.

use crate::music::perms::{resolve, MusicPermissions};
use crate::{Context, Error};
use poise::serenity_prelude::all::Permissions;

fn tick(ok: bool) -> &'static str {
    if ok {
        "✅"
    } else {
        "❌"
    }
}

/// Render the permission picture. Pure, so all three states are tested
/// without Discord.
fn render(p: &MusicPermissions) -> String {
    let mut problems: Vec<String> = Vec::new();

    if !p.text.is_whole() {
        problems.push(format!(
            "• **{}** in {} — now-playing posts will be skipped",
            p.text.missing(),
            p.text.channel.mention()
        ));
    }
    if let Some(v) = &p.voice {
        if !v.can_join() {
            problems.push(format!(
                "• **{}** in {} — /play will refuse to connect",
                v.missing(),
                v.channel.mention()
            ));
        }
    }

    // All-clear requires that we actually checked both halves. With no voice
    // channel to check we fall through to the table, which says so — claiming
    // "all clear" on a voice channel we never looked at would be a lie the
    // user only discovers when /play refuses.
    if problems.is_empty() {
        if let Some(v) = &p.voice {
            return format!(
                "✅ All clear — I have everything I need in {} and {}.",
                p.text.channel.mention(),
                v.channel.mention()
            );
        }
    }

    let mut out = String::from("🔍 **Permission check**\n");
    out.push_str(&format!(
        "**Text** {}  {} View  {} Send  {} Embed Links\n",
        p.text.channel.mention(),
        tick(p.text.view()),
        tick(p.text.send()),
        tick(p.text.embed()),
    ));
    match &p.voice {
        Some(v) => out.push_str(&format!(
            "**Voice** {}  {} Connect  {} Speak\n",
            v.channel.mention(),
            tick(v.connect()),
            tick(v.speak()),
        )),
        // Absent is not denied.
        None => out.push_str(
            "**Voice** — you're not in a voice channel, so I can't check \
             Connect/Speak. Join one and run this again.\n",
        ),
    }

    // Omitted entirely when nothing is wrong -- reached only via the
    // no-voice-channel path above, where there is nothing to report.
    if !problems.is_empty() {
        let n = problems.len();
        out.push_str(&format!(
            "\n**{n} problem{}**\n{}\n\nFix: Server Settings → Roles → CrackTunes",
            if n == 1 { "" } else { "s" },
            problems.join("\n"),
        ));
    }
    out
}

/// Report what the bot can and cannot do in this channel and your voice channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral
)]
pub async fn diagnose(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let out = match resolve(ctx.cache(), guild_id, ctx.channel_id(), ctx.author().id) {
        Some(p) => render(&p),
        // Fail open in the wording too: say we could not read, not that
        // something is wrong.
        None => "I couldn't read my own permissions from cache just now — \
                 try again in a moment."
            .to_string(),
    };
    ctx.say(out).await?;
    Ok(())
}
```

Add `use poise::serenity_prelude::all::Mentionable;` if `.mention()` is not otherwise in scope, and drop the `Permissions` import if it proves unused (clippy runs with `-D warnings`).

- [ ] **Step 4: Register the command**

In `crack-core/src/commands/music/mod.rs`: add `pub mod diagnose;` and `pub use diagnose::*;` beside the other modules (alphabetical — `diagnose` sits between `dosearch` and `gambling` in the `pub mod` block; match the existing ordering), then add `diagnose(),` to the `vec![...]` in `music_commands()`, keeping that list's alphabetical order (between `clear()` and `grab()`).

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p crack-core --lib diagnose`
Expected: PASS, 4 tests.

- [ ] **Step 6: Run the full gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo check
cargo test --workspace
git add crack-core/src/commands/music/diagnose.rs crack-core/src/commands/music/mod.rs
git commit -F - <<'EOF'
feat(perms): /diagnose, the one command that works when nothing else does

No `check` on purpose. Every other music command carries
cmd_check_music, which refuses outside the configured music channel -- a
diagnostic that can itself be refused for a configuration reason is the
worst possible diagnostic, since the reason to run it is that the bot is
silent. Slash replies use the interaction webhook, so it also answers in
a channel where the bot has no SEND_MESSAGES at all.

🪤 A user who is in no voice channel is rendered as "join one and run
this again", never as a missing Connect -- otherwise it sends someone to
grant a permission that is already granted.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

## Task 7: Version bump, musicreco correction, and the release gate

**Files:**
- Modify: all nine `Cargo.toml` files (`crack-core`, `crack-cli`, `crack-types`, `crack-testing`, `crack-voting`, `crack-sleevenote`, and the remaining members — enumerate with the command below rather than trusting this list)
- Modify: `Cargo.lock`
- Modify: `docs/superpowers/plans/2026-09-12-musicreco.md`

**Interfaces:** none.

- [ ] **Step 1: Confirm every member and its current version**

```bash
grep -rn '^version = ' --include=Cargo.toml . | grep -v '^./target'
```
Expected: nine lines, all `0.9.6`. If any differs, stop and report — a partial bump means an earlier release was incomplete.

- [ ] **Step 2: Bump all nine to 0.9.7**

```bash
grep -rln '^version = "0.9.6"' --include=Cargo.toml . \
  | grep -v '^./target' \
  | xargs sed -i 's/^version = "0.9.6"/version = "0.9.7"/'
grep -rn '^version = ' --include=Cargo.toml . | grep -v '^./target'
```
Expected: nine lines, all `0.9.7`.

- [ ] **Step 3: Refresh the lockfile**

Run: `cargo check --workspace`
This rewrites `Cargo.lock` with the new versions. Confirm with `git diff --stat Cargo.lock`.

- [ ] **Step 4: Correct the musicreco plan's version**

`docs/superpowers/plans/2026-09-12-musicreco.md` Task 9 says to bump to `0.9.6`. That version is taken by the autocomplete panic fix (ct#493), and `0.9.7` is taken by this plan. Change it to `0.9.8`:

```bash
grep -n '0\.9\.6' docs/superpowers/plans/2026-09-12-musicreco.md
```
Edit each hit that refers to the version this plan should bump *to* (leave any that describe the *current* workspace version as historical context — read each in situ). Re-check:
```bash
grep -n '0\.9\.[678]' docs/superpowers/plans/2026-09-12-musicreco.md
```

- [ ] **Step 5: Run the full gate**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
SQLX_OFFLINE=true cargo check
cargo test --workspace
```
Expected: clean, except `crack-testing::tests::test_enqueue_query`, which fails on master too and is not a regression.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -F - <<'EOF'
v0.9.7: check permissions before joining, and explain what is missing

Bumps all nine members together -- each holds a literal version and
there is no [workspace.package] version, so a partial bump compiles
clean and is invisible (#424b).

Also corrects the musicreco plan's target from 0.9.6, which ct#493 took,
to 0.9.8.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

## Self-Review

**Spec coverage.** Every spec section maps to a task: the two-tier model and `compute` → Task 1; the typed error and copy → Task 2; `resolve`, `ensure_can_join`, no-HTTP and fail-open → Task 3; all three join sites and the derived-not-asserted guard test → Task 4; the degrading notice → Task 5; `/diagnose` and its three render states → Task 6; versioning and the musicreco correction → Task 7. The prefix-command log-channel fallback is covered where it actually arises (Task 4, `gp_persist`) rather than as a task of its own.

**Known gap, accepted:** the spec's fallback chain (invocation channel → log channel → silence) is specified for prefix commands generally, but only the `gp_persist` site implements it, because that is the only place in this plan that sends outside a reply. A prefix `r!play` refused for voice perms returns its error through `FrameworkError::Command`, whose delivery is poise's, not ours. If that proves to swallow the message in a channel without `SEND_MESSAGES`, it is a follow-up, not a defect in this plan.

**Type consistency.** `TextPerms`/`VoicePerms` store `granted: Permissions` in Task 1 and are read that way in Tasks 5 and 6. `compute` keeps the same three-parameter signature throughout. `ensure_can_join(cache, guild_id, channel_id)` is identical at all three call sites in Task 4. `build_play_embed` gains exactly one parameter, `Option<&TextPerms>`, in Task 5 and is called that way in the same task.

**Placeholders:** none. Every code step carries the code.
