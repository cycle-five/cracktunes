# Playback ownership: a per-guild lease for queue mutation

**Issue:** [#434](https://github.com/cycle-five/cracktunes/issues/434)
**Also closes:** [#333](https://github.com/cycle-five/cracktunes/issues/333)
**Date:** 2026-09-10
**Scope decided with the issue author:** Phase A (the lease) and Phase C
(report from the insertion result) land together, in one arc.

## The question this answers

**What owns playback in a guild right now, and what does that permit?**

Today the only answer is `GP_BLOCKED_COMMANDS` (`gp.rs:149`) plus the
`gp_is_active` check in `cmd_check_music` (`permissions.rs:18-24`). That is
correct and properly per-guild, but it is one hardcoded name list serving one
feature, and it is the only thing standing between a stray `/play` and a
corrupted `/gp` round.

## The two defects

### #333 — concurrent `/play` reports the wrong queue

`enqueue_resolved_tracks_back` (`music/queue.rs:112-134`) returns
`handler.queue().current_queue()` **after** enqueueing:

```rust
let mut handler = call.lock().await;
for resolved in &tracks { /* ... handler.enqueue(track).await ... */ }
Ok(handler.queue().current_queue())   // :133 -- the whole queue, not ours
```

Two `/play` calls in flight both read the post-both state, so both replies list
both sets of songs. That is the reported "one of the songs got queued twice".

### `queue_query_list_offset` — check-then-act across a slow await

`music/queue.rs:463-511`:

```rust
let queue_size = { call.lock().await.queue().len() };        // :473-476  read
verify(offset > 0 && offset <= queue_size + 1, ..)?;         // :482-485  validate
let tracks = ctx.data().ct_client.resolve_track_many(..).await?;  // :488  8-15s cold
let mut handler = call.lock().await;                          // :501     act
```

The lock is dropped between reading `queue_size` and acting on it, across a
boundary that takes eight to fifteen seconds cold. Not reported by anyone; real
regardless.

### The `/gp` guard is a "did we remember?" bug class

`GP_BLOCKED_COMMANDS` grows with every new queue-mutating command. Miss one and
it silently mutates the queue mid-round.

**It has grown from eleven entries to nineteen in the four days since #434 was
filed** — `voteskip`, `leave`, `seek`, `repeat`, `pause`, `summon`,
`summonchannel` were added after. The issue's thesis demonstrating itself is the
best argument for this work.

## Architecture

Two concepts, deliberately not merged, because their lifetimes differ by three
orders of magnitude.

### Ownership — long-lived, declarative, fails fast

Who owns playback in this guild. Claimed by `/gp start`, released at game end.
Checked cheaply.

```rust
pub enum PlaybackOwner { Free, Game }

fn claim_playback(&self, guild_id: GuildId, owner: PlaybackOwner) -> CrackedResult<()>;
fn release_playback(&self, guild_id: GuildId);
```

**Claim and release live inside the methods that already move `gp_games`.**
This is the design's load-bearing decision. An explicit lease is a second
source of truth, and a lease that outlives its game wedges `/play` forever with
no game left to end. Co-locating the two writes makes divergence structurally
impossible rather than merely unlikely.

There are exactly five such sites:

| site | role |
|---|---|
| `Data::gp_start` — `gp.rs:1003` | claim (`Entry::Vacant` insert) |
| `Data::gp_restore` — `gp.rs:1611` | claim (the resume path #431 asked for) |
| `Data::gp_remove` — `gp.rs:1596` | release |
| `Data::gp_remove_if_parked` — `gp.rs:1582` | release |
| `gp_persist.rs:619` | release — **a raw `gp_games.remove()` that bypasses the methods** |

The fifth is a defect in its own right: the rejoin-failure path in
`gp_resume_guild` removes from the map directly instead of going through
`Data::gp_remove`. Route it through a method as part of this work, so there is
exactly one way to end a game.

`gp_is_active` stays as it is — `gp_games.contains_key(guild_id)` — and becomes
the thing `claim_playback` is kept consistent *with*, not a competitor to it.

### Exclusion — short-lived, operational

A per-guild mutex serialising queue mutation, held for milliseconds.

```rust
async fn lock_queue(&self, guild_id: GuildId, as_: PlaybackOwner)
    -> CrackedResult<QueueGuard>;
```

- A game owns playback and you are not the game → `Err(GameInProgress)`,
  **immediately**. Nobody waits out a half-hour game.
- Otherwise → await the mutex. Short, because resolution happens outside it.

**`QueueGuard` is an RAII guard, not a capability token.** `JoinVCToken`
(`poise_ext.rs:584-595`) is worth contrasting explicitly, because #434 describes
`QueueGuard` as "in the shape of `JoinVCToken`" and the mechanism differs:

| | `JoinVCToken` | `QueueGuard` |
|---|---|---|
| `acquire` | not `async`, takes no lock | `async`, holds the lock on return |
| who locks | the consumer (`join_vc`, `poise_ext.rs:613`) | the constructor |
| what it proves | you went through the right door | you hold exclusion *now* |

Same intent — an unforgeable proof carried in the type system — different
mechanism. Document the difference where `QueueGuard` is defined; a reader who
assumes they are identical will write a bug.

### The funnel — why the token has teeth

If queue mutation *requires* a `&QueueGuard`, a command cannot mutate the queue
without passing the ownership check. `GP_BLOCKED_COMMANDS` survives only as a UX
nicety: failing early with a good message rather than deep in the call stack.
Forgetting to list a command stops being a corrupted round and becomes a worse
error message.

Every mutation moves behind a helper in `music/queue.rs` taking `&QueueGuard`.
There are **19 mutation sites** across 9 files. (Coincidentally also 19 — and
unrelated to — the `GP_BLOCKED_COMMANDS` entries above. The two lists overlap
only partly: `queue.rs` and `track_end.rs` mutate the queue without being
commands at all.)

| file | sites | disposition |
|---|---|---|
| `music/queue.rs` | 7 (`:40, :126, :187, :218, :294, :510, :511`) | become the helpers; take `&QueueGuard` |
| `commands/music/gp.rs` | 4 (`:2253, :2473, :3003, :3417`) | the owner; acquire `as_ Game` |
| `commands/music/skip.rs` | 3 (`:39, :118, :119`) | call helpers |
| `commands/music/shuffle.rs` | 2 (`:42, :71`) | call helpers |
| `commands/music/remove.rs` | 1 (`:72`) | call helper |
| `commands/music/clear.rs` | 1 (`:42`) | call helper |
| `handlers/track_end.rs` | 1 (`:138`) | event handler; see below |

**No plumbing is required to reach `Data` at any of these.** Verified:

- `TrackEndHandler` (`track_end.rs:36-42`) and `ModifyQueueHandler` (`:46-52`)
  each already carry `guild_id`, `data: Arc<Data>`, `cache`, `http`, `call`.
  `track_end.rs:130` already calls `self.data.gp_remove_if_parked(..)` three
  lines above the `pause()` that needs guarding.
- `GpPlayback` (`gp.rs:2155-2160`) carries `data: Arc<Data>`, `http`, `call`,
  `guild_id`. `gp_abort` already calls `pb.data.gp_remove(..)` immediately before
  its `queue().stop()`.
- `gp.rs:3417` has `data` and `guild_id` in scope one line earlier
  (`data.gp_park_for_end(..)`).
- The seven `music/queue.rs` sites need no `Data`: they *take* `&QueueGuard`, and
  the caller — which always has `ctx`/`data` — acquires it.

### What the funnel does NOT cover

The claim "forgetting to list a command becomes a worse error message" holds
only for commands that reach the queue. **Five of the twenty never do:**

| command | what it actually touches |
|---|---|
| `leave` | `manager.remove(guild_id)` — songbird voice state (`leave.rs:38`) |
| `summon` | joins a voice channel |
| `summonchannel` | joins a voice channel |
| `seek` | seeks the current `TrackHandle` (`seek.rs`) — track state, not queue structure |
| `repeat` | toggles looping on the current `TrackHandle` (`repeat.rs:36` reads only `handler.queue().current()`) — track state, not queue structure |

A `QueueGuard` cannot gate `leave`, `summon` or `summonchannel`; they change
voice membership, which is `join_vc_tokens`' territory. `seek` and `repeat`
mutate a single track in place rather than queue structure, so a guard would
not add anything a `TrackHandle` doesn't already own. All five stay guarded by
`GP_BLOCKED_COMMANDS` and the `cmd_check_music` check, and a future command of
either shape can still be forgotten.

So the honest claim is: **the funnel converts the bug class for queue mutation
(15 of 20), and leaves it intact for voice-state and track-state changes (5 of
20).** Closing the voice-state remainder is what subsuming `JoinVCToken` into
the lease would buy — listed under non-goals, and this is the argument for
eventually doing it.

### Phase C — report from the insertion result

Have the enqueue path return the positions it inserted at, and build the reply
from that rather than re-reading `current_queue()` afterwards. A reply describing
what *this* call did cannot be corrupted by a concurrent one.

This is not merely bundled: it **shrinks the critical section**. If the reply is
derived from the insertion result, exclusion need only cover the enqueue. If the
reply re-reads shared state, exclusion must span the enqueue *and* the read.

It does not fix `queue_query_list_offset`'s check-then-act; that needs the lease.

## Lock ordering

The lease sits **alongside** `join_vc_tokens` (`lib.rs:378`), not replacing it,
so this work does not touch functioning join code. That means two per-guild
locks, which is a textbook deadlock if ever taken in opposite orders.

**Order: playback lease first, join token second, never the reverse.** Carry a
`debug_assert`, not only a comment.

Subsuming `JoinVCToken` into the lease — one lock per guild, deadlock impossible
by construction — is a deliberate follow-up once the lease has proven itself. Not
part of this.

## What stays outside the lease

**Resolution.** `resolve_track_many` shells out to `yt-dlp` and takes eight to
fifteen seconds cold. It touches no shared state, so it must not be inside the
critical section — a lease held across it would make a second `/play` wait
fifteen seconds, which is barely an improvement on the bug. **Resolve first, then
take the guard for the enqueue and the reply.**

**Persistence.** The lease is process-local and is not written to Postgres. A
resume (`gp_restore`) reclaims it before restoring anything into the guild, which
is the arbitration #431 needed: a resumed game and a restored queue cannot both
take the voice channel, and `claim_playback` is where that is decided rather than
in ad-hoc ordering.

## Error handling

- `lock_queue` refusing on ownership returns the existing
  `CrackedError::GameInProgress`, so the message users see is unchanged.
- ⚠️ That error currently reaches users only from a command *body*. Raised from a
  poise **check** it becomes `FrameworkError::CommandCheckFailed`, which
  `on_error` (`config.rs:32`) has no arm for — see **#467**. Any lease refusal
  raised from a check inherits that silence. #467 should land first or alongside;
  this design does not fix it.
- `release_playback` is infallible and idempotent. Releasing a guild that owns
  nothing is a no-op, not an error: the event-driven paths can each arrive first.

## Testing

- **Ownership/lease unit tests** on `Data` with no Discord: claim, double-claim
  refused, release, release-when-free, and `lock_queue` refusing a non-owner
  while a game owns playback.
- **The invariant test that matters:** for each of the five game-lifecycle
  methods, assert the lease and `gp_games` agree afterwards. That is the
  regression guard for the drift this design is built to prevent.
- **Phase C:** a test that two interleaved enqueues each report only their own
  insertions — the #333 regression guard, expressible without a live voice
  connection.
- **Lock ordering:** a `debug_assert` plus a test that acquires in the sanctioned
  order and completes.
- No test may reach live YouTube. See `docs/testing.md` and ct#471.

## Non-goals

- Subsuming `JoinVCToken` into the lease.
- Removing `GP_BLOCKED_COMMANDS`. It stays as the early, friendly refusal.
- Persisting the lease.
- Any owner other than `Free` and `Game`. `PlaybackOwner` is an enum so a third
  can be added; adding one now would be speculative.

## Risks

**`gp.rs` collision.** This touches four sites in `gp.rs`, which is 5,651 lines
and has an open refactor issue — **#461**, assigned to ChristianMorton. If #461
starts moving, sequence around it; a rebase across a file split of that size is
expensive.

**The owner's own mutations are the sharp edge.** A bug where `/gp` fails to take
its guard correctly is worse than the bug being closed, because it would be
intermittent rather than deterministic. The four `gp.rs` sites deserve more review
attention than the twelve command sites.

**Scope.** 19 mutation sites, 5 ownership sites, a new guard type, and an
event-handler carve-out. Mechanical, but not small.

**The count moves under you.** `GP_BLOCKED_COMMANDS` went 11 → 19 in four days.
Re-derive both lists at implementation time rather than trusting the tables here:

```
grep -rn "\.enqueue(\|modify_queue\|\.dequeue(\|queue()\.\(skip\|stop\|pause\|resume\)()" \
     --include='*.rs' crack-core/src
grep -rn "gp_games\.\(insert\|remove\|entry\)" --include='*.rs' crack-core/src
```
