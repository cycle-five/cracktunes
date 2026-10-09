# Now-playing buttons: switch and debounce — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A per-server `/buttons` switch for the now-playing buttons, and a flat 2-second press debounce per (server, user).

**Architecture:** A new guild setting `now_playing_buttons`, plumbed exactly like v0.23.0's `control_echoes` (migration, `GuildSettings`, `GuildSettingsRead` + upsert, operations, admin toggle command). A small generic `Throttle<K>` on `Data` gates presses in `messaging::buttons::respond`; the setting gates the status render in `status::show_now_playing_on` and presses in `respond`; one `status::buttons_switched` function brings the screen in line when the setting flips.

**Tech Stack:** Rust, poise/serenity (next), songbird, sqlx 0.9 (offline `.sqlx`), dashmap, tokio.

**Spec:** `docs/superpowers/specs/2026-10-09-np-buttons-switch-and-debounce-design.md`

## Global Constraints

- Window: `NP_PRESS_WINDOW = Duration::from_secs(2)`, fixed from the last *accepted* press; a dropped press never extends it.
- A debounced press is acknowledged (as every press is) and gets **nothing else** — no private message, no control run.
- Order in `respond`: acknowledge → parse → throttle → switch → `music_access` → run.
- Setting default: **on** (`DEFAULT TRUE`, `default_true` serde, `true` in `GuildSettings::new`, `get_` returns `true` with no settings loaded).
- `/buttons`: category `Settings`, `slash_command, prefix_command, guild_only`, `required_permissions = "ADMINISTRATOR"`, `default_member_permissions = "ADMINISTRATOR"`, registered in `music_commands()`.
- User-facing strings are constants in `crack-core/src/messaging/messages.rs`, not literals at the use site.
- Migration version `20261009130000`, file in BOTH `migrations/` and `crack-core/test_migrations/`, identical.
- Never `git add -A`; add files by name. Every commit ends with the trailer `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line.
- Run commands from the repo root `/home/lothrop/projects/cracktunes`. Gate per task: `cargo fmt --all`, `cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return`, and the task's tests.
- Database tests (`--features db-tests`) connect to `postgresql://postgres:mysecretpassword@localhost:5432/postgres` no matter what `DATABASE_URL` says (a ctor overwrites it). A container `pg-ct` is already running there; if it is not, start one with `docker --context default run -d --rm --name pg-ct -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine` (bare `docker` targets a remote host here).
- **Sabotage every new test:** after it passes, break the code it guards, watch it fail, restore. Record each mutation and the test that caught it in your report.

## Review Focus

