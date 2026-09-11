# Production Database Prerequisites Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `v0.9.1`, which makes it safe to give production a Postgres: stop logging the database password, and stop the shutdown handler overwriting settings that never loaded.

**Architecture:** A `Provenance` tag on `GuildSettings` records whether the value came from Postgres or is a fallback default. Its `Default` is the *safe* value (`Fallback`), so anything not explicitly loaded is never written back. The shutdown loop filters on it. Separately, the config log switches from derived `Debug` to the existing redacting `Display`, and the Dockerfile gains a `migrate` target so migrations can be applied by a container instead of by hand.

**Tech Stack:** Rust (9-member cargo workspace), serenity/poise, sqlx 0.8.2 + Postgres, Docker multi-stage build, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-11-production-database-design.md`

## Global Constraints

- Every commit MUST end with exactly this trailer and **no other** `Co-Authored-By` line: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- `Provenance::Fallback` MUST be the `#[derive(Default)]` variant. The safe direction is "do not write back".
- `Provenance` MUST NOT become a database column and MUST NOT appear in serialized output — it is `#[serde(skip)]`.
- `GuildSettings`'s `PartialEq` MUST ignore `provenance`, so existing equality tests are unperturbed.
- sqlx-cli is pinned `~0.8` to match the `sqlx = "0.8.2"` that writes `_sqlx_migrations`.
- The Dockerfile's existing `runner` stage MUST remain what the bot image builds. Adding a later stage silently changes the default target — both build steps get an explicit `target:`.
- Do NOT fix the 11 `database_pool.as_ref().unwrap()` calls or the `No database pool available` log level. Both are explicitly out of scope.
- Run `cargo fmt --all` before every commit; `cargo clippy --workspace --all-targets` must be clean.

---

### Task 1: Stop logging the database password (#473)

**Files:**
- Modify: `crack-cli/src/main.rs:70`
- Test: `crack-cli/src/main.rs` (new `#[cfg(test)] mod tests` at end of file)
- Test: `crack-core/src/lib.rs` (add to the existing test module, or create one)

**Interfaces:**
- Consumes: `crack_core::BotConfig`'s existing `Display` impl (`crack-core/src/lib.rs:237`) and `crack_core::redact_url` (`:169`).
- Produces: nothing later tasks depend on.

**Background:** `BotConfig` has a hand-written `Display` that redacts `database_url` through `redact_url`. The startup log uses `{:?}` — the *derived* `Debug` — which prints the password verbatim. Observed live on TuneTitan 2026-09-10:

```
WARN cracktunes: Using config: BotConfig { ... database_url: Some("postgres://cracktunes:xYNH...@postgres:5432/cracktunes"), ... }
```

`redact_url` turns `postgres://user:pw@host:5432/db` into `postgres://user:<redacted>@host:5432/db`.

- [ ] **Step 1: Write the failing test for `Display` redaction**

Add to `crack-core/src/lib.rs`, at the end of the file:

```rust
#[cfg(test)]
mod redaction_tests {
    use super::*;

    /// The config is logged at startup. A password must not survive the trip
    /// through `Display`. See #473.
    #[test]
    fn display_redacts_the_database_password() {
        let mut config = BotConfig::default();
        config.database_url =
            Some("postgres://cracktunes:sup3rs3cr3t@postgres:5432/cracktunes".to_string());

        let rendered = format!("{}", config);

        assert!(
            !rendered.contains("sup3rs3cr3t"),
            "Display leaked the password:\n{rendered}"
        );
        assert!(
            rendered.contains("<redacted>"),
            "Display did not redact at all:\n{rendered}"
        );
    }
}
```

- [ ] **Step 2: Run it and confirm it PASSES**

Run: `cargo test -p crack-core redaction_tests -- --nocapture`
Expected: **PASS**. `Display` already redacts — this test pins behaviour that exists, so that the fix in Step 4 has something guarding it. If it FAILS, `Display` is broken too and that is a bigger bug: stop and report.

- [ ] **Step 3: Write the failing test for the call site**

The actual bug is the *formatter used at the log site*, not `Display`. Add to the end of `crack-cli/src/main.rs`:

