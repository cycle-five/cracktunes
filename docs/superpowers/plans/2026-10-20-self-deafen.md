# Self-deafen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CrackTunes joins voice self-deafened and stops decoding incoming voice, and the dead receive code and the inverted self-deafen handler are deleted. Ships as v0.30.1.

**Architecture:** songbird's `DecodeMode` goes from `Decode` to `Pass`. The crate's one join, `music_utils::join_permitted`, runs songbird's two join stages itself instead of calling `Songbird::join`. Stage 1 sends the join over the gateway. While still holding the `Call` lock it then deafens, so the second voice-state update carries the target channel. Stage 2 is awaited after the lock is released. Dead code goes; the `guild_settings.self_deafen` column stays.

**Tech Stack:** Rust, songbird `serenity-next` @ `3fe7289` (0.6), serenity `next` @ `37b9f43`, tokio.

**Spec:** no design doc. The design is the owner-approved patch (2026-10-20), restated here because an executor sees only this file:

- `DecodeMode::Pass`: nothing in the bot reads incoming voice. Under `Pass` songbird neither decrypts nor decodes, which also ends the harmless `RTCP decryption failed` warning.
- Join self-deafened. Discord stops sending a deafened member voice audio (RTP).
- Delete:
  - `crack-core/src/handlers/voice.rs`, a voice-receive example adaptation nothing has called since v0.3.11;
  - the inverted handler. It acts only when the guild setting is `false` (every guild has `true`), then *server*-deafens the bot with `edit_member(..).unwrap()`.
- Any future voice feature undeafens only the call that needs it.

## Global Constraints

- **`join_permitted` stays the only door into voice.** `clippy.toml` bans `Songbird::join`, `Songbird::join_gateway`, `Call::join` and `Call::join_gateway`. The one permitted call site carries `#[allow(clippy::disallowed_methods)]` with a comment saying why. Do not add a second.
- **Never deafen a `Call` before stage 1.** `Call::deafen` calls `update()`, which sends a voice-state update for `self.connection`'s channel, and on a fresh `Call` that is `None`. Discord reads op 4 with `channel_id: null` as "leave voice". After a restart the bot can still be shown in a channel, and the resulting `VOICE_STATE_UPDATE` (bot, no channel) runs `SerenityHandler::on_voice_state_update`'s disconnect cleanup in the middle of the `/gp` resume rejoin.
- **Release the `Call` lock before awaiting stage 2**, exactly as songbird's `Songbird::join_inner` does (`manager.rs:247-260`). The gateway events that complete the join need that lock.
- **Do not drop or rename the `guild_settings.self_deafen` column**, and keep `GuildSettings::self_deafen`. Images v0.30.0 and older name the column in their INSERT (`db/guild.rs:198`), so dropping it breaks rollback.
- Tests that build a `Call` through a `Songbird` manager must first create songbird's scheduler on a runtime that outlives the test (the trap documented on `music::ops::test_support::standalone_call`).
- CI runs `cargo fmt --all -- --check`, `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`, and `cargo test --workspace`. All three must pass.
- Never `git add -A`; add paths explicitly. Commit trailer: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`, and no other Co-Authored-By line.

## Review Focus

1. **The first join on a fresh `Call` must never send `channel_id: None`.** Pinned by `first_join_ends_self_deafened_and_never_sends_a_leave` (Task 1).
2. **A join whose stage 1 fails must not try to deafen.** If `Call::join` returns `Err`, return it unchanged. There is nothing to deafen in, and `deafen` would send the `None` update. Enforced by the `joining.is_ok()` guard in Task 1. Stage 1 fails only if the shard sender is gone, which can't be produced offline, so this one is reviewed by reading.
3. **A failed `deafen` must not fail the join.** It logs at `warn` and the join proceeds; an undeafened bot is no worse than today. Reviewed by reading Task 1's helper.
4. **Removing `BotConfig::self_deafen` must not break loading an existing config that still has the key.** `BotConfig` has no `deny_unknown_fields`, so serde ignores it. Pinned by `bot_config_ignores_a_leftover_self_deafen_key` (Task 2).
5. **The recovered-join arm** of `join_permitted` (where the handshake errs but `connected_call` finds the connection up) must still return that call. The restructure must leave the `match` on the join result untouched apart from where the result comes from.

---

### Task 1: Decode nothing, join self-deafened

**Files:**
- Modify: `crack-core/src/config.rs:20,291-293`, then add a `#[cfg(test)] mod tests` at the end
- Modify: `crack-core/src/commands/music_utils.rs:173-199` (the `join_permitted` doc and its join), then add a `#[cfg(test)] mod tests` at the end
- Modify: `crack-core/src/music/ops/test_support.rs:20-48` (extract the scheduler set-up)