1. **Two presses at the same instant** (a double-click delivered as two interactions): exactly one runs. → Task 1 test `two_presses_at_the_same_instant_let_one_through`; Task 3 test `a_second_press_inside_the_window_is_acknowledged_and_nothing_else`.
2. **A press that never parses for this guild** (forged id, DM, another guild's button) must not use up the presser's window. → Task 3 test `an_out_of_date_press_does_not_use_up_the_window`.
3. **Mashing an old button after `/buttons` off**: one private refusal per window, not one per press. → Task 3 test `mashing_with_the_buttons_off_is_refused_once_per_window`.
4. **`/buttons` off when the status message is gone** (deleted by hand or by `/clean`): the clear fails, it is logged, nothing panics, the toggle still stands. → Task 4 test `off_survives_a_status_message_that_is_gone`.
5. **`/buttons` off when the status already says Finished, or nothing is tracked**: no Discord call at all. → Task 4 test `off_with_a_finished_or_empty_status_does_nothing`.

---

## File Structure

| file | change |
| --- | --- |
| `crack-core/src/messaging/throttle.rs` | **new**: `Throttle<K>` |
| `crack-core/src/messaging/mod.rs` | `pub mod throttle;` |
| `migrations/20261009130000_now_playing_buttons.sql`, `crack-core/test_migrations/…` | **new**: the column |
| `crack-core/src/guild/settings.rs` | field, `PartialEq`, row → settings, `new`, `toggle_now_playing_buttons`, row test |
| `crack-core/src/db/guild.rs` | `GuildSettingsRead` field, upsert, db round-trip test |
| `crack-core/src/guild/operations.rs` | `get_/toggle_now_playing_buttons` + tests |
| `.sqlx/` | regenerated with `scripts/sqlx_sync.py` |
| `crack-core/src/lib.rs` | `DataInner.np_presses` + its default |
| `crack-core/src/messaging/buttons.rs` | `NP_PRESS_WINDOW`; `respond` gains `now`, throttle, switch; tests |
| `crack-core/src/messaging/messages.rs`, `message.rs` | three strings, three `CrackedMessage` variants |
| `crack-core/src/messaging/status.rs` | gate in `show_now_playing_on`; `buttons_switched`; tests |
| `crack-core/src/commands/music/np_buttons.rs` | **new**: `/buttons` |
| `crack-core/src/commands/music/mod.rs` | module, re-export, register |
| `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock` | release v0.25.0 |

---

### Task 1: `Throttle<K>`

**Files:**
- Create: `crack-core/src/messaging/throttle.rs`
- Modify: `crack-core/src/messaging/mod.rs` (add `pub mod throttle;` in alphabetical order, after `pub mod status;`)

**Interfaces:**
- Produces: `crate::messaging::throttle::Throttle<K>` with `pub fn new(window: Duration) -> Self` and `pub fn allow(&self, key: K, now: std::time::Instant) -> bool`, where `K: Eq + Hash`. `#[derive(Debug)]`.

- [ ] **Step 1: Write the failing tests** — create `crack-core/src/messaging/throttle.rs` containing ONLY the test module first (plus the `use`s it needs), so it fails to compile for want of `Throttle`:

```rust
//! A per-key minimum gap between accepted events: the now-playing buttons'
//! press debounce.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const W: Duration = Duration::from_secs(2);
    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn the_first_press_is_allowed() {
        let t = Throttle::new(W);
        assert!(t.allow(1, Instant::now()));
    }

    #[test]
    fn a_press_inside_the_window_is_refused_and_one_at_its_end_is_allowed() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow(1, t0));
        assert!(!t.allow(1, t0 + W - MS));
        assert!(t.allow(1, t0 + W));
    }

    /// Fixed from the last accepted press: a masher gets one through every
    /// window. A sliding window would refuse the press at `t0 + W`.
    #[test]
    fn a_refused_press_does_not_extend_the_window() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow(1, t0));
        assert!(!t.allow(1, t0 + W / 2));
        assert!(!t.allow(1, t0 + W - MS));
        assert!(t.allow(1, t0 + W));
    }

    #[test]
    fn two_presses_at_the_same_instant_let_one_through() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow(1, t0));
        assert!(!t.allow(1, t0));
    }

    #[test]
    fn keys_are_independent() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow((10, 1), t0));
        assert!(t.allow((10, 2), t0), "another user");
        assert!(t.allow((20, 1), t0), "another server");
        assert!(!t.allow((10, 1), t0));
    }

    /// Past `PRUNE_ABOVE` keys, a press first drops every stamp whose window
    /// has passed; stamps still inside their window stay and still refuse.
    #[test]
    fn a_full_map_drops_only_stale_stamps() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        for k in 0..=PRUNE_ABOVE {
            assert!(t.allow(k, t0));
        }
        // Inside the window nothing is stale: all kept, the old ones still refuse.
        assert!(t.allow(PRUNE_ABOVE + 1, t0 + MS));
        assert_eq!(t.len(), PRUNE_ABOVE + 2);
        assert!(!t.allow(0, t0 + MS));
        // At the window every stamp from t0 is stale and goes.
        assert!(t.allow(PRUNE_ABOVE + 2, t0 + W));
        assert!(t.len() < 3, "only the stamps from t0 + 1ms and t0 + W remain");
    }
}
```

- [ ] **Step 2: Add the module and run the tests to verify they fail**

Add `pub mod throttle;` to `crack-core/src/messaging/mod.rs`.
Run: `cargo test -p crack-core --lib -- messaging::throttle`
Expected: compile error, `cannot find type Throttle` / `PRUNE_ABOVE`.

- [ ] **Step 3: Implement** — insert above the test module:

```rust
use dashmap::{mapref::entry::Entry, DashMap};
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Past this many keys, `allow` first drops the stamps whose window has
/// passed, so the map stays about as large as the set of people pressing
/// right now.
const PRUNE_ABOVE: usize = 1024;

/// Accepts at most one event per key per `window`, measured from the last
/// *accepted* one: a refused event does not extend the window. In memory
/// only; a restart forgets every stamp.
#[derive(Debug)]
pub struct Throttle<K: Eq + Hash> {
    window: Duration,
    last: DashMap<K, Instant>,
}

impl<K: Eq + Hash> Throttle<K> {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last: DashMap::new(),
        }
    }

    /// Whether an event for `key` at `now` is accepted, stamping it if so.
    /// One atomic step on the key's entry: two events arriving together
    /// cannot both get through.
    pub fn allow(&self, key: K, now: Instant) -> bool {
        if self.last.len() > PRUNE_ABOVE {
            self.last
                .retain(|_, at| now.saturating_duration_since(*at) < self.window);
        }
        match self.last.entry(key) {
            Entry::Occupied(mut stamp) => {
                if now.saturating_duration_since(*stamp.get()) >= self.window {
                    stamp.insert(now);
                    true
                } else {
                    false
                }
            },
            Entry::Vacant(slot) => {
                slot.insert(now);
                true
            },
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.last.len()
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p crack-core --lib -- messaging::throttle`
Expected: 6 passed.

- [ ] **Step 5: Sabotage.** One at a time, restore after each: `>=` → `>` (caught by the window-end test); insert `now` on the refused branch too (caught by `a_refused_press_does_not_extend_the_window`); `< self.window` → `<= self.window` in `retain` (caught by `a_full_map_drops_only_stale_stamps`); remove the `retain` call (same test). Record each.

- [ ] **Step 6: fmt, clippy, commit**

```bash
cargo fmt --all
cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-core/src/messaging/throttle.rs crack-core/src/messaging/mod.rs
git commit -m "throttle: at most one accepted event per key per window

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: The `now_playing_buttons` guild setting

**Files:**
- Create: `migrations/20261009130000_now_playing_buttons.sql`, `crack-core/test_migrations/20261009130000_now_playing_buttons.sql`
- Modify: `crack-core/src/guild/settings.rs`, `crack-core/src/db/guild.rs`, `crack-core/src/guild/operations.rs`, `.sqlx/` (generated)

**Interfaces:**
- Produces: `GuildSettings.now_playing_buttons: bool`; `GuildSettings::toggle_now_playing_buttons(&mut self) -> &mut Self`; on the `GuildSettingsOperations` trait (implemented for `Data`): `fn get_now_playing_buttons(&self, guild_id: GuildId) -> impl Future<Output = bool>` and `fn toggle_now_playing_buttons(&self, guild_id: GuildId) -> impl Future<Output = Result<bool, CrackedError>>`. `GuildSettingsRead.now_playing_buttons: bool`.

This task mirrors `control_echoes` exactly; search the files for `control_echoes` and add the same thing beside each occurrence.

- [ ] **Step 1: Write the failing tests**

In `crack-core/src/guild/operations.rs`, inside `mod test`, after `toggling_control_echoes_flips_it_and_reports_the_new_value`:

```rust
    #[tokio::test]
    async fn now_playing_buttons_are_on_for_a_guild_with_no_settings() {
        let data = crate::Data::default();
        assert!(data.get_now_playing_buttons(GuildId::new(123)).await);
    }

    #[tokio::test]
    async fn now_playing_buttons_follow_the_guild_setting() {
        let data = crate::Data::default();
        let guild_id = GuildId::new(123);
        let mut settings = GuildSettings::new(guild_id, None, None);
        settings.now_playing_buttons = false;
        data.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings);
        assert!(!data.get_now_playing_buttons(guild_id).await);
    }

    #[tokio::test]
    async fn toggling_now_playing_buttons_flips_it_and_reports_the_new_value() {
        let data = crate::Data::default();
        let guild_id = GuildId::new(123);
        assert!(!data.toggle_now_playing_buttons(guild_id).await.unwrap());
        assert!(!data.get_now_playing_buttons(guild_id).await);
        assert!(data.toggle_now_playing_buttons(guild_id).await.unwrap());
        assert!(data.get_now_playing_buttons(guild_id).await);
    }
