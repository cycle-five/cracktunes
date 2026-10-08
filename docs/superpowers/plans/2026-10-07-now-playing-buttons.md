# Now-Playing Buttons (PR 2, v0.23.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The floating now-playing status message carries one row of buttons (Pause/Resume, Skip, Repeat, Shuffle) that anyone allowed to run the music slash commands can press. A press runs the same control the dashboard runs, and posts an echo line only if the guild's new `control_echoes` setting is on (`/echoes` toggles it). A control that changed nothing posts no echo.

**Architecture:**
- **Ids name intent.** `messaging::buttons::NowPlayingButton` formats and parses `np:` custom ids. The ids carry the guild, and Skip also carries the track it was drawn for, so a press works after a restart and a stale Skip cannot double-skip.
- **The card carries its controls.** `NowPlayingCard` gains `controls: Option<Controls>`, and `cards::now_playing` renders the row from it. Only the status message's card has controls: the `/play` reply and `/grab`'s DM do not.
- **Answering a press goes through a seam.** A new `Press` trait (acknowledge, then a follow-up) sits beside `Transport` in `messaging::transport`, with `DiscordPress` for the real thing and `FakePress` for tests. The courier exposes `acknowledge` and `answer_privately` over it.
- **One access rule, one control path.** Access is `commands::permissions::music_access`, extracted from `cmd_check_music` so the slash check and the buttons cannot drift. The control is `music::remote::control`, generalised by a `Via` (`Dashboard` or `Button`) that picks the audit actor and the echo wording.

**Tech Stack:** Rust 2021; serenity `next` (rev 37b9f43); poise `serenity-next`; songbird (rev 3fe7289); sqlx 0.9 (offline `.sqlx` cache); tokio; `uuid`; `async-trait`.

