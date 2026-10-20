# Economy Core (PR 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** per-server credits. Members earn them in voice and by chatting, see them, gift them,
bet them on a coinflip, and spend them on immunity, the shop's first item. Mods grant and lift
immunity and adjust balances, and the mod log records both. Admins configure all of it on the
dashboard. Ships as **v0.32.0**.

**Architecture:** a new `crack-core/src/economy/` module.
- **Pure core:** settings and validation, immunity, earning eligibility, purchase checks, the
  coinflip.
- **Database:** all SQL in `crack-core/src/db/economy.rs`. Every credit movement is one
  transaction that locks the wallet row. Glue reads the cache and the database into plain
  structs and calls the pure core.
- **Memory:** settings, the opted-out sets, and pending message credits live in an
  `EconomyRegistry` on `Data`, loaded at `GuildCreate` like `room_settings` and
  `mod_log_settings`.
- **Earning:** one 60 s task, started once from `Ready`.
- **Mod log:** grants, lifts and adjustments are posted as new `ModLogEvent` variants through
  the existing `ChannelSink`.

**Tech Stack:** Rust 2021, serenity `next`, poise, sqlx 0.9/Postgres, `rand` 0.10 (`rand::rng()`,
`RngExt`), chrono, dashmap, axum (crack-web).

**Spec:** `docs/superpowers/specs/2026-10-20-economy-design.md`. This plan is PR 1 of its
section 12: sections 1, 2, 3 (the machinery, with immunity as its only item), 6 (with nothing yet
to end on opt-out or grant: pranks arrive in PR 2), 9, the PR 1 parts of 5.1 and 5.4, and 13.
**Read the spec before any task.**

## Global Constraints

- **Off by default.**
  - Enabling needs a database (refused otherwise, as for rooms).
  - Free on every server.
  - Every economy command in a server with the economy off replies privately
    `ECONOMY_OFF` and does nothing.
- **Defaults (spec section 1), verbatim:**
  - `currency_name` "credits", `currency_emoji` "🪙";
  - `voice_credits_per_minute` 1, `message_credits` 1, `message_cooldown_seconds` 60;
  - immunity price 200, on;
  - `immunity_optout` / `immunity_shop` / `immunity_mods` all true;
  - `optout_cooldown_days` 7, `bought_immunity_hours` 24;
  - `gambling_enabled` false, `max_bet` 500.
- **Validation bounds (my calls, enforce exactly):**
  - currency name: 1..=24 chars after trim, no control chars;
  - currency emoji: 1..=64 chars after trim, no control chars, no whitespace;
  - `voice_credits_per_minute` and `message_credits`: 0..=100;
  - `message_cooldown_seconds`: 10..=3600;
  - every price: 1..=1_000_000;
  - `optout_cooldown_days`: 0..=90;
  - `bought_immunity_hours`: 1..=720;
  - `max_bet`: 1..=1_000_000.
- **Money rules:**
  - A balance is never negative. A DB `CHECK (balance >= 0)` backs every debit, which runs
    `SELECT … FOR UPDATE` on the wallet row in the same transaction as its ledger row.
  - A wallet that doesn't exist reads as balance 0. Credits create it with an upsert.
  - Earning writes the wallet's counters, never ledger rows.
- **Opted-out members:**
  - they don't earn, buy, gamble, gift or receive gifts;
  - their balance is kept;
  - switching either way is refused within `optout_cooldown_days` of the last switch.
- **Immunity:**
  - `is_protected` reads stored grants and the opt-out flag, never the three switches;
  - the switches only gate *creating* (opt-out, buying, mod grant);
  - buying extends from whichever is later, now or the current end;
  - `/economy lift` removes only the mod grant.
- **The coinflip:** 50/50, no house edge. A win pays the stake back plus the same again (ledger:
  `coinflip_stake −bet`, `coinflip_win +2·bet`). A loss takes the stake.