```

In `crack-core/src/guild/settings.rs`'s test module, add `now_playing_buttons: true,` to the `GuildSettingsRead` literal in `a_database_row_carries_ephemeral_replies` (after `control_echoes: true,`), and add:

```rust
    #[test]
    fn a_database_row_carries_now_playing_buttons() {
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
            control_echoes: true,
            now_playing_buttons: false,
        };
        assert!(!GuildSettings::from(row).now_playing_buttons);
    }

    #[test]
    fn new_settings_have_buttons_and_settings_differing_in_them_are_not_equal() {
        let on = GuildSettings::new(GuildId::new(123), None, None);
        assert!(on.now_playing_buttons);
        let mut off = on.clone();
        off.toggle_now_playing_buttons();
        assert!(!off.now_playing_buttons);
        assert_ne!(on, off);
    }
```

In `crack-core/src/db/guild.rs`, after `mod control_echoes_db_tests { … }`, add:

```rust
#[cfg(test)]
mod now_playing_buttons_db_tests {
    use super::*;
    use std::str::FromStr;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    /// Off must survive a restart, and a new guild starts on.
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn now_playing_buttons_survive_a_save_and_load(
        pool: PgPool,
    ) -> Result<(), SerenityError> {
        let name = FixedString::from_str("buttons test").expect("a short name");
        let (_guild, mut settings) =
            GuildEntity::get_or_create(&pool, 454545, name, "r!".to_string()).await?;
        assert!(settings.now_playing_buttons);

        settings.now_playing_buttons = false;
        GuildEntity::write_settings(&pool, &settings).await?;

        let reloaded = GuildEntity::new_guild(454545, "buttons test".to_string())
            .get_settings(&pool)
            .await?;
        assert!(!reloaded.now_playing_buttons);
        Ok(())
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p crack-core --lib -- now_playing_buttons`
Expected: compile errors — no field `now_playing_buttons`, no method `get_now_playing_buttons`.

- [ ] **Step 3: Migration** — write this to BOTH `migrations/20261009130000_now_playing_buttons.sql` and `crack-core/test_migrations/20261009130000_now_playing_buttons.sql`:

```sql
-- `/buttons` turns the now-playing message's buttons off or on for a server.
-- On by default: the buttons were always shown before v0.25.0.
ALTER TABLE guild_settings
    ADD COLUMN IF NOT EXISTS now_playing_buttons BOOLEAN NOT NULL DEFAULT TRUE;
```

Then check no migration version is duplicated (an existing `.up`/`.down` pair from 2024 is expected and fine): `ls migrations | sort | uniq -w14 -d` must print only `20240428013633_permission_settings.down.sql`.

- [ ] **Step 4: `GuildSettings`** (`crack-core/src/guild/settings.rs`), beside each `control_echoes` occurrence:

```rust
    // struct field, after control_echoes:
    /// Whether the now-playing status message carries its buttons. Off is
    /// how a server that restricted the music commands under Integrations
    /// keeps them from being reached by a button.
    #[serde(default = "default_true")]
    pub now_playing_buttons: bool,

    // PartialEq, after the control_echoes line:
            && self.now_playing_buttons == other.now_playing_buttons

    // From<GuildSettingsRead>, after the control_echoes line:
        settings.now_playing_buttons = settings_db.now_playing_buttons;

    // GuildSettings::new, after control_echoes: true,
            now_playing_buttons: true,

    // after toggle_control_echoes:
    /// Toggle whether the now-playing message carries its buttons.
    pub fn toggle_now_playing_buttons(&mut self) -> &mut Self {
        self.now_playing_buttons = !self.now_playing_buttons;
        self
    }
```

- [ ] **Step 5: `db/guild.rs`** — add `pub now_playing_buttons: bool,` after `pub control_echoes: bool,` in `GuildSettingsRead`. In the upsert (`INSERT INTO guild_settings (… ephemeral_replies, control_echoes)`), append `now_playing_buttons` to the column list, `$17` to `VALUES`, `, now_playing_buttons = $17` to `DO UPDATE SET`, and `settings.now_playing_buttons,` after `settings.control_echoes,` in the arguments.

- [ ] **Step 6: Operations** (`crack-core/src/guild/operations.rs`). In the trait, after `toggle_control_echoes`:

```rust
    fn get_now_playing_buttons(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn toggle_now_playing_buttons(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<bool, CrackedError>>;
```

In `impl GuildSettingsOperations for Data`, after `toggle_control_echoes`:

```rust
    /// Whether the now-playing message carries its buttons. On when the guild
    /// has no settings loaded: the buttons were always shown before v0.25.0.
    async fn get_now_playing_buttons(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .is_none_or(|settings| settings.now_playing_buttons)
    }

    /// Flip whether the now-playing message carries its buttons, save it, and
    /// return the new value.
    ///
    /// 🔑 As with `toggle_control_echoes`: load the stored row first, or the
    /// full-row upsert writes defaults over it.
    async fn toggle_now_playing_buttons(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.ensure_settings_loaded(guild_id).await?;
        let settings = self
            .guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|settings| {
                settings.toggle_now_playing_buttons();
            })
            .or_insert_with(|| {
                let mut settings =
                    GuildSettings::new(guild_id, Some(&self.bot_settings.get_prefix()), None);
                settings.toggle_now_playing_buttons();
                settings
            })
            .clone();
        if let Some(pool) = self.database_pool.as_ref() {
            settings.save(pool).await?;
        }
        Ok(settings.now_playing_buttons)
    }
```

(If the file has a test that lists every toggle to check the "load the stored row first" guard — search `unguarded` — add `toggle_now_playing_buttons` to it the same way `toggle_control_echoes` is.)

- [ ] **Step 7: Regenerate `.sqlx`.** Five files change: the upsert (new hash) and the four `SELECT * FROM guild_settings` queries (same hash, one more column). Do not hand-edit them:

```bash
export DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres
sqlx migrate run --source migrations
out=$(mktemp -d)
SQLX_OFFLINE=false SQLX_OFFLINE_DIR="$out" cargo build -p crack-core --all-targets --features db-tests
python3 scripts/sqlx_sync.py "$out"
git status --short .sqlx
```

Expected: `written:` lists 5 files (4 modified, 1 new), `removed stale:` lists 1 (the old upsert). Then confirm an offline build agrees: `SQLX_OFFLINE=true cargo build -p crack-core --all-targets`.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p crack-core --lib --features db-tests -- now_playing_buttons a_database_row_carries control_echoes`
Expected: all pass, including `now_playing_buttons_survive_a_save_and_load` (not ignored).

- [ ] **Step 9: Sabotage**, one at a time: drop the `From` line (row test fails); default `false` in `new` (equality/new test fails); `is_none_or` → `is_some_and` (no-settings test fails); leave `$17` out of the upsert's `DO UPDATE` (db round trip fails — the row exists, so only the update path stores it); swap `toggle_now_playing_buttons` to toggle `control_echoes` (toggle test fails). Record each.

- [ ] **Step 10: fmt, clippy, commit**

```bash
cargo fmt --all
cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add migrations/20261009130000_now_playing_buttons.sql crack-core/test_migrations/20261009130000_now_playing_buttons.sql crack-core/src/guild/settings.rs crack-core/src/db/guild.rs crack-core/src/guild/operations.rs
git add .sqlx   # only after `git status --short .sqlx` shows exactly the files the sync reported
git commit -m "now_playing_buttons: a guild setting for the now-playing buttons, on by default

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: The press path — debounce and switch in `respond`

**Files:**
- Modify: `crack-core/src/lib.rs` (`DataInner` field + `Default`), `crack-core/src/messaging/buttons.rs`, `crack-core/src/messaging/messages.rs`, `crack-core/src/messaging/message.rs`

**Interfaces:**
- Consumes: `Throttle<K>::{new, allow}` (Task 1); `GuildSettingsOperations::get_now_playing_buttons` (Task 2).
- Produces: `pub const NP_PRESS_WINDOW: Duration` in `messaging::buttons`; `DataInner.np_presses: crate::messaging::throttle::Throttle<(GuildId, UserId)>`; `CrackedMessage::NowPlayingButtonsDisabled`; `messages::NP_BUTTONS_DISABLED`; `respond` gains a `now: std::time::Instant` parameter after `who`.

- [ ] **Step 1: Write the failing tests** in `buttons.rs`'s test module. First change the helper so a test can choose the time, keeping `press_with` for the existing tests:

```rust
    /// Records what `respond` asked to run; answers with `answer`; presses at `now`.
    async fn press_at(
        data: &Data,
        id: &str,
        who: Presser<'_>,
        now: Instant,
        answer: Result<Option<Echo>, ControlRefused>,
    ) -> (Vec<PressOp>, Vec<(GuildId, UserId, Via, Control)>) {
        let p = FakePress::default();
        let ran = Mutex::new(Vec::new());
        respond(data, &p, id, who, now, |g, u, v, c| {
            ran.lock().unwrap().push((g, u, v, c));
            async move { answer }
        })
        .await;
        (p.ops(), ran.into_inner().unwrap())
    }

    /// [`press_at`] now.
    async fn press_with(
        data: &Data,
        id: &str,
        who: Presser<'_>,
        answer: Result<Option<Echo>, ControlRefused>,
    ) -> (Vec<PressOp>, Vec<(GuildId, UserId, Via, Control)>) {
        press_at(data, id, who, Instant::now(), answer).await
    }
```

(add `use std::time::Instant;` to the test module's imports.) Then add:

```rust
    fn pause() -> String {
        NowPlayingButton::Pause { guild: G }.custom_id()
    }

    async fn buttons_off(data: &Data) {
        let mut s = crate::guild::settings::GuildSettings::new(G, None, None);
        s.now_playing_buttons = false;
        data.guild_settings_map.write().await.insert(G, s);
    }

    #[tokio::test]
    async fn a_second_press_inside_the_window_is_acknowledged_and_nothing_else() {
        let data = Data::default();
        let t0 = Instant::now();
        let (_, ran) = press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
        let (ops, ran) = press_at(
            &data,
            &pause(),
            presser(Some(G)),
            t0 + NP_PRESS_WINDOW - Duration::from_millis(1),
            Ok(None),
        )
        .await;
        assert_eq!(ops, vec![PressOp::Acknowledge]);
        assert!(ran.is_empty());
        let (_, ran) = press_at(&data, &pause(), presser(Some(G)), t0 + NP_PRESS_WINDOW, Ok(None)).await;
        assert_eq!(ran.len(), 1, "after the window it runs again");
    }

    /// One window across all the buttons: a Skip right after a Pause is dropped too.
    #[tokio::test]
    async fn the_window_covers_every_button() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        let shuffle = NowPlayingButton::Shuffle { guild: G }.custom_id();
        let (_, ran) = press_at(&data, &shuffle, presser(Some(G)), t0, Ok(None)).await;
        assert!(ran.is_empty());
    }

    #[tokio::test]
    async fn another_user_is_not_held_up_by_the_window() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        let other = Presser {
            user: UserId::new(43),
            ..presser(Some(G))
        };
        let (_, ran) = press_at(&data, &pause(), other, t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
    }

    /// The window is per server too: the same person pressing in another
    /// server is not held up.
    #[tokio::test]
    async fn another_server_is_not_held_up_by_the_window() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        let g2 = GuildId::new(222);
        let id = NowPlayingButton::Pause { guild: g2 }.custom_id();
        let (_, ran) = press_at(&data, &id, presser(Some(g2)), t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
    }

    /// Review Focus 2: a press that never parses for this guild is not a press
    /// here, and must not use up the presser's window.
    #[tokio::test]
    async fn an_out_of_date_press_does_not_use_up_the_window() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(&data, "np:skip:1:not-a-uuid", presser(Some(G)), t0, Ok(None)).await;
        let (_, ran) = press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
    }