**Spec:** `docs/superpowers/specs/2026-10-07-messaging-layer-and-now-playing-buttons-design.md`. Read its "Architecture" section and its "PR 2" section before starting any task. PR 1 (#586, v0.22.0, master `c412741b`) built the layer this plan stands on.

## Global Constraints

- **Branch:** `feat/now-playing-buttons`, cut from master at `c412741b`, where PR 1 is already merged. PR 2 is therefore *not* stacked, unlike the spec's release section assumed.
- **Formatting:** the crates are edition 2021. Format with `cargo +nightly fmt --all`, never bare `rustfmt --edition 2024`.
- **Lint:** `cargo clippy --workspace --all-targets` must be clean. Use `#[expect(lint, reason = "…")]`, never `#[allow]`. Raw serenity sends and interaction responses are banned outside `messaging` (`clippy.toml`). The only new exemptions are in `messaging::transport`, with the reason `"messaging is where sends are made"`.
- **Commits:** every commit message ends with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`, and has no other Co-Authored-By line.
- **Staging:** never `git add -A` or `git add .`. Stage named paths only.
- **Serialization:** typed serde only. Never `serde_json::json!` or `serde_json::Value` for data we own. Tests may read a serialized serenity builder (`CreateEmbed`, `CreateComponent`, `CreateInteractionResponseFollowup`) as a `Value`, because that is a third-party shape.
- **Strings:** user-facing strings are constants in `crack-core/src/messaging/messages.rs`. Log text stays inline.
- **Tests:**
  - every new test is shown to fail once by breaking the code it covers, then restored (the standing rule). Report each sabotage and what caught it;
  - wording is pinned against **literal strings**, never against the constant itself;
  - tests assert on what is **sent**, through `FakeTransport`, `FakeReplies` and the new `FakePress`.
- **`get_info`:** `TrackHandle::get_info()` never answers on an offline `Call::standalone`. Every read is bounded by `crate::music::ops::TRACK_INFO_TIMEOUT` (1 s), and tests that touch it carry an outer `tokio::time::timeout`.
- **Test calls:** build songbird calls only through `music::ops::test_support::{queue_of, offline_call, standalone_call}`. clippy bans bare `Call::standalone`.
- **Database tests:** they run only with the `db-tests` feature, against `postgresql://postgres:mysecretpassword@localhost:5432/postgres`. A ctor overwrites `DATABASE_URL`, so no other address works. Start one with `docker --context default run -d --rm --name pg -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine`. Bare `docker` on this machine targets a remote host, so the `--context default` matters.
- **Wording:** the button labels, echo lines and setting replies are exactly the strings this plan gives.

## Rulings (plan-level; the spec is the authority)

1. **`Press` is a seam of its own, not part of `Transport`.** `Transport` addresses channels, but a press is answered through its interaction token. `Press` is bound to one interaction, as `ReplySink` is bound to one command. The spec's `Destination::Interaction` therefore becomes `courier::acknowledge` and `courier::answer_privately` over `Press`. The intent is kept: one path, faked in tests. If this is wrong, it costs moving two methods.
2. **The echo gate lives in `courier::post`'s `Destination::Echo` arm.** It covers both the dashboard and the buttons in one place, as the spec's `Destination` section says.
3. **"Changed nothing" is decided from the track's state read *before* the op** (`(paused, looping)`, bounded). If that state is unknown, the control counts as a change and echoes, which is the old behaviour. A shuffle of fewer than two upcoming tracks changes nothing. The op itself still runs and the status still settles. Only the echo is skipped.
4. **Button echoes have no source suffix.** "⏭ Skipped **t0** — <@42>". Dashboard echoes keep "from the dashboard".
5. **`music_access` refuses a bot or an unauthorised member with `CrackedError::UnauthorizedUser`.** The slash check maps exactly that error back to `Ok(false)`, which keeps today's silent check failure. Every other refusal keeps its words.
6. **The ack comes first, for every `np:` press, even a malformed one**, so Discord never shows "This interaction failed".

## Review Focus

These are the inputs most likely to bite someone pressing the buttons. The spec implies them, but no single happy-path test covers them. Each is pinned in the task noted.

1. **A press on an old status message:** an earlier song's Skip, or Pause after playback stopped. It must answer the presser privately ("That track is no longer playing" / "🔈 Nothing is playing!") and change nothing. Tasks 4 and 7.
2. **Two members pressing Skip on the same song at once.** Only one skip may happen. The second is refused as stale, because the id carries the track. Tasks 4 and 7.
3. **A forged or malformed id:** `np:skip:1:not-a-uuid`, `np:pause:0`, `np:pause:1:extra`, `np:`, `np:dance:1`, or a guild id that is not the guild the press came from. It must never panic (note that `GuildId::new(0)` panics), never run a control, and must say "This button is out of date." Tasks 5 and 7.
4. **A press while `/gp` owns playback, or from outside the music channel.** It must be refused privately in the slash command's words. No control runs, and no echo is posted. Tasks 6 and 7.
5. **A driver that never answers `get_info`.** The status must still render within about 1 s, with the default buttons (Pause, and Repeat off), and the before-read must not hang the press. Tasks 4 and 5.

---

## File structure

| File | Responsibility | Task |
|---|---|---|
| `migrations/20261007120000_control_echoes.sql` + the same file in `crack-core/test_migrations/` (new) | the `control_echoes` column | 1 |
| `crack-core/src/guild/settings.rs` | `control_echoes` field, default, equality, row conversion, toggle | 1 |
| `crack-core/src/db/guild.rs`, `.sqlx/*.json` | the row struct, the upsert, the offline query cache | 1 |
| `crack-core/src/guild/operations.rs` | `get_control_echoes`, `toggle_control_echoes` | 1 |
| `crack-core/src/commands/music/echoes.rs` (new), `commands/music/mod.rs` | `/echoes`, registered | 2 |
| `crack-core/src/messaging/{message,messages}.rs` | new variants and strings | 2, 5, 7 |
| `crack-core/src/messaging/courier.rs` | the echo gate; `acknowledge`, `answer_privately` | 2, 6 |
| `crack-core/src/music/audit.rs`, `music/audit_view.rs`, `crack-web/src/{page,history}.rs`, `docs/queue-audit.md` | `Source::Button`, `Actor::button`, the `button` filter | 3 |
| `crack-core/src/messaging/cards.rs` | `Via::Button`, `Echo::line(user, via)`, controls on the card | 4, 5 |
| `crack-core/src/music/remote.rs`, `crack-web/src/lib.rs` | `control(.., via, ..) -> Option<Echo>`, the before-read, `changes` | 4 |
| `crack-core/src/messaging/buttons.rs` (new) | `NowPlayingButton`, `Controls`, `now_playing_row`; `respond`, `handle` | 5, 7 |
| `crack-core/src/messaging/{interface,status}.rs` | `now_playing_status_card`; the status shows the controls | 5 |
| `crack-core/src/messaging/{transport,render,test_support}.rs` | `Press`, `DiscordPress`, `Rendered::to_followup`, `FakePress` | 6 |
| `crack-core/src/commands/permissions.rs` | `music_access`; `cmd_check_music` calls it | 6 |
| `crack-core/src/handlers/serenity.rs` | routes `np:` presses to `buttons::handle` | 7 |
| `Cargo.toml` (workspace version), `Cargo.lock`, `CHANGELOG.md` | release | 8 |

---

### Task 1: The `control_echoes` guild setting (storage and operations)

**Files:**
- Create: `migrations/20261007120000_control_echoes.sql` and `crack-core/test_migrations/20261007120000_control_echoes.sql` (identical; the db tests migrate from `test_migrations`)
- Modify: `crack-core/src/guild/settings.rs`: the field near `ephemeral_replies` (line ~349), `PartialEq` (~388), `From<GuildSettingsRead>` (~470), `GuildSettings::new` (~521), a toggle beside `toggle_ephemeral_replies` (~621), and the test at ~1273 that builds a `GuildSettingsRead`
- Modify: `crack-core/src/db/guild.rs`: `GuildSettingsRead` (line ~37) and the upsert in `write_settings` (~194)
- Modify: `.sqlx/`: the 4 files that return a whole `guild_settings` row; the upsert's file is replaced by one with a new hash
- Modify: `crack-core/src/guild/operations.rs`: the trait (~65) and the impl (~443)
- Test: `settings.rs` tests, `operations.rs` tests, a new `control_echoes_db_tests` module in `db/guild.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `GuildSettings.control_echoes: bool` (default `true`)
  - `GuildSettings::toggle_control_echoes(&mut self) -> &mut Self`
  - `GuildSettingsRead.control_echoes: bool`
  - `GuildSettingsOperations::get_control_echoes(&self, GuildId) -> impl Future<Output = bool>`: **true** when the guild has no settings loaded
  - `GuildSettingsOperations::toggle_control_echoes(&self, GuildId) -> impl Future<Output = Result<bool, CrackedError>>`: returns the new value

- [ ] **Step 1: Write the migration** (both paths, same bytes):

```sql
-- Whether a dashboard control or a now-playing button press posts an echo
-- line ("⏭ Skipped **Title** — @user") in the channel. On by default, the
-- behaviour before v0.23.0. See
-- docs/superpowers/specs/2026-10-07-messaging-layer-and-now-playing-buttons-design.md.
ALTER TABLE guild_settings
    ADD COLUMN IF NOT EXISTS control_echoes BOOLEAN NOT NULL DEFAULT TRUE;
```

- [ ] **Step 2: Write the failing settings tests** in `settings.rs`'s test module, beside `ephemeral_replies_are_off_by_default`:

```rust
#[test]
fn control_echoes_are_on_by_default() {
    assert!(GuildSettings::new(GuildId::new(123), None, None).control_echoes);
}

#[test]
fn toggling_control_echoes_flips_them() {
    let mut settings = GuildSettings::new(GuildId::new(123), None, None);
    settings.toggle_control_echoes();
    assert!(!settings.control_echoes);
    settings.toggle_control_echoes();
    assert!(settings.control_echoes);
}

#[test]
fn a_database_row_carries_control_echoes() {
    let row = crate::db::GuildSettingsRead {
        guild_id: 123,
        guild_name: "guild".to_string(),
        prefix: "r!".to_string(),
        premium: false,
        autopause: false,
        allow_all_domains: true,
        allowed_domains: vec![],
        banned_domains: vec![],
        ignored_channels: vec![],
        old_volume: 1.0,
        volume: 1.0,
        self_deafen: true,
        timeout_seconds: Some(360),
        additional_prefixes: vec![],
        ephemeral_replies: false,
        control_echoes: false,
    };
    assert!(!GuildSettings::from(row).control_echoes);
}

#[test]
fn settings_differing_only_in_control_echoes_are_not_equal() {
    let on = GuildSettings::new(GuildId::new(123), None, None);
    let mut off = on.clone();
    off.control_echoes = false;
    assert_ne!(on, off);
}
```

Also add `control_echoes: true,` to the existing `a_database_row_carries_ephemeral_replies` row (it will not compile without it).

> The field's `#[serde(default = "default_true")]` gets no test of its own. Testing it would mean editing our own serialized settings as a `serde_json::Value`, which the typed-serde rule forbids. It is the same attribute `autoplay` uses, and the database column's `DEFAULT TRUE` is what production loads from.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p crack-core --lib guild::settings 2>&1 | tail -20`
Expected: compile errors (no field `control_echoes`, no method `toggle_control_echoes`).

- [ ] **Step 4: Implement in `settings.rs`**

```rust
    /// Whether a dashboard control or a now-playing button press posts an
    /// echo line in the channel.
    #[serde(default = "default_true")]
    pub control_echoes: bool,
```
Add `&& self.control_echoes == other.control_echoes` to `PartialEq`; add `settings.control_echoes = settings_db.control_echoes;` in `From<GuildSettingsRead>`; add `control_echoes: true,` in `GuildSettings::new`; and add the toggle:

```rust
    /// Toggle whether controls (dashboard, buttons) echo in the channel.
    pub fn toggle_control_echoes(&mut self) -> &mut Self {
        self.control_echoes = !self.control_echoes;
        self
    }
```

In `db/guild.rs`, add `pub control_echoes: bool,` after `ephemeral_replies` in `GuildSettingsRead`, and extend the upsert in `write_settings`. Add `control_echoes` to the column list, `$16` to `VALUES`, and `, control_echoes = $16` to `DO UPDATE SET`. Pass `settings.control_echoes,` as the last argument. The query text must read exactly:

```rust
            r#"
            INSERT INTO guild_settings (guild_id, guild_name, prefix, premium, autopause, allow_all_domains, allowed_domains, banned_domains, ignored_channels, old_volume, volume, self_deafen, timeout_seconds, additional_prefixes, ephemeral_replies, control_echoes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::FLOAT, $11::FLOAT, $12, $13, $14, $15, $16)
            ON CONFLICT (guild_id)
            DO UPDATE SET guild_name = $2, prefix = $3, premium = $4, autopause = $5, allow_all_domains = $6, allowed_domains = $7, banned_domains = $8, ignored_channels = $9, old_volume = $10::FLOAT, volume = $11::FLOAT, self_deafen = $12, timeout_seconds = $13, additional_prefixes = $14, ephemeral_replies = $15, control_echoes = $16
            "#,
```

- [ ] **Step 5: Update the offline query cache.**

`query_as!` checks the `.sqlx` files when `SQLX_OFFLINE=true` (CI's lint leg). Each file's name is the sha256 of its query text, and the JSON round-trips byte-for-byte through Python's `json.dumps(indent=2, ensure_ascii=False) + "\n"`. Do not run the local `sqlx` CLI: it is 0.8.6 against a 0.9 crate, and it rewrites every file. Save this script in the session scratchpad (not the repo) and run it from the repo root:

```python
import glob, hashlib, json, os

def dump(path, d):
    with open(path, "w") as f:
        f.write(json.dumps(d, indent=2, ensure_ascii=False) + "\n")

rows = 0
for path in sorted(glob.glob(".sqlx/query-*.json")):
    d = json.load(open(path))
    cols = d["describe"]["columns"]
    names = [c["name"] for c in cols]
    # Queries that return a whole guild_settings row: ALTER TABLE appends,
    # so the new column is last.
    if "ephemeral_replies" in names and "control_echoes" not in names:
        assert names[-1] == "ephemeral_replies", path
        cols.append({"ordinal": len(cols), "name": "control_echoes", "type_info": "Bool"})
        d["describe"]["nullable"].append(False)
        dump(path, d)
        rows += 1
    # The upsert: its text changes, so its file (named by hash) is replaced.
    elif "ephemeral_replies = $15" in d["query"] and "control_echoes" not in d["query"]:
        q = d["query"]
        q = q.replace("additional_prefixes, ephemeral_replies)", "additional_prefixes, ephemeral_replies, control_echoes)")
        q = q.replace("$14, $15)", "$14, $15, $16)")
        q = q.replace("ephemeral_replies = $15\n", "ephemeral_replies = $15, control_echoes = $16\n")
        assert q.count("control_echoes") == 2 and "$16)" in q, q
        d["query"] = q
        d["describe"]["parameters"]["Left"].append("Bool")
        d["hash"] = hashlib.sha256(q.encode()).hexdigest()
        os.remove(path)
        dump(f".sqlx/query-{d['hash']}.json", d)
        print("upsert ->", d["hash"])
print("row queries updated:", rows)
```

Expected output: `upsert -> <hash>` and `row queries updated: 4`. Then check that the new upsert file's `query` is byte-identical to the Rust string. The macro looks the file up by the sha256 of the literal, so `SQLX_OFFLINE=true cargo check -p crack-core --all-targets` fails with "no cached data for this query" if they differ by even a space. `git status --short .sqlx` should show 4 modified, 1 deleted and 1 untracked file, and nothing else.

- [ ] **Step 6: Write the failing operations tests** in `operations.rs`'s test module:

```rust
#[tokio::test]
async fn control_echoes_are_on_for_a_guild_with_no_settings() {
    let data = crate::Data::default();
    assert!(data.get_control_echoes(GuildId::new(123)).await);
}

#[tokio::test]
async fn control_echoes_follow_the_guild_setting() {
    let data = crate::Data::default();
    let guild_id = GuildId::new(123);
    let mut settings = GuildSettings::new(guild_id, None, None);
    settings.control_echoes = false;
    data.guild_settings_map
        .write()
        .await
        .insert(guild_id, settings);
    assert!(!data.get_control_echoes(guild_id).await);
}

#[tokio::test]
async fn toggling_control_echoes_flips_it_and_reports_the_new_value() {
    let data = crate::Data::default();
    let guild_id = GuildId::new(123);
    assert!(!data.toggle_control_echoes(guild_id).await.unwrap());
    assert!(!data.get_control_echoes(guild_id).await);
    assert!(data.toggle_control_echoes(guild_id).await.unwrap());
    assert!(data.get_control_echoes(guild_id).await);
}
```

Run: `cargo test -p crack-core --lib guild::operations 2>&1 | tail -20`. Expected: compile errors (no such methods).

- [ ] **Step 7: Implement the operations.** Add these to the trait, beside the `ephemeral_replies` pair:

```rust
    fn get_control_echoes(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn toggle_control_echoes(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<bool, CrackedError>>;
```
Then add these to the impl:

```rust
    /// Whether controls (dashboard, buttons) post an echo line. On when the
    /// guild has no settings loaded: echoes were always on before v0.23.0.
    async fn get_control_echoes(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .is_none_or(|settings| settings.control_echoes)
    }

    /// Flip whether controls echo, save it, and return the new value.
    ///
    /// 🔑 As with `toggle_ephemeral_replies`: load the stored row first, or
    /// the full-row upsert writes defaults over it.
    async fn toggle_control_echoes(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.ensure_settings_loaded(guild_id).await?;
        let settings = self
            .guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|settings| {
                settings.toggle_control_echoes();
            })
            .or_insert_with(|| {
                let mut settings =
                    GuildSettings::new(guild_id, Some(&self.bot_settings.get_prefix()), None);
                settings.toggle_control_echoes();
                settings
            })
            .clone();
        if let Some(pool) = self.database_pool.as_ref() {
            settings.save(pool).await?;
        }
        Ok(settings.control_echoes)
    }
```

- [ ] **Step 8: Add the database round-trip test** at the end of `db/guild.rs`. Model it on `ephemeral_replies_db_tests`:

```rust
#[cfg(test)]
mod control_echoes_db_tests {
    use super::*;
    use std::str::FromStr;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    /// Off must survive a restart, and a new guild starts on.
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn control_echoes_survive_a_save_and_load(pool: PgPool) -> Result<(), SerenityError> {
        let name = FixedString::from_str("echo test").expect("a short name");
        let (_guild, mut settings) =
            GuildEntity::get_or_create(&pool, 434343, name, "r!".to_string()).await?;
        assert!(settings.control_echoes);

        settings.control_echoes = false;
        GuildEntity::write_settings(&pool, &settings).await?;

        let reloaded = GuildEntity::new_guild(434343, "echo test".to_string())
            .get_settings(&pool)
            .await?;
        assert!(!reloaded.control_echoes);
        Ok(())
    }
}
```

- [ ] **Step 9: Run everything for this task**

Run:
```bash
cargo test -p crack-core --lib guild:: 2>&1 | grep -E "test result|FAILED|panicked"
SQLX_OFFLINE=true cargo check -p crack-core --all-targets 2>&1 | tail -3
cargo test -p crack-core --features db-tests --lib db::guild 2>&1 | grep -E "test result|FAILED|panicked"
```
Expected: all pass, and the offline check is clean. The third command needs the postgres from Global Constraints. If you cannot start it, say so in the report and do not claim it passed.

- [ ] **Step 10: Sabotage**, one at a time, restoring each:
  - `control_echoes: true` → `false` in `GuildSettings::new` (the default test must fail);
  - `is_none_or` → `is_some_and` (the no-settings test must fail);
  - drop `&& self.control_echoes == other.control_echoes` (the equality test must fail);
  - drop `, control_echoes = $16` from the upsert. The db test must fail. If postgres is unavailable, report this one as not run.

- [ ] **Step 11: Commit**

```bash
git add migrations/20261007120000_control_echoes.sql crack-core/test_migrations/20261007120000_control_echoes.sql \
  crack-core/src/guild/settings.rs crack-core/src/guild/operations.rs crack-core/src/db/guild.rs .sqlx
git commit -m "control_echoes: a guild setting for control echo lines, on by default

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
(`git add .sqlx` stages the deleted upsert file too. It is the one directory you may stage whole, and only after the `git status` check in Step 5.)

---

### Task 2: `/echoes`, and the echo gate in the courier

**Files:**
- Create: `crack-core/src/commands/music/echoes.rs`
- Modify: `crack-core/src/commands/music/mod.rs`: `pub mod echoes;`, `pub use echoes::*;`, and `echoes(),` in `music_commands()` after `diagnose(),`
- Modify: `crack-core/src/messaging/messages.rs`, `crack-core/src/messaging/message.rs`: two variants, appended at the **end** of `CrackedMessage` (`PartialEq` compares discriminants), plus their `Display` arms
- Modify: `crack-core/src/messaging/courier.rs`: the `Destination::Echo` arm
- Test: `echoes.rs`, `message.rs`, `courier.rs` tests

**Interfaces:**
- Consumes: `GuildSettingsOperations::{get_control_echoes, toggle_control_echoes}` (Task 1).
- Produces:
  - `commands::music::echoes::echoes() -> crate::Command`, registered
  - `CrackedMessage::ControlEchoesOn`, `CrackedMessage::ControlEchoesOff`
  - `courier::post(.., Destination::Echo(guild), ..)` returns `None` and sends nothing when the guild's echoes are off

- [ ] **Step 1: Strings** in `messages.rs`, beside `EPHEMERAL_REPLIES_*`:

```rust
pub const CONTROL_ECHOES_ON: &str = "📣 Button and dashboard controls now post a line in the channel.";
pub const CONTROL_ECHOES_OFF: &str = "🔇 Button and dashboard controls no longer post a line in the channel.";
```

- [ ] **Step 2: Write the failing tests.** In `message.rs`'s tests:

```rust
#[test]
fn control_echoes_messages_say_what_changed() {
    assert_eq!(
        CrackedMessage::ControlEchoesOn.to_string(),
        "📣 Button and dashboard controls now post a line in the channel."
    );
    assert_eq!(
        CrackedMessage::ControlEchoesOff.to_string(),
        "🔇 Button and dashboard controls no longer post a line in the channel."
    );
}
```

In `courier.rs`'s tests:

```rust
#[tokio::test]
async fn an_echo_is_not_posted_when_the_guild_turned_echoes_off() {
    use crate::guild::settings::GuildSettings;
    let data = Data(Arc::new(DataInner::default()));
    let guild = GuildId::new(1);
    status::note_command_channel(&data, guild, GenericChannelId::new(10)).await;
    let mut settings = GuildSettings::new(guild, None, None);
    settings.control_echoes = false;
    data.guild_settings_map.write().await.insert(guild, settings);
    let t = FakeTransport::default();
    let at = post(&data, &t, Destination::Echo(guild), &CrackedMessage::Clear, &cx()).await;
    assert_eq!(at, None);
    assert!(t.ops().is_empty(), "{:?}", t.ops());
}
```

The existing `an_echo_lands_where_the_status_would_and_leaves_it_alone` covers the "on" case. Its guild has no settings, which reads as on.

Create `echoes.rs` with only the test module first (copy the shape of `ephemeral.rs`'s test):

```rust
#[cfg(test)]
mod tests {
    use poise::serenity_prelude::all::Permissions;

    /// Unlike `/ephemeral`, this one is registered: an echo is a channel
    /// message, so nothing like #535 stands between the setting and its effect.
    #[test]
    fn echoes_is_an_admin_only_registered_guild_command() {
        let command = super::echoes();
        assert!(command.slash_action.is_some(), "a slash command");
        assert!(command.guild_only, "guild only");
        assert!(command.required_permissions.contains(Permissions::ADMINISTRATOR));
        assert!(command.default_member_permissions.contains(Permissions::ADMINISTRATOR));
        assert!(
            crate::commands::commands_to_register()
                .into_iter()
                .any(|command| command.name == "echoes"),
            "registered"
        );
    }
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p crack-core --lib -- control_echoes echoes_is an_echo_is_not 2>&1 | tail -20`
Expected: compile errors (no `echoes`, no variants).

- [ ] **Step 4: Implement.** Append the variants at the end of `CrackedMessage`, after `Queued(Box<QueuedCard>)`:

```rust
    ControlEchoesOn,
    ControlEchoesOff,
```
Add their `Display` arms:
```rust
            Self::ControlEchoesOn => f.write_str(crate::messaging::messages::CONTROL_ECHOES_ON),
            Self::ControlEchoesOff => f.write_str(crate::messaging::messages::CONTROL_ECHOES_OFF),
```

`echoes.rs` (above its test module):

```rust
use crate::{
    commands::help,
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    messaging::{courier, message::CrackedMessage},
    Context, Error,
};

/// Toggle whether button and dashboard controls post a line in the channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn echoes(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    flag: bool,
) -> Result<(), Error> {
    if flag {
        return help::wrapper(ctx).await;
    }
    echoes_internal(ctx).await
}

/// Flip and save the guild's `control_echoes`, then say which way it went.
#[cfg(not(tarpaulin_include))]
pub async fn echoes_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let on = ctx.data().toggle_control_echoes(guild_id).await?;
    let msg = if on {
        CrackedMessage::ControlEchoesOn
    } else {
        CrackedMessage::ControlEchoesOff
    };
    courier::reply(ctx, msg).await?;
    Ok(())
}
```

In `courier.rs`, add `use crate::guild::operations::GuildSettingsOperations;` and gate the arm:

```rust
        // A guild can turn control echoes off (`/echoes`); the control still ran.
        Destination::Echo(guild) => {
            if !data.get_control_echoes(guild).await {
                return None;
            }
            status::announce(data, transport, guild, out).await
        },
```
Update `Destination::Echo`'s doc comment to: `/// Where Status would land; the tracked status message is left alone. Nothing when the guild's control_echoes is off.`

- [ ] **Step 5: Run them to see them pass**, and check the registered count went from 43 to 44:

```bash
cargo test -p crack-core --lib 2>&1 | grep -E "test result|FAILED|panicked"
cargo test -p crack-core --lib -- --nocapture registered_count_probe 2>/dev/null; true
```
For the count, add a temporary `#[test] fn registered_count_probe() { println!("{}", crate::commands::commands_to_register().len()); }` to `echoes.rs`, run it with `--nocapture`, then **delete it before committing**. Report the number (want 44). Do not keep a test that pins the count: it would break on every new command.

- [ ] **Step 6: Sabotage:**
  - delete the gate's `return None;` (the echo-off test must fail);
  - drop `echoes(),` from `music_commands()` (the registration test must fail);
  - swap the two strings (the wording test must fail).

- [ ] **Step 7: Commit**

```bash
git add crack-core/src/commands/music/echoes.rs crack-core/src/commands/music/mod.rs \
  crack-core/src/messaging/messages.rs crack-core/src/messaging/message.rs crack-core/src/messaging/courier.rs
git commit -m "/echoes: turn control echo lines off or on; the courier honours it

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: The audit log knows a button press (`Source::Button`)

**Files:**
- Modify: `crack-core/src/music/audit.rs`: `Source::Button`, `as_str`, `Actor::button`
- Modify: `crack-core/src/music/audit_view.rs`: `how_text`, `SourceChoice::Button`, `source()`, `from_name`, and the tests at ~800
- Modify: `crack-web/src/page.rs` (~line 148): a `button` option in the source filter
- Modify: `crack-web/src/history.rs` (~line 27): `BAD_SOURCE`
- Modify: `docs/queue-audit.md` (line 4, and a sentence near line 26)
- Test: `audit.rs`, `audit_view.rs`, `crack-web/src/history.rs` tests

**Interfaces:**
- Produces:
  - `Source::Button`, stored as `"button"`
  - `Actor::button(user: UserId, op: &'static str) -> Actor` with `source() == Source::Button` and `command() == "button {op}"`
  - `SourceChoice::Button`
  - `how_text` shows a button row as `"button"`

- [ ] **Step 1: Write the failing tests.** In `audit.rs` (add a `#[cfg(test)] mod tests` if it has none; otherwise extend the existing one):

```rust
#[test]
fn a_button_press_is_its_own_source() {
    let a = Actor::button(UserId::new(42), "skip");
    assert_eq!(a.source(), Source::Button);
    assert_eq!(a.source().as_str(), "button");
    assert_eq!(a.command(), "button skip");
    assert_eq!(a.user(), Some(UserId::new(42)));
    assert_eq!(a.origin_channel(), None);
    // The stored spelling is the serde name.
    assert_eq!(serde_json::to_string(&Source::Button).unwrap(), "\"button\"");
}
```

In `audit_view.rs`, add `(SourceChoice::Button, Source::Button),` to `source_choices_match_source_names`, and add:

```rust
#[test]
fn a_button_row_reads_button() {
    // `row(source, command, user, action)` is the test module's AuditRow builder.
    let r = row("button", "button skip", Some(42), Action::Skip { track: None });
    assert_eq!(how_text(&r), "button");
    assert_eq!(SourceChoice::from_name("button"), Some(SourceChoice::Button));
}
```

In `crack-web/src/history.rs`'s tests, beside the `source=web` parse test:

```rust
#[test]
fn source_button_parses() {
    let got = q("source=button").unwrap();
    assert_eq!(got.source, Some(SourceChoice::Button));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p crack-core --lib music::audit 2>&1 | tail -5; cargo test -p crack-web 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement.**

`audit.rs`:
```rust
pub enum Source {
    Slash,
    Prefix,
    Web,
    /// A now-playing button (`np:`), pressed in Discord.
    Button,
    Bot,
}
// as_str:
            Source::Button => "button",
```
```rust
    /// A member pressing a now-playing button; `op` names the control.
    #[must_use]
    pub fn button(user: UserId, op: &'static str) -> Self {
        Self {
            user: Some(user),
            source: Source::Button,
            command: Cow::Owned(format!("button {op}")),
            origin_channel: None,
        }
    }
```
`audit_view.rs`: add `"button" => "button".to_owned(),` to `how_text` (before `_`); add `Button` to `SourceChoice` (between `Web` and `Bot`); add `SourceChoice::Button => Source::Button` to `source()` and `"button" => SourceChoice::Button` to `from_name`. Update `from_name`'s doc comment to list `button`.

`crack-web/src/page.rs`: after `<option value=\"web\">dashboard</option>`, insert `<option value=\"button\">button</option>`.

`crack-web/src/history.rs`: `pub const BAD_SOURCE: &str = "source must be one of slash, prefix, web, button, bot";`. Update any test that pins the old literal.

`docs/queue-audit.md` line 4: `(`source`: slash, prefix, web, button, bot; `command`)`. After the paragraph ending near line 30, add: "A now-playing button press (v0.23.0) records source `button` and command `button <op>`, for example `button skip`."

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test -p crack-core --lib music:: 2>&1 | grep -E "test result|FAILED"; cargo test -p crack-web 2>&1 | grep -E "test result|FAILED"`
Expected: PASS.

- [ ] **Step 5: Sabotage:**
  - `as_str` → `"web"` for `Button` (the button source test must fail);
  - drop the `how_text` arm (it falls to `_`, which returns `button skip`, so the view test must fail);
  - drop `"button" =>` from `from_name` (the web parse test must fail).

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/music/audit.rs crack-core/src/music/audit_view.rs \
  crack-web/src/page.rs crack-web/src/history.rs docs/queue-audit.md
git commit -m "audit: a now-playing button press is source 'button'

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: One control path for the dashboard and the buttons; no echo for a no-op

**Files:**
- Modify: `crack-core/src/messaging/cards.rs`: `Via::Button`; `Echo::line(user, via)`; `EchoLine::line`; tests
- Modify: `crack-core/src/music/remote.rs`: `control` takes `via`, returns `Option<Echo>`; `actor_for`; `flags_before`; `changes`; `run_control` takes `before` and returns `Option<Echo>`; tests
- Modify: `crack-web/src/lib.rs` (~line 173): pass `Via::Dashboard`
- Test: `cards.rs` and `remote.rs` tests

**Interfaces:**
- Consumes: `Actor::button` (Task 3); `courier::post`'s echo gate (Task 2).
- Produces (Task 7 relies on these exact shapes):
  - `pub enum Via { Dashboard, Button }`
  - `Echo::line(&self, user: UserId, via: Via) -> String`
  - `pub async fn control(data: Arc<Data>, http: Arc<Http>, cache: Arc<Cache>, guild_id: GuildId, user: UserId, via: Via, c: Control) -> Result<Option<Echo>, ControlRefused>`. `None` means the control ran but changed nothing, so no echo was posted.
  - `pub(crate) fn changes(c: Control, before: Option<(bool, bool)>) -> bool`
  - `pub(crate) async fn run_control(guard: &QueueGuard, call: &Arc<Mutex<Call>>, c: Control, before: Option<(bool, bool)>) -> Result<(Option<Echo>, ops::Settle), ops::OpRefused>`

- [ ] **Step 1: Write the failing echo-wording tests** in `cards.rs`:

```rust
/// Ruling 4: a press in Discord needs no "from …"; the dashboard keeps its.
#[test]
fn a_button_echo_has_no_source_suffix() {
    let skipped = Echo::Skipped { title: Some("t0".into()) };
    assert_eq!(
        skipped.line(UserId::new(42), Via::Button),
        "⏭ Skipped **t0** — <@42>"
    );
    assert_eq!(
        skipped.line(UserId::new(42), Via::Dashboard),
        "⏭ Skipped **t0** from the dashboard — <@42>"
    );
    assert_eq!(Echo::Paused.line(UserId::new(42), Via::Button), "⏸ Paused — <@42>");
    assert_eq!(
        Echo::Repeat { on: true }.line(UserId::new(42), Via::Button),
        "🔁 Repeat on — <@42>"
    );
}
```
Update `an_echo_of_a_blank_title_says_untitled` to call `echo.line(UserId::new(42), Via::Dashboard)`.

- [ ] **Step 2: Write the failing `changes` and dispatch tests** in `remote.rs`. Add these to `mod test`:

```rust
#[test]
fn a_control_that_would_change_nothing_is_not_a_change() {
    // (paused, looping) before the control ran.
    assert!(!changes(Control::Pause, Some((true, false))));
    assert!(changes(Control::Pause, Some((false, false))));
    assert!(!changes(Control::Resume, Some((false, true))));
    assert!(changes(Control::Resume, Some((true, true))));
    assert!(!changes(Control::Repeat { on: true }, Some((false, true))));
    assert!(changes(Control::Repeat { on: true }, Some((false, false))));
    assert!(!changes(Control::Repeat { on: false }, Some((true, false))));
    // Unknown state: echo, as before.
    assert!(changes(Control::Pause, None));
    assert!(changes(Control::Repeat { on: false }, None));
    // Skip and remove always change something (a stale skip is refused).
    assert!(changes(Control::Skip { expect: uuid::Uuid::nil() }, Some((true, true))));
}

#[test]
fn a_button_acts_as_a_button_and_the_dashboard_as_the_web() {
    let b = actor_for(Via::Button, UserId::new(9), "skip");
    assert_eq!(b.source(), crate::music::audit::Source::Button);
    assert_eq!(b.command(), "button skip");
    let w = actor_for(Via::Dashboard, UserId::new(9), "skip");
    assert_eq!(w.source(), crate::music::audit::Source::Web);
    assert_eq!(w.command(), "dashboard skip");
}
```

In `mod dispatch`, change every `run_control(&g, &call, X)` to `run_control(&g, &call, X, None)` and every `let (echo, settle) = …` assertion from `echo == Echo::Y` to `echo == Some(Echo::Y)`. Then add:

```rust
/// A redundant pause (a stale tab, an old status message) still pauses and
/// still settles, but there is nothing to echo.
#[tokio::test]
async fn a_pause_of_a_paused_track_echoes_nothing() {
    let (data, call, _, mut rx) = queue_of(2).await;
    let g = guard(&data).await;
    let (echo, settle) = run_control(&g, &call, Control::Pause, Some((true, false)))
        .await
        .unwrap();
    assert_eq!(echo, None);
    assert_eq!(settle, Settle::NowPlaying);
    assert_eq!(recorded(&mut rx), vec![Action::Pause]);
}

#[tokio::test]
async fn shuffling_one_track_echoes_nothing() {
    let (data, call, _, _) = queue_of(1).await;
    let g = guard(&data).await;
    let (echo, _) = run_control(&g, &call, Control::Shuffle, None).await.unwrap();
    assert_eq!(echo, None);
}

/// Review Focus 5: a driver that never answers must not hang the before-read.
#[tokio::test]
async fn the_before_read_of_a_stalled_driver_is_unknown_within_the_bound() {
    let (_data, call, _, _) = queue_of(1).await;
    let got = tokio::time::timeout(std::time::Duration::from_secs(5), flags_before(&call))
        .await
        .expect("flags_before outlived TRACK_INFO_TIMEOUT");
    assert_eq!(got, None);
}
```

Change the two existing `control(...)` tests (`a_game_refuses_a_control_before_the_call_is_looked_up`, `a_control_with_no_call_is_not_playing`) to pass `Via::Dashboard` before the control. Their expected results are unchanged.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p crack-core --lib music::remote messaging::cards 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 4: Implement `cards.rs`.**

```rust
/// Where a control came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Dashboard,
    /// A now-playing button, pressed in Discord.
    Button,
}
```
```rust
impl EchoLine {
    #[must_use]
    pub fn line(&self) -> String {
        self.echo.line(self.user, self.via)
    }
}
```
In `Echo::line`, take `via: Via` and build the suffix once:
```rust
    pub fn line(&self, user: UserId, via: Via) -> String {
        let (what, title) = /* unchanged match */;
        // A press in Discord is visibly in Discord; the dashboard says so.
        let from = match via {
            Via::Dashboard => format!(" {ECHO_FROM_DASHBOARD}"),
            Via::Button => String::new(),
        };
        match title {
            Some(t) => format!(
                "{what} **{}**{from} — <@{user}>",
                TrackLabel {
                    title: Some(t.to_owned()),
                    ..TrackLabel::default()
                }
                .title_text(INLINE_TITLE_MAX)
            ),
            None => format!("{what}{from} — <@{user}>"),
        }
    }
```
Update the module doc to say "the echo of a dashboard or button control".

- [ ] **Step 5: Implement `remote.rs`.** Update the module doc's first line to "Queue operations for callers with no poise `Context`: the web dashboard and the now-playing buttons." Then:

```rust
/// The audit actor for a control from `via`.
pub(crate) fn actor_for(via: Via, user: UserId, op: &'static str) -> Actor {
    match via {
        Via::Dashboard => Actor::web(user, op),
        Via::Button => Actor::button(user, op),
    }
}

/// Whether `c` changes anything, given the playing track's `(paused, looping)`
/// before it ran. Unknown (`None`) is a change: echoing a no-op is the old
/// behaviour, and staying silent about a real change would be worse.
pub(crate) fn changes(c: Control, before: Option<(bool, bool)>) -> bool {
    match (c, before) {
        (Control::Pause, Some((paused, _))) => !paused,
        (Control::Resume, Some((paused, _))) => paused,
        (Control::Repeat { on }, Some((_, looping))) => on != looping,
        _ => true,
    }
}

/// The playing track's `(paused, looping)`, or `None` when nothing plays or
/// the driver does not answer within the bound. The Call lock is held only to
/// clone the handle.
pub(crate) async fn flags_before(call: &Arc<Mutex<Call>>) -> Option<(bool, bool)> {
    let current = call.lock().await.queue().current()?;
    let info = tokio::time::timeout(ops::TRACK_INFO_TIMEOUT, current.get_info())
        .await
        .ok()?
        .ok()?;
    Some(playback_flags(Some(&info)))
}
```

`control` becomes the following. The doc comment and the spawn stay as they are, except where shown:

```rust
/// Run a control for `user`, from the dashboard or a button. On success the
/// echo is returned at once, or `None` when the control changed nothing;
/// posting it (if the guild's `control_echoes` is on) and settling happen in
/// the background. The settle is anchored after the echo, so after a skip,
/// pause, resume or repeat the now-playing message is re-rendered below it.
pub async fn control(
    data: Arc<Data>,
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild_id: GuildId,
    user: UserId,
    via: Via,
    c: Control,
) -> Result<Option<Echo>, ControlRefused> {
    let op = /* unchanged match */;
    let cx = ops::OpCx {
        data,
        http,
        cache,
        guild_id,
        actor: actor_for(via, user, op),
    };
    let (guard, call) = ops::begin(&cx)
        .await
        .map_err(|r| ControlRefused::from(&r))?;
    // 🔑 Under the lease, bounded: what the track was doing decides whether
    // the control changes anything worth echoing.
    let before = flags_before(&call).await;
    let (echo, settle) = run_control(&guard, &call, c, before)
        .await
        .map_err(|r| ControlRefused::from(&r))?;
    drop(guard);
    let posted = echo.clone();
    tokio::spawn(async move {
        let transport = DiscordTransport {
            http: cx.http.clone(),
            cache: cx.cache.clone(),
        };
        let anchor = match posted {
            Some(echo) => {
                let line = EchoLine { echo, user, via };
                courier::post(
                    &cx.data,
                    &transport,
                    Destination::Echo(guild_id),
                    &CrackedMessage::Echo(Box::new(line)),
                    &RenderCx::now(),
                )
                .await
            },
            None => None,
        };
        settle.after(&cx, Some(&call), anchor).await;
    });
    Ok(echo)
}
```

`run_control` takes `before: Option<(bool, bool)>` and returns `(Option<Echo>, ops::Settle)`. Restructure it so each arm yields `(echo, settle, changed)`:

```rust
pub(crate) async fn run_control(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    c: Control,
    before: Option<(bool, bool)>,
) -> Result<(Option<Echo>, ops::Settle), ops::OpRefused> {
    let changed = changes(c, before);
    let (echo, settle, changed) = match c {
        Control::Skip { expect } => {
            let (s, settle, _) = ops::skip_on(guard, call, 1, Some(expect))
                .await?
                .into_parts();
            (
                Echo::Skipped {
                    title: s.skipped.and_then(|t| t.title),
                },
                settle,
                changed,
            )
        },
        Control::Pause => {
            let (_, settle, _) = ops::pause_on(guard, call).await?.into_parts();
            (Echo::Paused, settle, changed)
        },
        Control::Resume => {
            let (_, settle, _) = ops::resume_on(guard, call).await?.into_parts();
            (Echo::Resumed, settle, changed)
        },
        Control::Repeat { on } => {
            let (_, settle, _) = ops::repeat_on(guard, call, Some(on)).await?.into_parts();
            (Echo::Repeat { on }, settle, changed)
        },
        Control::Remove { id } => {
            let (r, settle, _) = ops::remove_on(guard, call, ops::Target::Id(id))
                .await?
                .into_parts();
            (
                Echo::Removed {
                    title: r.first.title,
                },
                settle,
                changed,
            )
        },
        Control::Shuffle => {
            let (s, settle, _) = ops::shuffle_on(guard, call).await?.into_parts();
            // Fewer than two upcoming tracks: nothing moved.
            (Echo::Shuffled, settle, s.count > 0)
        },
    };
    Ok((changed.then_some(echo), settle))
}
```

`crack-web/src/lib.rs` `control`: add `crack_core::messaging::cards::Via::Dashboard,` before `c,` in the `remote::control(...)` call. It still ends `.map(|_| ())`. Use the import path crack-web already uses for crack-core items.

- [ ] **Step 6: Run them to see them pass**

Run:
```bash
cargo test -p crack-core --lib music:: messaging:: 2>&1 | grep -E "test result|FAILED|panicked"
cargo test -p crack-web 2>&1 | grep -E "test result|FAILED"
cargo clippy --workspace --all-targets 2>&1 | grep -E "^(warning|error)" | head
```
Expected: all pass, and clippy is clean.

- [ ] **Step 7: Sabotage:**
  - `changes`' Pause arm → `paused` (the no-op test and the `changes` test must fail);
  - `s.count > 0` → `true` (the one-track shuffle must fail);
  - `actor_for`'s Button arm → `Actor::web` (the actor test must fail);
  - `Via::Button => String::new()` → the dashboard suffix (the wording test must fail);
  - remove the `TRACK_INFO_TIMEOUT` timeout in `flags_before` (the stalled-driver test must time out at 5 s, not hang).

- [ ] **Step 8: Commit**

```bash
git add crack-core/src/messaging/cards.rs crack-core/src/music/remote.rs crack-web/src/lib.rs
git commit -m "remote::control: one path for dashboard and buttons; a no-op posts no echo

Fixes the dashboard's parked 'a stale tab's redundant pause still echoes'.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: The buttons on the status message

**Files:**
- Create: `crack-core/src/messaging/buttons.rs` (ids, `Controls`, the row; `respond` and `handle` come in Task 7)
- Modify: `crack-core/src/messaging/mod.rs`: `pub mod buttons;`
- Modify: `crack-core/src/messaging/messages.rs`: the five labels
- Modify: `crack-core/src/messaging/cards.rs`: `NowPlayingCard.controls`; `now_playing` adds the row
- Modify: `crack-core/src/messaging/interface.rs`: `read_card`, `now_playing_card`, `now_playing_status_card`
- Modify: `crack-core/src/messaging/status.rs` (~line 307): `show_now_playing_after` uses `now_playing_status_card(&track, guild)`
- Test: `buttons.rs`, `cards.rs`, `interface.rs` tests

**Interfaces:**
- Consumes: `music::remote::{Control, playback_flags}` (`playback_flags` is `pub(crate)`).
- Produces (Task 7 relies on these):
  - `pub const NP_PREFIX: &str = "np:";`
  - `pub enum NowPlayingButton { Pause { guild: GuildId }, Resume { guild: GuildId }, Skip { guild: GuildId, track: Uuid }, RepeatOn { guild: GuildId }, RepeatOff { guild: GuildId }, Shuffle { guild: GuildId } }` (`Debug, Clone, Copy, PartialEq, Eq`)
  - `NowPlayingButton::{custom_id(&self) -> String, parse(&str) -> Option<Self>, guild(&self) -> GuildId, control(&self) -> Control, command(&self) -> &'static str}`
  - `pub struct Controls { pub guild: GuildId, pub track: Uuid, pub paused: bool, pub looping: bool }` (`Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn now_playing_row(c: &Controls) -> CreateComponent<'static>`
  - `NowPlayingCard.controls: Option<Controls>`
  - `interface::now_playing_status_card(track: &TrackHandle, guild: GuildId) -> NowPlayingCard`

- [ ] **Step 1: Labels** in `messages.rs`:

```rust
pub const NP_BUTTON_PAUSE: &str = "⏸ Pause";
pub const NP_BUTTON_RESUME: &str = "▶ Resume";
pub const NP_BUTTON_SKIP: &str = "⏭ Skip";
pub const NP_BUTTON_REPEAT: &str = "🔁 Repeat";
pub const NP_BUTTON_SHUFFLE: &str = "🔀 Shuffle";
```

- [ ] **Step 2: Write the failing id tests** in a new `buttons.rs` that has only a test module and `use` lines so far:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const G: GuildId = GuildId::new(123456789012345678);
    fn uuid() -> Uuid {
        Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap()
    }
    fn every() -> Vec<NowPlayingButton> {
        vec![
            NowPlayingButton::Pause { guild: G },
            NowPlayingButton::Resume { guild: G },
            NowPlayingButton::Skip { guild: G, track: uuid() },
            NowPlayingButton::RepeatOn { guild: G },
            NowPlayingButton::RepeatOff { guild: G },
            NowPlayingButton::Shuffle { guild: G },
        ]
    }

    #[test]
    fn ids_read_as_the_spec_spells_them() {
        let ids: Vec<String> = every().iter().map(NowPlayingButton::custom_id).collect();
        assert_eq!(
            ids,
            vec![
                "np:pause:123456789012345678",
                "np:resume:123456789012345678",
                "np:skip:123456789012345678:67e55044-10b1-426f-9247-bb680e5fe0c8",
                "np:repeat-on:123456789012345678",
                "np:repeat-off:123456789012345678",
                "np:shuffle:123456789012345678",
            ]
        );
    }

    #[test]
    fn every_id_round_trips() {
        for b in every() {
            assert_eq!(NowPlayingButton::parse(&b.custom_id()), Some(b), "{b:?}");
        }
    }

    /// Discord's custom_id limit is 100 characters; the longest is 65.
    #[test]
    fn the_longest_id_fits_discords_limit() {
        let b = NowPlayingButton::Skip { guild: GuildId::new(u64::MAX), track: uuid() };
        assert_eq!(b.custom_id().len(), 65);
    }

    /// Review Focus 3: nothing malformed parses, and nothing panics
    /// (`GuildId::new(0)` would).
    #[test]
    fn malformed_ids_are_rejected() {
        for bad in [
            "",
            "np:",
            "np:pause",
            "np:pause:",
            "np:pause:0",
            "np:pause:-1",
            "np:pause:abc",
            "np:pause:1:extra",
            "np:skip:1",
            "np:skip:1:not-a-uuid",
            "np:skip:1:67e55044-10b1-426f-9247-bb680e5fe0c8:x",
            "np:dance:1",
            "np:repeat:1",
            "gp:pause:1",
            "pause:1",
        ] {
            assert_eq!(NowPlayingButton::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn each_button_runs_its_control_and_names_its_command() {
        use crate::music::remote::Control;
        let got: Vec<(Control, &str)> = every().iter().map(|b| (b.control(), b.command())).collect();
        assert_eq!(
            got,
            vec![
                (Control::Pause, "pause"),
                (Control::Resume, "resume"),
                (Control::Skip { expect: uuid() }, "skip"),
                (Control::Repeat { on: true }, "repeat"),
                (Control::Repeat { on: false }, "repeat"),
                (Control::Shuffle, "shuffle"),
            ]
        );
        assert!(every().iter().all(|b| b.guild() == G));
    }

    fn row_json(c: &Controls) -> serde_json::Value {
        serde_json::to_value(now_playing_row(c)).unwrap()
    }
    fn ids_labels_styles(v: &serde_json::Value) -> Vec<(String, String, u64)> {
        v["components"]
            .as_array()
            .expect("an action row")
            .iter()
            .map(|b| {
                (
                    b["custom_id"].as_str().unwrap().to_owned(),
                    b["label"].as_str().unwrap().to_owned(),
                    b["style"].as_u64().unwrap(),
                )
            })
            .collect()
    }

    /// Playing, repeat off: Pause, Skip, Repeat (grey, turns it on), Shuffle.
    #[test]
    fn a_playing_track_shows_pause_and_repeat_off() {
        let c = Controls { guild: G, track: uuid(), paused: false, looping: false };
        assert_eq!(
            ids_labels_styles(&row_json(&c)),
            vec![
                ("np:pause:123456789012345678".into(), "⏸ Pause".into(), 2),
                (
                    "np:skip:123456789012345678:67e55044-10b1-426f-9247-bb680e5fe0c8".into(),
                    "⏭ Skip".into(),
                    2
                ),
                ("np:repeat-on:123456789012345678".into(), "🔁 Repeat".into(), 2),
                ("np:shuffle:123456789012345678".into(), "🔀 Shuffle".into(), 2),
            ]
        );
    }

    /// Paused and on repeat: Resume, and Repeat green (style 3), which turns it off.
    #[test]
    fn a_paused_track_on_repeat_shows_resume_and_repeat_on() {
        let c = Controls { guild: G, track: uuid(), paused: true, looping: true };
        let got = ids_labels_styles(&row_json(&c));
        assert_eq!(got[0], ("np:resume:123456789012345678".into(), "▶ Resume".into(), 2));
        assert_eq!(got[2], ("np:repeat-off:123456789012345678".into(), "🔁 Repeat".into(), 3));
    }
}
```

Run: `cargo test -p crack-core --lib messaging::buttons 2>&1 | tail -5`. Expected: compile errors.

- [ ] **Step 3: Implement `buttons.rs`** (above the tests):

```rust
//! The now-playing buttons: their custom ids, the row on the status message,
//! and (Task 7) what a press does. Spec: the messaging-layer design, "PR 2".
//!
//! 🔑 An id names an intent, never a toggle, and carries everything a press
//! needs: buttons on an old status message still work after a restart, and a
//! Skip names the track it was drawn for, so a stale or doubled press cannot
//! skip the next song.
use crate::messaging::messages::{
    NP_BUTTON_PAUSE, NP_BUTTON_REPEAT, NP_BUTTON_RESUME, NP_BUTTON_SHUFFLE, NP_BUTTON_SKIP,
};
use crate::music::remote::Control;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateComponent, GuildId};
use std::borrow::Cow;
use uuid::Uuid;

/// Every now-playing button's custom id starts with this.
pub const NP_PREFIX: &str = "np:";

/// One now-playing button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NowPlayingButton {
    Pause { guild: GuildId },
    Resume { guild: GuildId },
    /// Only if `track` is still the one playing.
    Skip { guild: GuildId, track: Uuid },
    RepeatOn { guild: GuildId },
    RepeatOff { guild: GuildId },
    Shuffle { guild: GuildId },
}

impl NowPlayingButton {
    #[must_use]
    pub fn custom_id(&self) -> String {
        match *self {
            Self::Pause { guild } => format!("{NP_PREFIX}pause:{guild}"),
            Self::Resume { guild } => format!("{NP_PREFIX}resume:{guild}"),
            Self::Skip { guild, track } => format!("{NP_PREFIX}skip:{guild}:{track}"),
            Self::RepeatOn { guild } => format!("{NP_PREFIX}repeat-on:{guild}"),
            Self::RepeatOff { guild } => format!("{NP_PREFIX}repeat-off:{guild}"),
            Self::Shuffle { guild } => format!("{NP_PREFIX}shuffle:{guild}"),
        }
    }

    /// The button an id names, or `None` for anything malformed. Never panics:
    /// a zero guild id is rejected before `GuildId::new`, which panics on it.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        let mut parts = id.strip_prefix(NP_PREFIX)?.split(':');
        let kind = parts.next()?;
        let guild = parts
            .next()?
            .parse::<u64>()
            .ok()
            .filter(|&n| n != 0)
            .map(GuildId::new)?;
        let button = match kind {
            "pause" => Self::Pause { guild },
            "resume" => Self::Resume { guild },
            "skip" => Self::Skip {
                guild,
                track: Uuid::parse_str(parts.next()?).ok()?,
            },
            "repeat-on" => Self::RepeatOn { guild },
            "repeat-off" => Self::RepeatOff { guild },
            "shuffle" => Self::Shuffle { guild },
            _ => return None,
        };
        // Nothing may follow what the kind needs.
        parts.next().is_none().then_some(button)
    }

    #[must_use]
    pub fn guild(&self) -> GuildId {
        match *self {
            Self::Pause { guild }
            | Self::Resume { guild }
            | Self::Skip { guild, .. }
            | Self::RepeatOn { guild }
            | Self::RepeatOff { guild }
            | Self::Shuffle { guild } => guild,
        }
    }

    #[must_use]
    pub fn control(&self) -> Control {
        match *self {
            Self::Pause { .. } => Control::Pause,
            Self::Resume { .. } => Control::Resume,
            Self::Skip { track, .. } => Control::Skip { expect: track },
            Self::RepeatOn { .. } => Control::Repeat { on: true },
            Self::RepeatOff { .. } => Control::Repeat { on: false },
            Self::Shuffle { .. } => Control::Shuffle,
        }
    }

    /// The slash command this button stands in for: the name `/gp` blocks
    /// while a game runs, and the op the audit log records.
    #[must_use]
    pub fn command(&self) -> &'static str {
        match self {
            Self::Pause { .. } => "pause",
            Self::Resume { .. } => "resume",
            Self::Skip { .. } => "skip",
            Self::RepeatOn { .. } | Self::RepeatOff { .. } => "repeat",
            Self::Shuffle { .. } => "shuffle",
        }
    }
}