- **Repo rules (as every recent plan):**
  - strings go in `pub const` in `crack-core/src/messaging/messages.rs`;
  - sends go through `messaging::courier`/`Transport` only;
  - typed serde (never `serde_json::json!`/`Value` for owned data);
  - ids are parsed defensively (reject 0 and `u64::MAX`);
  - migrations go in BOTH `migrations/` and `crack-core/test_migrations/`;
  - `.sqlx` is regenerated with `scripts/sqlx_sync.py`, following its header recipe. First
    migrate a fresh local DB with `sqlx migrate run --source migrations/`. Never use
    `cargo sqlx prepare`;
  - never `git add -A`;
  - the commit trailer is exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`;
  - sabotage every new test: record a mutation → caught-by row in the report;
  - assert on what is sent;
  - no source-scan tests;
  - no stray background processes.
- **Local postgres** (db-tests; a ctor forces
  `DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres`):
  - start it with `docker --context default run -d --rm --name pg -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine`;
  - 🪤 bare `docker` targets a remote host, so always pass `--context default`.
- **Gate per task**, from the worktree, with `CARGO_TARGET_DIR=/home/lothrop/projects/cracktunes/target`:
  - `cargo fmt --all -- --check`;
  - `cargo clippy --all --locked -- -D clippy::all -D warnings --allow clippy::needless_return`;
  - `cargo test --workspace --locked`;
  - for any DB change: `cargo test --workspace --locked --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests`.
- **Shell:** the Bash tool runs zsh. No `for f in $files` word-splitting, and no `echo ====`.
  Use `bash <<'EOF'` with `set -euo pipefail` for multi-step scripts.

## Review Focus

1. **Two debits racing on one wallet** (a gift and a coinflip pressed together) must never
   overdraw: one succeeds and the other is refused. Pinned in Task 2 by
   `concurrent_debits_never_overdraw`.
2. **A stored config written by a later or earlier release** (fields missing, a price key this
   build doesn't know) must load, and saving it must keep the unknown price. Pinned in Task 1 by
   `config_with_missing_fields_and_unknown_price_loads_and_keeps_it`.
3. **A server whose settings aren't loaded yet, or failed to load**, must earn nothing and refuse
   every save, never writing defaults over stored settings. Pinned in Task 1
   (`save_refuses_before_load_with_a_pool`) and Task 5 (`tick_skips_unloaded_guilds`).
4. **An opted-out member** must earn nothing from voice or messages, can't buy, gamble, gift or
   be gifted, and is still counted as company for others in voice. Pinned in Tasks 3, 5 and 7.
5. **Adjusting below zero** (`/economy adjust @x -500` when they hold 200) must be refused with
   their balance, not clamped silently. Pinned in Task 2 by `adjust_below_zero_is_refused`.

---

### Task 1: Schema, settings, registry, load and save

**Files:**
- Create: `migrations/20261020120000_economy.sql` and the identical
  `crack-core/test_migrations/20261020120000_economy.sql`.
- Create: `crack-core/src/economy/mod.rs`, `crack-core/src/economy/settings.rs`,
  `crack-core/src/economy/registry.rs`, `crack-core/src/db/economy.rs` (settings functions only
  in this task).
- Modify: `crack-core/src/lib.rs` (`pub mod economy;`, plus a `pub economy: economy::registry::EconomyRegistry`
  field on `DataInner`), `crack-core/src/db/mod.rs`, and `crack-core/src/handlers/serenity.rs`
  (`on_guild_create`, beside the rooms and modlog loads).

**Interfaces:**
- Produces, in `economy::settings`:
  - `enum ActionKind { Immunity }`, with `const ALL: &'static [ActionKind]`,
    `fn key(self) -> &'static str` (`"immunity"`), `fn from_key(&str) -> Option<Self>`,
    `fn default_price(self) -> ActionPrice` (immunity: `{ price: 200, on: true }`) and
    `fn label(self) -> &'static str`. PR 2 adds variants.
  - `struct ActionPrice { pub price: u32, pub on: bool }` (serde).
  - `struct Prices(BTreeMap<String, ActionPrice>)`, `#[serde(transparent)]`, with
    `fn get(&self, ActionKind) -> ActionPrice` (the default when missing) and
    `fn set(&mut self, ActionKind, ActionPrice)`. Unknown keys are kept as-is.
  - `struct EconomyConfig` (`#[serde(default)]` on the struct), with the fields of the Global
    Constraints defaults: `currency_name: String`, `currency_emoji: String`,
    `voice_credits_per_minute: u32`, `message_credits: u32`, `message_cooldown_seconds: u32`,
    `prices: Prices`, `immunity_optout: bool`, `immunity_shop: bool`, `immunity_mods: bool`,
    `optout_cooldown_days: u32`, `bought_immunity_hours: u32`, `gambling_enabled: bool` and
    `max_bet: u32`. `Default` gives the spec values.
  - `struct EconomySettings { pub guild: GuildId, pub enabled: bool, pub config: EconomyConfig }`,
    with `fn off(GuildId) -> Self` and
    `fn validate(&self, has_db: bool) -> Result<(), EconomySettingsError>`.
  - `enum EconomySettingsError { NoDatabase, BadCurrencyName, BadCurrencyEmoji, BadRate, BadCooldown, BadPrice, BadOptoutCooldown, BadImmunityHours, BadMaxBet }`.
    Validate checks every bound, even when `enabled` is false, except that `NoDatabase` applies
    only when enabling.
- Produces, in `economy::registry::EconomyRegistry` (Default):
  - `settings(&self, GuildId) -> EconomySettings` (`off` when absent), `set_settings(EconomySettings)`,
    `is_enabled(GuildId) -> bool`, `mark_loaded(GuildId)` and `loaded(GuildId) -> bool`;
  - `set_opted_out(GuildId, HashSet<UserId>)`, `is_opted_out(GuildId, UserId) -> bool`,
    `opted_out(GuildId) -> HashSet<UserId>` and `set_one_opted_out(GuildId, UserId, bool)`.
  - Task 5 adds its earning fields here.
- Produces, in `economy` (mod.rs), mirroring `modlog/mod.rs`:
  - `enum EconomySaveError { Invalid(EconomySettingsError), NotLoaded, Db }`;
  - `async fn load_settings(&Data, GuildId) -> bool`, which reads the settings AND the
    opted-out set, then marks loaded;
  - `async fn save_settings(&Data, EconomySettings) -> Result<(), EconomySaveError>`, which
    refuses `NotLoaded` when there's a pool and the guild wasn't loaded, then validates, writes
    the DB, then memory.
- Produces, in `db::economy`:
  - `load_settings(&PgPool, GuildId) -> sqlx::Result<Option<EconomySettings>>`;
  - `save_settings(&PgPool, &EconomySettings) -> sqlx::Result<()>`;
  - `load_opted_out(&PgPool, GuildId) -> sqlx::Result<HashSet<UserId>>`.

**The migration** creates all five tables PR 1 needs (later tasks only write SQL against them):