    #[tokio::test]
    async fn with_the_buttons_off_a_press_is_refused_privately_and_runs_nothing() {
        let data = Data::default();
        buttons_off(&data).await;
        let (ops, ran) = press_with(&data, &pause(), presser(Some(G)), Ok(None)).await;
        assert_eq!(
            ops,
            vec![
                PressOp::Acknowledge,
                private("The now-playing buttons are turned off in this server.")
            ]
        );
        assert!(ran.is_empty());
    }

    /// Review Focus 3: mashing an old button after `/buttons` off draws one
    /// refusal per window, not one per press.
    #[tokio::test]
    async fn mashing_with_the_buttons_off_is_refused_once_per_window() {
        let data = Data::default();
        buttons_off(&data).await;
        let t0 = Instant::now();
        let (first, _) = press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        assert_eq!(first.len(), 2, "acknowledged and refused");
        for ms in [1, 500, 1999] {
            let (ops, _) = press_at(
                &data,
                &pause(),
                presser(Some(G)),
                t0 + Duration::from_millis(ms),
                Ok(None),
            )
            .await;
            assert_eq!(ops, vec![PressOp::Acknowledge], "at +{ms}ms");
        }
    }
```

Also fix `refusals_from_the_control_are_answered_privately`: it presses four times in a row as the same user, which the window would now drop. Give each iteration its own `Data` — move `let data = Data::default();` inside the `for` loop.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p crack-core --lib -- messaging::buttons`
Expected: compile errors (`respond` takes 5 arguments; `NP_PRESS_WINDOW` not found; no field `now_playing_buttons` is fine — that exists after Task 2).

- [ ] **Step 3: Strings.** `messages.rs`, after `BUTTON_OUT_OF_DATE`:

```rust
pub const NP_BUTTONS_DISABLED: &str = "The now-playing buttons are turned off in this server.";
```

`message.rs`: add the variant `NowPlayingButtonsDisabled,` at the END of the `CrackedMessage` enum (after `Gp(…)`; the enum's comment says variants are appended), and in `Display`:

```rust
            Self::NowPlayingButtonsDisabled => {
                f.write_str(crate::messaging::messages::NP_BUTTONS_DISABLED)
            },
```

If `CrackedMessage` has an exhaustive match elsewhere that fails to compile (e.g. deciding embed vs plain text), put `NowPlayingButtonsDisabled` with `ButtonOutOfDate` in each.

- [ ] **Step 4: `Data`.** In `buttons.rs`, near `NP_PREFIX`:

```rust
/// One accepted press per person per server per this long, across all the
/// buttons. A press inside it is acknowledged and dropped.
pub const NP_PRESS_WINDOW: Duration = Duration::from_secs(2);
```

(add `use std::time::{Duration, Instant};`). In `crack-core/src/lib.rs`, in `DataInner` after `failure_notices`:

```rust
    /// The now-playing buttons' press debounce; see `messaging::buttons`.
    pub np_presses: crate::messaging::throttle::Throttle<(GuildId, UserId)>,
```

and in `impl Default for DataInner`, after `failure_notices: Default::default(),`:

```rust
            np_presses: crate::messaging::throttle::Throttle::new(
                crate::messaging::buttons::NP_PRESS_WINDOW,
            ),
```

(`UserId` may need importing in lib.rs; use the same path style as `GuildId` there.)

- [ ] **Step 5: `respond`.** Add `now: Instant` after `who: Presser<'_>,` in its signature, and pass `Instant::now()` from `handle` (`respond(data, &press, &interaction.data.custom_id, who, Instant::now(), |guild, user, via, c| …)`). Then, between the parse and `music_access`:

```rust
    let guild = button.guild();
    // One accepted press per person per server per window; the rest are
    // acknowledged above and dropped here, before anything can answer them.
    if !data.np_presses.allow((guild, who.user), now) {
        return;
    }
    // A server can turn the buttons off (`/buttons`); old messages keep theirs.
    if !data.get_now_playing_buttons(guild).await {
        courier::answer_privately(press, &CrackedMessage::NowPlayingButtonsDisabled, &cx).await;
        return;
    }
```

(replacing the existing `let guild = button.guild();` line; add `use crate::guild::operations::GuildSettingsOperations;`). Update the doc comment on `handle` to mention the window and the switch in one sentence each.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p crack-core --lib -- messaging::buttons`
Expected: all pass (the 7 new ones and every existing one).

- [ ] **Step 7: Sabotage**, one at a time: move the throttle above the parse (`an_out_of_date_press_does_not_use_up_the_window` fails); key it on `(GuildId::new(1), who.user)`, ignoring the server (`another_server_is_not_held_up_by_the_window` fails); key it on `(guild, UserId::new(1))`, ignoring the presser (`another_user_is_not_held_up_by_the_window` fails); swap the throttle and the switch (`mashing_with_the_buttons_off…` fails); drop the `return` after the refusal (`with_the_buttons_off…` fails: the control runs); invert the switch check (`with_the_buttons_off…` and the existing happy-path test fail). Record each.

- [ ] **Step 8: fmt, clippy, commit**

```bash
cargo fmt --all
cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-core/src/lib.rs crack-core/src/messaging/buttons.rs crack-core/src/messaging/messages.rs crack-core/src/messaging/message.rs
git commit -m "buttons: one press per person per server per 2s, and refused when the server turned them off

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: The screen — render gate, `buttons_switched`, and `/buttons`

**Files:**
- Modify: `crack-core/src/messaging/status.rs`, `crack-core/src/messaging/messages.rs`, `crack-core/src/messaging/message.rs`, `crack-core/src/commands/music/mod.rs`
- Create: `crack-core/src/commands/music/np_buttons.rs`

**Interfaces:**
- Consumes: `get_now_playing_buttons` / `toggle_now_playing_buttons` (Task 2); `Transport::clear_components`; `Data::status_slot(guild) -> Arc<tokio::sync::Mutex<StatusSlot>>`; `music_utils::connected_call(&Songbird, GuildId, None) -> Option<Arc<Mutex<Call>>>`; `DiscordTransport::of(&serenity::all::Context)`.
- Produces: `pub(crate) async fn buttons_switched(data: &Data, transport: &dyn Transport, guild: GuildId, on: bool, call: Option<&Arc<Mutex<Call>>>)` in `messaging::status`; the `/buttons` command (`commands::music::buttons()`); `CrackedMessage::{NowPlayingButtonsOn, NowPlayingButtonsOff}`; `messages::{NP_BUTTONS_ON, NP_BUTTONS_OFF}`.

- [ ] **Step 1: Write the failing tests.** In `status.rs`'s test module, after `the_now_playing_status_carries_the_button_row`:

```rust
    async fn buttons_off(data: &crate::Data) {
        let mut s = crate::guild::settings::GuildSettings::new(GUILD, None, None);
        s.now_playing_buttons = false;
        data.guild_settings_map.write().await.insert(GUILD, s);
    }

    /// A server that turned the buttons off gets the card without them.
    #[tokio::test]
    async fn the_now_playing_status_has_no_buttons_when_the_server_turned_them_off() {
        use crate::music::ops::test_support::queue_of;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (data, call, _ids, _rx) = queue_of(1).await;
            buttons_off(&data).await;
            note_command_channel(&data, GUILD, ch(10)).await;
            let t = FakeTransport::default();

            assert!(show_now_playing_on(&data, &t, GUILD, &call, None).await.is_some());
            let sent = t.sent.lock().unwrap();
            assert!(sent[0].components.is_empty());
            assert!(sent[0].embed.is_some(), "still the now-playing card");
        })
        .await
        .expect("get_info is bounded, so the render finishes");
    }

    async fn tracked(data: &crate::Data, phase: Phase) {
        data.status_slot(GUILD).lock().await.message = Some(StatusMessage {
            channel: ch(10),
            id: MessageId::new(500),
            phase,
        });
    }

    #[tokio::test]
    async fn off_takes_the_buttons_off_the_status_on_screen_and_nothing_else() {
        let data = crate::Data::default();
        tracked(&data, Phase::Playing).await;
        let t = FakeTransport::default();
        buttons_switched(&data, &t, GUILD, false, None).await;
        assert_eq!(t.ops(), vec![Op::ClearComponents(10, 500)]);
    }

    /// Review Focus 5.
    #[tokio::test]
    async fn off_with_a_finished_or_empty_status_does_nothing() {
        let data = crate::Data::default();
        let t = FakeTransport::default();
        buttons_switched(&data, &t, GUILD, false, None).await;
        tracked(&data, Phase::Finished).await;
        buttons_switched(&data, &t, GUILD, false, None).await;
        assert!(t.ops().is_empty());
    }

    /// Review Focus 4: the message was deleted by hand; the clear fails and
    /// that is all.
    #[tokio::test]
    async fn off_survives_a_status_message_that_is_gone() {
        let data = crate::Data::default();
        tracked(&data, Phase::Playing).await;
        let t = FakeTransport::default();
        *t.edit_error.lock().unwrap() = Some(TransportError::Other("Unknown Message".into()));
        buttons_switched(&data, &t, GUILD, false, None).await;
        assert_eq!(t.ops(), vec![Op::ClearComponents(10, 500)]);
    }

    #[tokio::test]
    async fn on_with_nothing_playing_does_nothing() {
        let data = crate::Data::default();
        tracked(&data, Phase::Playing).await;
        let t = FakeTransport::default();
        buttons_switched(&data, &t, GUILD, true, None).await;
        assert!(t.ops().is_empty());
    }

    /// On re-renders the status with its buttons.
    #[tokio::test]
    async fn on_with_a_song_playing_shows_the_status_with_its_buttons() {
        use crate::music::ops::test_support::queue_of;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (data, call, _ids, _rx) = queue_of(1).await;
            note_command_channel(&data, GUILD, ch(10)).await;
            let t = FakeTransport::default();
            buttons_switched(&data, &t, GUILD, true, Some(&call)).await;
            assert_eq!(t.ops(), vec![Op::Send(10)]);
            let sent = t.sent.lock().unwrap();
            let row = serde_json::to_value(&sent[0].components[0]).unwrap();
            assert_eq!(row["components"][0]["custom_id"], "np:pause:1");
        })
        .await
        .expect("get_info is bounded, so the render finishes");
    }
```

(Check the test module's imports: it needs `MessageId`, `Phase`, `StatusMessage`, `Op`, `TransportError`, `FakeTransport`; most are already there for the existing tests.)

Create `crack-core/src/commands/music/np_buttons.rs` with only its test for now:

```rust
#[cfg(test)]
mod tests {
    use poise::serenity_prelude::all::Permissions;

    #[test]
    fn buttons_is_an_admin_only_registered_guild_command() {
        let command = super::buttons();
        assert_eq!(command.name, "buttons");
        assert!(command.slash_action.is_some(), "a slash command");
        assert!(command.guild_only, "guild only");
        assert!(command
            .required_permissions
            .contains(Permissions::ADMINISTRATOR));
        assert!(command
            .default_member_permissions
            .contains(Permissions::ADMINISTRATOR));
        assert!(
            crate::commands::commands_to_register()
                .into_iter()
                .any(|command| command.name == "buttons"),
            "registered"
        );
    }
}
```

and in `message.rs`'s tests, after `control_echoes_messages_say_what_changed`:

```rust
    #[test]
    fn now_playing_buttons_messages_say_what_changed() {
        assert_eq!(
            CrackedMessage::NowPlayingButtonsOn.to_string(),
            "🎛️ The now-playing message now has buttons."
        );
        assert_eq!(
            CrackedMessage::NowPlayingButtonsOff.to_string(),
            "🚫 The now-playing message no longer has buttons. The commands still work."
        );
    }
```

Add `pub mod np_buttons;` and `pub use np_buttons::*;` to `crack-core/src/commands/music/mod.rs` (alphabetical, beside the others).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p crack-core --lib -- status:: np_buttons now_playing_buttons_messages`
Expected: compile errors (`buttons_switched`, `buttons`, `NowPlayingButtonsOn` not found).

- [ ] **Step 3: Strings.** `messages.rs`, after `CONTROL_ECHOES_OFF`:

```rust
pub const NP_BUTTONS_ON: &str = "🎛️ The now-playing message now has buttons.";
pub const NP_BUTTONS_OFF: &str =
    "🚫 The now-playing message no longer has buttons. The commands still work.";
```

`message.rs`: append `NowPlayingButtonsOn,` and `NowPlayingButtonsOff,` at the end of the enum (after `NowPlayingButtonsDisabled`), with `Display` arms writing `NP_BUTTONS_ON` / `NP_BUTTONS_OFF`, and in any exhaustive match put them beside `ControlEchoesOn` / `ControlEchoesOff`.

- [ ] **Step 4: The render gate.** In `show_now_playing_on`, replace `let card = now_playing_status_card(&track, guild).await;` with:

```rust
    let mut card = now_playing_status_card(&track, guild).await;
    // A server can turn the buttons off (`/buttons`).
    if !data.get_now_playing_buttons(guild).await {
        card.controls = None;
    }
```

(`GuildSettingsOperations` is already imported in `status.rs`.)

- [ ] **Step 5: `buttons_switched`**, in `status.rs` after `show_now_playing_on`:

```rust
/// Bring the screen in line with a change to the server's now-playing buttons
/// setting. Off takes the buttons off the status on screen now, leaving its
/// embed; on re-renders the status, buttons and all, when something is playing
/// (`call`). Best effort: a failure is logged.
///
/// 🔑 The same lock order as [`show_now_playing`]: hold no Call lock.
pub(crate) async fn buttons_switched(
    data: &Data,
    transport: &dyn Transport,
    guild: GuildId,
    on: bool,
    call: Option<&Arc<Mutex<Call>>>,
) {
    if on {
        if let Some(call) = call {
            show_now_playing_on(data, transport, guild, call, None).await;
        }
        return;
    }
    let shown = data.status_slot(guild).lock().await.message;
    if let Some(StatusMessage {
        channel,
        id,
        phase: Phase::Playing,
    }) = shown
    {
        if let Err(err) = transport.clear_components(channel, id).await {
            tracing::warn!("taking the buttons off the status in {guild}: {err:?}");
        }
    }
}
```

- [ ] **Step 6: The command** — put this above the test module in `np_buttons.rs`:

```rust
//! `/buttons`: turn the now-playing message's buttons off or on for a server.
use crate::{
    commands::{help, music_utils::connected_call},
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    messaging::{
        courier,
        message::CrackedMessage,
        status::{buttons_switched, DiscordTransport},
    },
    Context, Error,
};

/// Turn the buttons on the now-playing message off or on for this server.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn buttons(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    flag: bool,
) -> Result<(), Error> {
    if flag {
        return help::wrapper(ctx).await;
    }
    buttons_internal(ctx).await
}

/// Flip and save the server's `now_playing_buttons`, say which way it went,
/// then bring the status message in line.
#[cfg(not(tarpaulin_include))]
pub async fn buttons_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let on = data.toggle_now_playing_buttons(guild_id).await?;
    let msg = if on {
        CrackedMessage::NowPlayingButtonsOn
    } else {
        CrackedMessage::NowPlayingButtonsOff
    };
    courier::reply(ctx, msg).await?;
    let call = connected_call(&data.songbird, guild_id, None).await;
    let transport = DiscordTransport::of(ctx.serenity_context());
    buttons_switched(&data, &transport, guild_id, on, call.as_ref()).await;
    Ok(())
}
```

Register it: in `music_commands()` add `buttons(),` in alphabetical position (after `autoplay(),`, before the `ephemeral` comment). If `connected_call`'s visibility (`pub(crate)`) or `DiscordTransport`'s re-export path differs, use the path that compiles (`crate::messaging::transport::DiscordTransport` is the original).

- [ ] **Step 7: Run the tests**

Run: `cargo test -p crack-core --lib -- status:: np_buttons now_playing_buttons_messages commands::`
Expected: all pass. Then the whole crate: `cargo test -p crack-core --lib` — note the total.

- [ ] **Step 8: Sabotage**, one at a time: remove the render gate (`…has_no_buttons_when…` fails); drop `phase: Phase::Playing` from the pattern, matching any phase (`off_with_a_finished…` fails); call `show_now_playing_on` regardless of `on` (`off_takes…` fails with an extra op, or `on_with_nothing…`); never call `clear_components` (`off_takes…` fails); drop `buttons(),` from `music_commands()` (registration test fails). Record each. The command body (`buttons_internal`) is glue no test reaches; say so in your report.

- [ ] **Step 9: fmt, clippy, commit**

```bash
cargo fmt --all
cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-core/src/messaging/status.rs crack-core/src/messaging/messages.rs crack-core/src/messaging/message.rs crack-core/src/commands/music/np_buttons.rs crack-core/src/commands/music/mod.rs
git commit -m "/buttons: turn the now-playing buttons off or on; the status follows at once

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: Release v0.25.0

**Files:**
- Modify: `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`

- [ ] **Step 1: CHANGELOG.** Under `## Unreleased` → `### Added`, directly after the `/echoes` bullet (search `- **\`/echoes\`** (admins)`), add:

```markdown
- **`/buttons`** (admins) turns the now-playing message's buttons off or on for
  the server. On by default. Off takes them off the message on screen at once,
  and a press on an older message is answered privately that they are off.
  This is the switch for a server that restricted the music commands under
  Server Settings → Integrations, which buttons do not see.
- **One button press per person per server every 2 seconds.** A press inside
  that is ignored, so mashing a button cannot flood the channel with echo lines.
```

- [ ] **Step 2: Version.** In the root `Cargo.toml`, `[workspace.package] version = "0.24.3"` → `"0.25.0"`. Run `cargo update -w`. `git diff Cargo.lock` must show only the workspace members' version lines.

- [ ] **Step 3: Full gate**

```bash
cargo fmt --all -- --check
cargo clippy --all --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
cargo test --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests
```

Expected: fmt clean, clippy clean, 0 failed. Report the passed count.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "v0.25.0: /buttons, and a 2-second press debounce on the now-playing buttons

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