**Interfaces:**
- Produces: `pub(crate) fn songbird_config() -> songbird::Config` in `config.rs`. The `Songbird::serenity_from_config` call at `:293` uses it.
- Produces: `async fn join_deafened(manager: &songbird::Songbird, guild_id: GuildId, channel_id: ChannelId) -> Result<Arc<Mutex<Call>>, JoinError>` (private) in `music_utils.rs`. It replaces the `manager.join(guild_id, channel_id)` at `:197`, and the `#[allow(clippy::disallowed_methods)]` moves into it.
- Produces: `pub fn lasting_scheduler()` in `test_support.rs`. It holds the `LASTING` runtime body that `standalone_call` currently inlines, and `standalone_call` calls it.

- [ ] **Step 1: Write the failing decode-mode test** in `config.rs`:

```rust
#[test]
fn songbird_decodes_nothing() {
    assert_eq!(songbird_config().decode_mode, songbird::driver::DecodeMode::Pass);
}
```

- [ ] **Step 2: Write the failing join test** in `music_utils.rs`, with a helper that builds an offline manager:

```rust
const G: GuildId = GuildId::new(7);
const C: ChannelId = ChannelId::new(70);

/// An offline manager whose gateway messages land in the returned receiver.
/// Nothing answers a join, so it times out after 50 ms.
async fn offline_manager() -> (Arc<songbird::Songbird>, futures::channel::mpsc::UnboundedReceiver<ShardRunnerMessage>)
// body: test_support::lasting_scheduler(); Songbird::serenity_from_config(
//   songbird::Config::default().gateway_timeout(Some(Duration::from_millis(50))));
//   initialise_client_data(1, UserId::new(2)); then `register_shard(0, tx).await`
//   from the serenity::gateway::VoiceGatewayManager trait (an async trait method).

/// (channel, self_deaf) of every voice-state update sent for G, in order.
fn voice_updates(rx: &mut UnboundedReceiver<ShardRunnerMessage>) -> Vec<(Option<ChannelId>, bool)>
// drain with try_next() until it yields Err/None; keep only UpdateVoiceState for G.

#[tokio::test]
async fn first_join_ends_self_deafened_and_never_sends_a_leave() {
    let (manager, mut rx) = offline_manager().await;
    let err = join_deafened(&manager, G, C).await.expect_err("nothing answers offline");
    assert!(matches!(err, JoinError::TimedOut), "{err:?}");
    let sent = voice_updates(&mut rx);
    assert!(sent.iter().all(|(ch, _)| *ch == Some(C)), "a leave was sent: {sent:?}");
    assert_eq!(sent.last(), Some(&(Some(C), true)), "{sent:?}");
}
```

- [ ] **Step 3: Run both tests and confirm they fail.**
Run: `cargo test -p crack-core songbird_decodes_nothing`, then `cargo test -p crack-core first_join_ends_self_deafened`.
Expected: compile errors (`songbird_config` and `join_deafened` not found). That counts as failing.

- [ ] **Step 4: Implement.**
  - `songbird_config()` returns `songbird::Config::default().decode_mode(DecodeMode::Pass)`. Its doc comment says nothing reads incoming voice, so it is neither decrypted nor decoded.
  - `join_deafened` follows songbird's `join_inner`:
    1. `manager.get_or_insert(guild_id)`;
    2. lock, then `handler.join(channel_id).await` (the permitted call, with the `#[allow]` and its comment);
    3. if that is `Ok` and `!handler.is_deaf()`, `handler.deafen(true).await`, logging any error at `warn` and carrying on;
    4. drop the lock, then await the `Join` and map `Ok(())` to the call.

    Its doc comment states the two constraints from Global Constraints: no deafen before stage 1, and the lock released before stage 2.
  - In `join_permitted`, replace the `manager.join` line with `join_deafened(manager, guild_id, channel_id).await`. Update the doc comment at `:173-186`: "the crate's only caller of `Songbird::join`" becomes "the crate's only way into voice", and add that it joins self-deafened. Leave the `match joined` arms as they are.
  - In `test_support.rs`, extract `lasting_scheduler()` and have `standalone_call` call it.

- [ ] **Step 5: Run the two tests.** Both pass.

- [ ] **Step 6: Sabotage each test and restore it.** Record the results for the PR's mutation table:
  - (a) delete the `deafen` call: the join test fails on `last()`;
  - (b) move the `deafen` before `handler.join`: the join test fails on "a leave was sent";
  - (c) `DecodeMode::Decode(Default::default())`: `songbird_decodes_nothing` fails.

  After restoring, `git diff --stat` shows only the intended files.

