# Postgres in production — design

**Status:** approved 2026-09-11
**Scope:** cracktunes `v0.9.1` (code) + homelab `bots/` (infrastructure)
**Closes:** #473. Prerequisite for giving production a database at all.

## Goal

Production (the `bots` stack, VM 108, 139 guilds) runs with no `DATABASE_URL`
today, deliberately. This turns that on: guild settings, play history, playlists
and `/gp` persistence start working for the guilds that actually use the bot.

The beta tenant (TuneTitan) has had a Postgres since homelab #58, so the pattern
exists. Production is not a copy of it, because production has data worth
keeping and TuneTitan explicitly does not.

## Constraints that shaped this

**Durability: real data, backups deferred.** Production's rows are worth
keeping, but the backup job is a follow-up rather than a blocker. That decision
is what makes the corruption path below a must-fix rather than a nice-to-have:
without backups, anything silently destroyed is destroyed for good.

**Sequencing: code first, then infrastructure.** `v0.9.1` ships both code fixes
and reaches production *before* any `DATABASE_URL` exists. Production never runs
for a moment with a database and a known unrecoverable-overwrite path.

**Placement: same compose project.** Postgres joins `bots/`, like TuneTitan's.
Service-name resolution, no published port, no firewall in the path. The stack's
own comments already argue for co-location; nothing here overturns that.

---

## Part 1 — cracktunes `v0.9.1`

### 1.1 #473 — the password is logged in cleartext

`BotConfig` has a hand-written `Display` impl that redacts the URL through
`redact_url` (`crack-core/src/lib.rs:237`, redacting at `:266`). `crack-cli/src/main.rs:70` logs the
config with `{:?}` — the **derived `Debug`**, which does not redact:

```
WARN cracktunes: Using config: BotConfig { ... database_url: Some("postgres://cracktunes:<PASSWORD>@postgres:5432/cracktunes"), ... }
```

**Fix:** `{:?}` → `{}` at that call site.

🔑 **This is a prerequisite, not a follow-up.** Production has no
`DATABASE_URL` today, so it has no password to leak. Adding one is precisely
what creates the exposure. Observed live on TuneTitan on 2026-09-10.

**Guard:** a test asserting the rendered config contains no `@`-delimited
password segment, so a future edit cannot silently fall back to `Debug`.

⚠️ **TuneTitan's existing password still needs rotating.** This stops new
leaks; it cannot un-log what is already in that host's `docker logs`. Out of
scope here, tracked separately.

### 1.2 The shutdown write can overwrite settings that never loaded

**The mechanism.** `config.rs:410` writes every guild in the in-memory
`guild_settings_map` back to Postgres on `SIGTERM`. `on_guild_create`
(`handlers/serenity.rs:347`) populates that map, with three outcomes:

| arm | result | today |
|---|---|---|
| pool present, `get_or_create` `Ok` | the stored row | safe to write back |
| pool present, `get_or_create` `Err` | `GuildSettings::new()` defaults | **unsafe** |
| no pool | `GuildSettings::new()` defaults | **unsafe** |

A **transient read failure at boot therefore becomes a permanent write at
shutdown.** One slow or wedged Postgres, and that guild's stored settings are
silently replaced by defaults. Harmless today (nothing to lose); unrecoverable
once there is data and no backups.

🔑 **Half of this was already fixed.** Settings used to load in
`on_cache_ready`, which never fires at ~150 guilds — the code carries a long
comment explaining the move to the per-guild event. "Settings never load" is
fixed. The defaults fallback is what survived.

**The fix: a provenance tag.**

```rust
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum Provenance {
    #[default]
    Fallback,   // defaults -- NOT safe to write back
    Database,   // round-tripped through Postgres -- safe
}
```

`Fallback` is the `Default` **deliberately**: anything not explicitly loaded
from Postgres is never written back. `GuildSettings::new()` gets the safe value
for free, and so does anything deserialized.

Set in exactly one place — the `Ok` arm of `on_guild_create`:

```rust
Ok((_guild, settings)) => settings.with_provenance(Provenance::Database),
```