/// What the status message's buttons are drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Controls {
    pub guild: GuildId,
    /// The playing track, for Skip.
    pub track: Uuid,
    pub paused: bool,
    pub looping: bool,
}

/// The one action row: Pause or Resume, Skip, Repeat (green while on), Shuffle.
#[must_use]
pub fn now_playing_row(c: &Controls) -> CreateComponent<'static> {
    let guild = c.guild;
    let play = if c.paused {
        button(NowPlayingButton::Resume { guild }, NP_BUTTON_RESUME, ButtonStyle::Secondary)
    } else {
        button(NowPlayingButton::Pause { guild }, NP_BUTTON_PAUSE, ButtonStyle::Secondary)
    };
    let repeat = if c.looping {
        button(NowPlayingButton::RepeatOff { guild }, NP_BUTTON_REPEAT, ButtonStyle::Success)
    } else {
        button(NowPlayingButton::RepeatOn { guild }, NP_BUTTON_REPEAT, ButtonStyle::Secondary)
    };
    CreateComponent::ActionRow(CreateActionRow::Buttons(Cow::Owned(vec![
        play,
        button(
            NowPlayingButton::Skip { guild, track: c.track },
            NP_BUTTON_SKIP,
            ButtonStyle::Secondary,
        ),
        repeat,
        button(NowPlayingButton::Shuffle { guild }, NP_BUTTON_SHUFFLE, ButtonStyle::Secondary),
    ])))
}