- [ ] **Step 7: Run the gates.** Each must pass:
  - `cargo fmt --all -- --check`;
  - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`;
  - `cargo clippy -p crack-core --tests -- -D warnings`;
  - `cargo test -p crack-core`.

- [ ] **Step 8: Commit.**

```bash
git add crack-core/src/config.rs crack-core/src/commands/music_utils.rs crack-core/src/music/ops/test_support.rs
git commit -m "fix(voice): join self-deafened and decode nothing"
```

### Task 2: Delete the dead receiver and the inverted self-deafen handler

**Files:**
- Delete: `crack-core/src/handlers/voice.rs`; `crack-core/src/commands/settings/toggle/self_deafen.rs`
- Modify:
  - `crack-core/src/handlers/mod.rs:7,15` (the `pub mod voice;` and the commented `//pub use self::voice::...` line);
  - `crack-core/src/handlers/serenity.rs:357-373` (the `if new.channel_id.is_some()` body), `:599-616` (`async fn self_deafen`), and imports that go unused (`EditMember`);
  - `crack-core/src/commands/settings/toggle/mod.rs` (drop `self_deafen` and `selfdeafen()`);
  - `crack-core/src/guild/settings.rs:372` (doc comment) and `:653-657` (delete `toggle_self_deafen`);
  - `crack-core/src/lib.rs:213,233,260` (remove `BotConfig::self_deafen`, its default and its `Display` line).
- Test: `crack-core/src/lib.rs` (the existing `#[cfg(test)]` module there, or a new one)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: nothing used by Task 3.

- [ ] **Step 1: Write the serde-compatibility test.** Deserialize a `BotConfig` from the minimal input the existing config loader accepts, plus `"self_deafen": true` (or the TOML equivalent). Find which format the loader uses and how `BotConfig` is read in the binary crate before choosing. Assert that it parses (`is_ok()`). It passes today; it guards against a future `deny_unknown_fields`.
- [ ] **Step 2: Delete and edit as listed under Files.**
  - In `on_voice_state_update`, the `if new.channel_id.is_some() { … }` block becomes `if new.channel_id.is_some() { return; }`, with a one-line comment that only a disconnect (no channel) needs handling here.
  - Delete the stale Q&A comment block below it, the one claiming the update arrives "because the bot is deafened". Keep the 🪤 `remove` comment and the code it describes.
  - The doc comment on `GuildSettings::self_deafen` says it is unused since v0.30.1, because CrackTunes always joins self-deafened, and that the column stays so older images (which write it) can still roll back.
- [ ] **Step 3: Confirm nothing else referenced what was deleted.** `rg -n "handlers::voice|register_voice_handlers|toggle_self_deafen|selfdeafen|bot_settings\.self_deafen" crack-*/src` returns no hits.
- [ ] **Step 4: Run the gates** from Task 1 Step 7, plus `cargo build --workspace`. All pass.
- [ ] **Step 5: Commit.**

```bash
git add -u crack-core/src
git commit -m "refactor(voice): delete the dead receiver and the inverted self-deafen handler"
```

(`git add -u` stages only tracked files, so it picks up the deletions without sweeping in anything new. Run `git status` first and confirm.)

### Task 3: Docs, changelog, version

**Files:**
- Modify: `docs/rooms-and-mod-log.md:298-300`
- Modify: `CHANGELOG.md`, under `## Unreleased`: `### Changed` and `### Fixed`
- Modify: `Cargo.toml:24` (`version = "0.30.0"` becomes `"0.30.1"`); `Cargo.lock` (workspace members' versions)

- [ ] **Step 1: Fix the mod-log doc.** It says a server mute or deafen "that CrackTunes applies to itself (it deafens itself when it joins a channel)" is not logged. Rewrite it so that:
  - CrackTunes self-deafens when it joins, like pressing the headphones button;
  - like every self-deafen, that is never logged (the next section already says self-deafens aren't);
  - a server mute or deafen CrackTunes applied to itself would also not be logged. This is still true: `modlog/live.rs:75` skips actor == bot == target.
- [ ] **Step 2: Add the CHANGELOG entries.**
  - **Changed:** CrackTunes joins voice deafened and no longer decrypts or decodes what it receives (nothing used it), so it shows the deafened icon.
  - **Fixed:** the self-deafen handler never fired and, where it would have, server-deafened the bot and panicked without the permission. It is removed, along with the unregistered `/toggle selfdeafen` and the unused voice receiver.
- [ ] **Step 3: Bump the version.** Set `Cargo.toml` `[workspace.package] version = "0.30.1"`, then run `cargo build --workspace` so `Cargo.lock` follows. `git diff Cargo.lock` shows only `0.30.0` becoming `0.30.1` for the workspace's own crates.
- [ ] **Step 4: Commit.**

```bash
git add docs/rooms-and-mod-log.md CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "v0.30.1: join voice self-deafened, decode nothing"
```
