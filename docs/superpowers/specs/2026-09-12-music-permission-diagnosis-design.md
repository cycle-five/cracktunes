# Music permission diagnosis — design

**Date:** 2026-09-12
**Target version:** 0.9.7
**Status:** approved, ready for planning

## Problem

When cracktunes lacks a Discord permission it needs, it fails in a way that is
indistinguishable from being broken.

Three concrete failures, all live on master today:

1. **The music commands declare no bot permissions at all.**
   `required_bot_permissions` appears on ~20 admin and settings commands.
   `play`, `skip`, `summon`, `leave` and the rest declare only `guild_only`.
   Poise therefore never performs a permission check for any of them, and
   `FrameworkError::MissingBotPermissions` is never raised on a music path.

2. **A missing voice permission surfaces as a timeout.** `music_utils.rs:144`
   turns a failed `songbird.join` into `CrackedError::JoinChannelError`.
   Discord's behaviour when a bot lacks `CONNECT` is to accept the voice state
   update and silently do nothing with it, so songbird waits and eventually
   returns `JoinError::TimedOut`. The user waits roughly ten seconds and is
   then told, in effect, "something timed out".

3. **Nothing can answer the question "what is wrong?"** There is no command
   that reports what the bot can and cannot do. A commented-out block at
   `poise_ext.rs:612-622` was reaching for this — it checked `send_messages`
   and `embed_links` and asked the user to grant embed links — and has been
   dead for a long time.

### What poise cannot do for us

Poise's permission check is structurally unable to cover the permission that
matters most here. `dispatch/permissions.rs::calculate_missing` resolves
permissions in `ctx.channel_id()` — the channel the command was invoked in.
`CONNECT` and `SPEAK` live on the **voice channel the invoking user is sitting
in**, which poise never looks at.

This is not a gap in how we call poise. There is no value of
`required_bot_permissions` that expresses "SPEAK, in whatever voice channel
the author happens to be in". The voice half of this feature has to be ours.

## Permission model

Permissions fall into two tiers, and the tiers behave differently. This is the
central design decision and everything else follows from it.

| tier | permissions | behaviour |
|---|---|---|
| **Blocking** | `CONNECT`, `SPEAK` on the author's voice channel | Refuse before attempting the join. Say which permission is missing and where. |
| **Degrading** | `VIEW_CHANNEL`, `SEND_MESSAGES`, `EMBED_LINKS` on the invocation channel | Connect and play anyway. Note the limitation on the reply that is already being sent. |

The asymmetry is real, not a stylistic choice: audio playback requires no text
permission whatsoever. A bot that cannot post in `#general` can still join a
voice channel and play music perfectly. What it loses is the now-playing
posts, which are plain channel sends from the track event handlers
(`track_end.rs:209` and `:241` via `interface.rs:228::send_now_playing`) and
do genuinely require `SEND_MESSAGES` and `EMBED_LINKS`.

Refusing to play because we cannot announce the song would be strictly worse
for the user than playing silently.

### Out of scope

- The voice channel's **user limit**, and `MOVE_MEMBERS`. A full channel is a
  real join failure, but it has a different cause and a different fix, and is
  not a permission.
- Non-music commands. The ~20 admin and settings commands keep the
  `required_bot_permissions` attribute they already carry; poise's builtin
  handler already replies for those, and they do not touch voice.

## Architecture

### One pure function, three consumers

New module `crack-core/src/music/perms.rs`.

```rust
pub struct MusicPermissions {
    pub text: TextPerms,
    /// `None` when the author is in no voice channel — a distinct state from
    /// "in a channel we cannot join", and it needs a different message.
    pub voice: Option<VoicePerms>,
}

pub struct TextPerms {
    pub channel: GenericChannelId,
    pub view: bool,
    pub send: bool,
    pub embed: bool,
}

pub struct VoicePerms {
    pub channel: ChannelId,
    pub connect: bool,
    pub speak: bool,
}

/// Pure: no cache reads, no await, no poise Context.
pub fn compute(
    guild: &Guild,
    bot: &Member,
    text: GenericChannelId,
    voice: Option<ChannelId>,
) -> MusicPermissions;
```