```rust
#[cfg(test)]
mod log_site_tests {
    /// #473: the startup config log must use `Display` (`{}`), which redacts,
    /// not the derived `Debug` (`{:?}`), which does not. A unit test cannot
    /// observe a `tracing` macro's format string, so this reads the source.
    #[test]
    fn the_config_is_logged_with_display_not_debug() {
        let src = include_str!("main.rs");
        let line = src
            .lines()
            .find(|l| l.contains("Using config:"))
            .expect("the startup config log line vanished; update this test");

        assert!(
            !line.contains("{:?}"),
            "config is logged with derived Debug, which leaks the password (#473): {line}"
        );
        assert!(
            line.contains("{}"),
            "config log line no longer uses Display: {line}"
        );
    }
}
```

- [ ] **Step 4: Run it and confirm it FAILS**

Run: `cargo test -p cracktunes log_site_tests`
Expected: **FAIL** — `config is logged with derived Debug, which leaks the password (#473)`.

- [ ] **Step 5: Apply the one-character fix**

`crack-cli/src/main.rs:70`, change:

```rust
    tracing::warn!("Using config: {:?}", config);
```

to:

```rust
    // 🔑 `{}` not `{:?}`. BotConfig's hand-written Display redacts
    // database_url through redact_url; the derived Debug does not, and this
    // line is what put the Postgres password into `docker logs` (#473).
    tracing::warn!("Using config: {}", config);
```

- [ ] **Step 6: Run both tests and confirm they PASS**

Run: `cargo test -p cracktunes log_site_tests && cargo test -p crack-core redaction_tests`
Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crack-cli/src/main.rs crack-core/src/lib.rs
git commit -m "$(cat <<'EOF'
fix(config): log the config with Display, not Debug, so the password is redacted

BotConfig has a hand-written Display impl that runs database_url through
redact_url. main.rs logged it with {:?} -- the derived Debug -- so the
Postgres password went into docker logs verbatim on every boot.

Production has no DATABASE_URL today and therefore no password to leak;
this lands before it gets one. Guarded by a test that reads the log line
and rejects {:?}, because a tracing macro's format string is not otherwise
observable from a test.

Closes #473.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 2: The `Provenance` type

**Files:**
- Modify: `crack-core/src/guild/settings.rs` (add enum near the `GuildSettings` struct at `:309`; add field to the struct; add field to the struct literal in `GuildSettings::new` at `:451`; add `with_provenance`; hand-write `PartialEq`)
- Test: `crack-core/src/guild/settings.rs` (existing `mod test` at `:1065`)

**Interfaces:**
- Produces, for Task 3:
  - `pub enum Provenance { Fallback, Database }` — `Default`, `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`
  - `GuildSettings.provenance: Provenance` — public field
  - `pub fn GuildSettings::with_provenance(self, provenance: Provenance) -> Self` — consuming builder, matching the existing `with_volume` style
  - `pub fn GuildSettings::is_persistable(&self) -> bool` — `provenance == Provenance::Database`

**Background:** `GuildSettings` currently derives `Deserialize, Serialize, Debug, Clone, PartialEq`. It is the type `save()` serializes, so the new field must not reach the wire or the database.

- [ ] **Step 1: Write the failing tests**

Add to the existing `mod test` in `crack-core/src/guild/settings.rs`:

```rust
    use crate::guild::settings::{GuildSettings, Provenance};

    #[test]
    fn a_fresh_settings_value_is_fallback() {
        // The safe direction: anything not explicitly loaded from Postgres
        // must never be written back over a stored row.
        let settings = GuildSettings::new(GuildId::new(123), None, None);
        assert_eq!(settings.provenance, Provenance::Fallback);
        assert!(!settings.is_persistable());
    }

    #[test]
    fn the_default_provenance_is_the_safe_one() {
        assert_eq!(Provenance::default(), Provenance::Fallback);
    }

    #[test]
    fn with_provenance_marks_it_persistable() {
        let settings = GuildSettings::new(GuildId::new(123), None, None)
            .with_provenance(Provenance::Database);
        assert_eq!(settings.provenance, Provenance::Database);
        assert!(settings.is_persistable());
    }

    #[test]
    fn provenance_does_not_affect_equality() {
        // GuildSettings derives PartialEq and is compared in existing tests.
        // Provenance is bookkeeping, not part of a guild's settings.
        let fallback = GuildSettings::new(GuildId::new(123), None, None);
        let from_db = fallback.clone().with_provenance(Provenance::Database);
        assert_eq!(fallback, from_db);
    }

    #[test]
    fn provenance_is_not_serialized() {
        // It is not a column and must not reach the wire.
        let from_db = GuildSettings::new(GuildId::new(123), None, None)
            .with_provenance(Provenance::Database);
        let json = serde_json::to_string(&from_db).expect("serialize");
        assert!(
            !json.contains("provenance"),
            "provenance leaked into serialized output: {json}"
        );
    }

    #[test]
    fn a_builder_that_copies_self_preserves_provenance() {
        // `with_volume` and friends use `..self`. A mutation must not silently
        // downgrade a Database-loaded value back to Fallback.
        let from_db = GuildSettings::new(GuildId::new(123), None, None)
            .with_provenance(Provenance::Database)
            .with_volume(0.8);
        assert_eq!(from_db.provenance, Provenance::Database);
    }
```