The `Err` and no-pool arms leave the default untouched.

The shutdown loop skips `Fallback` entries and logs how many it skipped, so a
skip storm is visible rather than silent.

🔑 **`Database` means "Postgres and memory agree the row exists."**
`GuildEntity::get_or_create` is an upsert
(`INSERT … ON CONFLICT (guild_id) DO UPDATE SET guild_name = $2 RETURNING *`),
so both branches return the live row — an existing guild only has its name
refreshed. A **brand-new** guild is therefore `Database` too, correctly: the
`INSERT` created the row, so writing back updates a row we read.

🪤 **Mutations must not reset provenance.** `set_volume` and friends use
`and_modify`, which mutates in place and leaves the tag alone — the behaviour we
want. A future mutation site that *replaces* the entry would silently re-arm the
bug. This goes in a doc comment on the enum.

🪤 **`set_volume` has a second route into the same hole.** It ends with
`or_insert(GuildSettings::new(...))`, lazily creating a defaults entry for a
guild not in the map. Under this change that entry is `Fallback` and cannot
overwrite a stored row.

### 1.3 The shutdown write must NOT simply be deleted

Recorded because it is the obvious "simplification" and it is wrong.

Of the 8 commands under `commands/settings/set/` (excluding `mod.rs`), 4 call
`save()` and 4 do not. These mutate the in-memory map and **never** touch
Postgres:

- `set_volume.rs`
- `set_idle_timeout.rs`
- `set_all_log_channel.rs`
- `set_premium.rs`

`commands/settings/prefix.rs` — a directory up, not under `set/` — is a fifth,
and also mutates the map without saving.

For those five, the shutdown write is the **only** persistence path. Deleting it
trades a rare data-loss bug for a guaranteed one. Verified by reading
`set_volume`, which does `and_modify`/`or_insert` and no `save()`.

Dirty-tracking (write only mutated guilds) is the better long-term model and
was considered. It was rejected **for this change** because it inverts the
default to "don't write", so a missed mutation site silently breaks persistence
forever — and five sites already mutate without saving. Provenance fails safe in
the other direction: a mistag costs a redundant write, never a lost row.

### 1.4 Cost accepted

`GuildSettings` derives `PartialEq`. A provenance field would make two otherwise
identical settings compare unequal, perturbing existing tests. `PartialEq` is
hand-written to exclude it.

### 1.5 Explicitly out of scope

- **The 11 `database_pool.as_ref().unwrap()` calls.** They panic when the pool
  is `None`, which is production's state *today* — so `/playlist create`,
  `/delete_playlist`, `set_music_channel` and `set_auto_role` panic in
  production right now. Adding a pool makes them succeed. They still deserve
  real errors, but fixing them here is scope creep on a release that gates
  infrastructure. Own issue.
- **`No database pool available` logging at ERROR on nearly every gateway
  event.** Moot once production has a pool. Own issue.

---

## Part 2 — infrastructure

### 2.1 The migrate image

The bot does not run migrations: every `sqlx::migrate!` in the tree is inside a
test module, pointed at `./test_migrations`. All 22 production migrations are
applied out of band. A manual step that must not be forgotten is the same shape
as the skipped Docker job that let ct#451's broken image reach master, so this
automates it.

The runtime image has no sqlx, so the one-shot needs its own. New target in the
existing Dockerfile, reusing the builder already present:

```dockerfile
FROM rust:1.98.0-alpine3.22 AS builder
RUN apk add --no-cache build-base musl-dev cmake git
# 🔑 BEFORE `COPY . .` deliberately: sqlx-cli takes minutes to build and never
# changes with our source, so this layer caches across every source change.
# Pinned to ~0.8 to match the sqlx 0.8.2 that writes _sqlx_migrations.
RUN cargo install sqlx-cli --version '~0.8' --no-default-features --features rustls,postgres
WORKDIR /app
COPY . .
RUN cargo build -p cracktunes --profile=dist

FROM alpine:3.22 AS migrate
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
COPY --from=builder /app/migrations /migrations
ENTRYPOINT ["sqlx", "migrate", "run", "--source", "/migrations"]
```