```sql
CREATE TABLE IF NOT EXISTS economy_settings (
    guild_id   BIGINT PRIMARY KEY,
    enabled    BOOLEAN NOT NULL DEFAULT false,
    -- EconomyConfig (crack-core economy::settings), typed serde; missing fields take defaults.
    config     JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS economy_wallets (
    guild_id          BIGINT NOT NULL,
    user_id           BIGINT NOT NULL,
    balance           BIGINT NOT NULL DEFAULT 0 CHECK (balance >= 0),
    earned_voice      BIGINT NOT NULL DEFAULT 0,
    earned_messages   BIGINT NOT NULL DEFAULT 0,
    opted_out_at      TIMESTAMPTZ,
    optout_changed_at TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id)
);
CREATE INDEX IF NOT EXISTS economy_wallets_top ON economy_wallets (guild_id, balance DESC);
CREATE TABLE IF NOT EXISTS economy_ledger (
    id         BIGSERIAL PRIMARY KEY,
    guild_id   BIGINT NOT NULL,
    user_id    BIGINT NOT NULL,
    delta      BIGINT NOT NULL,
    kind       TEXT NOT NULL,      -- LedgerKind
    action_id  BIGINT,
    by_user_id BIGINT,             -- adjust: who
    reason     TEXT,               -- adjust: why
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS economy_ledger_user ON economy_ledger (guild_id, user_id, created_at DESC);
CREATE TABLE IF NOT EXISTS economy_actions (
    id          BIGSERIAL PRIMARY KEY,
    guild_id    BIGINT NOT NULL,
    kind        TEXT NOT NULL,     -- ActionKind::key
    buyer_id    BIGINT NOT NULL,
    target_id   BIGINT,
    price       BIGINT NOT NULL,
    note        TEXT,
    signed      BOOLEAN NOT NULL DEFAULT false,
    payload     JSONB NOT NULL,    -- ActionPayload, typed serde
    state       TEXT NOT NULL,     -- ActionState
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ,
    ended_at    TIMESTAMPTZ,
    end_reason  TEXT,
    revealed_at TIMESTAMPTZ,
    revealed_by BIGINT
);
CREATE INDEX IF NOT EXISTS economy_actions_live ON economy_actions (guild_id, state)
    WHERE state IN ('pending', 'active', 'ending');
CREATE INDEX IF NOT EXISTS economy_actions_target ON economy_actions (guild_id, target_id, created_at DESC);
CREATE TABLE IF NOT EXISTS economy_immunity (
    guild_id   BIGINT NOT NULL,
    user_id    BIGINT NOT NULL,
    source     TEXT NOT NULL CHECK (source IN ('bought', 'mod')),
    until      TIMESTAMPTZ,        -- NULL: until lifted (mod grants only)
    granted_by BIGINT,
    reason     TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id, source)
);
```

- [ ] **Step 1: Write the failing tests.** In `economy/settings.rs`:
  - `defaults_match_the_spec`: asserts every Global Constraints default, and that
    `Prices::default().get(ActionKind::Immunity)` is `{200, true}`.
  - `validate_bounds`: one assertion per bound, at the edge both ways. For example, a currency
    name of 24 chars is Ok and 25 is `BadCurrencyName`; an empty name is `BadCurrencyName`; a
    cooldown of 9 is `BadCooldown` and 10 is Ok; a price of 0 is `BadPrice`; `max_bet` 0 is
    `BadMaxBet`.
  - `validate_refuses_enabling_without_db`: enabled with `has_db = false` → `NoDatabase`;
    disabled → Ok.
  - `config_with_missing_fields_and_unknown_price_loads_and_keeps_it`: deserialize
    `{"max_bet": 50, "prices": {"rename": {"price": 9, "on": false}}}`. It gives `max_bet` 50,
    every other field at its default, `get(Immunity)` at the default, and re-serializing it still
    contains `"rename"`.

  In `economy/mod.rs`, mirror `modlog/mod.rs`'s three tests: `save_refuses_invalid_and_leaves_registry`,
  `disabling_without_a_pool_is_allowed`, and the db-test `save_refuses_before_load_with_a_pool`.
  In `db/economy.rs`: the db-tests `settings_survive_a_save_and_load` (including an upsert to
  off) and `load_opted_out_reads_only_opted_out_wallets`. Insert the wallets with raw SQL in the
  test.