- [ ] **Step 2: Run and confirm they FAIL**

Run: `cargo test -p crack-core --lib guild::settings`
Expected: FAIL to compile — `cannot find type Provenance in this scope`.

- [ ] **Step 3: Add the enum**

Insert immediately **above** `pub struct GuildSettings` (currently `crack-core/src/guild/settings.rs:309`):

```rust
/// Where a guild's in-memory settings came from, and therefore whether writing
/// them back to Postgres is safe.
///
/// 🔑 `Fallback` is the `Default` **deliberately**. The shutdown handler writes
/// every in-memory guild back to the database, and `on_guild_create` falls back
/// to `GuildSettings::new()` defaults when the load fails. Without this tag a
/// transient read failure at boot becomes a permanent write at shutdown --
/// defaults silently replacing a guild's stored settings, with no backup to
/// restore from. Defaulting to `Fallback` means anything we did not explicitly
/// load from Postgres is never written back.
///
/// 🪤 **Mutations must not reset this.** `set_volume` and friends use
/// `and_modify` / `..self`, which leave the tag alone -- the behaviour we want.
/// A future mutation site that *replaces* the map entry with a fresh
/// `GuildSettings::new()` would silently re-arm the bug.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Defaults, built in memory. NOT safe to write back.
    #[default]
    Fallback,
    /// Round-tripped through Postgres, so the row exists and a write updates
    /// a row we actually read. Safe to write back.
    Database,
}
```

- [ ] **Step 4: Add the field to the struct**

In `pub struct GuildSettings`, after `pub additional_prefixes: Vec<String>,`, add:

```rust
    /// Not a column and not on the wire -- see [`Provenance`].
    #[serde(skip)]
    pub provenance: Provenance,
```

- [ ] **Step 5: Add the field to the struct literal in `new`**

`GuildSettings::new` (currently `:451`) builds a full struct literal. After `additional_prefixes: Vec::new(),` add:

```rust
            provenance: Provenance::Fallback,
```

Note: `guild/operations.rs:335` also constructs `GuildSettings { .. }` but ends with `..Default::default()`, so it needs no change — it inherits `Fallback`, which is correct.

- [ ] **Step 6: Replace the derived `PartialEq` with a hand-written one**

Change the derive on `GuildSettings` from:

```rust
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
```

to:

```rust
#[derive(Deserialize, Serialize, Debug, Clone)]
```

and add, immediately after the struct's closing brace:

```rust
/// 🔑 Hand-written to exclude `provenance`, which is bookkeeping about where a
/// value came from rather than part of a guild's settings. Two guilds with
/// identical settings are equal regardless of how they were loaded, and the
/// existing equality tests should not care.
impl PartialEq for GuildSettings {
    fn eq(&self, other: &Self) -> bool {
        self.guild_id == other.guild_id
            && self.guild_name == other.guild_name
            && self.prefix == other.prefix
            && self.premium == other.premium
            && self.command_settings == other.command_settings
            && self.autopause == other.autopause
            && self.autoplay == other.autoplay
            && self.reply_with_embed == other.reply_with_embed
            && self.allow_all_domains == other.allow_all_domains
            && self.allowed_domains == other.allowed_domains
            && self.banned_domains == other.banned_domains
            && self.authorized_users == other.authorized_users
            && self.authorized_groups == other.authorized_groups
            && self.ignored_channels == other.ignored_channels
            && self.old_volume == other.old_volume
            && self.volume == other.volume
            && self.self_deafen == other.self_deafen
            && self.timeout == other.timeout
            && self.welcome_settings == other.welcome_settings
            && self.log_settings == other.log_settings
            && self.additional_prefixes == other.additional_prefixes
    }
}
```