The purity is what makes this testable. Every interesting case — missing
`SPEAK`, missing `CONNECT` and `SPEAK` together, missing `EMBED_LINKS`, the
author not in voice — is a synthetic `Permissions` bitset and a struct
comparison, with no Discord, no network and no async runtime. A thin async
wrapper performs the cache reads and calls `compute`.

**No HTTP on any path.** `GatewayIntents::GUILD_MEMBERS` is enabled
(`config.rs:321`), so the bot's own `Member` is in the cache and
`guild.user_permissions_in(channel, &member)` resolves locally. This matters:
the blocking gate runs on every join, and a per-join HTTP round trip would be
a latency and rate-limit regression.

### The blocking gate lives at the join sites

```rust
pub fn ensure_can_join(cache: &Cache, guild: GuildId, channel: ChannelId)
    -> Result<(), CrackedError>;
```

It takes cache handles rather than a poise `Context` deliberately, because one
of the three call sites has no invoking user and therefore no `Context`.

There are three `songbird.join` call sites in the workspace:

| site | context | on refusal |
|---|---|---|
| `commands/music_utils.rs:144` (`do_join`) | the command path | Return the error. It propagates out of the command body, and the existing `FrameworkError::Command` arm (`config.rs:48`) replies. No new error plumbing. |
| `commands/music/gp_persist.rs:615` | restart-resume of a guilty-pleasure game | Nobody to reply to. Log at WARN and post to the guild's log channel. |
| `poise_ext.rs:699` (`join_vc`) | **zero callers** — this is ct#481 | Guard it regardless. An unguarded fourth door that someone wires up later is exactly the failure this design is trying to prevent; ct#481 decides whether the function lives at all. |

#### The trap this section exists to avoid

The obvious test is "assert the three join sites are guarded". That is a
survey of what was found on 2026-09-12, not a property. It would pass while a
fourth site was live.

This is not hypothetical. In v0.9.5 a guard test for play-history writes
asserted a hard-coded count of the three call sites then known, passed, and
the bug it was written to prevent was live the entire time — playlists routed
through a fourth path. Pinning the whole surface instead immediately found two
more entry points.

So the test **scans source for `songbird.join` call sites and asserts each one
is preceded by `ensure_can_join`, deriving the count rather than asserting
it**. It must also avoid counting its own source text, which is the standing
self-reference trap in this repo's source-scanning guards.

### The degrading path cannot be a check

A poise check can only permit or refuse; it has no way to say "proceed, with a
caveat". The footer is therefore appended in `build_play_embed`
(`doplay.rs:268`) from the same `MusicPermissions` value, recomputed at reply
time.

Recomputing rather than threading the value through from the gate costs a few
cache reads and cannot go stale. State threaded through a command body can.

## User-facing behaviour

### Blocking refusal

```
❌ I can't play in **General** — I'm missing **Speak** there.

Ask an admin to grant it, then try again.
```

A new typed error carries this, rather than a formatted string:

```rust
MissingBotPermissions { scope: PermScope, channel: ChannelId, missing: Permissions }
pub enum PermScope { Text, Voice }
```

`serenity::Permissions` already implements `Display` as comma-separated names
— poise's own builtin handler depends on this — so two missing permissions
render as "**Connect, Speak**" with no special-casing in our code.

### Degrading footer

Appended to the reply the command is already sending. No separate message is
ever posted for this, at any cadence:

```
▸ Added to queue
  Never Gonna Give You Up — 3:32
  Position 4 · requested by lothrop
  ──────────────────────────────────
  ⚠ Missing **Embed Links** here — I won't post now-playing.
    /diagnose for detail.
```

It costs no extra message, stays contextual to the action that triggered it,
and disappears the moment the permission is granted — with no state to keep
and nothing to expire.

### Prefix commands with no way to reply

Replying to a **slash** command does not require `SEND_MESSAGES`: interaction
responses go out over the interaction webhook and are not subject to channel
send permissions. So `/play` can always deliver its refusal, even in a channel
where the bot is otherwise muted.

**Prefix** commands (`r!play`) have no such escape. If the bot has
`VIEW_CHANNEL` but not `SEND_MESSAGES` it receives the command and cannot
answer in channel. The fallback order is:

1. The invocation channel — fails.
2. The guild's configured log channel — `GuildSettings::get_all_log_channel()`
   (`guild/settings.rs:94`) — if set and usable.
