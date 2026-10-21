# Economy Music and Group Play (PR 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the last of the catalogue. Members can:
- buy a walk-on **theme** that plays when someone next joins voice;
- **dedicate** a song to someone;
- **jump** the queue;
- put a **bounty** on a member's head;
- **crowdfund** a prank together.

Ships as **v0.34.0**, on top of v0.33.0 (#619).

**Architecture:** this extends `crack-core/src/economy/` from PRs 1 and 2. Nothing new is invented:
- **The pranks' row, sweeper and seam are reused.** A theme and a bounty are `economy_actions` rows that are `active` until they're used or they expire. Where a prank's end undoes its effect, theirs **refunds** the buyer (the "escrow" kinds).
- **A crowdfund is a prank row of its prank kind,** in a new state `funding`. Pledges sit in a new `economy_pledges` table. When the pledges reach the price, the row goes `pending` and runs the normal prank path. From then on the sweeper, cleanse, cancel and reconcile treat it like any prank. Every refund of it pays back the pledgers.
- **Dedicate and jump run `/play` and `/playnext`'s own path.** They charge first and refund if nothing was queued.
- **The theme player** mixes a short clip into the bot's call (songbird plays a non-queued track alongside the queue). When the bot is idle, it joins, plays and leaves.

**Tech Stack:** Rust 2021, serenity `next`, poise (choice enums, autocomplete), songbird `serenity-next` (`Call::play_input`, `TrackEvent::Playable`, `TrackHandle::seek`), sqlx 0.9/Postgres, chrono, axum (crack-web).

**Spec:** `docs/superpowers/specs/2026-10-20-economy-design.md`. This plan covers:
- 4.4 and 4.5;
- 4.2's rule that dedications, themes, bounties and crowdfunds neither trigger a mirror nor use it up;
- 4.3 for the new kinds;
- the parts of 5, 6, 7, 8, 13 and 14 that touch them.

**Read the spec before any task,** especially section 3 (the purchase steps), 4.4, 4.5, 6 and 14.

## Global Constraints

- **Everything from PRs 1 and 2 still holds:** off by default, the money rules, the audit reason, "never undo a moderator", the repo rules, the local postgres, the gate and the shell notes. The copy is at `.superpowers/sdd/2026-10-21-economy-music-and-groups/constraints.md`.
- **New `ActionKind` variants, keys and default prices** (all on by default). `ALL` lists them in this order after `Reveal`:
  - `Theme` "theme" 100;
  - `Dedicate` "dedicate" 40;
  - `Jump` "jump" 40;
  - `Bounty` "bounty" 50: the price is the **minimum stake**;
  - `Crowdfund` "crowdfund" 10: the price is the **minimum pledge**. It is never a row's kind, only a switch and a minimum.
- **Fixed by the spec** (consts, not settings):
  - a theme waits 7 days for its target's next join, and its clip plays at most 10 s;
  - a bounty lasts 7 days;
  - a crowdfund has 24 hours to fill.
- **Theme start:** `SS`, `M:SS` or `H:MM:SS`, at most `12:00:00`. Absent means 0.
- **The theme's link** goes through `crack_types::ytdl_for_url` and nothing else. It is never shown on a card.
- **New `ActionState::Funding` ("funding").** New `LedgerKind`s:
  - `BountyPlaced` "bounty_placed";
  - `BountyWon` "bounty_won";
  - `Pledge` "pledge";
  - `PledgeRefund` "pledge_refund".

  A theme's or a bounty's refund is the existing `Refund`.
- **One migration,** `20261021120000_economy_pledges.sql`, in both `migrations/` and `crack-core/test_migrations/`. It is additive:
  - the `economy_pledges` table (Task 1);
  - a partial unique index allowing one open fund per (guild, target, kind).

  Older images ignore both: a rollback only stops funds from filling or expiring. That is the PR body's rollback note, not code.
- **Rollback safety, as in PR 2:**
  - every query that decodes payloads filters on `kind = ANY(known)`;
  - a state this build doesn't know is skipped, never an error.
- **No mirror for these kinds.** A theme, dedication, bounty or crowdfund aimed at a mirror holder neither bounces nor uses the mirror up (spec 4.2).
- **The mod log always names the buyer:**
  - a dedication, a theme or a bounty placement posts `ShopAction`, naming the buyer;
  - an executed crowdfund posts `CrowdfundExecuted`, naming every contributor;
  - a jump posts nothing, since it is aimed at nobody.
- **Charging for nothing is refunded:**
  - a theme that never plays;
  - a bounty nobody collects;
  - a crowdfund that doesn't fill or can't land;
  - a dedication or jump that queued nothing.

## Rulings made in this plan (my calls, from the spec's silences)

- **P1: drag can't be crowdfunded.** Its destination is the buyer's voice channel at the moment it runs, and a fund fills at a moment nobody chose. The `start` choice offers rename, clown, lock, mute, deafen and jail.
- **P2: who collects a bounty, and when.**
  - Only the buyer of a prank that **landed** on the target collects. A bounced or blocked prank collects nothing, and an executed crowdfund collects nothing (it has many buyers).
  - Bounties the claimer placed are left standing.
  - Every other active bounty on the target goes to the claimer in one transaction.
- **P3: cleanse, cancel, opt-out and a moderator's grant also end a waiting theme on the member.** Its buyer is refunded, because it never played.
  - Running bounties and open funds are not ended. A bounty can't be collected from a protected member, and a fund re-checks its target when it fills; each refunds when its time runs out.
- **P4: reveal covers the new aimed kinds.** Themes, dedications and bounties can be revealed, as can crowdfunded pranks, which reveal their starter. A `funding` row can't be revealed yet.
- **P5: a crash before the play counts as nothing played.** A `pending` dedication or jump found at reconcile is refunded: a restart loses the play in flight. A theme and a bounty are charged and made `active` in one transaction, so they are never `pending`.
- **P6: dedicate and jump don't defer before the play.** `/play`'s own path chooses the reply's visibility (#535). So every check before it reads only the cache and the database, the same budget `/play` has today.
- **P7: a theme's card and Reveal.** The theme card posts when it plays, in the chat of the target's voice channel. The new cards carry no buttons; `/shop reveal` covers them.

## Review Focus

1. **A theme whose target joins while the bot is busy elsewhere, or during a `/gp` game.** It waits for a later join, and after 7 days it is refunded. Pinned by Task 7's `theme_plan_*` tests and Task 3's `an_expired_theme_refunds_its_buyer`.
2. **A crowdfund that fills after its target became immune, opted out or left voice.** Nothing happens in Discord, and every pledger is refunded. Pinned by Task 5's `a_filled_fund_whose_target_is_immune_refunds_everyone` and `..._not_in_voice_...`.
3. **Two pledges racing to fill one fund.** It executes exactly once, and the pledges never total more than the price. Pinned by Task 2's `racing_pledges_fill_a_fund_once`.
4. **A bounty claim.**
   - The placer's own prank doesn't collect their own bounty.
   - A bounced prank collects nothing.
   - A bounty is collected once.

   Pinned by Task 2's `claim_skips_the_placers_own_bounty` and `a_bounty_is_claimed_once`, and Task 4's `a_bounced_prank_collects_no_bounty`.
5. **A dedication or jump that queues nothing** (no results, the buyer not in voice, a `/gp` game running). It is refused before charging or refunded. Pinned by Task 8's `play_settlement_*` tests and the `/gp` block-list test.

---

### Task 1: Catalogue, states and the pledges table

**Files:**
- `crack-core/src/economy/settings.rs`;
- `crack-core/src/economy/wallet.rs`;
- `crack-core/src/economy/target.rs` (`check_target`);
- `crack-core/src/messaging/messages.rs`;
- `crack-core/src/commands/economy/credits.rs` (`ledger_label`);
- the migration in both directories;
- `.sqlx` if any query changes.

**Interfaces:**
- **`ActionKind`** gains the five variants from Global Constraints, with keys, labels and default prices.
- **New groups** on `ActionKind`:
  - `const RUNNING_ON_TARGET: &[ActionKind]`: `PRANKS` plus `Theme`. This is what cleanse, cancel, opt-out and a grant end (P3).
  - `const REVEALABLE: &[ActionKind]`: `PRANKS` plus `Theme`, `Dedicate` and `Bounty` (P4).
  - `const FUNDABLE: &[ActionKind]`: `Rename`, `Clown`, `Lock`, `Mute`, `Deafen` and `Jail` (P1).
  - `fn is_aimed(self) -> bool`: true for `Theme`, `Dedicate` and `Bounty`. These are aimed at a member but are not pranks.
  - `fn is_escrow(self) -> bool`: true for `Theme` and `Bounty`. Their row's end refunds the buyer.
- **`ActionPayload`** gains:
  - `Theme { url: String, start_secs: u32 }`;
  - `Dedicate { song: String }`;
  - `Jump { song: String }`;
  - `Bounty`.
- **`ActionState`** gains `Funding`. **`LedgerKind`** gains the four new kinds. Each kind gets a `ledger_label`.
- **`check_target`:**
  - an aimed kind refuses self (`TargetIsSelf`) as a prank does;
  - the owner and role-reach checks stay prank-only;
  - a mirror is read for pranks only;
  - `running_same_kind` applies to `Theme` (one waiting theme per target).
- **The migration:**

```sql
CREATE TABLE IF NOT EXISTS economy_pledges (
    id          BIGSERIAL PRIMARY KEY,
    action_id   BIGINT NOT NULL,          -- the economy_actions row in state 'funding'
    guild_id    BIGINT NOT NULL,
    user_id     BIGINT NOT NULL,
    amount      BIGINT NOT NULL CHECK (amount > 0),
    signed      BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    refunded_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS economy_pledges_action ON economy_pledges (action_id);
CREATE UNIQUE INDEX IF NOT EXISTS economy_actions_one_fund
    ON economy_actions (guild_id, target_id, kind) WHERE state = 'funding';
```

- [ ] **Step 1: Failing tests.**
  - `group_defaults_match_the_spec`: the five prices and keys, and `ALL`'s order;
  - `kind_groups`: the exact members of `RUNNING_ON_TARGET`, `REVEALABLE` and `FUNDABLE`, plus `is_aimed` and `is_escrow`;
  - `payload_round_trips_every_variant`, extended with the four new wire forms, e.g. `{"kind":"theme","url":"https://x","start_secs":83}` and `{"kind":"bounty"}`;
  - `action_state_text_round_trips`, with `funding` added;
  - `ledger_labels_are_named_and_unknown_kinds_show_raw`, extended;
  - `a_pr2_config_loads_with_pr3_defaults`: a stored price map without the new keys loads with the defaults;
  - `check_target_aimed_kinds`:
    - a theme, dedication or bounty at yourself is refused `TargetIsSelf`;
    - one at a mirror holder lands, with the mirror untouched;
    - one at a member above the bot's role lands (no reach check);
    - one at an immune or opted-out member is refused;
    - a second theme gives `AlreadyRunning`.
- [ ] **Step 2: Run; fail.** **Step 3: Implement,** including the migration in both directories. **Step 4: Gate.**
- [ ] **Step 5: Commit:** `feat(economy): the music and group-play catalogue`.

### Task 2: The money layer for escrow, bounties and pledges

**Files:** `crack-core/src/db/economy.rs` and `.sqlx` (via `scripts/sqlx_sync.py`).

**Interfaces.** Each of these is one transaction. Each locks wallets with PR 1's `lock_wallet`, so the `Locked` rule holds, and writes ledger rows with `action_id`.
- **Theme and bounty purchases:**
  - `charge_active(pool, a: &NewAction<'_>, ledger: LedgerKind) -> Result<Charged, MoveError>`. It debits the buyer and inserts the row directly as `active`, with `a.expires_at`. A theme uses it with `Purchase`, a bounty with `BountyPlaced`.
  - `refund_active(pool, id: i64, reason: &str) -> sqlx::Result<Option<i64>>`. Only when the row is `active`: it ends the row (`ended`, the reason), credits `price` back to the buyer with `Refund`, and returns the buyer's new balance. `None` when the row was not `active`.
- **Bounty claims:** `claim_bounties(pool, guild, target, claimer, by_action: i64) -> sqlx::Result<i64>`.
  - It locks every `active` `bounty` row on `target` whose buyer isn't `claimer`, `FOR UPDATE`.
  - It ends each one `ended` with `claimed` and credits its price to `claimer` with `BountyWon`, one ledger row per bounty, each with that bounty's id.
  - It returns the total, 0 when none.
- **Funds:**
  - `open_fund(pool, a: &NewAction<'_>, first: Option<(i64, bool)>) -> Result<OpenedFund, FundError>`. It inserts the row as `funding`, with `expires_at` = now + 24 h and the prank's `price`. It then takes the optional first pledge as `pledge` does.
    - A unique violation on `economy_actions_one_fund` becomes `FundError::AlreadyOpen`.
    - `OpenedFund { action_id, pledged: i64, balance: Option<i64> }`.
  - `pledge(pool, fund: i64, guild, user, amount: i64, signed: bool) -> Result<Pledged, FundError>`:
    1. Lock the fund row `FOR UPDATE`. Refuse `FundError::Closed` unless it is `funding` and unexpired.
    2. `taken = min(amount, price − sum(pledges))`.
    3. Debit the pledger `taken` (`Pledge`) and insert the pledge.
    4. If the pledges now equal the price, set the row `pending` in the same transaction.

    `Pledged { taken, total, price, filled: bool, balance }`.
  - `FundError` is `AlreadyOpen`, `Closed`, `Money(MoveError)` or `Db(sqlx::Error)`.
  - `fund_pledges(pool, fund) -> sqlx::Result<Vec<PledgeRow>>`, with `PledgeRow { user: UserId, amount: i64, signed: bool }` in pledge order.
  - `refund_fund(pool, id, reason: &str) -> sqlx::Result<bool>`. From `funding` or `pending`, it credits every unrefunded pledge back with `PledgeRefund`, stamps `refunded_at`, and ends the row: `ended` from `funding`, `failed` from `pending`, with `reason`.
- **`refund_action`** (PR 1's pending refund) becomes pledge-aware. A `pending` row with pledges refunds the pledgers, as `refund_fund` does, never the starter's `price`. That keeps reconcile's refund right for a crowdfund that crashed mid-run.
- **Reads:**
  - `set_payload(pool, id, payload: &ActionPayload) -> sqlx::Result<bool>`, only while `pending`;
  - `due_funds(pool, now, limit) -> sqlx::Result<Vec<(GuildId, ActionRow)>>`, for `funding` rows past `expires_at`, known kinds only;
  - `open_funds(pool, guild) -> sqlx::Result<Vec<(ActionRow, i64)>>`, each with its pledged total, newest first;
  - `active_bounties(pool, guild) -> sqlx::Result<Vec<ActionRow>>`.
- **`reveal_candidates`** filters on `ActionKind::REVEALABLE` instead of `PRANKS` (P4).

- [ ] **Step 1: Failing db-tests (`sqlx::test`).**
  - `charge_active_debits_and_starts_active`;
  - `refund_active_refunds_once`: a second call returns `None`, and the balance moves once;
  - `claim_skips_the_placers_own_bounty`;
  - `a_bounty_is_claimed_once`: two claims in sequence, and the second gets 0;
  - `pledges_cap_at_the_price`: 30 of 40 pledged, then 25 asked, takes 10 and fills;
  - `racing_pledges_fill_a_fund_once`: two concurrent `pledge` calls that would each fill it. Exactly one is `filled`, and the sum equals the price;
  - `one_open_fund_per_target_and_kind`: a second `open_fund` gives `AlreadyOpen`; one for another kind opens;
  - `refund_fund_pays_back_every_pledger`, from both `funding` and `pending`. The ledger and wallets agree;
  - `a_pending_funded_row_refunds_pledgers_not_the_starter` (`refund_action`);
  - `due_funds_finds_expired_funding_rows_only`;
  - `reveal_now_covers_themes_dedications_and_bounties`, and never a `funding` row.
- [ ] **Step 2-4.** Regenerate `.sqlx` with the `sqlx_sync.py` recipe. **Step 5: Commit:** `feat(economy): escrow, bounty and pledge money paths`.

### Task 3: Endings for the new kinds (sweeper, cleanse, reconcile)

**Files:** `crack-core/src/economy/effects.rs`.

**Interfaces:**
- **`finish`:**
  - An escrow kind (`row.kind.is_escrow()`) never calls Discord. Its end is `refund_active(pool, row.id, reason)`, giving `Finished::Ended` when it refunded and `Skipped` otherwise.
  - The `reason` is what the caller passes: `expired` from the sweeper; `cleansed`, `mod_cancelled`, `optout` or `immunity` from `end_running_pranks`.
- **`end_running_pranks`** reads `ActionKind::RUNNING_ON_TARGET` instead of `PRANKS` (P3).
- **`sweep`** also takes `due_funds` (same batch size) and ends each one with `refund_fund(pool, id, "unfunded")`. The cached-guild skip doesn't apply, since there is no Discord call.
- **`landed`:** `Dedicate` and `Jump` give `Some(false)`, so reconcile refunds them (P5).

- [ ] **Step 1: Failing tests** (db-tests with `FakeEconomyDiscord`):
  - `an_expired_theme_refunds_its_buyer`: no Discord call is recorded, and the balance is back;
  - `an_expired_bounty_refunds_its_placer`;
  - `cleanse_ends_a_waiting_theme_and_refunds_it`: it counts as ended in `EndedPranks.were_active`;
  - `an_unfilled_fund_is_refunded_after_24h`;
  - `reconcile_refunds_a_pending_dedication`.
- [ ] **Step 2-4.** **Step 5: Commit:** `feat(economy): themes, bounties and funds end by refund`.

### Task 4: Bounties

**Files:**
- create `crack-core/src/economy/bounty.rs` and `crack-core/src/commands/economy/bounty.rs`;
- `crack-core/src/commands/economy/shop.rs` (`run_prank`'s claim hook);
- `crack-core/src/commands/economy/mod.rs`;
- `crack-core/src/economy/cards.rs`;
- `messages.rs`.

**Interfaces:**
- **`pub const BOUNTY_DAYS: i64 = 7;`**
- **Placing:** `pub async fn place(data, facts: TargetFacts, req: BountyRequest) -> Result<Placed, BuyError>`.
  - `BountyRequest { guild, placer, target, amount: i64, note, signed, now }`.
  - **Order:**
    1. `check_purchase`, with the price set to the amount;
    2. `amount >= prices.get(Bounty).price`, or `Refusal::BountyTooSmall { min }`;
    3. `check_target(Bounty, ..)`;
    4. `charge_active(.., BountyPlaced)`, with `expires_at` = now + 7 days.
  - `Placed { action_id, balance, until }`.
- **`/bounty`**, guild-only:
  - **`place <member> <amount> [note] [signed]`.** The gate and defer as the shop does (PR 2's I3). The reply is private and gives the balance. A public card: "💰 {amount} {currency} on @target's head until <t:…:f>. Land a prank on them to collect it." Then the note, then "— @placer" or "— someone". The mod log gets a `ShopAction` naming the placer, with `until`.
  - **`list`.** Each target with active bounties, the largest total first. Each line gives the total, the soonest end and the signed placers.
- **The claim hook.** In `run_prank`, after a purchase whose outcome is `BuyOutcome::Landed` and whose kind `is_prank()`, call `claim_bounties(pool, guild, target, buyer, action_id)`. When it is above 0:
  - the private reply gets the line "💰 You collected {n} {currency} of bounty.";
  - the card gets "💰 The bounty on @target was collected.", which doesn't name the claimer.

  Never on `Bounced` or `Blocked` (P2).
- **New refusals:** `BountyTooSmall { min }`, with copy "A bounty starts at {min} {currency}."

- [ ] **Step 1: Failing tests.**
  - pure `bounty_refusal_order`: economy off, not for sale, below the minimum, can't afford, bot, self, opted out, immune;
  - db `placing_a_bounty_escrows_it`;
  - db `a_landed_prank_collects_every_other_bounty`;
  - db `a_bounced_prank_collects_no_bounty`;
  - `bounty_card_text` and `bounty_list_text`: unsigned placers are not named;
  - the registry test from PR 2 extended: `/bounty` with `place` and `list`.
- [ ] **Step 2-4.** **Step 5: Commit:** `feat(economy): bounties`.

### Task 5: Crowdfunds

**Files:**
- create `crack-core/src/economy/crowdfund.rs` and `crack-core/src/commands/economy/crowdfund.rs`;
- `crack-core/src/economy/shop.rs`: make `payload_for`, `act` and the expiry computation `pub(crate)`, so a fund reuses them instead of copying;
- `crack-core/src/modlog/event.rs`;
- `cards.rs`;
- `messages.rs`;
- the commands `mod.rs`.

**Interfaces:**
- **`pub const FUND_HOURS: i64 = 24;`**
- **`ModLogEvent::CrowdfundExecuted { action: ActionKind, target: UserId, contributors: Vec<UserId>, until: Option<DateTime<Utc>> }`.** The starter comes first, then each pledger once, in pledge order. It is rendered like `ShopAction`, naming them all.
- **Starting a fund:** `pub async fn start(data, facts: TargetFacts, req: FundRequest) -> Result<OpenedFund, BuyError>`, with `FundRequest { guild, starter, target, kind, nick: Option<String>, note, signed, first_pledge: Option<i64>, now }`.
  - **Checks,** in order:
    1. the economy is on, `Crowdfund` is on and the fund's kind is on;
    2. the starter hasn't opted out;
    3. a rename has a valid name;
    4. `pub fn check_fund_start(kind, &TargetFacts) -> Result<(), Refusal>`: `check_target`'s order without the voice step (voice is re-checked at fill: spec "re-checked at that moment") and without the mirror;
    5. the preflight for the kind;
    6. a first pledge, if given, of at least the minimum and affordable.
  - Then `open_fund`. `AlreadyOpen` becomes `Refusal::FundAlreadyOpen`.
  - **The payload is the prank's.** A rename stores its name with `restore: None`; the restore is captured at fill.
- **Executing a filled fund:** `pub async fn execute(data, discord: &dyn EconomyDiscord, facts: TargetFacts, fund: &ActionRow, cfg, now) -> Result<Executed, NotLanded>`.
  1. `check_target` with `has_mirror: false`, then the preflight.
  2. Rebuild the payload, capturing the restore as a purchase does, and `set_payload`.
  3. `act`.
  4. `activate_action`, with the kind's expiry from now.

  On a refusal or a Discord failure, `refund_fund(pool, id, "refused" | "failed")`. `NotLanded(Refusal | EconDiscordError)` carries the reason. `Executed { action_id, until: Option<DateTime<Utc>>, contributors: Vec<UserId>, signers: Vec<UserId> }`, where `signers` is the starter if the row is signed, plus each signed pledger once.
- **`/crowdfund`**, guild-only:
  - **`start <action: FundKind> <member> [name] [note] [signed] [pledge]`.** `FundKind` is a poise choice enum over `FUNDABLE` only (P1).
  - **`pledge <fund> <amount> [signed]`.**
    - `fund` autocompletes from `open_funds` as "{label} on {target name}: {pledged}/{price}".
    - A pledge to a fund aimed at yourself is refused, `Refusal::PledgeOnSelf`.
    - A pledge below the minimum is refused, `Refusal::PledgeTooSmall { min }`, unless it is exactly what is left and that is below the minimum.
    - When `filled`, the same command executes it. It fetches the target with `guild.member(&http, id)` (cache, then HTTP); a member who has left gives `refund_fund(.., "left")`. It then gathers facts and calls `execute`.
  - **`list`.**
- **Cards:**
  - On a fill that lands: "🎉 A crowdfund {did the prank} @target until <t:…:f>", the note, then "— @a, @b and {n} others" for the signed contributors, or "— a crowd" when none signed.
  - On a fill that can't land: "The crowdfund against @target couldn't land ({reason}). Everyone was refunded."
  - The card goes where the prank's card would (`card_destination`).
- **The mod log** gets `CrowdfundExecuted` on a landing only.
- **New refusals:** `FundAlreadyOpen`, `FundClosed`, `PledgeOnSelf` and `PledgeTooSmall { min }`, with their copy in messages.rs.

- [ ] **Step 1: Failing tests.**
  - pure `fund_start_refusal_order`;
  - pure `fund_card_names_only_signers`: "a crowd" when nobody signed;
  - db + fake `a_filled_fund_lands_and_runs_like_a_prank`. The fake records the nick; the row is `active` with an expiry; the sweeper later restores the name;
  - db + fake `a_filled_fund_whose_target_is_immune_refunds_everyone`: no Discord call, and every pledger refunded;
  - db + fake `a_filled_fund_whose_target_is_not_in_voice_refunds_everyone`, for mute;
  - db + fake `a_discord_failure_at_fill_refunds_everyone`;
  - `crowdfund_executed_renders_every_contributor`;
  - the registry test: `/crowdfund` with `start`, `pledge` and `list`; `start`'s choices are exactly `FUNDABLE`.
- [ ] **Step 2-4.** **Step 5: Commit:** `feat(economy): crowdfunds`.

### Task 6: Buying a theme

**Files:**
- create `crack-core/src/economy/theme.rs` (the pure part and the purchase);
- `crack-core/src/commands/economy/shop.rs` (the `theme` subcommand);
- `messages.rs`.

**Interfaces:**
- `pub const THEME_DAYS: i64 = 7;`, `pub const THEME_CLIP: Duration = Duration::from_secs(10);`, and `pub const THEME_START_MAX: Duration = 12 h`.
- `pub fn parse_start(raw: &str) -> Result<Duration, StartError>`.
  - It takes `SS`, `M:SS` and `H:MM:SS`; minutes and seconds after the first field must be below 60.
  - `StartError` is `Malformed` or `TooLate`.
- `pub async fn buy_theme(data, facts: TargetFacts, req: ThemeRequest) -> Result<Bought, BuyError>`, with `ThemeRequest { guild, buyer, target, url: String, start: Duration, note, signed, now }`; `Bought.outcome` is `Landed`.
  - Order: `check_purchase`, then the link guard (`crack_types::ytdl_for_url` must accept it, else `Refusal::BadLink`), then `check_target(Theme, ..)`.
  - Then `charge_active(.., Purchase)`, with `expires_at` = now + 7 days. The payload is `Theme { url, start_secs }`.
- **`/shop theme <member> <link> [start] [note] [signed]`.**
  - The gate and defer, as the pranks do.
  - The private reply: "Their theme plays the next time they join voice, within 7 days. If it never does, you get your {currency} back."
  - The mod log gets a `ShopAction` naming the buyer, `until` = the 7-day end. No card yet (P7).

- [ ] **Step 1: Failing tests.**
  - `parse_start_edges`: "0", "83", "1:23", "1:02:03", "12:00:00" (ok), "12:00:01" (TooLate), "1:60", "a", "" and "1::2" (Malformed);
  - db `a_theme_purchase_escrows_and_waits`;
  - pure `theme_refusals`: a bad link ("file:///etc/passwd", "ftp://x"), self, immune, a second theme;
  - the registry test: `/shop` has 13 subcommands.
- [ ] **Step 2-4.** **Step 5: Commit:** `feat(economy): buy a walk-on theme`.

### Task 7: Playing a theme when its target joins voice

**Files:**
- `crack-core/src/economy/theme.rs` (the player);
- `crack-core/src/handlers/serenity.rs` (the voice-join hook);
- `crack-core/src/economy/registry.rs` (in-memory guards);
- `crack-core/src/music/audit.rs` (`BotReason::Theme`, named "theme");
- the global `TrackEnd` handler's module (the guard below);
- `cards.rs`;
- `messages.rs`.

**Interfaces:**
- **The decision is pure:** `pub fn theme_plan(bot_in: Option<ChannelId>, target_in: ChannelId, game_running: bool, playing_elsewhere: bool) -> ThemePlan`, where `ThemePlan` is `MixHere`, `JoinPlayLeave` or `Wait`.
  - A game running, another theme playing in this guild (`playing_elsewhere`), or the bot in another channel gives `Wait`.
  - The bot in `target_in` gives `MixHere`.
  - No call gives `JoinPlayLeave`.
- **The hook:** `pub fn on_voice_join(data: Arc<Data>, http, cache, guild, user, channel)`. It is spawned with `spawn_supervised` from the existing join branch in `handlers/serenity.rs`, and never awaited on the event path.
  1. Read the target's oldest `active` theme. Without one, nothing happens.
  2. **Re-check protection.** If they're now immune or opted out: `refund_active(.., "immunity" | "optout")`, and stop.
  3. Take the guild's theme guard (`registry`: a `DashSet<GuildId>`, released on drop).
  4. Plan.
     - **`Wait`** leaves the row for a later join.
     - **`JoinPlayLeave`:** `ensure_can_join` and `join_permitted`, then `set_global_handlers_with` (as `music::resume` does). A join that is refused or fails is `Wait`.
  5. **Play.**
     - Build the input with `crack_types::ytdl_for_url(http_utils::get_client_old().clone(), &url)`, and play it with `Call::play_input`, not the queue. It is not a queued track, so `music::queue::build_track` doesn't apply.
     - Record its UUID in the registry's theme-track set.
     - Arm `Playable` and `Error` handlers. Seek to `start` once playable, as `/gp`'s clip does: a failed seek removes the track.
     - Stop the track `THEME_CLIP` after `Playable`.
  6. **The outcome.**
     - **`Playable` within 20 s:** `finish_action(Done)`. Post the theme card ("🎺 @target's walk-on theme", the note, the signer) in the voice channel's chat. A failed post is logged.
     - **An error, a failed seek, or no `Playable` in 20 s:** stop the track and `refund_active(.., "unplayable")`.
  7. **Leave.** After `JoinPlayLeave`, once the clip has stopped, leave only if the queue is empty: `music::disconnect::disconnect(.., Actor::bot(BotReason::Theme))`. Otherwise someone started music meanwhile; stay.
- **The `TrackEnd` guard.** The global `TrackEnd` handler must ignore a track whose UUID is in the theme-track set, and remove it. A theme ending is not the queue moving on. Check what the handler does today (autoplay, status, play log) and make the guard its first line.

- [ ] **Step 1: Failing tests.**
  - pure `theme_plan_*`: all four inputs, each branch, with the game and another-theme `Wait`s first;
  - pure `track_end_ignores_theme_tracks`, on whatever pure seam the handler has. If it has none, add `fn is_theme_track(registry, uuid) -> bool` and test the guard through it;
  - db `a_protected_target_gets_no_theme_and_the_buyer_is_refunded`, driven up to step 2 with no voice: factor steps 1-2 into `async fn claim_theme(data, guild, user, now) -> Option<ActionRow>`, which returns `None` after refunding.
  - The audio itself can't be tested offline: list it in the report for the TuneTitan checklist.
- [ ] **Step 2-4.** **Step 5: Commit:** `feat(economy): walk-on themes play on join`.

### Task 8: Dedicate and jump

**Files:**
- `crack-core/src/commands/music/doplay.rs`;
- `crack-core/src/commands/music/gp/state.rs` (`GP_BLOCKED_COMMANDS`);
- `crack-core/src/commands/economy/shop.rs`;
- `cards.rs`;
- `messages.rs`.

**Interfaces:**
- **Split `play_internal`:**
  - `pub async fn play_core(ctx, mode, file, query) -> Result<PlayOutcome, Error>` holds today's body, and `play_internal` becomes `play_core(..).await.map(|_| ())`. Behaviour is unchanged for every existing caller.
  - `PlayOutcome` is `Queued` or `NothingQueued`. It is `Queued` only when `fill_search_reply` returned `Ok` and the queue is longer than it was before `match_mode`; read the length where `was_empty` is read.
  - The paused/no-query branch is `NothingQueued`.
- **Pure:** `pub fn play_settlement(r: &Result<PlayOutcome, ()>) -> Settlement { Keep, Refund }`.
- **`/shop dedicate <member> <song> [note] [signed]`** and **`/shop jump <song> [note] [signed]`.**
  - They carry `check = "cmd_check_music"`, and `"shop dedicate"` and `"shop jump"` join `GP_BLOCKED_COMMANDS`.
  - **Order:**
    1. the gate, with **no defer** (P6);
    2. validate the note;
    3. for dedicate, facts and `check_target(Dedicate, ..)`;
    4. `check_purchase`;
    5. `charge_for_action` (`pending`; the payload `Dedicate { song }` or `Jump { song }`);
    6. `play_core(ctx, None or Some("next"), None, Some(song))`.
  - **Afterwards:**
    - **`Refund`:** `refund_action`. On `Err`, return the error so poise's handler shows it as `/play`'s does. On `NothingQueued`, a private follow-up says "Nothing was queued, so nothing was charged."
    - **`Keep`:** `finish_action(Done)`, then a private follow-up with the balance. Then a public card in the invoking channel, or the announce channel when that's set:
      - dedicate: "🎵 This one goes out to @target", the note, and the signer;
      - jump: "⏭️ Someone paid to play this next", the note, and the signer.
    - A dedication also logs a `ShopAction`.

- [ ] **Step 1: Failing tests.**
  - pure `play_settlement_*`: `Ok(Queued)` keeps; `Ok(NothingQueued)` and `Err` refund;
  - `gp_blocks_dedicate_and_jump`: both qualified names are in `GP_BLOCKED_COMMANDS`;
  - `dedication_card_text` and `jump_card_text`, signed and not;
  - pure `dedicate_refusals`: self, a bot, immune, and a mirror holder (lands, mirror untouched);
  - the registry test: `/shop` has 15 subcommands, and dedicate and jump carry the music check.
- [ ] **Step 2-4.** **Step 5: Commit:** `feat(economy): dedicate a song, jump the queue`.

### Task 9: Menu, dashboard, docs, version

**Files:**
- `crack-core/src/economy/preflight.rs`;
- `crack-core/src/commands/economy/shop.rs` (`menu_text`);
- `crack-web/src/settings.rs`, `crack-web/src/page.rs` and `crack-web/assets/settings.js`;
- `docs/economy.md`;
- `CHANGELOG.md`;
- `Cargo.toml` (0.34.0) and `Cargo.lock`.

**Interfaces:**
- **`needs`:** the five new kinds need nothing server-wide. A theme's Connect and Speak are checked where it plays (Task 7), and the music commands check themselves.
- **`menu_text`** lists the new items under "Music" (theme, dedicate, jump) and "Group play" (bounty, crowdfund), each with its price. For bounty and crowdfund it reads "from {price}".
- **Dashboard:** `PriceView` gains `hint: Option<String>`, shown under the row:
  - bounty: "Minimum stake.";
  - crowdfund: "Minimum pledge. A fund's goal is its prank's price.";
  - jump: "To make jumping the queue a paid perk, restrict /playnext for members in Server Settings → Integrations." (spec 4.4).
- **`docs/economy.md`:** a "Music" and "Group play" section per spec 4.4/4.5, with this plan's rulings P1-P7 in plain words, the refunds, and the mirror rule. **CHANGELOG** gets a v0.34.0 entry. **The version** goes to 0.34.0 everywhere PR 2 bumped it.

- [ ] **Step 1: Failing tests.**
  - `menu_lists_music_and_group_play`;
  - `price_hints_for_bounty_crowdfund_and_jump`;
  - crack-web `the_view_lists_prices_in_action_order`, extended to 16 kinds;
  - crack-web `save_round_trips_the_new_prices`.
- [ ] **Step 2-4.** **Step 5: Commit:** `v0.34.0: the economy, part 3 — music and group play`.