⚠️ If the struct has gained or lost a field since this plan was written, match the *current* field list — every field except `provenance`.

- [ ] **Step 7: Add the two methods**

Inside `impl GuildSettings` (starts at `:433`), next to `with_volume`:

```rust
    /// Record where these settings came from. See [`Provenance`].
    pub fn with_provenance(self, provenance: Provenance) -> Self {
        Self { provenance, ..self }
    }

    /// Whether the shutdown handler may write these settings back to Postgres.
    ///
    /// Only settings that round-tripped through the database may be written:
    /// writing a fallback default over a stored row destroys it.
    pub fn is_persistable(&self) -> bool {
        self.provenance == Provenance::Database
    }
```

- [ ] **Step 8: Run the tests and confirm they PASS**

Run: `cargo test -p crack-core --lib guild::settings`
Expected: all PASS, including the pre-existing `test_default`.

- [ ] **Step 9: Confirm nothing else broke**

Run: `SQLX_OFFLINE=true cargo check -p crack-core --all-targets`
Expected: clean. If a struct literal elsewhere now fails to compile, add `provenance: Provenance::Fallback` to it — `Fallback` is always the correct default for a value not loaded from the database.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
git add crack-core/src/guild/settings.rs
git commit -m "$(cat <<'EOF'
feat(settings): record whether settings came from Postgres or are defaults

The shutdown handler writes every in-memory guild back to the database, and
on_guild_create falls back to GuildSettings::new() defaults when the load
fails. A transient read failure at boot therefore becomes a permanent write
at shutdown, replacing a guild's stored settings with defaults.

Provenance tags each value with where it came from. Fallback is the Default
deliberately: anything not explicitly loaded from Postgres is never written
back, so a mistag costs a redundant write rather than a lost row.

Not a column and not on the wire (serde(skip)), and PartialEq is hand-written
to ignore it so existing equality tests are unperturbed.

No behaviour change yet -- nothing reads the tag until the next commit.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 3: Set the tag on load, honour it at shutdown

**Files:**
- Modify: `crack-core/src/handlers/serenity.rs` (the `Ok` arm of `on_guild_create`, around `:356`)
- Modify: `crack-core/src/config.rs` (the shutdown loop, around `:410-428`)
- Test: `crack-core/src/handlers/serenity.rs` (new `#[cfg(test)] mod provenance_wiring_tests`)

**Interfaces:**
- Consumes from Task 2: `Provenance::{Fallback, Database}`, `GuildSettings::with_provenance`, `GuildSettings::is_persistable`.
- Produces: nothing later tasks depend on.

**Background:** `on_guild_create` has exactly three outcomes. Only one is safe to write back:

| arm | value | tag |
|---|---|---|
| pool present, `get_or_create` → `Ok` | the stored row | `Database` |
| pool present, `get_or_create` → `Err` | `GuildSettings::new()` | `Fallback` (leave alone) |
| no pool | `GuildSettings::new()` | `Fallback` (leave alone) |

`GuildEntity::get_or_create` is an upsert (`INSERT … ON CONFLICT DO UPDATE … RETURNING *`), so `Ok` means the row exists in Postgres — including for a guild the bot just joined. That is exactly the condition that makes a write-back safe.

- [ ] **Step 1: Write the failing wiring test**

Add at the end of `crack-core/src/handlers/serenity.rs`:

```rust
#[cfg(test)]
mod provenance_wiring_tests {
    /// `on_guild_create` is an async serenity event handler taking a live
    /// `Context`; it cannot be called from a unit test. The property that
    /// matters is which of its three arms marks settings persistable, so this
    /// reads the source.
    ///
    /// If this test becomes annoying, the fix is to extract the arm selection
    /// into a pure function and test that -- not to delete the test.
    #[test]
    fn only_the_database_arm_marks_settings_persistable() {
        let src = include_str!("serenity.rs");

        let marks: Vec<&str> = src
            .lines()
            .filter(|l| l.contains("with_provenance"))
            .map(|l| l.trim())
            .collect();

        assert_eq!(
            marks.len(),
            1,
            "expected exactly one with_provenance call in this file, found {}: {:#?}",
            marks.len(),
            marks
        );
        assert!(
            marks[0].contains("Provenance::Database"),
            "the only with_provenance call should mark Database: {}",
            marks[0]
        );
        assert!(
            marks[0].contains("Ok(("),
            "with_provenance must be on the Ok arm of get_or_create, so a failed \
             load keeps the Fallback default: {}",
            marks[0]
        );
    }
}
```