- [ ] **Step 2: Run them; they fail to compile** (the types don't exist).
- [ ] **Step 3: Implement** the interfaces above, the migration in both directories, and the
  `on_guild_create` call (`economy::load_settings`, warn on false, as rooms does).
- [ ] **Step 4: Regenerate `.sqlx`** following the `scripts/sqlx_sync.py` header. Run the gate
  and the db-tests; all pass.
- [ ] **Step 5: Commit**: `feat(economy): settings, schema and registry`.

### Task 2: The money layer (wallets, ledger, actions, immunity storage)

**Files:**
- Modify: `crack-core/src/db/economy.rs`.
- Create: `crack-core/src/economy/wallet.rs` (types only: `LedgerKind`, `ActionState`,
  `ActionPayload` and `MoveError`).

**Interfaces:**
- Consumes: the Task 1 tables and `ActionKind`.
- Produces, in `economy::wallet`:
  - `enum LedgerKind { Purchase, Refund, GiftSent, GiftReceived, CoinflipStake, CoinflipWin, Adjust }`,
    with `fn as_str(self) -> &'static str` (snake_case, e.g. `"coinflip_win"`);
  - `enum ActionState { Pending, Active, Ending, Done, Ended, Failed }`, with `as_str`;
  - `#[serde(tag = "kind", rename_all = "snake_case")] enum ActionPayload { Immunity { hours: u32 } }`.
    PR 2 adds variants;
  - `enum MoveError { Insufficient { balance: i64 }, Db(sqlx::Error) }`.
- Produces, in `db::economy` (every amount is `i64`, every id a serenity id):
  - `struct EarnRow { pub user: UserId, pub voice: i64, pub messages: i64 }`, and
    `credit_earnings(&PgPool, GuildId, &[EarnRow]) -> sqlx::Result<()>`. It is one statement
    (`UNNEST` arrays) that upserts wallets, adding `voice + messages` to `balance` and each to
    its counter;
  - `balance(&PgPool, GuildId, UserId) -> sqlx::Result<i64>` (0 when there is no wallet);
  - `struct NewAction<'a> { guild, kind: ActionKind, buyer: UserId, target: Option<UserId>, price: i64, note: Option<&'a str>, signed: bool, payload: &'a ActionPayload, expires_at: Option<DateTime<Utc>> }`;
  - `charge_for_action(&PgPool, &NewAction<'_>) -> Result<Charged, MoveError>`, where
    `struct Charged { pub action_id: i64, pub balance: i64 }`. It is one transaction: lock the
    buyer's wallet, refuse `Insufficient`, debit, write a `purchase` ledger row with the action
    id, and insert the action as `pending`;
  - `finish_action(&PgPool, action_id: i64, ActionState) -> sqlx::Result<()>`;
  - `refund_action(&PgPool, action_id: i64) -> sqlx::Result<Option<i64>>`. It is one
    transaction: only a `pending` row becomes `failed`, its price is credited back with a
    `refund` ledger row, and it returns the new balance. It returns `None` (and changes nothing)
    when the row isn't `pending`, so it's idempotent;
  - `transfer(&PgPool, GuildId, from: UserId, to: UserId, amount: i64) -> Result<(i64, i64), MoveError>`.
    It locks both wallet rows in ascending user-id order (no deadlock), creates the receiver's,
    and writes `gift_sent −amount` and `gift_received +amount`. It returns
    (from_balance, to_balance);
  - `settle_coinflip(&PgPool, GuildId, UserId, bet: i64, won: bool) -> Result<i64, MoveError>`:
    one transaction, as in the Global Constraints;
  - `adjust(&PgPool, GuildId, UserId, amount: i64, by: UserId, reason: &str) -> Result<i64, MoveError>`.
    A negative amount beyond the balance is `Insufficient`. It writes an `adjust` row with `by_user_id` and `reason`;
  - `top(&PgPool, GuildId, limit: i64) -> sqlx::Result<Vec<(UserId, i64)>>`. Balances above 0,
    highest first, ties by user id;
  - `struct LedgerEntry { pub delta: i64, pub kind: String, pub created_at: DateTime<Utc> }`,
    and `history(&PgPool, GuildId, UserId, limit: i64) -> sqlx::Result<Vec<LedgerEntry>>`, newest
    first;
  - `struct OptOutState { pub out: bool, pub changed_at: Option<DateTime<Utc>> }`,
    `optout_state(&PgPool, GuildId, UserId) -> sqlx::Result<OptOutState>`, and
    `set_opted_out(&PgPool, GuildId, UserId, out: bool, now: DateTime<Utc>) -> sqlx::Result<()>`
    (upserts the wallet and sets both columns);
  - `grants(&PgPool, GuildId, UserId) -> sqlx::Result<Vec<economy::immunity::Grant>>`;
  - `extend_bought(&PgPool, GuildId, UserId, hours: u32, now) -> sqlx::Result<DateTime<Utc>>`,
    where until = `GREATEST(COALESCE(until, now), now) + hours`; it returns until;
  - `grant_mod(&PgPool, GuildId, UserId, until: Option<DateTime<Utc>>, by: UserId, reason: Option<&str>) -> sqlx::Result<()>`
    (upsert);
  - `lift_mod(&PgPool, GuildId, UserId) -> sqlx::Result<bool>` (true when a row was deleted).
  - (`Grant` is defined in Task 3's `economy::immunity`; Task 2 needs the type. **Create
    `economy/immunity.rs` in this task with only `GrantSource` and `Grant`**, and Task 3 adds
    the logic.)

- [ ] **Step 1: Write the failing db-tests** (all `#[sqlx::test(migrator = …)]` with the
  `db-tests` ignore attribute, as in `db/modlog.rs`):
  - `concurrent_debits_never_overdraw`: a wallet with 100 runs two `charge_for_action` calls of
    80 via `tokio::join!`. Exactly one is `Ok`, the other `Insufficient { balance: 20 }`, and the
    final balance is 20.
  - `refund_restores_once`: after a charge of 50 from 100, `refund_action` gives `Some(100)` and
    a second call gives `None`. Balance 100, action `failed`, one `refund` ledger row.
  - `ledger_and_wallet_agree`: after a charge, a refund, a transfer, a won and a lost coinflip,
    and an adjust, the sum of ledger deltas plus the earned counters equals the balance for each
    wallet.
  - `transfer_moves_both_sides_or_neither`: an insufficient transfer leaves both balances and
    writes no ledger rows.
  - `adjust_below_zero_is_refused`: a wallet with 200, `adjust(-500)` →
    `Insufficient { balance: 200 }`, unchanged.
  - `credit_earnings_creates_and_adds`: a new and an existing wallet both get
    `balance += voice + messages`, and the counters match.
  - `top_orders_and_skips_zero`; `history_is_newest_first_and_limited`.
  - `extend_bought_extends_from_the_later_of_now_and_until`: an active grant ending in 2 h,
    extended 24 h, ends at +26 h; an expired one ends at now + 24 h.
  - `lift_removes_only_the_mod_grant`.
  - `optout_round_trip`.
- [ ] **Step 2: Run them; they fail to compile.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Regenerate `.sqlx`; the gate and the db-tests pass.**
- [ ] **Step 5: Commit**: `feat(economy): wallets, ledger, actions and immunity storage`.

### Task 3: Immunity and opt-out rules

**Files:**
- Modify: `crack-core/src/economy/immunity.rs` (pure).
- Modify: `crack-core/src/economy/mod.rs` (glue).

**Interfaces:**
- Consumes: Task 2's `grants`, `optout_state`, `set_opted_out`, `grant_mod` and `lift_mod`, and
  Task 1's registry opt-out set.
- Produces, pure:
  - `enum GrantSource { Bought, Mod }` and
    `struct Grant { pub source: GrantSource, pub until: Option<DateTime<Utc>>, pub by: Option<UserId>, pub reason: Option<String> }`
    (both from Task 2);
  - `enum Protection { OptedOut, Until(DateTime<Utc>), Indefinitely }`;
  - `fn is_protected(opted_out: bool, grants: &[Grant], now: DateTime<Utc>) -> Option<Protection>`.
    Opted out → `OptedOut`. Otherwise any grant with `until: None` → `Indefinitely`. Otherwise the
    latest `until` that is still after `now` → `Until` of it. Otherwise `None`;
  - `fn can_switch_optout(changed_at: Option<DateTime<Utc>>, now: DateTime<Utc>, cooldown_days: u32) -> Result<(), DateTime<Utc>>`.
    `Err` carries when switching is next allowed. Cooldown 0 always allows.
- Produces, glue in `economy`:
  - `enum OptOutError { Disabled, Cooldown(DateTime<Utc>), AlreadyThere, NoDatabase, Db }`;
  - `async fn opt_out(&Data, GuildId, UserId, out: bool, now: DateTime<Utc>) -> Result<(), OptOutError>`.
    It refuses `Disabled` when opting **out** while `immunity_optout` is off (opting back **in**
    is always allowed), then checks the cooldown, writes the DB, and updates the registry set;
  - `async fn protection(&Data, GuildId, UserId, now) -> Result<Option<Protection>, sqlx::Error>`
    (with no pool: `Ok(None)`);
  - `async fn mod_grant(&Data, GuildId, target: UserId, until: Option<DateTime<Utc>>, by: UserId, reason: Option<&str>) -> Result<(), GrantError>`
    and `async fn mod_lift(&Data, GuildId, target: UserId) -> Result<bool, GrantError>`.
    `enum GrantError { Disabled, NoDatabase, Db }`; `Disabled` when `immunity_mods` is off, for
    grant only (lifting always works).
  - PR 2 makes `opt_out(true)` and `mod_grant` end running pranks. Leave a
    `// PR 2: end running pranks on the target here` marker at the one call site in each.

- [ ] **Step 1: Write the failing tests:**
  - `is_protected_matrix`: a table test over opted_out × {no grant, an expired bought grant, an
    active bought grant, an indefinite mod grant, a bought and a mod grant together}, with the
    exact expected `Option<Protection>` for each, including that the later of two finite untils
    wins;
  - db-test `turning_a_source_off_keeps_existing_immunity`: a bought grant made while
    `immunity_shop` was on still protects after a settings save that turns it off;
  - `optout_cooldown`: changed 6 days ago with 7 → `Err(changed + 7d)`; 7 days → Ok; cooldown
    0 → Ok; never changed → Ok;
  - db-tests `opt_out_updates_db_and_registry`, `opting_out_is_refused_when_disabled_but_opting_in_is_not`,
    and `mod_grant_then_lift`.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: The gate and db-tests pass.**
- [ ] **Step 5: Commit**: `feat(economy): immunity and opt-out`.

### Task 4: The purchase machinery, and immunity for sale

**Files:**
- Create: `crack-core/src/economy/shop.rs`.
- Modify: `crack-core/src/messaging/messages.rs`.

**Interfaces:**
- Consumes: Task 2's `charge_for_action`, `finish_action` and `refund_action`, Task 3's
  `opt_out`/registry checks, and `db::economy::extend_bought`.
- Produces:
  - `struct PurchaseFacts { pub enabled: bool, pub price: ActionPrice, pub source_on: bool, pub buyer_opted_out: bool, pub balance: i64 }`.
    `source_on` is the action's own switch (for Immunity, `immunity_shop`);
  - `enum Refusal { EconomyOff, NotForSale, BuyerOptedOut, CantAfford { short: i64 } }`. PR 2
    adds the target refusals;
  - `fn check_purchase(f: &PurchaseFacts) -> Result<(), Refusal>`, checking in exactly that order;
  - `fn refusal_text(&Refusal, currency: &str) -> String`, using messages.rs constants. For
    example `CantAfford { short: 40 }` → "You need 40 more credits.";
  - `enum BuyError { Refused(Refusal), NoDatabase, Db }`;
  - `struct ImmunityBought { pub until: DateTime<Utc>, pub balance: i64 }`;
  - `async fn buy_immunity(&Data, GuildId, buyer: UserId, now: DateTime<Utc>) -> Result<ImmunityBought, BuyError>`.
    It runs the facts check, then `charge_for_action` (payload `Immunity { hours }`, target
    `None`) mapping `Insufficient` to `CantAfford`, then `extend_bought`. On success it calls
    `finish_action(Done)`. On an `extend_bought` error it calls `refund_action` and returns `Db`.
    (This is section 3's step order. PR 2 adds the target and Discord steps between.)

- [ ] **Step 1: Write the failing tests:**
  - `check_purchase_order`: for each refusal, facts where it is the first that applies, and
    that the earlier ones win when several apply (economy off beats not-for-sale beats opted-out
    beats can't-afford);
  - `cant_afford_says_how_many_more`: balance 160, price 200 → `CantAfford { short: 40 }`;
  - db-tests `buying_immunity_charges_and_protects` (after it, `protection` is `Until(now+24h)`,
    with a 200 debit and a `done` action), `buying_twice_extends`, and
    `a_failed_grant_is_refunded`. For that last one, simulate by dropping the `economy_immunity`
    table inside the test before buying: the balance is unchanged, the action `failed`, and one
    refund row is written.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: The gate and db-tests pass.**
- [ ] **Step 5: Commit**: `feat(economy): purchase machinery; immunity for sale`.

### Task 5: Earning in voice and from messages

**Files:**
- Create: `crack-core/src/economy/earn.rs`.
- Modify: `crack-core/src/economy/registry.rs` (pending messages, last-counted times, a
  started flag) and `crack-core/src/handlers/serenity.rs` (a `FullEvent::Message` route; start
  the tick in `on_ready`).

**Interfaces:**
- Consumes: Task 1's registry and settings, and Task 2's `credit_earnings`/`EarnRow`.
- Produces, pure:
  - `struct VoiceMember { pub user: UserId, pub channel: ChannelId, pub bot: bool, pub self_deaf: bool }`
    and `struct VoiceSnapshot { pub afk: Option<ChannelId>, pub members: Vec<VoiceMember> }`;
  - `fn voice_earners(s: &VoiceSnapshot, opted_out: &HashSet<UserId>) -> Vec<UserId>`, sorted.
    A member earns when not a bot, not self-deafened, not opted out, not in the AFK channel, and
    at least one OTHER non-bot member is in the same channel. That other member counts whether
    or not they are deafened or opted out (my call: company is company);
  - `fn cooldown_allows(last: Option<Instant>, now: Instant, cooldown: Duration) -> bool`;
  - `fn earn_rows(voice: &[UserId], per_minute: u32, messages: &HashMap<UserId, u32>, per_message: u32) -> Vec<EarnRow>`.
    It merges by user, drops all-zero rows, and sorts by user.
- Produces, glue:
  - registry: `count_message(GuildId, UserId, now: Instant, cooldown: Duration) -> bool`
    (records the time and increments pending when allowed),
    `take_pending_messages(GuildId) -> HashMap<UserId, u32>`, and `start_once() -> bool` (true
    the first time only);
  - `fn on_message(&Data, &Message)`, synchronous. It counts only a non-bot author, in a guild
    channel, where the guild is enabled and loaded, the author isn't opted out, and
    `message_credits > 0`;
  - `#[async_trait] trait EarnWriter: Send + Sync { async fn write(&self, guild: GuildId, rows: &[EarnRow]) -> sqlx::Result<()>; }`,
    implemented for `PgPool` (calls `credit_earnings`). Tests use a recording fake;
  - `async fn tick_with(data: &Data, snapshots: Vec<(GuildId, VoiceSnapshot)>, writer: &dyn EarnWriter)`.
    For each guild with pending messages or a snapshot:
    - not loaded or not enabled: its pending messages are dropped and nothing is written;
    - otherwise it builds `earn_rows` from the current rates (voice earners from the
      snapshot, minus the registry's opted-out set) and calls `writer.write` once when there
      are rows. An error is logged at warn and that minute's messages are dropped.
  - `fn snapshot(cache: &Cache, guild: GuildId) -> Option<VoiceSnapshot>` (thin glue: the AFK
    channel from the guild's AFK metadata, the bot flag from the cached user);
  - `fn start(data: Data, cache: Arc<Cache>)`. When `start_once()` is true and there's a pool,
    it spawns a 60-second `tokio::time::interval` loop (`MissedTickBehavior::Skip`). The loop
    snapshots every enabled guild and calls `tick_with(&data, snaps, pool)`. It is called at the
    end of `on_ready` (`CacheReady` never fires on a bot in many servers).

- [ ] **Step 1: Write the failing tests:**
  - `voice_earners_rules`: one snapshot covering:
    - a lone member (earns nothing);
    - a pair, both earning;
    - a pair where one is self-deafened (only the other earns);
    - a member with only a bot for company (nothing);
    - an opted-out member with a partner (the partner earns, they don't);
    - the AFK channel (nothing);
    - two members split across two channels (nothing);
  - `cooldown_allows_edges`: `None` → true; exactly the cooldown elapsed → true; 1 ms short →
    false;
  - `earn_rows_merges_and_drops_zero`: rate 0 for voice with 2 messages at 1 gives a messages-only
    row; both rates 0 give no rows;
  - `count_message_respects_cooldown_and_take_drains`, on the registry with tokio's paused
    clock;
  - `tick_skips_unloaded_guilds`: a guild that is enabled but not loaded, with pending messages
    and a two-member snapshot. After `tick_with` the fake writer recorded no call and
    `take_pending_messages` is empty;
  - `tick_writes_rows_for_enabled_loaded_guilds`: a loaded, enabled guild with rates 1/1, a pair
    in voice, and one counted message from a third member. The fake writer recorded exactly one
    call with three rows: the pair with `voice: 1, messages: 0`, and the third member with
    `voice: 0, messages: 1`;
  - `tick_drops_messages_for_disabled_guilds`.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: The gate passes.**
- [ ] **Step 5: Commit**: `feat(economy): earn credits in voice and from messages`.

### Task 6: Mod log events for the economy

**Files:**
- Modify: `crack-core/src/modlog/event.rs` and `crack-core/src/messaging/messages.rs`.

**Interfaces:**
- Produces three new `ModLogEvent` variants, with `Display` kinds `immunity_granted`,
  `immunity_lifted` and `credits_adjusted`:
  - `ImmunityGranted { by: UserId, target: UserId, until: Option<DateTime<Utc>>, reason: Option<String> }`;
  - `ImmunityLifted { by: UserId, target: UserId }`;
  - `CreditsAdjusted { by: UserId, target: UserId, amount: i64, reason: String }`.
- Rendering follows the existing `Parts`/`plain` shape. Mentions go inside the embed. `until`
  renders as a Discord timestamp `<t:{unix}:f>`; `None` renders as `MODLOG_UNTIL_LIFTED`
  ("until lifted"). A reason goes in the reason field. The amount is signed, e.g. "+150" or
  "−200".

- [ ] **Step 1: Write the failing tests**, next to `room_kick_shows_actor_target_room_and_reason`:
  - `immunity_granted_shows_mod_target_until_and_reason`: finite and indefinite;
  - `immunity_lifted_shows_mod_and_target`;
  - `credits_adjusted_shows_signed_amount_and_reason`: one positive, one negative.

  Each asserts the rendered title, description and reason field strings exactly.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: The gate passes.**
- [ ] **Step 5: Commit**: `feat(modlog): immunity and credit adjustment events`.

### Task 7: `/credits` and `/coinflip`

**Files:**
- Create: `crack-core/src/commands/economy/mod.rs` (`pub fn commands() -> Vec<crate::Command>`),
  `crack-core/src/commands/economy/credits.rs`, `crack-core/src/commands/economy/coinflip.rs`
  and `crack-core/src/economy/gamble.rs`.
- Modify: `crack-core/src/commands/mod.rs` (`.chain(economy::commands())`) and
  `crack-core/src/messaging/messages.rs`.

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces, pure in `economy::gamble`:
  - `#[derive(poise::ChoiceParameter)] enum CoinSide { Heads, Tails }`;
  - `enum BetRefusal { GamblingOff, OptedOut, NonPositive, OverMax { max: u32 }, CantAfford { balance: i64 } }`;
  - `fn check_bet(gambling: bool, opted_out: bool, bet: i64, max_bet: u32, balance: i64) -> Result<(), BetRefusal>`,
    in that order;
  - `fn flip(rng: &mut impl rand::Rng) -> CoinSide`;
  - `fn coinflip_text(call: CoinSide, landed: CoinSide, bet: i64, balance: i64, currency: &str, emoji: &str) -> String`.
- Produces, pure in `commands/economy/credits.rs`:
  - `enum GiftRefusal { SelfGift, ToBot, FromOptedOut, ToOptedOut, NonPositive }`;
  - `fn check_gift(from: UserId, to: UserId, to_bot: bool, from_opted_out: bool, to_opted_out: bool, amount: i64) -> Result<(), GiftRefusal>`,
    checked in the enum's order;
  - `fn balance_text(..)`, `fn top_text(rows: &[(UserId, i64)], currency: &str, emoji: &str) -> String`,
    and `fn history_text(&[LedgerEntry], ..)`.
- **The commands** (`#[cfg(not(tarpaulin_include))]` on the poise fns; a `guild_only` slash
  command each):
  - `/credits`, with subcommands:
    - `balance [member]`: public;
    - `top`: public;
    - `gift <member> <amount>`: public on success, private when refused;
    - `history`: private;
    - `optout`, `optin`: private.
  - `/coinflip <bet> <call>`: public result, private refusal. It reads the balance, runs
    `check_bet`, flips with `rand::rng()`, and calls `settle_coinflip`. A `MoveError::Insufficient`
    from the race becomes the `CantAfford` text.
  - Every command first checks `economy.is_enabled(guild)`, else it replies `ECONOMY_OFF`
    privately.

- [ ] **Step 1: Write the failing tests:**
  - `check_bet_order_and_edges`: bet 0 → `NonPositive`; bet 501 with max 500 → `OverMax`; bet
    500 → Ok; balance 499 with bet 500 → `CantAfford { balance: 499 }`; gambling off beats
    everything;
  - `flip_is_fair_enough`: with `rand::rngs::StdRng::seed_from_u64(7)`, 10 000 flips give
    between 4 800 and 5 200 heads (this pins the mapping, not the RNG);
  - `coinflip_text_win_and_loss` (exact strings);
  - `check_gift_order` (each refusal first in turn);
  - `top_text_ranks_and_formats` (exact);
  - `history_text_signs_amounts` (exact).

  The commands themselves are glue and get no offline test. Name them in the report.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: The gate passes.**
- [ ] **Step 5: Commit**: `feat(economy): /credits and /coinflip`.

### Task 8: `/shop` and `/economy`

**Files:**
- Create: `crack-core/src/commands/economy/shop.rs` and
  `crack-core/src/commands/economy/admin.rs`.
- Modify: `crack-core/src/commands/economy/mod.rs` and `crack-core/src/messaging/messages.rs`.

**Interfaces:**
- Consumes: Task 4 (`buy_immunity`, `refusal_text`), Task 3 (`mod_grant`, `mod_lift`,
  `protection`), Task 2 (`adjust`), and Task 6's events. Events are posted through
  `modlog::live::ChannelSink { data, transport }`, as `rooms/interact.rs` builds it.
- Produces, pure:
  - `fn menu_text(settings: &EconomySettings) -> String`. It lists every `ActionKind::ALL` that
    is on, with its label and price, and for immunity, whether `immunity_shop` is off ("not sold
    here"). The last line links `docs/economy.md` on GitHub;
  - `fn economy_status_text(settings: &EconomySettings, wallets: i64, credits: i64) -> String`;
  - `fn adjust_allowed(member_permissions: Permissions) -> bool` (Manage Server).
- **The commands:**
  - `/shop`, with subcommands:
    - `menu`: public;
    - `immunity`: a public card "🛡️ @buyer is immune until <t:…:f>" plus their private
      balance. A refusal is private, via `refusal_text`.
  - `/economy` (`default_member_permissions = "MODERATE_MEMBERS"`), with subcommands:
    - `status`: private;
    - `grant <member> [hours] [reason]`: no hours means indefinite;
    - `lift <member>`;
    - `adjust <member> <amount> <reason>`: refused privately unless the invoker has Manage
      Server, checked in code with `adjust_allowed`.

    Grant, lift and adjust each post their mod log event naming the invoker, and reply privately.
  - `/economy grant` and `/shop immunity` refuse bots.

- [ ] **Step 1: Write the failing tests:**
  - `menu_lists_on_items_with_prices` (exact string with the defaults);
  - `menu_says_immunity_not_sold_when_the_switch_is_off`;
  - `economy_status_text_exact`;
  - `adjust_allowed_needs_manage_guild`: Moderate Members alone → false; with Manage Server →
    true.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: The gate passes.**
  `commands::tests` (the registry/registrable checks) must still pass with the four new
  top-level commands.
- [ ] **Step 5: Commit**: `feat(economy): /shop immunity and /economy for mods`.

### Task 9: The dashboard's Economy section

**Files:**
- Modify: `crack-web/src/settings.rs`, `crack-web/src/backend.rs`, `crack-web/src/lib.rs`
  (`LiveBackend`), `crack-web/src/routes.rs`, `crack-web/src/page.rs`,
  `crack-web/src/test_support.rs`, `crack-web/assets/settings.js` and
  `crack-core/src/messaging/messages.rs`.

**Interfaces:**
- Consumes: Task 1's `economy::{load_settings, save_settings, EconomySaveError}` and
  `EconomySettings`.
- Produces, in `crack-web/src/settings.rs`:
  - `struct PriceView { pub kind: String, pub label: String, pub price: u32, pub on: bool }`;
  - `struct EconomySettingsView`: `enabled`, every `EconomyConfig` field except `prices`, plus
    `prices: Vec<PriceView>` in `ActionKind::ALL` order;
  - `struct PriceForm { pub kind: String, pub price: u32, pub on: bool }`;
  - `struct EconomySettingsForm`: the same fields with `prices: Vec<PriceForm>`;
  - `enum EconomySaveRefused { Invalid(EconomySettingsError), UnknownAction, Failed, NotLoaded }`;
  - `fn economy_invalid_message(EconomySettingsError) -> &'static str`.
- `Backend` gains `economy_settings(&self, GuildId)` and
  `save_economy_settings(&self, UserId, GuildId, EconomySettingsForm)`, the same shapes as the
  mod log pair.
  - The save maps each `PriceForm.kind` through `ActionKind::from_key`; an unknown key is
    `UnknownAction`.
  - It starts from the stored config, so unknown stored price keys survive.
- Route `POST /g/{guild}/settings/economy`, with exactly `save_modlog`'s guards: JSON only,
  same origin, `history_access`, and the rate limit.
- The page gains an **Economy** section after the mod log. It has the on switch, the currency
  name and emoji, the two rates and the cooldown, a price table (on checkbox plus price per
  row), the three immunity switches, the opt-out cooldown, the immunity hours, and the gambling
  switch and max bet. Under the gambling switch is `DASH_ECONOMY_GAMBLING_NOTE` ("Credits have
  no cash value. Only turn this on where your members are adults."), and a link to
  `docs/economy.md` comes last.
- `settings.js` posts it like the mod log form: DOM APIs only (Trusted Types CSP).

- [ ] **Step 1: Write the failing tests**, in `routes.rs`, mirroring the mod log ones:
  - `economy_save_403s_for_non_managers`;
  - `economy_save_passes_the_exact_form_to_the_backend`;
  - `economy_save_refuses_an_unknown_action`;
  - `economy_save_shows_validation_messages`;
  - `settings_page_shows_the_economy_section_with_current_values` (asserts the rendered prices
    row and the gambling note).

  In `lib.rs` (`LiveBackend`, db-test if needed): `saving_keeps_unknown_stored_price_keys`.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: The gate passes.**
- [ ] **Step 5: Commit**: `feat(web): economy settings on the dashboard`.

### Task 10: Docs, CHANGELOG, version

**Files:** `docs/economy.md` (new), `CHANGELOG.md` (`## Unreleased`, at the top of `### Added`),
`Cargo.toml` (`[workspace.package] version = "0.32.0"`) and `Cargo.lock`.

- [ ] **Step 1: `docs/economy.md`.** For a server admin and for members:
  - what the economy is;
  - turning it on (dashboard → Settings → Economy);
  - how credits are earned (the exact voice rule from spec 2.2, and the message cooldown);
  - `/credits`, `/coinflip` and `/shop`, and `/economy` for mods;
  - immunity, its three sources, and the switches-don't-revoke rule;
  - "credits have no cash value";
  - a short "coming next" naming the prank shop.

  Every command and default must match the code.
- [ ] **Step 2: CHANGELOG.** One **Added** entry in the house style (a bold lead, then a plain
  explanation for a server owner).
- [ ] **Step 3: Version 0.32.0**, then `cargo build --workspace` so `Cargo.lock` follows. Only
  the workspace crates' versions change.
- [ ] **Step 4: Commit**: `v0.32.0: the economy, part 1 — credits, immunity, a coinflip`.