3. `tracing::warn!` and silence.

The bot never DMs anyone. Unsolicited bot DMs draw user reports, guild DMs are
closed for many users, and the log channel is where an admin is already
looking.

### `/diagnose`

New command at `crack-core/src/commands/music/diagnose.rs`. `guild_only`,
slash and prefix, ephemeral on slash, registered in `music_commands()`.

**It deliberately carries no `check`.** Every other music command has
`check = "cmd_check_music"`, which refuses invocation outside the configured
music channel (`CrackedError::NotInMusicChannel`). A diagnostic that can
itself be refused for a configuration reason is the worst possible
diagnostic — the reason to run it is that the bot is being silent.

This gives it a property worth having on purpose: because slash replies use
the interaction webhook, **`/diagnose` still answers in a channel where the
bot has no `SEND_MESSAGES` at all**. The tool works precisely when everything
else is broken.

Three render states, because the voice half is only meaningful relative to a
channel the author is actually in:

```
✅ All clear — I have everything I need in #general and General.
```

```
🔍 Permission check
  Text   #general      ✅ View  ✅ Send  ❌ Embed Links
  Voice  General       ✅ Connect  ❌ Speak

  2 problems
   • Embed Links in #general — now-playing posts will be skipped
   • Speak in General — /play will refuse to connect
  Fix: Server Settings → Roles → CrackTunes
```

```
🔍 Permission check
  Text   #general      ✅ View  ✅ Send  ✅ Embed Links
  Voice  — you're not in a voice channel, so I can't check
         Connect/Speak. Join one and run this again.
```

## Testing

Five of the six groups need no Discord, no network and no async runtime. That
is the return on splitting `compute` out as a pure function.

| test | shape |
|---|---|
| `compute` | synthetic bitsets: all present; missing `SPEAK`; missing `CONNECT`+`SPEAK`; missing `EMBED_LINKS`; `voice: None` |
| `ensure_can_join` | returns an error carrying the right channel and the exact missing set |
| refusal copy | names the channel and **every** missing permission, not just the first |
| footer | present iff text perms are degraded; absent when whole |
| `/diagnose` render | all three states, asserted on rendered text |
| join-site guard | source scan; every `songbird.join` is preceded by `ensure_can_join`; count derived, not hard-coded; does not count its own source |

## Files

**Create**
- `crack-core/src/music/perms.rs`
- `crack-core/src/commands/music/diagnose.rs`

**Modify**
- `crack-core/src/music/mod.rs` — register the module
- `crack-core/src/errors.rs` — `MissingBotPermissions` variant, `PermScope`, `Display`
- `crack-core/src/messaging/messages.rs` — copy constants, following the existing const pattern
- `crack-core/src/commands/music_utils.rs` — guard `do_join`
- `crack-core/src/commands/music/gp_persist.rs` — guard, with log-channel fallback
- `crack-core/src/poise_ext.rs` — guard `join_vc`
- `crack-core/src/commands/music/mod.rs` — register `diagnose()` in `music_commands()`
- `crack-core/src/commands/music/doplay.rs` — footer in `build_play_embed`

## Versioning

All nine workspace members 0.9.6 → **0.9.7**. Each member holds a literal
`version`; there is no `[workspace.package] version`, so a partial bump
compiles clean and is invisible (#424b).

The musicreco plan
(`docs/superpowers/plans/2026-09-12-musicreco.md`) Task 9 currently says to
bump to 0.9.6, which this release line has taken. It must read **0.9.8**.

## What this does not fix

**This is hardening, not a fix for an observed outage.** The reported symptom
that started this — a bot that said "Searching…" and then did nothing in the
guild "shameless" — was **not** a permission problem. It was a UTF-8 panic in
`crack-testing/src/resolve.rs::suggest_string`, fixed in v0.9.6 (ct#493).

The work here is worth doing on its own merits, because a missing `CONNECT`
produces the same user-visible symptom by a completely different route and
currently takes ten seconds to say nothing useful. But it should not be
judged by whether "shameless" starts working — v0.9.6 is what determines
that.

Related, and deliberately not folded in: **ct#494**, the orphaned
`🔎 Searching...` placeholder. It shares a symptom with this work and has an
unrelated cause (a `ReplyHandle` that no error path owns).