- [ ] **Step 2: Run and confirm it FAILS**

Run: `cargo test -p crack-core --lib provenance_wiring_tests`
Expected: FAIL — `expected exactly one with_provenance call in this file, found 0`.

- [ ] **Step 3: Mark the `Ok` arm**

In `on_guild_create`, change:

```rust
                Ok((_guild, settings)) => settings,
```

to:

```rust
                // 🔑 The ONLY place settings become persistable. get_or_create
                // is an upsert returning the live row, so Ok means Postgres and
                // memory agree the row exists -- including for a guild just
                // joined. The Err and no-pool arms below deliberately keep the
                // Fallback default, so a transient read failure cannot become a
                // permanent write at shutdown.
                Ok((_guild, settings)) => settings.with_provenance(Provenance::Database),
```

Add the import at the top of the file:

```rust
use crate::guild::settings::Provenance;
```

(Adjust to match the file's existing import style.)

- [ ] **Step 4: Run and confirm it PASSES**

Run: `cargo test -p crack-core --lib provenance_wiring_tests`
Expected: PASS.

- [ ] **Step 5: Make the shutdown loop honour the tag**

In `crack-core/src/config.rs`, the shutdown block currently reads:

```rust
        println!("Saving guilds...");
        if let Some(p) = pool {
            for (k, v) in guilds {
                //tracing::warn!("Saving Guild: {}", k);
                match v.save(&p).await {
                    Ok(_) => {
                        saved_guilds.push(k);
                    },
                    Err(e) => {
                        tracing::error!("Error saving guild settings: {}", e);
                    },
                }
            }
            p.close().await;
        }
```

Replace with:

```rust
        println!("Saving guilds...");
        if let Some(p) = pool {
            let mut skipped = 0usize;
            for (k, v) in guilds {
                // 🔑 Only write back settings that came FROM the database. A
                // guild whose load failed is holding defaults, and writing
                // those over its stored row destroys it -- permanently, and
                // with no backup behind it. See guild::settings::Provenance.
                if !v.is_persistable() {
                    skipped += 1;
                    continue;
                }
                match v.save(&p).await {
                    Ok(_) => {
                        saved_guilds.push(k);
                    },
                    Err(e) => {
                        tracing::error!("Error saving guild settings: {}", e);
                    },
                }
            }
            if skipped > 0 {
                // Loud on purpose: a large skip count means many guilds failed
                // to load at boot, which is worth knowing about.
                tracing::warn!(
                    "Skipped {} guild(s) at shutdown whose settings never loaded from the database",
                    skipped
                );
            }
            p.close().await;
        }
```

`GuildSettings::save` is `pub async fn save(&self, pool: &PgPool)` (`crack-core/src/guild/settings.rs:506`) — one argument, verified. This step only adds the guard and the counter; the `save` call is unchanged.

- [ ] **Step 6: Verify the whole crate still builds and tests pass**

Run: `SQLX_OFFLINE=true cargo check -p crack-core --all-targets && cargo test -p crack-core --lib`
Expected: clean, all PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crack-core/src/handlers/serenity.rs crack-core/src/config.rs
git commit -m "$(cat <<'EOF'
fix(settings): never write fallback defaults over a guild's stored settings

on_guild_create marks settings Database only on the Ok arm of get_or_create,
which is an upsert returning the live row -- so Ok means Postgres and memory
agree the row exists. The Err and no-pool arms keep the Fallback default.

The shutdown handler now skips guilds whose settings are not persistable and
logs how many it skipped, so a boot where many guilds failed to load is
visible rather than silent.

Without this a transient read failure at boot became a permanent write at
shutdown: defaults replacing a guild's stored settings, unrecoverably, since
production has no backups yet.

The wiring is guarded by a source-reading test because on_guild_create takes
a live serenity Context and cannot be called from a unit test.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 4: A `migrate` image, so migrations are never applied by hand

**Files:**
- Modify: `Dockerfile`
- Modify: `.github/workflows/docker.yml`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: the image `ghcr.io/cycle-five/cracktunes-migrate:<tag>`, which homelab's `bots/docker-compose.yml` will run as a one-shot before the bot starts. Its entrypoint runs `sqlx migrate run --source /migrations`; it needs `DATABASE_URL` in the environment and nothing else.

**Background:** the bot does not run migrations — every `sqlx::migrate!` in the tree is inside a test module pointed at `./test_migrations`. All 22 production migrations are applied out of band today. A manual step that must not be forgotten is the same shape as the skipped Docker job that let ct#451's broken image reach master.

⚠️ **The stage-ordering trap.** The Dockerfile's last stage is the default build target. `runner` is last today. Adding `migrate` after it would silently make the *migrate* image the bot image. Both build steps therefore get an explicit `target:`.

- [ ] **Step 1: Install sqlx-cli in the builder, before the source copy**

In `Dockerfile`, the builder stage currently reads:

```dockerfile
RUN apk add --no-cache \
  build-base \
  musl-dev \
  cmake \
  git

# Default directory
WORKDIR /app
```

Insert the install **between** the `apk add` and the `WORKDIR`:

```dockerfile
# 🔑 BEFORE `COPY . .`, deliberately. sqlx-cli takes minutes to build and never
# changes with our source, so keeping it above the copy means the layer caches
# across every source change and only rebuilds when this base image moves.
#
# Pinned to ~0.8 to match the sqlx 0.8.2 that writes the _sqlx_migrations
# ledger; a mismatched CLI can disagree with the library about that table.
RUN cargo install sqlx-cli --version '~0.8' --no-default-features --features rustls,postgres
```

- [ ] **Step 2: Add the migrate stage**

Append to the **end** of `Dockerfile`:

```dockerfile
# STAGE3: the migration runner, used as a one-shot before the bot starts.
#
# 🪤 This stage is LAST, which makes it the default build target. The bot image
# must therefore be built with an explicit `--target runner`; the Docker
# workflow does this. Building with no target gets you this image, which is
# emphatically not the bot.
FROM alpine:3.22 AS migrate
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
COPY --from=builder /app/migrations /migrations
# Needs DATABASE_URL in the environment and nothing else.
ENTRYPOINT ["sqlx", "migrate", "run", "--source", "/migrations"]
```

- [ ] **Step 3: Pin the bot image's target explicitly**

In `.github/workflows/docker.yml`, the existing build step is:

```yaml
      - 
        name: Build & Push Docker Image
        uses: docker/build-push-action@v5
        with:
          context: .
          push: ${{ github.event_name != 'pull_request' }}
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
```

Add `target: runner`:

```yaml
      - 
        name: Build & Push Docker Image
        uses: docker/build-push-action@v5
        with:
          context: .
          # 🪤 EXPLICIT. The Dockerfile's last stage is `migrate`, and an
          # unspecified target builds the last stage -- which would publish the
          # migration runner as the bot.
          target: runner
          push: ${{ github.event_name != 'pull_request' }}
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
```

- [ ] **Step 4: Publish the migrate image alongside**

Append two steps to the same job, after the existing build step:

```yaml
      -
        name: Extract Git Metadata (migrate image)
        id: meta-migrate
        uses: docker/metadata-action@v5
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}-migrate
      -
        name: Build & Push Migration Image
        uses: docker/build-push-action@v5
        with:
          context: .
          target: migrate
          push: ${{ github.event_name != 'pull_request' }}
          tags: ${{ steps.meta-migrate.outputs.tags }}
          labels: ${{ steps.meta-migrate.outputs.labels }}
```

This publishes `ghcr.io/cycle-five/cracktunes-migrate` with the same `v`-prefixed tags the bot image gets, because it uses the same `metadata-action` defaults.

- [ ] **Step 5: Validate the workflow YAML parses**

Run: `python3 -c "import yaml,sys; d=yaml.safe_load(open('.github/workflows/docker.yml')); steps=d['jobs']['push']['steps']; names=[s.get('name') for s in steps]; print(names); assert 'Build & Push Migration Image' in names"`
Expected: prints the step names including the new one, no assertion error.

- [ ] **Step 6: Verify both targets resolve**

Do **not** build the images here — container DNS is broken on this workstation and image builds happen on the staging host. Confirm the stage names exist and are distinct:

Run: `grep -n '^FROM' Dockerfile`
Expected: exactly three lines, ending with `FROM alpine:3.22 AS migrate`. Confirm `runner` and `migrate` are both present and are different stages.

- [ ] **Step 7: Commit**

```bash
git add Dockerfile .github/workflows/docker.yml
git commit -m "$(cat <<'EOF'
build(docker): publish a migration-runner image

The bot does not run migrations -- every sqlx::migrate! in the tree is in a
test module -- so all 22 are applied out of band. Production is about to get
a database, and a manual step that must not be forgotten is the same shape
as the skipped Docker job that let #451's broken image reach master.

Adds a `migrate` stage carrying sqlx-cli and the migrations, published as
cracktunes-migrate with the same tags as the bot image. homelab runs it as a
one-shot with service_completed_successfully before the bot starts.

sqlx-cli is installed above `COPY . .` so the layer caches across source
changes, and pinned to ~0.8 to match the sqlx that writes _sqlx_migrations.

🪤 The new stage is last, which makes it the default build target, so the bot
image's build step now names `target: runner` explicitly. Without that, an
unspecified target publishes the migration runner as the bot.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 5: Version bump to 0.9.1

**Files:**
- Modify: `crack-bf/Cargo.toml`, `crack-cli/Cargo.toml`, `crack-core/Cargo.toml`, `crack-gpt/Cargo.toml`, `crack-osint/Cargo.toml`, `crack-sleevenote/Cargo.toml`, `crack-testing/Cargo.toml`, `crack-types/Cargo.toml`, `crack-voting/Cargo.toml`
- Modify: `Cargo.lock` (by running cargo, not by hand)

**Interfaces:** none.

**Background:** there is no `[workspace.package] version`; all nine members carry a literal `version`, so a partial bump compiles clean and ships a lie. That is issue #424(b).

- [ ] **Step 1: Confirm the current version is 0.9.0 everywhere**

Run: `grep -m1 '^version = ' */Cargo.toml`
Expected: nine lines, every one `version = "0.9.0"`. If any differs, stop and report — a partial bump already happened.

- [ ] **Step 2: Bump all nine**

```bash
sed -i '0,/^version = "0.9.0"/s//version = "0.9.1"/' \
  crack-bf/Cargo.toml crack-cli/Cargo.toml crack-core/Cargo.toml \
  crack-gpt/Cargo.toml crack-osint/Cargo.toml crack-sleevenote/Cargo.toml \
  crack-testing/Cargo.toml crack-types/Cargo.toml crack-voting/Cargo.toml
```

- [ ] **Step 3: Verify all nine moved**

Run: `grep -m1 '^version = ' */Cargo.toml`
Expected: nine lines, every one `version = "0.9.1"`. This is the check #424(b) exists because of — count them.

- [ ] **Step 4: Update the lockfile**

Run: `cargo metadata --format-version 1 > /dev/null && grep -A1 '^name = "cracktunes"' Cargo.lock`
Expected: `version = "0.9.1"`.

- [ ] **Step 5: Confirm the lock is coherent**

Run: `cargo metadata --locked --format-version 1 > /dev/null && echo OK`
Expected: `OK`. A failure here means the lockfile disagrees with the manifests and CI would reject it.

- [ ] **Step 6: Commit**

```bash
git add */Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
chore: bump to 0.9.1

Patch release: the two prerequisites for giving production a database --
redacting the config log (#473) and refusing to write fallback defaults over
stored guild settings -- plus the migration-runner image.

All nine members bumped together; there is no [workspace.package] version,
so a partial bump compiles clean and ships a lie (#424).

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

## Final verification (run before finishing the branch)

- [ ] `cargo fmt --all --check` — clean
- [ ] `cargo clippy --workspace --all-targets` — clean
- [ ] `SQLX_OFFLINE=true cargo check -p crack-core --all-targets` — clean
- [ ] `cargo test --workspace` — all pass except the known-unrelated `crack-testing::tests::test_enqueue_query`, which fails on master too (a deleted YouTube video id; fixed in open PR #471)
- [ ] `grep -m1 '^version = ' */Cargo.toml` — nine lines, all `0.9.1`
- [ ] `grep -c '^FROM' Dockerfile` — `3`
