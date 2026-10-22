# CrackCoin: withdraw credits as a Solana devnet token — design

**Date:** 2026-10-21 · **Status:** the owner approved this as economy PR 4
(v0.35.0) and answered the open questions. The economy go-ahead runs through
the TuneTitan deploy. Anything not marked **(owner)** is my call, and is
listed as such so it can be checked.

## Why

On 2026-10-20 the owner asked, "just for fun", what crypto integration would
take. My answer drew the line at a **freely tradeable mainnet token**. Once a
token has a price, the coinflip becomes real-money gambling, a payout becomes a
transfer of value, and Discord's policies apply. A token on a test network is
**worthless by design**, and that is the whole point.

The owner: "Testnet sounds perfect … Let's add that testnet token as PR 4 after
the economy" **(owner)**. Solana devnet was my recommendation, and the owner
didn't object: it has the memecoin feel, the token is a plain SPL mint with no
contract to write, and Token-2022's NonTransferable extension is the path to a
soulbound mainnet token later, if that is ever wanted.

**The moment it exists for:** a member who has earned 500 credits in Saucey's
server types `/wallet withdraw 500`. Their wallet app, switched to devnet, shows
500 tokens arrive from the bot a few seconds later, and they can show them off.

## Owner decisions

| Question | Decision |
|---|---|
| Real money? | Never. Testnet only, worthless by design |
| Where it sits | PR 4 of the economy arc, v0.35.0 |
| Packaging | Its own module, preferably its own repo: "all those dependencies just poof with a press of a compile option" |
| The repo | `cycle-five/crack-coin`, **public** |
| The signing key | I generate the devnet keypair into a 0600 file outside any repo and never print it. The owner stores it in Vaultwarden. `.env.tunetitan` gets it by key name only |
| Deploying | TuneTitan runs a **separate `vX.Y.Z-coin` image** built with the feature on. The production image stays lean; the migrate image is unchanged |
| Name and logo | **In v1.** Named "CrackCoin", symbol `CRACK`, with the owner's cracked-coin emblem (`~/Pictures/cracktunes/segments/circular-emblem.png`) as its logo: "CrackCoin is perfect and here's the logo" |

## Goals

- `/wallet link <address>` records a Solana address for a Discord user.
- `/wallet withdraw <amount>` spends that many credits in this server and mints
  the same number of tokens to the linked address on devnet, 1:1.
- The bot never loses a member's credits. A withdrawal either mints, or its
  credits come back. It never does both, and never neither.
- Without the `crack-coin` feature, none of the Solana code is compiled, and the
  lean build's dependency tree is unchanged. `Cargo.lock` gains inactive entries
  (the crack-coin git entry and `bs58`), but nothing more is compiled.
- A server admin decides whether withdrawals are possible in their server. They
  are off by default.

## Non-goals (v1)

- **Deposits** (tokens back into credits). Withdrawal is one-way.
- **Proving wallet ownership** by signature. Paste first; proof can come later.
  Pasting someone else's address only gifts them worthless tokens.
- Any exchange rate other than 1 credit = 1 token, per-server rates, or caps on
  total supply.
- Mainnet, of any kind.
- Announcing withdrawals in a channel.

## Anti-goals (would make this a failure even if it works)

- **Touching mainnet.** The bot must refuse to run CrackCoin against a mainnet
  RPC, even if someone misconfigures the URL (section 4).
- **Minting twice for one debit, or keeping credits when nothing was minted.**
  This is the money rule of the whole economy, carried over the chain boundary
  (section 6).
- **Leaking the signing key.** It may never appear in logs, `Debug` output,
  replies, test output, or a terminal.
- **Exposing a member's wallet address.** Phantom and similar wallets use the
  same address on mainnet and devnet, so a linked address is usually someone's
  real wallet. Linking it to their Discord name in public would leak what they
  hold. The bot shows an address only to its owner, ephemerally.
  - The mint's holder list is public on-chain, so the bot can't fully prevent a
    match. That's why `/wallet link` recommends a fresh address made just for
    devnet (section 7), and why nothing ever announces a withdrawal publicly.
- **Heavy dependencies in the default build.**

## 1. Two repos and the seam between them

Mirrors how `crack-osint` and `crack-bf` were extracted (spec
`2026-09-18-optional-module-extraction-design.md`):