Published as `ghcr.io/cycle-five/cracktunes-migrate:<tag>` by the existing
Docker workflow, with the same `v`-prefixed tag as the bot image.

**Cost, stated:** a second published image and a CI change. Layer ordering keeps
the sqlx-cli build off the hot path, but the first build after any base-image
bump is slow.

### 2.2 Compose

```yaml
  postgres:
    image: postgres:16-alpine
    # Named and DURABLE. Unlike tunetitan's, this volume is NOT test data.
    volumes: [bots-pgdata:/var/lib/postgresql/data]
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U cracktunes -d cracktunes"]
      interval: 5s
      retries: 20

  migrate:
    image: ghcr.io/cycle-five/cracktunes-migrate:v0.9.1
    environment:
      DATABASE_URL: postgres://cracktunes:${POSTGRES_PASSWORD:?}@postgres:5432/cracktunes
    depends_on:
      postgres: {condition: service_healthy}
    restart: "no"

  cracktunes:
    environment:
      DATABASE_URL: postgres://cracktunes:${POSTGRES_PASSWORD:?}@postgres:5432/cracktunes
    depends_on:
      postgres: {condition: service_healthy}
      migrate:  {condition: service_completed_successfully}
```

🪤 **`service_healthy`, not `service_started`.** Postgres' entrypoint starts the
cluster, initialises it, then **restarts** it, so the port is briefly open to
nothing. The bot builds its pool once at startup and does not retry, so racing
that window is a bot with no database that looks configured. Carried over from
TuneTitan, where it was found the hard way.

🔑 **`service_completed_successfully` on `migrate`.** The ledger must be current
before the first query.

**No published port**, unlike TuneTitan — automating migrations removes the only
reason that loopback tunnel existed.

### 2.3 Secrets

`POSTGRES_PASSWORD` joins the gitignored `.env.bots`, passed via `--env-file`
from the workstation, so it never lands on the host — the same treatment the
Discord token gets. Source of truth in Vaultwarden.

This is a **new** password; nothing here needs rotating.

### 2.4 verify.sh

```
applied=$(psql -tAc "SELECT count(*) FROM _sqlx_migrations WHERE success")
ondisk=$(ls migrations/*.sql | wc -l)     # 22 at time of writing
[ "$applied" = "$ondisk" ] || FAIL
```

Catches a forgotten migration at deploy time rather than at first query. Plus:
Postgres healthy, and the **absence** of `No database pool available` in the
startup log — the bot does not crash when the pool fails to build, so its
silence is the only signal that it worked.

🪤 Assertions read the **head** of the log, and use bash `case` rather than
`grep -q`, for the two reasons documented in `tunetitan/verify.sh`.

### 2.5 Cutover and rollback

**Cutover is a clean slate.** Production has never had a Postgres, so there is
no data to migrate. First boot creates 139 rows at defaults via the upsert —
which is exactly the effective state today, since settings currently live only
in memory and are discarded on every restart.

**Rollback is cheap.** Remove `DATABASE_URL` and redeploy. The bot returns to
today's behaviour, the volume keeps its rows, nothing is destroyed.

### 2.6 Deferred: backups

From this landing until a backup job exists, production has data worth losing
and no way to restore it. The provenance fix removes the *silent* corruption
path; what remains is blunt loss — volume destroyed, host lost.

Filed as its own issue with a concrete shape (`pg_dump` on a timer to somewhere
off VM 108) so it does not drift. The window should be days, not weeks.

---

## Success criteria

1. `v0.9.1` reaches production with no `DATABASE_URL` and the config log shows
   no password.
2. A guild whose settings fail to load is skipped by the shutdown write, and the
   skip is logged.
3. The five save-less settings commands still persist across a restart.
4. `bots` comes up with Postgres healthy, all 22 migrations applied, and no
   `No database pool available` in the log.
5. `verify bots` fails if a migration is missing.
6. Removing `DATABASE_URL` returns the bot to its current behaviour.