fn button(b: NowPlayingButton, label: &'static str, style: ButtonStyle) -> CreateButton<'static> {
    CreateButton::new(b.custom_id()).label(label).style(style)
}
```
Add `pub mod buttons;` to `messaging/mod.rs`.

- [ ] **Step 4: Run the id and row tests to see them pass**

Run: `cargo test -p crack-core --lib messaging::buttons 2>&1 | grep -E "test result|FAILED"`
Expected: PASS.

- [ ] **Step 5: Write the failing card tests** in `cards.rs`:

```rust
#[test]
fn a_card_with_controls_carries_one_row_and_one_without_carries_none() {
    use crate::messaging::buttons::Controls;
    let mut c = card(None, None);
    assert!(now_playing(&c, &RenderCx::default()).components.is_empty());
    c.controls = Some(Controls {
        guild: serenity::all::GuildId::new(1),
        track: uuid::Uuid::nil(),
        paused: false,
        looping: false,
    });
    let r = now_playing(&c, &RenderCx::default());
    assert_eq!(r.components.len(), 1);
    let row = serde_json::to_value(&r.components[0]).unwrap();
    assert_eq!(row["components"][0]["custom_id"], "np:pause:1");
}

/// "None on Finished": the edit to Finished clears the row.
#[test]
fn the_finished_card_has_no_buttons() {
    assert!(finished().components.is_empty());
    let edit = serde_json::to_value(finished().to_edit()).unwrap();
    assert_eq!(edit["components"].as_array().map(Vec::len), Some(0));
}
```
Add `controls: None,` to the `card()` test helper.

In `interface.rs`'s tests, beside `a_driver_that_never_answers_does_not_hang_the_card`:

```rust
/// Review Focus 5: a stalled driver still gets its buttons, at their defaults
/// (Pause, repeat off), for the playing track, within the bound.
#[tokio::test]
async fn the_status_card_of_a_stalled_driver_has_default_controls() {
    use super::now_playing_status_card;
    use crate::messaging::buttons::Controls;
    use std::time::Duration;
    let (_data, call, ids, _rx) = crate::music::ops::test_support::queue_of(1).await;
    let handle = call.lock().await.queue().current_queue()[0].clone();
    let guild = serenity::all::GuildId::new(1);
    let card = tokio::time::timeout(Duration::from_secs(5), now_playing_status_card(&handle, guild))
        .await
        .expect("now_playing_status_card outlived its bound");
    assert_eq!(
        card.controls,
        Some(Controls { guild, track: ids[0], paused: false, looping: false })
    );
}

#[tokio::test]
async fn a_reply_card_has_no_controls() {
    use super::now_playing_card;
    use std::time::Duration;
    let (_data, call, _ids, _rx) = crate::music::ops::test_support::queue_of(1).await;
    let handle = call.lock().await.queue().current_queue()[0].clone();
    let card = tokio::time::timeout(Duration::from_secs(5), now_playing_card(&handle))
        .await
        .expect("bounded");
    assert_eq!(card.controls, None);
}
```

Run: `cargo test -p crack-core --lib messaging:: 2>&1 | tail -5`. Expected: compile errors.

- [ ] **Step 6: Implement the card and the status.** In `cards.rs`:

```rust
/// The now-playing card.
#[derive(Debug, Clone)]
pub struct NowPlayingCard {
    pub label: TrackLabel,
    pub thumbnail: Option<String>,
    pub requester: Option<UserId>,
    pub progress: Progress,
    /// The buttons: only the status message has them, never a reply or a DM.
    pub controls: Option<crate::messaging::buttons::Controls>,
}
```
At the end of `now_playing`, replace `Rendered::embed(embed)` with:
```rust
    let out = Rendered::embed(embed);
    match &card.controls {
        Some(c) => out.with_components(vec![crate::messaging::buttons::now_playing_row(c)]),
        None => out,
    }
```

In `interface.rs`, split the reader so the flags come from the same bounded `get_info`:

```rust
/// The card, and the track's `(paused, looping)` read from the same bounded
/// `get_info` (`(false, false)` when it does not answer).
async fn read_card(track: &TrackHandle) -> (NowPlayingCard, (bool, bool)) {
    let metadata = get_track_handle_metadata(track).await.unwrap_or_default();
    let requester = get_requesting_user(track).await.ok();
    let label = TrackLabel::from_metadata(&metadata);
    let info = tokio::time::timeout(crate::music::ops::TRACK_INFO_TIMEOUT, track.get_info())
        .await
        .ok()
        .and_then(Result::ok);
    let progress = progress_of(info.as_ref(), label.duration);
    let flags = crate::music::remote::playback_flags(info.as_ref());
    (
        NowPlayingCard {
            label,
            thumbnail: metadata.thumbnail,
            requester,
            progress,
            controls: None,
        },
        flags,
    )
}

/// The card for a reply or a DM: no buttons.
pub async fn now_playing_card(track: &TrackHandle) -> NowPlayingCard {
    read_card(track).await.0
}

/// The status message's card: the same, with the buttons for `guild`. On a
/// driver that does not answer, they show their defaults (Pause, repeat off).
pub async fn now_playing_status_card(track: &TrackHandle, guild: GuildId) -> NowPlayingCard {
    let (mut card, (paused, looping)) = read_card(track).await;
    card.controls = Some(crate::messaging::buttons::Controls {
        guild,
        track: track.uuid(),
        paused,
        looping,
    });
    card
}
```
Add `GuildId` to `interface.rs`'s serenity import if it is not there.

In `status.rs` `show_now_playing_after`, change `let card = now_playing_card(&track).await;` to `let card = now_playing_status_card(&track, guild).await;` and update the import.

- [ ] **Step 7: Run everything**

Run:
```bash
cargo test -p crack-core --lib 2>&1 | grep -E "test result|FAILED|panicked"
cargo clippy --workspace --all-targets 2>&1 | grep -E "^(warning|error)" | head
```
Expected: all pass, and clippy is clean.

- [ ] **Step 8: Sabotage:**
  - drop `.filter(|&n| n != 0)` (`np:pause:0` must panic or fail the rejection test);
  - drop the trailing `parts.next().is_none()` check (`np:pause:1:extra` must parse and fail the test);
  - swap Resume/Pause in the row (both row tests must fail);
  - give RepeatOff `Secondary` (the repeat-on style assertion must fail);
  - in `now_playing_status_card`, set `track: uuid::Uuid::nil()` (the stalled-driver card test must fail);
  - leave `status.rs` on `now_playing_card`. **Expect no test to catch this one**: it is glue that needs a live context. Report it as uncaught; the TuneTitan checklist covers it.

- [ ] **Step 9: Commit**

```bash
git add crack-core/src/messaging/buttons.rs crack-core/src/messaging/mod.rs crack-core/src/messaging/messages.rs \
  crack-core/src/messaging/cards.rs crack-core/src/messaging/interface.rs crack-core/src/messaging/status.rs
git commit -m "Now playing: Pause/Resume, Skip, Repeat and Shuffle buttons on the status message

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 6: Answering a press (`Press`), and one access rule (`music_access`)

**Files:**
- Modify: `crack-core/src/messaging/transport.rs`: `Press`, `DiscordPress`
- Modify: `crack-core/src/messaging/render.rs`: `Rendered::to_followup`
- Modify: `crack-core/src/messaging/courier.rs`: `acknowledge`, `answer_privately`
- Modify: `crack-core/src/messaging/test_support.rs`: `FakePress`, `PressOp`
- Modify: `crack-core/src/commands/permissions.rs`: `music_access`; `cmd_check_music` calls it; `cmd_check_music_internal` is deleted (its only caller was `cmd_check_music`)
- Test: `render.rs`, `courier.rs`, `permissions.rs` tests

**Interfaces:**
- Produces (Task 7 relies on these):
  - `#[async_trait] pub trait Press: Send + Sync { async fn acknowledge(&self) -> Result<(), TransportError>; async fn followup(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError>; }`
  - `pub struct DiscordPress<'a> { pub http: &'a Http, pub interaction: &'a ComponentInteraction }`
  - `Rendered::to_followup(&self, ephemeral: bool) -> CreateInteractionResponseFollowup<'static>`
  - `courier::acknowledge(press: &dyn Press) -> bool` (best effort; `false` when it failed)
  - `courier::answer_privately(press: &dyn Press, msg: &CrackedMessage, cx: &RenderCx)` (best effort)
  - `test_support::{FakePress, PressOp::{Acknowledge, Followup { ephemeral: bool, text: String }}}`
  - `pub async fn music_access(data: &Data, guild: GuildId, member: Option<&Member>, is_bot: bool, channel: GenericChannelId, command: &str) -> Result<(), CrackedError>`

- [ ] **Step 1: Write the failing tests.** In `render.rs`:

```rust
/// A follow-up to a press: private when asked, pings nobody, carries the embed.
#[test]
fn a_followup_is_private_when_asked_and_pings_nobody() {
    let r = render(&CrackedMessage::Other("@everyone".into()), &cx());
    let private = serde_json::to_value(r.to_followup(true)).unwrap();
    assert_eq!(private["flags"].as_u64(), Some(64));
    assert_eq!(private["allowed_mentions"]["parse"], serde_json::json!([]));
    assert_eq!(private["embeds"].as_array().map(Vec::len), Some(1));
    let public = serde_json::to_value(r.to_followup(false)).unwrap();
    assert_eq!(public["flags"].as_u64().unwrap_or(0) & 64, 0);
}
```

In `courier.rs`:

```rust
#[tokio::test]
async fn a_press_is_acknowledged_then_answered_privately() {
    use crate::messaging::test_support::{FakePress, PressOp};
    let p = FakePress::default();
    assert!(acknowledge(&p).await);
    answer_privately(&p, &CrackedMessage::CrackedError(crate::errors::CrackedError::NothingPlaying), &cx()).await;
    assert_eq!(
        p.ops(),
        vec![
            PressOp::Acknowledge,
            PressOp::Followup { ephemeral: true, text: "🔈 Nothing is playing!".into() },
        ]
    );
}

#[tokio::test]
async fn a_failed_acknowledge_is_reported_not_raised() {
    use crate::messaging::test_support::FakePress;
    let p = FakePress::default();
    *p.ack_error.lock().unwrap() = Some(TransportError::Other("Unknown interaction".into()));
    assert!(!acknowledge(&p).await);
}
```

In `permissions.rs` (add `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::guild::settings::GuildSettings;
    use serenity::all::{ChannelId, GuildId, UserId};

    const G: GuildId = GuildId::new(1);
    const MUSIC: GenericChannelId = GenericChannelId::new(10);
    const ELSEWHERE: GenericChannelId = GenericChannelId::new(11);

    async fn with_music_channel() -> Data {
        let data = Data::default();
        let mut s = GuildSettings::new(G, None, None);
        s.set_music_channel(MUSIC.get());
        data.guild_settings_map.write().await.insert(G, s);
        data
    }

    #[tokio::test]
    async fn anyone_may_use_music_where_no_channel_is_set() {
        let data = Data::default();
        assert!(music_access(&data, G, None, false, ELSEWHERE, "skip").await.is_ok());
    }

    #[tokio::test]
    async fn the_music_channel_applies() {
        let data = with_music_channel().await;
        assert!(music_access(&data, G, None, false, MUSIC, "skip").await.is_ok());
        let got = music_access(&data, G, None, false, ELSEWHERE, "skip").await;
        assert!(matches!(got, Err(CrackedError::NotInMusicChannel(c)) if c == ELSEWHERE), "{got:?}");
    }

    #[tokio::test]
    async fn a_bot_is_refused() {
        let data = Data::default();
        let got = music_access(&data, G, None, true, MUSIC, "skip").await;
        assert!(matches!(got, Err(CrackedError::UnauthorizedUser)), "{got:?}");
    }

    /// Review Focus 4: while `/gp` owns playback, the blocked commands are
    /// refused in its words, and the rest still run.
    #[tokio::test]
    async fn a_game_refuses_what_it_blocks_and_only_that() {
        let data = Data::default();
        data.gp_start(
            G,
            UserId::new(100),
            "alice".into(),
            ChannelId::new(10),
            GenericChannelId::new(20),
            crate::commands::music::gp_prompts::GpCategory::Nostalgia,
            vec!["p1".into()],
            120,
            None,
            crate::commands::music::GpReveal::default(),
            true,
            1_700_000_000,
        )
        .expect("a game starts");
        for blocked in ["pause", "resume", "skip", "repeat", "shuffle"] {
            let got = music_access(&data, G, None, false, MUSIC, blocked).await;
            assert!(matches!(got, Err(CrackedError::GameInProgress)), "{blocked}: {got:?}");
        }
        assert!(music_access(&data, G, None, false, MUSIC, "volume").await.is_ok());
    }
}
```

Run: `cargo test -p crack-core --lib commands::permissions messaging::courier messaging::render 2>&1 | tail -10`. Expected: compile errors.

- [ ] **Step 2: Implement `Press`** in `transport.rs`, after `DiscordTransport`'s impl:

```rust
/// One button press: acknowledge it, then answer whoever pressed it. Bound
/// to its interaction, as `ReplySink` is bound to a command. Tests swap in
/// `test_support::FakePress`.
#[async_trait]
pub trait Press: Send + Sync {
    /// A deferred update: Discord stops waiting, and the message is left as is.
    async fn acknowledge(&self) -> Result<(), TransportError>;
    async fn followup(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError>;
}

/// The real Discord behind [`Press`].
pub struct DiscordPress<'a> {
    pub http: &'a Http,
    pub interaction: &'a ComponentInteraction,
}

#[async_trait]
impl Press for DiscordPress<'_> {
    #[expect(
        clippy::disallowed_methods,
        reason = "messaging is where sends are made"
    )]
    async fn acknowledge(&self) -> Result<(), TransportError> {
        Ok(self
            .interaction
            .create_response(self.http, CreateInteractionResponse::Acknowledge)
            .await?)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "messaging is where sends are made"
    )]
    async fn followup(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError> {
        self.interaction
            .create_followup(self.http, out.to_followup(ephemeral))
            .await?;
        Ok(())
    }
}
```
Add `ComponentInteraction, CreateInteractionResponse` to the serenity import. If clippy reports the `#[expect]` as unfulfilled on the method, move it onto the statement, as `DiscordTransport::edit` does.

`render.rs`, beside `to_interaction_message`:

```rust
    /// A follow-up to a component interaction (a button press's answer).
    pub fn to_followup(&self, ephemeral: bool) -> CreateInteractionResponseFollowup<'static> {
        let mut f = CreateInteractionResponseFollowup::new()
            .ephemeral(ephemeral)
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone());
        if let Some(content) = &self.content {
            f = f.content(content.clone());
        }
        let embeds = self.embeds();
        if !embeds.is_empty() {
            f = f.embeds(embeds);
        }
        f
    }
```

`courier.rs` (import `Press`: `use crate::messaging::transport::{Press, Transport, TransportError};`):

```rust
/// Tell Discord a button press arrived. Best effort: `false`, logged, when it
/// failed (the press is still handled; Discord shows its own failure notice).
pub async fn acknowledge(press: &dyn Press) -> bool {
    match press.acknowledge().await {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!("acknowledging a button press failed: {err:?}");
            false
        },
    }
}

/// Answer the presser only. Best effort: a failure is logged.
pub async fn answer_privately(press: &dyn Press, msg: &CrackedMessage, cx: &RenderCx) {
    if let Err(err) = press.followup(render(msg, cx), true).await {
        tracing::warn!("answering a button press failed: {err:?}");
    }
}
```

`test_support.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressOp {
    Acknowledge,
    Followup { ephemeral: bool, text: String },
}

/// A stand-in for one button press.
#[derive(Default)]
pub struct FakePress {
    pub ops: Mutex<Vec<PressOp>>,
    pub ack_error: Mutex<Option<TransportError>>,
}

impl FakePress {
    pub fn ops(&self) -> Vec<PressOp> {
        self.ops.lock().unwrap().clone()
    }
}

#[async_trait]
impl super::transport::Press for FakePress {
    async fn acknowledge(&self) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Acknowledge);
        match self.ack_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    async fn followup(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Followup {
            ephemeral,
            text: text_of(&out),
        });
        Ok(())
    }
}
```

- [ ] **Step 3: Implement `music_access`** in `permissions.rs`, and make `cmd_check_music` call it:

```rust
/// The rules every music control obeys, typed or pressed: the slash check and
/// the now-playing buttons both call this, so they cannot drift. `command` is
/// the qualified name `/gp` checks against [`GP_BLOCKED_COMMANDS`].
///
/// A bot, or a member the role check refuses, is `UnauthorizedUser`; the slash
/// check turns exactly that back into a silent `Ok(false)`, as before.
pub async fn music_access(
    data: &Data,
    guild: GuildId,
    member: Option<&Member>,
    is_bot: bool,
    channel: GenericChannelId,
    command: &str,
) -> Result<(), CrackedError> {
    if is_bot {
        return Err(CrackedError::UnauthorizedUser);
    }
    // While a guilty pleasure game owns playback, queue-mutating commands would
    // corrupt the round order. Matched on the qualified name so the game's own
    // `gp skip` is not caught by the top-level `skip`.
    if data.gp_is_active(guild) && GP_BLOCKED_COMMANDS.contains(&command) {
        return Err(CrackedError::GameInProgress);
    }
    let music_channel = data
        .get_guild_settings(guild)
        .await
        .and_then(|settings| settings.get_music_channel());
    if music_channel.is_some_and(|allowed| allowed != channel) {
        return Err(CrackedError::NotInMusicChannel(channel));
    }
    match is_authorized_music(member.map(Cow::Borrowed), None) {
        Ok(true) => Ok(()),
        _ => Err(CrackedError::UnauthorizedUser),
    }
}

pub async fn cmd_check_music(ctx: Context<'_>) -> Result<bool, Error> {
    let guild_id = ctx.guild_id().try_unwrap()?;
    let channel_id: GenericChannelId = ctx.channel_id();
    let member = ctx.author_member().await;
    match music_access(
        &ctx.data(),
        guild_id,
        member.as_deref(),
        ctx.author().bot(),
        channel_id,
        &ctx.command().qualified_name,
    )
    .await
    {
        Ok(()) => {
            // The floating status message follows the conversation: remember
            // where this guild's latest music command was run.
            crate::messaging::status::note_command_channel(&ctx.data(), guild_id, channel_id).await;
            Ok(true)
        },
        // Silent, as before: a bot, or a member the role check refuses.
        Err(CrackedError::UnauthorizedUser) => Ok(false),
        Err(e) => Err(e.into()),
    }
}
```
Delete `cmd_check_music_internal`. Fix the imports (`GuildId`, `Data`, `GuildSettingsOperations` for `get_guild_settings`, `GP_BLOCKED_COMMANDS`). `cargo build` lists anything missing.

> Behaviour note for the reviewer: a DM now fails at `try_unwrap` *before* the bot check, where before a bot in a DM returned `Ok(false)` first. Bots cannot invoke our slash commands in DMs, and prefix commands are unreachable (no MESSAGE_CONTENT intent), so this is unobservable. Say so in the report.

- [ ] **Step 4: Run them to see them pass**

Run:
```bash
cargo test -p crack-core --lib 2>&1 | grep -E "test result|FAILED|panicked"
cargo clippy --workspace --all-targets 2>&1 | grep -E "^(warning|error)" | head
```
Expected: all pass, and clippy is clean.

- [ ] **Step 5: Sabotage:**
  - `to_followup` ignores `ephemeral` (the flags test must fail);
  - `answer_privately` passes `false` (the courier test must fail);
  - drop the `is_bot` check (the bot test must fail);
  - drop `&& GP_BLOCKED_COMMANDS.contains(&command)` (`volume` must be refused, so the game test must fail);
  - compare `allowed == channel` (the music-channel test must fail).

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/messaging/transport.rs crack-core/src/messaging/render.rs crack-core/src/messaging/courier.rs \
  crack-core/src/messaging/test_support.rs crack-core/src/commands/permissions.rs
git commit -m "messaging: answer a button press through Press; music_access shared with the slash check

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 7: Pressing a button (`buttons::respond`, `handle`, and the route)

**Files:**
- Modify: `crack-core/src/messaging/buttons.rs`: `Presser`, `respond`, `refusal`, `handle`
- Modify: `crack-core/src/messaging/{message,messages}.rs`: `CrackedMessage::ButtonOutOfDate` (appended at the end) and `BUTTON_OUT_OF_DATE`
- Modify: `crack-core/src/handlers/serenity.rs` (~line 58): the `np:` route
- Test: `buttons.rs` tests

**Interfaces:**
- Consumes:
  - `NowPlayingButton` (Task 5)
  - `Press`, `courier::{acknowledge, answer_privately}`, `FakePress`, `music_access` (Task 6)
  - `remote::{control, Control, ControlRefused, Echo}`, `Via::Button` (Task 4)
- Produces:
  - `pub async fn handle(data: &Data, ctx: &serenity::all::Context, interaction: &ComponentInteraction)`
  - `pub(crate) async fn respond<F, Fut>(data: &Data, press: &dyn Press, custom_id: &str, who: Presser<'_>, run: F)` where `F: FnOnce(GuildId, UserId, Control) -> Fut` and `Fut: Future<Output = Result<Option<Echo>, ControlRefused>>`
  - `CrackedMessage::ButtonOutOfDate`

- [ ] **Step 1: The string and the variant.** `messages.rs`: `pub const BUTTON_OUT_OF_DATE: &str = "This button is out of date.";`. In `message.rs`, append `ButtonOutOfDate,` after `ControlEchoesOff` and give it a `Display` arm: `Self::ButtonOutOfDate => f.write_str(crate::messaging::messages::BUTTON_OUT_OF_DATE),`.

- [ ] **Step 2: Write the failing tests** in `buttons.rs`'s test module:

```rust
    use crate::messaging::test_support::{FakePress, PressOp};
    use crate::music::remote::{ControlRefused, Echo};
    use crate::Data;
    use serenity::all::{GenericChannelId, UserId};
    use std::sync::Mutex;

    const U: UserId = UserId::new(42);
    const CH: GenericChannelId = GenericChannelId::new(10);

    fn presser(guild: Option<GuildId>) -> Presser<'static> {
        Presser { guild, user: U, is_bot: false, member: None, channel: CH }
    }

    /// Records what `respond` asked to run; answers with `answer`.
    async fn press_with(
        data: &Data,
        id: &str,
        who: Presser<'_>,
        answer: Result<Option<Echo>, ControlRefused>,
    ) -> (Vec<PressOp>, Vec<(GuildId, UserId, Control)>) {
        let p = FakePress::default();
        let ran = Mutex::new(Vec::new());
        respond(data, &p, id, who, |g, u, c| {
            ran.lock().unwrap().push((g, u, c));
            async move { answer }
        })
        .await;
        (p.ops(), ran.into_inner().unwrap())
    }

    fn private(text: &str) -> PressOp {
        PressOp::Followup { ephemeral: true, text: text.into() }
    }

    #[tokio::test]
    async fn a_press_is_acknowledged_first_then_runs_its_control_and_says_nothing_more() {
        let data = Data::default();
        let id = NowPlayingButton::Skip { guild: G, track: uuid() }.custom_id();
        let (ops, ran) = press_with(&data, &id, presser(Some(G)), Ok(Some(Echo::Skipped { title: None }))).await;
        assert_eq!(ops, vec![PressOp::Acknowledge]);
        assert_eq!(ran, vec![(G, U, Control::Skip { expect: uuid() })]);
    }

    /// A control that changed nothing still says nothing to the presser: the
    /// status message is the answer.
    #[tokio::test]
    async fn a_no_op_press_is_silent() {
        let data = Data::default();
        let id = NowPlayingButton::Pause { guild: G }.custom_id();
        let (ops, ran) = press_with(&data, &id, presser(Some(G)), Ok(None)).await;
        assert_eq!(ops, vec![PressOp::Acknowledge]);
        assert_eq!(ran.len(), 1);
    }

    /// Review Focus 3.
    #[tokio::test]
    async fn a_malformed_id_is_out_of_date_and_runs_nothing() {
        let data = Data::default();
        let (ops, ran) = press_with(&data, "np:skip:1:not-a-uuid", presser(Some(G)), Ok(None)).await;
        assert_eq!(ops, vec![PressOp::Acknowledge, private("This button is out of date.")]);
        assert!(ran.is_empty());
    }

    /// An id for another guild than the one the press came from (a forged id).
    #[tokio::test]
    async fn an_id_for_another_guild_is_out_of_date_and_runs_nothing() {
        let data = Data::default();
        let id = NowPlayingButton::Pause { guild: GuildId::new(999) }.custom_id();
        let (ops, ran) = press_with(&data, &id, presser(Some(G)), Ok(None)).await;
        assert_eq!(ops, vec![PressOp::Acknowledge, private("This button is out of date.")]);
        assert!(ran.is_empty());
        let (_, ran) = press_with(&data, &id, presser(None), Ok(None)).await;
        assert!(ran.is_empty(), "a press from a DM runs nothing");
    }

    /// Review Focus 4: refused in the slash command's words, privately.
    #[tokio::test]
    async fn a_press_outside_the_music_channel_is_refused_privately() {
        let data = Data::default();
        let mut s = crate::guild::settings::GuildSettings::new(G, None, None);
        s.set_music_channel(77);
        data.guild_settings_map.write().await.insert(G, s);
        let id = NowPlayingButton::Shuffle { guild: G }.custom_id();
        let (ops, ran) = press_with(&data, &id, presser(Some(G)), Ok(None)).await;
        assert_eq!(
            ops,
            vec![
                PressOp::Acknowledge,
                private("⚠️ You are not in the music channel! Use <#10>"),
            ]
        );
        assert!(ran.is_empty());
    }

    /// Review Focus 1 and 2: a stale skip, and nothing playing.
    #[tokio::test]
    async fn refusals_from_the_control_are_answered_privately() {
        let data = Data::default();
        let id = NowPlayingButton::Skip { guild: G, track: uuid() }.custom_id();
        for (refused, words) in [
            (ControlRefused::Conflict, "That track is no longer playing"),
            (ControlRefused::NotPlaying, "🔈 Nothing is playing!"),
            (
                ControlRefused::GameInProgress,
                "🎭 A game is running; that command would break the rounds. `/gp skip`, `/gp close` or `/gp end` instead.",
            ),
            (ControlRefused::Failed, "Fatality! Something went wrong ☹️"),
        ] {
            let (ops, _) = press_with(&data, &id, presser(Some(G)), Err(refused)).await;
            assert_eq!(ops, vec![PressOp::Acknowledge, private(words)], "{refused:?}");
        }
    }
```

> These literals were checked against `Display` on master `c412741b`: `NotInMusicChannel(c)` is `"{NOT_IN_MUSIC_CHANNEL} {c.mention()}"`, and `Other(s)` is `s`. If one fails, read the `Display` impl before changing anything. Never change an error's wording to make a test pass.

Run: `cargo test -p crack-core --lib messaging::buttons 2>&1 | tail -10`. Expected: compile errors.

- [ ] **Step 3: Implement** in `buttons.rs` (above the tests; extend the `use` lines):

```rust
use crate::commands::permissions::music_access;
use crate::errors::CrackedError;
use crate::messaging::courier;
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::OP_TRACK_STALE;
use crate::messaging::render::RenderCx;
use crate::messaging::transport::{DiscordPress, Press};
use crate::music::remote::{self, ControlRefused, Echo};
use crate::messaging::cards::Via;
use crate::Data;
use serenity::all::{ComponentInteraction, GenericChannelId, Member, UserId};
use std::future::Future;
use std::sync::Arc;

/// Who pressed, and where: the parts of the interaction `respond` reads.
pub(crate) struct Presser<'a> {
    /// `None` for a press outside a guild (a DM).
    pub guild: Option<GuildId>,
    pub user: UserId,
    pub is_bot: bool,
    pub member: Option<&'a Member>,
    pub channel: GenericChannelId,
}

/// A press on an `np:` button. Acknowledged first, before anything slow,
/// inside Discord's 3-second window; every later outcome the presser needs
/// to hear is a private follow-up. Success says nothing more: the echo line
/// (if the guild has echoes on) and the re-rendered status are the answer.
pub async fn handle(data: &Data, ctx: &serenity::all::Context, interaction: &ComponentInteraction) {
    let press = DiscordPress {
        http: &ctx.http,
        interaction,
    };
    let who = Presser {
        guild: interaction.guild_id,
        user: interaction.user.id,
        is_bot: interaction.user.bot(),
        member: interaction.member.as_deref(),
        channel: interaction.channel_id,
    };
    let data_arc = Arc::new(data.clone());
    respond(data, &press, &interaction.data.custom_id, who, |guild, user, c| {
        remote::control(
            data_arc,
            ctx.http.clone(),
            ctx.cache.clone(),
            guild,
            user,
            Via::Button,
            c,
        )
    })
    .await;
}

/// [`handle`] without Discord: `run` is the control.
pub(crate) async fn respond<F, Fut>(
    data: &Data,
    press: &dyn Press,
    custom_id: &str,
    who: Presser<'_>,
    run: F,
) where
    F: FnOnce(GuildId, UserId, Control) -> Fut,
    Fut: Future<Output = Result<Option<Echo>, ControlRefused>>,
{
    courier::acknowledge(press).await;
    let cx = RenderCx::now();
    let button = match NowPlayingButton::parse(custom_id) {
        Some(b) if who.guild == Some(b.guild()) => b,
        _ => {
            tracing::warn!("np: press with an unusable id {custom_id:?} from {}", who.user);
            courier::answer_privately(press, &CrackedMessage::ButtonOutOfDate, &cx).await;
            return;
        },
    };
    let guild = button.guild();
    if let Err(err) = music_access(
        data,
        guild,
        who.member,
        who.is_bot,
        who.channel,
        button.command(),
    )
    .await
    {
        courier::answer_privately(press, &CrackedMessage::CrackedError(err), &cx).await;
        return;
    }
    if let Err(refused) = run(guild, who.user, button.control()).await {
        courier::answer_privately(press, &refusal(refused), &cx).await;
    }
}

/// A refused control, in the words the slash commands use.
fn refusal(r: ControlRefused) -> CrackedMessage {
    match r {
        ControlRefused::NotPlaying => CrackedMessage::CrackedError(CrackedError::NothingPlaying),
        ControlRefused::GameInProgress => CrackedMessage::CrackedError(CrackedError::GameInProgress),
        // Only a Skip drawn for an earlier track gets here from a button.
        ControlRefused::Conflict => CrackedMessage::CrackedError(CrackedError::Other(OP_TRACK_STALE)),
        ControlRefused::Failed => CrackedMessage::Error,
    }
}
```
If `remote::control` taking `Arc<Data>` by value fights the closure's `FnOnce`, keep it as written: `data_arc` is moved into the one call. If the borrow checker objects to `ctx` captured by reference inside the returned future, clone `ctx.http` and `ctx.cache` into locals before the closure and `move` them in.

Route it in `handlers/serenity.rs`, right after the `gp` arm:

```rust
            // Now-playing buttons: routed by custom-id prefix, like `/gp`'s.
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(mci),
            } if mci.data.custom_id.starts_with(crate::messaging::buttons::NP_PREFIX) => {
                crate::messaging::buttons::handle(&self.data, ctx, mci).await;
            },
```

- [ ] **Step 4: Run them to see them pass**

Run:
```bash
cargo test -p crack-core --lib 2>&1 | grep -E "test result|FAILED|panicked"
cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked"
cargo clippy --workspace --all-targets 2>&1 | grep -E "^(warning|error)" | head
cargo +nightly fmt --all -- --check
```
Expected: all pass; clippy and fmt are clean.

- [ ] **Step 5: Sabotage:**
  - move `courier::acknowledge` after the parse (the malformed test's op order must fail);
  - drop the `who.guild == Some(b.guild())` guard (the other-guild test must fail);
  - skip the access check (the music-channel test must fail);
  - `answer_privately` the `Conflict` refusal as `CrackedMessage::Error` (the refusal table must fail);
  - drop the `np:` arm in `serenity.rs`. **Expect nothing to catch this**: it is glue. Report it as uncaught; the TuneTitan checklist covers it.

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/messaging/buttons.rs crack-core/src/messaging/message.rs crack-core/src/messaging/messages.rs \
  crack-core/src/handlers/serenity.rs
git commit -m "Now-playing buttons: a press runs the control, refusals answer the presser privately

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 8: Release v0.23.0

**Files:**
- Modify: `Cargo.toml`: the one line `[workspace.package] version = "0.23.0"`. All members inherit it, and `docker.yml` fails a tag that is not `v` + this.
- Modify: `Cargo.lock`: regenerated by the build, workspace members only
- Modify: `CHANGELOG.md`, under `## Unreleased`

- [ ] **Step 1: Bump the version**, build to refresh the lock, and check that only workspace members moved:

```bash
sed -i '0,/^version = "0.22.0"/s//version = "0.23.0"/' Cargo.toml
grep -n '^version' Cargo.toml | head -2
cargo build --workspace 2>&1 | tail -2
git diff --stat Cargo.lock
git diff Cargo.lock | grep '^[-+]version' | sort | uniq -c
```
Expected: the root version reads 0.23.0, and the lock diff touches only our crates' `0.22.0 → 0.23.0` lines.

- [ ] **Step 2: CHANGELOG.** Under `## Unreleased` → `### Added`, add:

```markdown
- **Buttons on the now-playing message.** Pause or Resume, Skip, Repeat and
  Shuffle, for anyone who may use the music commands there (the music channel
  and `/gp` rules apply). A press posts the same echo line a dashboard control
  does, and the status message updates below it. Skip names the song it was
  drawn for, so an old message or two people pressing at once cannot skip the
  next song. Buttons on an old message still work after a restart.
- **`/echoes`** (admins) turns those echo lines off or on for the server, for
  buttons and the dashboard alike. On by default.
- The queue history's source filter (`/auditlog`, the dashboard) has `button`.
```
Under `### Fixed`:
```markdown
- A dashboard control that changed nothing (pausing a paused song from a stale
  tab) no longer posts an echo line.
```

- [ ] **Step 3: Full verification.** Run every one of these and paste the summary lines into your report:

```bash
cargo +nightly fmt --all -- --check
cargo clippy --workspace --all-targets 2>&1 | grep -cE "^(warning|error)"
SQLX_OFFLINE=true cargo check --workspace --all-targets 2>&1 | tail -1
cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked"
cargo test --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests 2>&1 | grep -E "test result|FAILED|panicked"
```
Expected: fmt clean, `0` warnings and errors, and every suite `ok`. The db-tests run needs the postgres from Global Constraints. Report the db-tests line as not run if you could not start it.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "v0.23.0: now-playing buttons and /echoes

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

- [ ] **Step 5 (controller, not a subagent): open the PR.** Title "v0.23.0: buttons on the now-playing message, and /echoes". The body covers:
  - the design;
  - the plan's rulings, plus any taken during the build;
  - a mutation → caught-by table, including the two uncaught glue sabotages;
  - follow-ups: a dashboard echoes toggle, `/gp` onto the courier, and real-time ticking;
  - the TuneTitan test plan below.

**TuneTitan test plan (for the PR body):**
- [ ] `/play`: the status message has ⏸ Pause, ⏭ Skip, 🔁 Repeat (grey) and 🔀 Shuffle.
- [ ] Press ⏸ Pause: "⏸ Paused — @you" posts, and the status re-renders below it, reading "Paused at m:ss" with a ▶ Resume button. Press ▶ Resume: the end time comes back.
- [ ] Press 🔁 Repeat: it turns green and the card reads "… · on repeat". Press it again: grey, with the end time back.
- [ ] Press ⏭ Skip: "⏭ Skipped **Title** — @you", and the next song's status appears.
- [ ] Press ⏭ Skip on an *older* status message (or twice quickly): the second press says "That track is no longer playing" only to you, and nothing else skips.
- [ ] Press 🔀 Shuffle with one song queued: no echo.
- [ ] `/echoes` (as admin): "🔇 Button and dashboard controls no longer post a line in the channel." A press now posts nothing, and the status edits in place. Run `/echoes` again to turn it back on.
- [ ] With a music channel set, the buttons work there. A member of a server whose music channel is elsewhere is refused privately.
- [ ] During `/gp`, pressing an old status message's button says the game is running, privately.
- [ ] Restart the bot (`./homelab.sh up tunetitan`), then press a button on the status message from before the restart: it still works.
- [ ] Dashboard: a pause from a stale tab while already paused posts no echo.
- [ ] `/auditlog source:button` lists the presses. The dashboard's history page filter has "button".
- [ ] `verify tunetitan`: 26/26 migrations, and "Registered 44 global commands" in the log.