- **`cycle-five/crack-coin`** is a library crate with its own version line
  (starts at **v0.1.0**), CI, tests, `LICENSE` (MIT), `rustfmt.toml` (a copy of
  the workspace's), `.gitignore` and `rust-toolchain.toml`. It knows nothing
  about Discord, Postgres or credits. It knows Solana devnet. It obeys the
  five-rule contract: no workspace dependencies, its own `thiserror` error, no
  Discord types in its API, its own tests, no shared schema.
- **crack-core** depends on it as `crack-coin = { git =
  "https://github.com/cycle-five/crack-coin", tag = "v0.1.0", optional = true }`
  behind a crack-core feature `crack-coin = ["dep:crack-coin"]`.
  **crack-cli** (the `cracktunes` binary) forwards it:
  `crack-coin = ["crack-core/crack-coin"]`. A git dependency is not a git
  submodule (that would need a recursive checkout in CI, Docker and every
  worktree).
- **What lives in crack-core:**
  - the migration and the `db::coin` SQL. These are **not** feature-gated. They
    are crack-core's schema, so they compile in every build, the `.sqlx` offline
    cache always has their queries, and the normal db-tests CI run covers the
    code that moves credits;
  - the `/wallet` commands, the withdrawal flow and the resolver, all gated on
    `#[cfg(feature = "crack-coin")]`;
  - the `coin_withdrawals` switch in `EconomyConfig`. It is not gated:
    it is one bool, and lean builds simply never read it.

🔑 Cargo clones git dependencies even when they are never activated (learned
from crack-osint). So the lean image build fetches `crack-coin` too, and the
repo must be **public and tagged before** crack-core's `Cargo.toml` names it.

## 2. The `crack-coin` crate

### 2.1 Dependencies

- Raw JSON-RPC over **reqwest 0.12** (`default-features = false`, `rustls-tls`
  and `json`), the same major version the workspace already uses.
- **ed25519-dalek 2.2** for signing (already in the tree via serenity, songbird
  and jsonwebtoken).
- **curve25519-dalek 4**, for the on-curve check.
- **sha2 0.10**, to derive program addresses.
- **bs58** (small, and the only new dependency), plus serde, serde_json,
  thiserror and base64.

Using the solana-* crates was considered and rejected. Older versions pinned
ed25519-dalek 1.x and an ancient `zeroize`, which notoriously conflicts with
rustls in a shared graph. Even the split crates add a large tree to a feature
whose whole job is to send two instructions. The cost of the raw approach is
hand-encoding a legacy transaction, about 300 lines, and the tests in 2.6 pin
that encoding against an independent implementation.

### 2.2 Public API (shape, not signatures)

- `Pubkey` (32 bytes): parses from base58, displays as base58, and has
  `is_on_curve()`.
- `Signature` (64 bytes): displays as base58.
- `Keypair`: wraps `SigningKey`. It loads from the Solana CLI keypair format (a
  JSON array of 64 numbers: the secret half, then the public half) and rejects
  a file whose public half doesn't match its secret. **Its `Debug` shows only
  the public key, and it implements neither `Serialize` nor `Display`.**
  `SigningKey` zeroizes on drop.
- `Devnet` client: wraps a URL and a `reqwest::Client`, with a timeout on every
  call (about 10 s). It makes typed JSON-RPC calls:
  - `getGenesisHash`, `getBalance`, `getAccountInfo` (base64) and
    `getMinimumBalanceForRentExemption`;
  - `getLatestBlockhash` (returns the blockhash and `lastValidBlockHeight`) and
    `getBlockHeight` (finalized);
  - `sendTransaction` (base64), `getSignatureStatuses` with
    `searchTransactionHistory`, and `requestAirdrop`.
- `Mint` view: parses the 82-byte SPL mint account into mint authority, supply,
  decimals, is-initialized and freeze authority.
- Building a mint transaction: given the authority, the mint, an owner, an
  amount and a blockhash, return a `SignedTx { signature, wire_bytes }`. It
  holds two instructions:
  1. Associated Token Account `CreateIdempotent`: payer = authority, the owner's
     ATA, owner, mint, System program, Token program;
  2. Token `MintToChecked`: mint, the ATA, authority, the amount, decimals 0.

  The authority is both fee payer and mint authority (one key). It uses a
  legacy, not versioned, message.
- `associated_token_address(owner, mint)`: `find_program_address` over
  `[owner, token_program, mint]`, searching bumps from 255 down for the first
  off-curve hash.
- Building the metadata transaction (2.5), plus `metadata_address(mint)`:
  `find_program_address` over `["metadata", metadata_program, mint]`.
- A status type: `Landed`, `Failed(reason)`, `Pending` (the node has seen it
  but it isn't confirmed) or `NotFound`.

All JSON goes through typed serde structs, per the owner's global rule. That
includes request envelopes (params as tuples serialize as JSON arrays) and
responses, with JSON-RPC errors as a typed error struct. Never use
`serde_json::json!` or `Value`.

### 2.3 The CLI (feature `cli`, its own binary)

The repo ships a small `crack-coin` binary behind a `cli` feature, so crack-core
never pulls in its argument parser or tokio's macros. It has four subcommands:

- `keygen <path>` refuses to overwrite a file, writes the Solana CLI format with
  mode **0600**, and prints only the public key. It seeds from the OS
  (`getrandom`), not from a userland RNG.
- `airdrop <keypair> [sol]` requests devnet SOL.
- `create-mint <keypair>` creates the mint in one transaction:
  - System `CreateAccount` (82 bytes, rent-exempt, owned by the Token program),
    then `InitializeMint2` with decimals 0, the keypair as mint authority, and
    **no freeze authority**;
  - the mint's own keypair is generated, signs once, and is dropped;
  - it prints the mint address.
- `set-metadata <keypair> <mint> <uri>` names the mint (2.5). It creates the
  metadata account the first time and updates it after that, so it is safe to
  run twice.
- `info <keypair> <mint>` prints the authority's SOL balance, the mint's supply
  and decimals, whether its authority is this key, and the metadata's name,
  symbol and uri if present.

Each subcommand refuses mainnet, by the same check as in section 4.

### 2.4 What it costs to run, and keeping the tap from draining

- Each withdrawal pays a fee of about 0.000005 SOL.
- A holder's first withdrawal also creates their token account, about 0.002 SOL
  of rent.
- So 1 devnet SOL covers roughly 450 new holders.

**The drain, and the cap that stops it.** The rent is the only real cost, and
it can be farmed. A member can relink a fresh address before each withdrawal,
or close their token account (which pays the rent back to them) and withdraw
again. Either way the bot pays to create another account.

So each member gets **at most 3 sponsored token accounts, ever, per mint**
(a constant).
- Before charging, the command looks up whether the destination's token account
  exists (`getAccountInfo`).
- If it doesn't, and the member already has 3 `done` withdrawals that created
  one, it refuses. The copy says to link an address that already holds
  CrackCoin.
- Normal members never close accounts or relink three times, so they never meet
  the cap.
- The withdrawal row records `sponsored` (section 5).
- A member closing their account between the check and the send can slip one
  extra past the cap. That's bounded, and accepted.

**The tap:**
- A withdrawal is refused when the authority holds under 0.01 SOL, and that
  logs a warning.
- In coin builds, the startup line and `/economy`'s admin output show the
  balance, so a moderator sees a dry tap without reading logs.
- I refill it with the CLI's `airdrop` when asked. That's rate-limited, so for
  larger top-ups faucet.solana.com needs the owner's GitHub login.

### 2.5 The name and logo **(owner: in v1)**

Wallet apps read a fungible token's name, symbol and logo from **Metaplex Token
Metadata**, and that works on a classic SPL mint, so the mint design doesn't
change.

**On chain:**
- An account at the metadata address (2.2), owned by the Token Metadata
  program `metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s`, the same id on devnet
  and mainnet.
- It is created by `CreateMetadataAccountV3`, with:
  - mint authority = payer = update authority = our key;
  - `name` "CrackCoin", `symbol` "CRACK", `uri` pointing to the JSON below;
  - `seller_fee_basis_points` 0, and no creators, collection or uses;
  - **`is_mutable` true**, so the logo can change later.
- Running `set-metadata` again sends `UpdateMetadataAccountV2` instead.
- The builder confirms the instruction indices and the Borsh layout against
  Metaplex's current docs or IDL. Its fixture comes from
  `@metaplex-foundation/mpl-token-metadata` (2.6).

**Off chain, in the public crack-coin repo:**
- `assets/crackcoin.png`: derived from the owner's emblem. The source is 980×990
  RGBA with transparent corners at 1.6 MB, and wallets want a small square. So
  pad it to a square on transparency, resize to **512×512**, and compress, with
  a few hundred KB as the target. The source file is not committed.
- `assets/crackcoin.json`: the Metaplex fungible JSON, with `name`, `symbol`,
  a one-line `description` that says it is a worthless devnet token earned on
  CrackTunes, and `image`, the PNG's URL.
- Both URLs are `raw.githubusercontent.com` links on the **default branch**, so
  a logo tweak is a commit, not an on-chain update. That's my call over pinning
  a tag, since only we can push there.

**In the bot:** the copy calls the token "CrackCoin", as in "50 CrackCoin
minted". The bot doesn't read metadata at runtime, and its startup checks
(section 4) don't require it. A nameless mint still works, just less nicely.

### 2.6 Tests

- **Encoding is pinned against an independent implementation.** These are
  generated once from fixed keys and a fixed blockhash, then checked in as
  fixtures:
  - the mint transaction bytes, the ATA derivation and the mint layout, from
    `@solana/web3.js` and `@solana/spl-token` (or Python `solders`);
  - the metadata create and update instructions and the metadata address, from
    `@metaplex-foundation/mpl-token-metadata`. A comment says
  exactly how they were made, so they can be regenerated.
- Well-known vectors where they exist: a published owner/mint → ATA pair, and
  the System and Token program ids decoding to the expected bytes.
- **The key stays secret:** `format!("{keypair:?}")` holds no secret bytes, and
  loading a file with a mismatched public half fails.
- Wire tests for every RPC request body (the exact JSON) and response parse,
  including a JSON-RPC error and a `null` status.
- **Live devnet tests are `#[ignore]`** and run by hand: send a mint to a
  throwaway address, read back its status, and check that a mainnet URL is
  refused.

## 3. Configuration and secrets

**In the environment (read only by coin builds):**

| Variable | Contents | Secret |
|---|---|---|
| `CRACK_COIN_KEYPAIR` | The keypair, in the exact text of the Solana CLI file (`[12,34,…]`) | **yes** |
| `CRACK_COIN_MINT` | The mint address (base58) | no |
| `CRACK_COIN_RPC_URL` | Optional. Defaults to `https://api.devnet.solana.com` | no |

The env var holds the file's exact text, so the owner vaults a single thing and
it pastes back verbatim, with no conversion step that could print it.

**Setup, done once, by me unless marked:**

1. `crack-coin keygen ~/.config/crack-coin/devnet-authority.json` creates the
   key at mode 0600, outside any repo. Only the public key is printed.
2. The owner stores the file's content in Vaultwarden **(owner)**.
3. `airdrop`, then `create-mint`, which prints the mint address. Then
   `set-metadata` with the JSON's URL. The assets must be pushed to the repo
   first.
4. Wire the variables into `.env.tunetitan` by key name: written by a script
   that reads the file and appends `CRACK_COIN_KEYPAIR=…` without echoing it.
   Also add the three variables to the tunetitan compose service's explicit
   `environment:` list as optional (`${CRACK_COIN_KEYPAIR:-}`, not `:?`), so a
   lean image deploys without them. That change goes in a homelab PR.

If any variable is missing, a coin build starts with CrackCoin off and logs why
(section 4). It is never a startup failure: the bot's music must not depend on
devnet.

## 4. Startup checks: refuse mainnet, and be loud about setup

A coin build checks once at startup, in a supervised task, before `/wallet`
will work:

1. **Genesis hash.** If `getGenesisHash` returns mainnet-beta's
   (`5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d`; the builder confirms this
   against Solana's docs), CrackCoin stays off and logs an error. The genesis
   hash it sees is remembered for section 6.
2. **The mint.** The account exists, is an initialized SPL mint, has decimals
   0, and its mint authority is our key. Otherwise CrackCoin stays off and logs
   what didn't match.
3. **The tap.** Log the authority's SOL balance.

Then it logs one line: either `crack-coin: ready (mint …, authority …, N.NN
devnet SOL)` or `crack-coin: off (<reason>)`. On TuneTitan, `verify.sh` can
check for "ready" when the image tag ends in `-coin`.

If devnet is unreachable at startup, the task retries with backoff, and
`/wallet withdraw` says CrackCoin is starting up. `/wallet link`, `unlink` and
`show` work as soon as the database is there.

## 5. Data (one additive crack-core migration)

```sql
-- Who withdraws where. One address per Discord user, used in every server.
CREATE TABLE coin_links (
    user_id   BIGINT PRIMARY KEY,
    address   TEXT NOT NULL,          -- base58, validated on the way in
    linked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE coin_withdrawals (
    id                      BIGSERIAL PRIMARY KEY,
    guild_id                BIGINT NOT NULL,
    user_id                 BIGINT NOT NULL,
    address                 TEXT   NOT NULL,   -- snapshot: a relink doesn't move it
    mint                    TEXT   NOT NULL,   -- snapshot of CRACK_COIN_MINT
    amount                  BIGINT NOT NULL CHECK (amount > 0),
    state                   TEXT   NOT NULL DEFAULT 'pending',  -- pending | done | refunded
    signature               TEXT,   -- base58; stored BEFORE the transaction is sent
    tx                      BYTEA,  -- the signed transaction, for rebroadcast
    last_valid_block_height BIGINT,
    genesis_hash            TEXT,
    sponsored               BOOLEAN NOT NULL DEFAULT false,  -- this tx creates the token account (2.4)
    reason                  TEXT,   -- why it was refunded, for the member and for us
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at                TIMESTAMPTZ
);

-- One withdrawal in flight per member, across all servers.
CREATE UNIQUE INDEX coin_withdrawals_one_pending
    ON coin_withdrawals (user_id) WHERE state = 'pending';

ALTER TABLE economy_ledger
    ADD COLUMN coin_withdrawal_id BIGINT REFERENCES coin_withdrawals (id);
```

- **Follow the migration conventions:** `CREATE TABLE IF NOT EXISTS` (and
  `IF NOT EXISTS` on the index and column), and a mirror copy in
  `crack-core/test_migrations/`, as every existing migration has.
- The ledger gains two kinds: `coin_withdraw` (the debit) and `coin_refund`.
  They need labels in `ledger_label` for `/credits history`, which otherwise
  shows a kind raw. The dashboard renders no ledger kinds.
- **The coin SQL reuses the money layer's lock.** `Locked`, `lock_wallet`,
  `set_balance` and `write_ledger` are private to `db::economy` today. Either
  `db::coin` becomes a child module of it, or they become `pub(crate)`. Either
  way, `Locked` keeps private fields, so only `lock_wallet` can make one.
- Withdrawals get **their own table, deliberately not `economy_actions`**. The
  economy's reconcile refunds `pending` action rows it finds at startup. A
  withdrawal whose transaction is in flight would be refunded and minted.
  Keeping them apart means that code can never see one.
- An index on `coin_withdrawals (state)` for the resolver is left to the
  builder.

## 6. A withdrawal's life, and the rule that keeps money whole

### 6.1 The flow

`/wallet withdraw <amount>`. All replies are ephemeral (anti-goal: addresses).

1. **Refuse before charging** (every refusal comes before any debit, as in PR
   2):
   - CrackCoin is off or not ready;
   - the economy is off in this server, or `coin_withdrawals` is off;
   - the member is opted out (out means out, as with gifts);
   - no linked address;
   - an amount below 1;
   - the authority holds under 0.01 SOL. Refuse with "the tap is dry" and log a
     warning;
   - the destination has no token account and the member has used their 3
     sponsored accounts (2.4).

   These are all checked in the command. Insufficient credits and an existing
   pending withdrawal are refused by the database in step 2.
2. **Charge.** One DB transaction locks the wallet (the `Locked` rule),
   debits `amount`, inserts the `coin_withdrawals` row as `pending` with the
   address and mint snapshotted, and writes the `coin_withdraw` ledger row. The
   unique index turns a second in-flight withdrawal into a refusal. Commit.
3. **Reply** with a placeholder: "Minting N to `ABCD…WXYZ`…".
4. **Prepare.** `getLatestBlockhash`, then build and sign. The signature is now
   known, before anything is sent.
5. **Record.** `UPDATE … SET signature, tx, last_valid_block_height,
   genesis_hash, sponsored WHERE id = $1 AND state = 'pending' AND signature IS
   NULL`. If
   no row moved, the row was resolved in the meantime (step 2 of 6.2's orphan
   rule): **do not send**, and say the withdrawal failed and the credits are
   back.
6. **Send** the stored bytes. Then poll `getSignatureStatuses` every ~2 s,
   resending the same bytes on the same cadence (Solana's recommended pattern;
   it is idempotent because the signature is the same), until the resolver
   below decides.
7. **Finish.**
   - On success, edit the reply: "N minted to `ABCD…WXYZ`", with a link to the
     transaction on explorer.solana.com (`?cluster=devnet`) and a line on how to
     see devnet tokens in a wallet app.
   - On a refund: "It didn't land, so your N credits are back" plus the
     reason.

🔑 **Type-enforce the order** (the owner prefers types to source scans; see
`Locked` and `JoinPermit`). The function that sends must accept only a value
produced by a successful step 5, so "send before recording" cannot compile.
The builder picks the mechanism.

### 6.2 The resolver: when a pending row may end

One pure function decides, and the in-command poll and the sweeper both call it:

| The row | The chain says | Decision |
|---|---|---|
| no `signature` | — | **Refund** ("never sent") only if it is an orphan: created before this process started (the DB-clock process start the economy already records), or more than 2 minutes old. Otherwise **wait** |
| has `signature` | status found, no error, confirmed or finalized | **Done** |
| has `signature` | status found **at confirmed or finalized**, with an error | **Refund** ("failed on chain"); the fee was paid but nothing was minted. An error seen only at `processed` counts as *processed*. It may sit on a minority fork while a resend of the same bytes lands elsewhere |
| has `signature` | status found, only processed | **Wait** |
| has `signature` | not found, and the genesis hash now differs from the row's | **Refund** ("devnet was reset"; that chain is gone) |
| has `signature` | not found, and the **finalized** block height > `last_valid_block_height` + 150 | **Refund** ("expired"); it can never land. The height is read **before** the status, and the 150-block margin (about a minute) covers a load-balanced RPC pool answering the two calls from nodes at different heights |
| has `signature` | not found, not expired | **Wait** (and resend the stored `tx`) |
| any | an RPC error or timeout | **Wait** |

- Checking expiry against the *finalized* height is conservative. Finalized
  lags, so a refund happens only once the blockhash is provably dead.
- **A send error does not refund on the spot.** Even a preflight rejection waits
  for expiry, about 135-140 s, which is why the command polls for 180 s (amended: R18). It is slower on a rare path, but it is the one rule
  that is safe without reasoning about which RPC errors mean "not forwarded".
- The no-signature orphan rule is safe because of 6.1 step 5, **and** because
  the refund itself re-checks that the row is still unsigned (6.3).
  - A row that has no signature was never sent. Once the sweeper ends it, the
    in-flight task can no longer record or send.
  - The other direction needs the guard. A sweeper that read the row as
    unsigned could otherwise refund it just after the task recorded and sent
    it, and both the mint and the refund would happen. That hole was found in
    review.

### 6.3 Ending a row

- **Done:** `UPDATE … SET state = 'done', ended_at = now() WHERE id = $1 AND
  state = 'pending'`.
- **Refund:** one transaction that moves the row to `refunded` with its reason
  **only if** the guarded `UPDATE` moved it, then locks the wallet, credits
  `amount` back, and writes a `coin_refund` ledger row. If two resolvers race,
  only one moves the row and credits.
  - **A "never sent" refund adds `AND signature IS NULL` to that guard.** If
    the row was recorded since the resolver read it, the refund moves nothing,
    and the row is settled later from the chain.
  - The chain-proven refunds (failed, reset, expired) arise only for signed
    rows, and a signature never changes once set, so their plain `pending`
    guard is enough.
- A refund needs no Discord, so it works in a server the bot has left.

### 6.4 The sweeper

A coin-build-only supervised task (`effects::spawn_supervised`) wakes every
~30 s and runs the resolver over every `pending` row, across all servers. It
covers crashes, a poll that died, and devnet being down for a while. The member
learns the outcome from `/wallet show` ("your last withdrawal: …"). The original
reply is not edited after the command's own poll ends; my call: no DMs.

While a row is pending, the member cannot start another withdrawal, and those
credits are held. If devnet is down for hours, they stay held. **Nothing ever
refunds without the chain's proof**, apart from the never-sent orphan.

## 7. Commands and copy

`/wallet` is guild-only, and every reply is ephemeral:

| Command | Does |
|---|---|
| `/wallet show` | The linked address (shortened, with an explorer link), the mint, the total withdrawn across servers, and the state of the last withdrawal |
| `/wallet link <address>` | Validates and stores the address (replacing any earlier one). Explains devnet in one line, and recommends a fresh address made just for devnet, since the holder list is public on-chain |
| `/wallet unlink` | Deletes the link. A pending withdrawal keeps its snapshot |
| `/wallet withdraw <amount>` | Section 6 |

**Validating an address:**
- base58 that decodes to 32 bytes;
- **on-curve.** An off-curve address is usually a token account or a program
  address pasted by mistake, so the copy says to paste the wallet's own
  address;
- not the mint itself.

**Other rules:**
- User-facing strings are constants in `messaging/messages.rs` (the owner's
  localization convention, #537). Logs stay put.
- `/wallet` sits in `economy::commands()` under the cfg, so it registers along
  with the rest of the economy, and only in coin builds.
- `/economy` admin output gains a "CrackCoin withdrawals: on/off" line in coin
  builds.

## 8. The per-server switch

- `EconomyConfig` gains `coin_withdrawals: bool`, default `false`. It goes
  through serde with the existing `#[serde(default)]` and needs no migration.
  On by itself it does nothing: the economy must also be enabled.
- On the dashboard, the economy page shows a "CrackCoin withdrawals" checkbox
  only when the running build has CrackCoin. crack-core exposes
  `pub const COIN_BUILD: bool = cfg!(feature = "crack-coin")`. crack-web is
  linked into the same binary, so feature unification makes the constant
  true there too.
  - crack-web reads it in one place and passes it into rendering as a plain
    `bool` parameter. That way both branches are testable in any build, which
    section 9's CI relies on.
- The switch flows through the economy settings view, form, `page.rs`, and
  `assets/settings.js`, beside `gambling_enabled`.
- A lean build saving the page keeps the field, as it does today for any field
  it doesn't show.

## 9. Build, CI, images, deploy

- **Dockerfile:**
  - adds `ARG CARGO_FEATURES=""`, used by both `cargo chef cook` and
    `cargo build` (the two must match, or the cook is wasted);
  - the lean default is unchanged;
  - the builder confirms that an empty `--features ""` is accepted, or builds
    the flag conditionally.
- **docker.yml:** on **tag pushes only**, a second runner image builds with
  `CARGO_FEATURES=crack-coin`:
  - it is tagged with metadata-action `flavor: suffix=-coin`, which gives
    `ghcr.io/cycle-five/cracktunes:v0.35.0-coin`;
  - it has its own GHA cache scope;
  - it is skipped on master pushes and PRs, since only TuneTitan pulls it, and
    only for a release;
  - the migrate image is untouched and the same for both.
- **lint.yml:** the "optional module seam" step adds `crack-coin`:
  - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`;
  - plus a `cargo clippy -p crack-core --features crack-coin -- -D warnings`.
- **build.yml:** its existing postgres-backed test run adds `crack-core/crack-coin`
  to its features.
  - That is where the fake-chain flow tests run: they need both a database and
    the feature.
  - It costs no extra compile.
  - The lean configuration stays compiled by lint.yml's featureless clippy and
    the lean image build.
  - **The rule that makes this safe:** no logic that needs a test sits behind
    `cfg(not(feature = "crack-coin"))`. Whatever differs between builds is
    passed in as a value, like the dashboard's `bool` in section 8.
- **The order across the two repos:**
  1. crack-coin is built, CI is green, then tagged **v0.1.0** (signed). Nothing
     creates its release, so it gets a hand-cut `gh release create` (global
     cadence, case B). Later changes there go through PRs.
  2. crack-core pins the tag in the `feat/crack-coin` PR.
  3. If review finds a crack-coin bug, it ships as crack-coin v0.1.1 and the
     pin moves.
- **cracktunes v0.35.0** runs the normal cadence: PR, merge, signed tag. cargo-dist
  owns the release. The tag push also builds both images.
- **TuneTitan:**
  - `TUNETITAN_CRACKTUNES_IMAGE=…:v0.35.0-coin` and
    `TUNETITAN_CRACKTUNES_MIGRATE_IMAGE=…:v0.35.0`;
  - the three CRACK_COIN variables (section 3);
  - `./homelab.sh up tunetitan`, then `verify tunetitan`;
  - the owner turns `coin_withdrawals` on in a test server's dashboard.
- **Production** stays lean and is untouched until the soak ends.

## 10. Testing

Follows the standing rules: sabotage every test (each must be seen to fail),
assert on what is sent, and use `Utc::now()` in any db test whose row the
database compares with `now()`.

- **The resolver** is a pure function, tested exhaustively over the 6.2 table,
  including the orphan boundary (before and after process start, and 2 minutes
  either side).
- **db-tests:**
  - charge, then done;
  - charge, then refund, which restores the balance and writes both ledger rows;
  - a second refund, and a refund after done, move nothing and credit nothing;
  - two racing refunds credit once;
  - a second pending withdrawal for the same user is refused, even in another
    server;
  - recording a signature on a row already refunded moves nothing;
  - insufficient credits are refused with the balance and write nothing;
  - the sponsored count includes only `done` rows that created an account. A
    refunded sponsored row doesn't use up one of the 3.
- **The sponsored-account cap (2.4), in the flow tests:** the fourth new
  account is refused before any charge, and a withdrawal to an existing account
  passes the cap.
- **The flow, against a fake chain:** the glue talks to the chain through a
  trait that crack-coin's client implements. The fake scripts these cases:
  - lands on the first poll;
  - fails on chain;
  - is never seen and expires;
  - the RPC is down, then comes back;
  - devnet is reset;
  - "crash" after recording and before sending: the sweeper sees the row and
    the fake reports it not found until expiry;
  - the sweeper refunds a stuck no-signature row, after which the late task's
    record fails and nothing is sent.

  Each case asserts what was sent to the chain and the final balance.
- **Address validation:** good, too short, bad base58, off-curve (a real ATA),
  and the mint itself.
- **Config:** a stored config without `coin_withdrawals` reads as false.
- **Live, by hand on TuneTitan** (in the PR body's checklist):
  - link;
  - withdraw 1 and see it in Phantom on devnet as **CrackCoin, with the
    emblem**;
  - withdraw more than the balance;
  - withdraw with the switch off;
  - a second withdraw while one is pending;
  - unlink;
  - check the `crack-coin: ready` log line.

## 11. Rollback

- The migration is additive, so rolling back the bot image is image-only. The
  migrate image STAYS at v0.35.0: the v0.34.0 sqlx migrate image stops with
  `VersionMissing` when it meets a migration it doesn't know (amended: final
  review).
- **Before rolling TuneTitan back to a lean image or below v0.35.0:**
  `SELECT count(*) FROM coin_withdrawals WHERE state = 'pending'` must return 0.
  A lean build has no resolver, so a pending row's credits would stay held
  until a coin build returns. They are not lost.
- Tokens already minted stay minted. They are worthless, which is the point.

## 12. Left to the builder

- Module layout inside crack-coin, and argument parsing for the CLI (by hand,
  or clap behind `cli`).
- The mechanism behind 6.1's type rule.
- Whether to cache the authority's SOL balance for the "tap is dry" check (for
  example for 60 s) or read it per withdrawal.
- The exact poll and resend cadence, and the sweeper interval, within the
  spirit of ~2 s and ~30 s.
- Copy wording, within the conventions above.
- The default branch name and CI matrix of the new repo (match crack-osint).
- An index on `coin_withdrawals (state)`.

## Follow-ups (not v1)

- Signature-proven wallet links (sign a nonce on the dashboard).
- Deposits: tokens back into credits.
- **Deliberately not a follow-up: a public "X cashed out N" announcement.**
  Combined with the public on-chain holder list, it would match a Discord name
  to an address by amount and time.
