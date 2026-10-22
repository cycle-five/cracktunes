# CrackCoin (economy PR 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** members withdraw server credits as **CrackCoin**, a worthless-by-design Solana devnet
token, 1:1, through `/wallet`. The feature ships as **v0.35.0**, and runs on TuneTitan from a
separate `-coin` image.

**Architecture:** two repos, in order.
- **Part A** builds the public crate `cycle-five/crack-coin` (v0.1.0):
  - Solana devnet over raw JSON-RPC, with transactions hand-encoded and signed with ed25519-dalek;
  - a CLI that makes the key and the mint, and names the mint through Metaplex metadata.
- **Part B** wires it into crack-core behind a `crack-coin` feature:
  - the SQL that moves credits is **not** gated, so the normal db-tests cover it;
  - the chain seam, the resolver, the sweeper and `/wallet` **are** gated.

  Part B then ships the tag, both images and the TuneTitan deploy.

**Tech Stack:**
- **crate:** Rust 2021, reqwest 0.12 (rustls, json), ed25519-dalek 2.2, curve25519-dalek 4.1,
  sha2 0.10, bs58 0.5, base64 0.22, thiserror 2, serde/serde_json, and tokio + getrandom 0.3 for
  the CLI only;
- **fixtures:** Node 26 with `@solana/web3.js` 1.x, `@solana/spl-token` 0.4.x and
  `@metaplex-foundation/mpl-token-metadata` 3.x (umi);
- **bot:** the crack-core stack as before (poise, sqlx 0.9/Postgres, axum for crack-web), with
  async-trait.

**Spec:** `docs/superpowers/specs/2026-10-21-crack-coin-design.md`, approved by the owner
2026-10-21. Read it before any task. **Section 6, the money rule, is the heart of it.**

## Global Constraints

- **Never mainnet.** These are the genesis hashes. Confirm them against Solana's docs in Task A4:
  - `MAINNET_GENESIS = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"` is refused;
  - `DEVNET_GENESIS = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG"`.

  The **only** constructor of the RPC client is `Devnet::connect`, which refuses mainnet.
- **Program ids:**
  - System `11111111111111111111111111111111`;
  - Token `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`;
  - Associated Token `ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL`;
  - Token Metadata `metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s`.
- **The token:**
  - name `"CrackCoin"`, symbol `"CRACK"`, decimals **0**, no freeze authority;
  - metadata `is_mutable = true`, seller fee 0, no creators, collection or uses;
  - metadata uri `https://raw.githubusercontent.com/cycle-five/crack-coin/master/assets/crackcoin.json`;
  - image `https://raw.githubusercontent.com/cycle-five/crack-coin/master/assets/crackcoin.png`.
- **Default RPC:** `https://api.devnet.solana.com`.
- **Environment** (read only by coin builds):
  - `CRACK_COIN_KEYPAIR`, the Solana CLI JSON text (`[n,…]`, 64 numbers), trimmed before parsing;
  - `CRACK_COIN_MINT`, base58;
  - `CRACK_COIN_RPC_URL`, optional.
- **Fixed values** (consts, not settings):
  - `SPONSORED_ACCOUNTS = 3`, per member per mint, counting `done` rows that had `sponsored = true`;
  - `MIN_TAP_LAMPORTS = 10_000_000` (0.01 SOL);
  - `ORPHAN_AFTER = 120 s`;
  - `POLL_EVERY = 2 s`, `POLL_FOR = 180 s` (amended: R18);
  - `SWEEP_EVERY = 30 s`;
  - `RPC_TIMEOUT = 10 s`;
  - the startup retry backoff is `5 s · 2^n`, capped at `300 s`.
- **Log lines, exact** (TuneTitan's verify greps the first):
  - `crack-coin: ready (mint {mint}, authority {authority}, {sol:.2} devnet SOL)`;
  - `crack-coin: off ({reason})`.
- **The signing key is never printed.** Not in logs, `Debug`, `Display`, replies, test output,
  shell output or commit text.
  - Its file is `~/.config/crack-coin/devnet-authority.json`, mode 0600, in a mode-0700
    directory.
  - Never `cat` it, never echo `CRACK_COIN_KEYPAIR`, and never `bash -n` or `set -x` a file that
    holds it.
  - Read it only through `"$(< file)"` inside a command whose output is the public key or nothing.
- **Typed serde only** (the owner's global rule). Never `serde_json::json!`; never `Value` for data
  we own. JSON-RPC params are tuples or arrays of typed structs. A chain error `err` payload is
  opaque third-party data: keep it as `Box<serde_json::value::RawValue>` and render it, never
  pluck fields from it.
- **The money rule** (spec 6):
  - a withdrawal mints or refunds, never both and never neither;
  - every transition is guarded `WHERE state = 'pending'`;
  - a refund credits **only** when its guarded `UPDATE` moved the row;
  - a send needs a `Recorded`, which only a successful `record_sent` makes.
- **Privacy:**
  - every `/wallet` reply is ephemeral;
  - an address appears only to its owner, shortened to `ABCD…WXYZ` (first 4, `…`, last 4);
  - nothing announces a withdrawal.
- **The cfg split:**
  - `db::economy::coin`, the migration, `LedgerKind`s, `EconomyConfig::coin_withdrawals` and the
    dashboard switch are **ungated**;
  - `crate::coin`, `commands/economy/wallet.rs` and the registry's coin field are
    `#[cfg(feature = "crack-coin")]`;
  - **no logic that needs a test sits behind `cfg(not(feature = "crack-coin"))`.** Pass a
    `bool` instead (`crack_core::COIN_BUILD`).
- **Repo rules, crack-core** (as every recent plan):
  - strings are `pub const` in `crack-core/src/messaging/messages.rs`;
  - sends go through `messaging::courier` only (`reply_rendered` / `edit_rendered`);
  - ids are parsed defensively;
  - migrations go in BOTH `migrations/` and `crack-core/test_migrations/` (identical,
    `IF NOT EXISTS`);
  - `.sqlx` is regenerated with `scripts/sqlx_sync.py`, following its header recipe, after
    `sqlx migrate run --source migrations/` on a fresh local DB. Never `cargo sqlx prepare`;
  - never `git add -A`;
  - the commit trailer is exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`;
  - sabotage every new test (record a mutation → caught-by row in the report);
  - assert on what is sent;
  - no source-scan tests;
  - no stray background processes;
  - use `Utc::now()` in any db test whose row the DB compares with `now()`.
- **Local postgres** for db-tests. A ctor forces
  `DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres`.
  - The container `pg` is usually up. Otherwise start it with
    `docker --context default run -d --rm --name pg -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine`.
  - 🪤 Bare `docker` targets a remote host.
- **Gate per Part B task**, from `/home/lothrop/projects/cracktunes-economy`, with
  `CARGO_TARGET_DIR=/home/lothrop/projects/cracktunes/target`:
  1. `cargo fmt --all -- --check`;
  2. `cargo clippy --all --locked -- -D clippy::all -D warnings --allow clippy::needless_return`;
  3. the same clippy with `--features crack-core/crack-coin`;
  4. `cargo test --workspace --locked --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`.

  Before Task B2 adds the feature, drop the coin flags.
- **Gate per Part A task**, in `/home/lothrop/projects/crack-coin`:
  - `cargo fmt --all -- --check`;
  - `cargo clippy --all-targets --all-features -- -D warnings`;
  - `cargo test --all-features`.
- **Shell:** the Bash tool runs zsh. Use `bash <<'EOF'` with `set -euo pipefail` for multi-step
  scripts. No `for f in $files`.
- **Never build Docker images locally.** Images build in CI.

## Review Focus

These five inputs are implied by the spec but untested by any section's own list. Each has its
test added to the task that owns it:

1. **A pasted address with surrounding whitespace or a trailing newline.** It is accepted and
   stored canonical. (Task B6, `check_address`.)
2. **A `CRACK_COIN_KEYPAIR` value pasted from the vault with a trailing newline.** It parses, and
   no error text ever contains the value. (Task A2 `Keypair::from_json`; Task B2 config.)
3. **devnet rate-limiting (HTTP 429) or timing out mid-settle.** That reads as *unreachable*, so
   the flow waits and never refunds until expiry is provable. (Task B3.)
4. **Relinking while a withdrawal is pending.** The mint still goes to the snapshotted address.
   (Task B3.)
5. **An absurd amount** (`i64::MAX`). It is refused as `Insufficient` with the balance, and nothing
   panics converting to `u64`. (Task B1, plus `check_withdraw_local` in B6.)

---

# Part A — the `crack-coin` crate (`/home/lothrop/projects/crack-coin`)

The repo is new and lives outside the cracktunes checkout, so it inherits no `rustfmt.toml` or
`clippy.toml` (see the memory "extracting a crate").
- Task A1 commits on `master`.
- Tasks A2–A5 commit on branch `feat/v0.1.0`.
- Task A6 publishes.

**Crate layout** (`src/`):

| File | Holds |
|---|---|
| `lib.rs` | module wiring, `Error`, re-exports, `NAME`, `SYMBOL`, `METADATA_URI` |
| `key.rs` | `Pubkey`, `Signature`, `Hash`, `Keypair` |
| `address.rs` | program-id consts, `find_program_address`, `associated_token_address`, `metadata_address` |
| `tx.rs` | `AccountMeta`, `Instruction`, the legacy message compiler, `sign_transaction`, `SignedTx` |
| `ix.rs` | the six instruction builders, `TokenInfo` |
| `rpc.rs` | `Devnet`, typed requests and responses, `TxStatus` |
| `cluster.rs` | genesis consts, `refuse_mainnet`, `DEFAULT_RPC_URL` |
| `mint.rs` | `MintAccount::parse`, `MINT_ACCOUNT_LEN` |
| `coin.rs` | `Coin`, `PreparedMint`, `create_mint`, `set_metadata`, `wait_for` |
| `bin/crack-coin.rs` | the CLI (`required-features = ["cli"]`) |

### Task A1: Scaffold the repo and generate the fixtures

**Files:**
- Create in `/home/lothrop/projects/crack-coin`: `Cargo.toml`, `LICENSE`, `README.md`,
  `rustfmt.toml`, `.gitignore`, `rust-toolchain.toml`, `.github/workflows/ci.yml`, `src/lib.rs`,
  `fixtures/gen/package.json`, `fixtures/gen/package-lock.json`, `fixtures/gen/gen.mjs`,
  `tests/fixtures/vectors.json`, `tests/common/mod.rs` and `tests/fixtures_parse.rs`.

**Interfaces:**
- Produces `tests/common/mod.rs`: `pub fn vectors() -> Vectors`, typed serde over
  `vectors.json`. Every later crate test reads its fixtures through it.
- The fixture keys are seeds `[1u8; 32]` (authority), `[2u8; 32]` (owner) and `[3u8; 32]` (mint).
  The blockhash is the base58 of `[4u8; 32]`.

- [ ] **Step 1: Scaffold.**
  - `git init -b master`.
  - `Cargo.toml`:
    - package `crack-coin` `0.1.0`, edition 2021, license MIT, authors
      `["Cycle Five <cycle.five@proton.me>"]`, and repository/homepage like crack-osint's;
    - dependencies as in the Tech Stack, with reqwest `default-features = false`,
      `features = ["json", "rustls-tls"]`, and serde_json with `raw_value`;
    - `[features] cli = ["dep:tokio", "dep:getrandom"]`;
    - `[[bin]] name = "crack-coin"`, `required-features = ["cli"]`;
    - dev-dependency tokio with `macros` and `rt-multi-thread`.
  - `rustfmt.toml`: copy cracktunes' `rustfmt.toml` byte for byte.
  - `.gitignore`: `/target/` and `/fixtures/gen/node_modules/`.
  - `rust-toolchain.toml`: `channel = "stable"`.
  - `LICENSE`: MIT, "Cycle Five".
  - `ci.yml`: crack-osint's, but clippy and test run with `--all-features`.
  - `README.md`: what it is (worthless devnet token, the mainnet refusal), the CLI subcommands, and
    how to regenerate fixtures.
- [ ] **Step 2: Write `fixtures/gen/gen.mjs`.**
  - Pin exact package versions in `package.json` and commit the lock.
  - It writes `tests/fixtures/vectors.json` with:
    - `programs`: the four program ids as base58 strings;
    - `keys`: per seed, `seed_hex` and `pubkey` (`Keypair.fromSeed`);
    - `blockhash`;
    - `on_curve`: `[{pubkey, on_curve}]`, holding the three keys, the ATA and the metadata address
      (`PublicKey.isOnCurve`);
    - `ata`: `{owner, mint, address, bump}`, the owner's ATA for the mint
      (`getAssociatedTokenAddressSync`, plus `PublicKey.findProgramAddressSync` for the bump);
    - `metadata_address`: `{mint, address, bump}`, seeds `["metadata", program, mint]`;
    - `mint_tx`: `{amount: 500, message_hex, signature, wire_hex}`. It has two instructions:
      1. `createAssociatedTokenAccountIdempotentInstruction(authority, ata, owner, mint)`;
      2. `createMintToCheckedInstruction(mint, ata, authority, 500, 0)`.

      Compile with `new TransactionMessage({payerKey: authority, recentBlockhash, instructions})
      .compileToLegacyMessage()`, serialize, sign with tweetnacl over the message bytes, and wire
      as shortvec(sig count) ‖ sigs ‖ message;
    - `create_mint_tx`: `{lamports: 1461600, message_hex, signatures: [...], wire_hex}`. It holds
      `SystemProgram.createAccount` (82 bytes, owner Token), then
      `createInitializeMint2Instruction(mint, 0, authority, null)`. It is signed by the authority,
      then the mint keypair, in the message's signer order;
    - `metadata_create_ix` and `metadata_update_ix`: each `{program, accounts: [{pubkey,
      is_signer, is_writable}], data_hex}`. They come from `createMetadataAccountV3` and
      `updateMetadataAccountV2` (umi, `createNoopSigner` for the authority), using name, symbol
      and uri from the Global Constraints. Update passes `data: some(...)`,
      `newUpdateAuthority: none()`, `primarySaleHappened: none()` and `isMutable: none()`.
- [ ] **Step 3:** Run `cd fixtures/gen && npm ci && node gen.mjs`. Expected: the command writes
  `tests/fixtures/vectors.json`, and `git diff --stat` shows it.
- [ ] **Step 4: Write the failing test** `tests/fixtures_parse.rs`:

  ```rust
  #[test]
  fn fixtures_parse_and_name_the_four_programs() {
      let v = common::vectors();
      assert_eq!(v.programs.token, "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
      assert_eq!(v.programs.metadata, "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
      assert_eq!(v.mint_tx.amount, 500);
      assert!(!v.on_curve.is_empty());
  }
  ```
- [ ] **Step 5:** Implement `Vectors` (typed serde structs mirroring the JSON) in
  `tests/common/mod.rs`. Run the Part A gate. Expected: PASS.
- [ ] **Step 6: Commit** on `master`, `chore: scaffold crack-coin and its fixtures`.

### Task A2: Keys and addresses

**Files:** create `src/key.rs` and `src/address.rs`, and modify `src/lib.rs` (`Error`). Tests go in
`tests/keys.rs` and `tests/addresses.rs`.

**Interfaces (produces):**
```rust
#[derive(Debug, thiserror::Error)] pub enum Error {
    Base58, Length { expected: usize, got: usize }, BadKeypair, NoSigner,
    Rpc { code: i64, message: String }, Http(String), Decode(String),
    Mainnet, MintMismatch(&'static str), Io(String),
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)] pub struct Pubkey(pub [u8; 32]);   // FromStr, Display = base58, Debug = base58
impl Pubkey { pub fn is_on_curve(&self) -> bool }
#[derive(Clone, Copy, PartialEq, Eq)] pub struct Signature(pub [u8; 64]);      // FromStr, Display = base58
#[derive(Clone, Copy, PartialEq, Eq)] pub struct Hash(pub [u8; 32]);           // FromStr, Display = base58
pub struct Keypair(/* ed25519_dalek::SigningKey */);
impl Keypair {
    pub fn from_seed(seed: [u8; 32]) -> Keypair;
    pub fn from_json(text: &str) -> Result<Keypair, Error>;   // trims; 64 numbers; public half must match
    pub fn secret_json(&self) -> String;                      // only keygen uses it
    pub fn pubkey(&self) -> Pubkey;
    pub fn sign(&self, msg: &[u8]) -> Signature;
}
pub const SYSTEM_PROGRAM: Pubkey; pub const TOKEN_PROGRAM: Pubkey;
pub const ASSOCIATED_TOKEN_PROGRAM: Pubkey; pub const TOKEN_METADATA_PROGRAM: Pubkey;
pub fn find_program_address(seeds: &[&[u8]], program: &Pubkey) -> (Pubkey, u8);
pub fn associated_token_address(owner: &Pubkey, mint: &Pubkey) -> Pubkey;
pub fn metadata_address(mint: &Pubkey) -> Pubkey;
```
`Keypair`'s `Debug` prints `Keypair(<pubkey>)` and nothing else. It implements neither `Display`
nor `Serialize`. `Error`'s messages never include input text.

- [ ] **Step 1: Write the failing tests.**
  - `program_ids_display_as_their_base58`: each const's `to_string()` equals `vectors.programs.*`.
  - `pubkey_round_trips_base58`, and `pubkey_refuses_bad_base58_and_wrong_length`:
    `"0OIl"` gives `Error::Base58`; a 31-byte key gives `Length { expected: 32, got: 31 }`.
  - `on_curve_matches_web3`: for every `vectors.on_curve` entry,
    `is_on_curve() == entry.on_curve`.
  - `ata_matches_spl_token`: `associated_token_address(owner, mint)` equals `vectors.ata.address`,
    and `find_program_address(...).1 == vectors.ata.bump`.
  - `metadata_address_matches_mpl`, the same way.
  - `keypair_reads_the_cli_format`: JSON built from `seed ‖ pubkey` gives `pubkey() ==
    vectors.keys.authority.pubkey`.
  - `keypair_trims_a_pasted_newline`: the same JSON plus `"\n"` parses.
  - `keypair_refuses_a_mismatched_public_half`: the last byte flipped gives `Err(BadKeypair)`.
  - `keypair_debug_shows_only_the_public_key`: `format!("{kp:?}") == format!("Keypair({})",
    kp.pubkey())`.
  - `keypair_errors_never_echo_the_input`: an invalid input containing the marker `"7777"` gives
    an error whose `to_string()` doesn't contain `"7777"`.
- [ ] **Step 2:** Run the Part A gate. Expected: FAIL (the items aren't defined).
- [ ] **Step 3: Implement.**
  - On-curve: `CompressedEdwardsY(bytes).decompress().is_some()`.
  - PDA: for bump in 255 down to 0, take `sha256(seeds… ‖ [bump] ‖ program ‖
    b"ProgramDerivedAddress")`, and the first result that is **not** on-curve wins.
  - The ATA seeds are `[owner, TOKEN_PROGRAM, mint]` under `ASSOCIATED_TOKEN_PROGRAM`.
  - Program consts are byte arrays, which the display test checks.
- [ ] **Step 4:** Run the gate. Expected: PASS. Then sabotage the bump order (0 upward) and see
  `ata_matches_spl_token` fail.
- [ ] **Step 5: Commit** on `feat/v0.1.0`, `feat: keys, program ids and derived addresses`.

### Task A3: Transactions and instructions

**Files:** create `src/tx.rs` and `src/ix.rs`. Tests go in `tests/transactions.rs`.

**Interfaces:**
- Consumes the A2 types.
- Produces:
  ```rust
  pub struct AccountMeta { pub pubkey: Pubkey, pub is_signer: bool, pub is_writable: bool }
  pub struct Instruction { pub program_id: Pubkey, pub accounts: Vec<AccountMeta>, pub data: Vec<u8> }
  pub struct SignedTx { pub signature: Signature /* the fee payer's */, pub wire: Vec<u8> }
  pub fn sign_transaction(signers: &[&Keypair], ixs: &[Instruction], blockhash: &Hash) -> Result<SignedTx, Error>;
  pub struct TokenInfo<'a> { pub name: &'a str, pub symbol: &'a str, pub uri: &'a str }
  pub fn create_ata_idempotent(payer: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> Instruction;
  pub fn mint_to_checked(mint: &Pubkey, dest: &Pubkey, authority: &Pubkey, amount: u64, decimals: u8) -> Instruction;
  pub fn create_account(from: &Pubkey, new: &Pubkey, lamports: u64, space: u64, owner: &Pubkey) -> Instruction;
  pub fn initialize_mint2(mint: &Pubkey, decimals: u8, mint_authority: &Pubkey, freeze: Option<&Pubkey>) -> Instruction;
  pub fn create_metadata_v3(mint: &Pubkey, authority: &Pubkey, info: &TokenInfo<'_>) -> Instruction;
  pub fn update_metadata_v2(mint: &Pubkey, authority: &Pubkey, info: &TokenInfo<'_>) -> Instruction;
  ```
- `signers[0]` is the fee payer. `Error::NoSigner` is returned when a meta needs a signature that
  no signer gives.

- [ ] **Step 1: Write the failing tests.**
  - `shortvec_encodes_lengths`: 0 → `[0]`, 127 → `[0x7f]`, 128 → `[0x80, 0x01]`,
    16383 → `[0xff, 0x7f]`.
  - `mint_tx_matches_web3`: the message bytes, `signature` and `wire` equal `vectors.mint_tx`.
  - `create_mint_tx_matches_web3`, the same way, with both signers.
  - `metadata_create_ix_matches_mpl` and `metadata_update_ix_matches_mpl`: each compares the
    program, the metas (pubkey, signer, writable, in order) and the data bytes.
  - `a_missing_signer_is_refused`: the mint tx signed by the owner only gives `Err(NoSigner)`.
- [ ] **Step 2:** Run the gate. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **The compiler mirrors Solana's `CompiledKeys`:**
    - insertion order is the payer, then for each instruction its program id, then its metas;
    - a duplicate ORs its signer and writable flags;
    - accounts are grouped as writable signers, readonly signers, writable non-signers, then
      readonly non-signers, keeping insertion order within each group;
    - the header is `[num_signers, num_readonly_signed, num_readonly_unsigned]`;
    - each instruction is program index u8 ‖ shortvec(indices) ‖ shortvec(data).
  - **Instruction data** (confirm each against the fixtures):

    | Instruction | Data | Accounts |
    |---|---|---|
    | ATA create-idempotent | `[1]` | payer s+w, ata w, owner, mint, system, token |
    | MintToChecked | `[14] ‖ amount u64 LE ‖ decimals` | mint w, dest w, authority s |
    | CreateAccount | `0u32 LE ‖ lamports u64 ‖ space u64 ‖ owner` | from s+w, new s+w |
    | InitializeMint2 | `[20, decimals] ‖ authority ‖ [0]` (or `[1] ‖ freeze`) | mint w |
    | Metaplex create | `[33]` then Borsh `DataV2` (`name`, `symbol`, `uri` as u32-len strings; `u16` fee 0; three `None` = `0` bytes), `is_mutable = 1`, `collection_details = None` | from the fixture |
    | Metaplex update | `[15]` then `Some(DataV2)`, `None`, `None`, `None` | from the fixture |
- [ ] **Step 4:** Run the gate. Expected: PASS. Then sabotage the group order (readonly signers
  before writable signers) and see `mint_tx_matches_web3` fail.
- [ ] **Step 5: Commit**, `feat: legacy transactions and the six instructions`.

### Task A4: The devnet client, the mainnet refusal and the mint account

**Files:** create `src/rpc.rs`, `src/cluster.rs` and `src/mint.rs`. Tests go in `tests/rpc.rs`
and `tests/mint.rs`.

**Interfaces (produces):**
```rust
pub const MAINNET_GENESIS: &str; pub const DEVNET_GENESIS: &str; pub const DEFAULT_RPC_URL: &str;
pub fn refuse_mainnet(genesis: &Hash) -> Result<(), Error>;          // Err(Mainnet) for mainnet-beta
pub enum TxStatus { Landed, Failed(String), Processed, NotFound }
pub struct Devnet { /* url, reqwest::Client with RPC_TIMEOUT */ }
impl Devnet {
    pub async fn connect(url: &str) -> Result<(Devnet, Hash), Error>;  // fetches genesis, refuses mainnet
    pub async fn balance(&self, of: &Pubkey) -> Result<u64, Error>;
    pub async fn account_data(&self, of: &Pubkey) -> Result<Option<Vec<u8>>, Error>;
    pub async fn rent_exempt_minimum(&self, size: u64) -> Result<u64, Error>;
    pub async fn latest_blockhash(&self) -> Result<(Hash, u64), Error>;   // confirmed; (hash, lastValidBlockHeight)
    pub async fn finalized_block_height(&self) -> Result<u64, Error>;
    pub async fn genesis_hash(&self) -> Result<Hash, Error>;
    pub async fn send(&self, wire: &[u8]) -> Result<Signature, Error>;
    pub async fn status(&self, sig: &Signature) -> Result<TxStatus, Error>;  // searchTransactionHistory: true
    pub async fn request_airdrop(&self, to: &Pubkey, lamports: u64) -> Result<Signature, Error>;
}
pub const MINT_ACCOUNT_LEN: u64 = 82;
pub struct MintAccount { pub mint_authority: Option<Pubkey>, pub supply: u64, pub decimals: u8,
                         pub is_initialized: bool, pub freeze_authority: Option<Pubkey> }
impl MintAccount { pub fn parse(data: &[u8]) -> Result<MintAccount, Error> }
```
- The request bodies are built by pure `pub(crate)` functions, so tests need no network.
- **HTTP failures** (a non-2xx response such as 429, a timeout, or a connect error) are
  `Error::Http`. **A JSON-RPC error** is `Error::Rpc { code, message }`.
- **Status mapping:**
  - `null` → `NotFound`;
  - an `err` that is not null → `Failed(err.get())`;
  - `confirmationStatus` of `"confirmed"` or `"finalized"` → `Landed`;
  - otherwise `Processed`.

- [ ] **Step 1: Write the failing tests.** Each request test asserts the exact JSON. The envelope
  is `{"jsonrpc":"2.0","id":1,"method":M,"params":P}`, with `P` as follows:

  | Method | Params |
  |---|---|
  | `getGenesisHash` | `[]` |
  | `getBalance` | `["<pk>",{"commitment":"confirmed"}]` |
  | `getAccountInfo` | `["<pk>",{"encoding":"base64","commitment":"confirmed"}]` |
  | `getMinimumBalanceForRentExemption` | `[82]` |
  | `getLatestBlockhash` | `[{"commitment":"confirmed"}]` |
  | `getBlockHeight` | `[{"commitment":"finalized"}]` |
  | `sendTransaction` | `["<base64>",{"encoding":"base64","preflightCommitment":"confirmed"}]` |
  | `getSignatureStatuses` | `[["<sig>"],{"searchTransactionHistory":true}]` |
  | `requestAirdrop` | `["<pk>",1000000000,{"commitment":"confirmed"}]` |

  Response tests, on canned JSON:
  - `latest_blockhash_reads_hash_and_last_valid_height`;
  - `status_null_is_not_found`, `status_with_err_is_failed_and_keeps_the_err_text`,
    `status_confirmed_is_landed`, `status_processed_is_processed`;
  - `an_rpc_error_envelope_is_error_rpc`: code `-32002` and the message are kept;
  - `account_info_null_value_is_none`.

  Cluster test: `mainnet_genesis_is_refused_and_devnet_is_not`.

  Mint tests:
  - `mint_account_parses_82_bytes`: built by hand as tag 1 + authority + supply 7 + decimals 0 +
    init 1 + tag 0 + 32 zero bytes;
  - `mint_account_refuses_short_data`.
- [ ] **Step 2:** Run the gate. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Requests and responses use typed serde structs: `Request<'a, P>` and `Response<T> { result:
    Option<T>, error: Option<RpcError> }`.
  - `connect` is the only constructor.
  - Before relying on the two genesis consts, confirm them against Solana's current docs
    (`getGenesisHash` on each cluster). A mismatch is a stop-and-report.
- [ ] **Step 4:** Run the gate. Expected: PASS. Then sabotage `refuse_mainnet` (compare to
  devnet) and see the cluster test fail.
- [ ] **Step 5: Commit**, `feat: the devnet client, refusing mainnet`.

### Task A5: The coin, the CLI and the logo

**Files:** create `src/coin.rs`, `src/bin/crack-coin.rs`, `assets/crackcoin.png`,
`assets/crackcoin.json`, `tests/coin.rs` and `tests/live.rs`. Modify `src/lib.rs`.

**Interfaces (produces; Part B consumes `Coin`, `PreparedMint`, `TxStatus`, `Pubkey`, `Signature`,
`Keypair` and `Error`):**
```rust
pub const NAME: &str = "CrackCoin"; pub const SYMBOL: &str = "CRACK";
pub const METADATA_URI: &str = "https://raw.githubusercontent.com/cycle-five/crack-coin/master/assets/crackcoin.json";
pub fn check_mint(data: Option<&[u8]>, authority: &Pubkey) -> Result<MintAccount, Error>; // MintMismatch("absent"|"not a mint"|"not initialized"|"decimals"|"authority")
pub struct Coin { /* devnet, authority, mint, genesis */ }
impl Coin {
    pub async fn open(url: &str, authority: Keypair, mint: Pubkey) -> Result<Coin, Error>; // connect + check_mint
    pub fn mint(&self) -> &Pubkey; pub fn authority(&self) -> Pubkey; pub fn devnet(&self) -> &Devnet;
    pub async fn prepare_mint(&self, owner: &Pubkey, amount: u64) -> Result<PreparedMint, Error>;
    pub async fn has_token_account(&self, owner: &Pubkey) -> Result<bool, Error>;
}
pub struct PreparedMint { pub signature: Signature, pub wire: Vec<u8>, pub last_valid_block_height: u64 }
pub fn mint_transaction(authority: &Keypair, mint: &Pubkey, owner: &Pubkey, amount: u64, blockhash: &Hash) -> Result<SignedTx, Error>;
pub async fn create_mint(devnet: &Devnet, authority: &Keypair, mint: &Keypair) -> Result<Signature, Error>;
pub async fn set_metadata(devnet: &Devnet, authority: &Keypair, mint: &Pubkey, info: &TokenInfo<'_>) -> Result<Signature, Error>; // create if absent, else update
pub async fn wait_for(devnet: &Devnet, sig: &Signature, last_valid_block_height: u64) -> Result<TxStatus, Error>; // 2 s polls; NotFound once finalized height > last valid
pub fn write_keypair_file(path: &std::path::Path, kp: &Keypair) -> Result<(), Error>; // create_new, mode 0o600; never overwrites
```
**The CLI** parses its own arguments (no clap) and prints only public data:
- `keygen <path>`: `from_seed(getrandom)`, then `write_keypair_file`, and prints the pubkey;
- `airdrop <keypair> [sol=1]`;
- `create-mint <keypair>`: prints the mint;
- `set-metadata <keypair> <mint> [uri]`: the uri defaults to `METADATA_URI`;
- `info <keypair> <mint>`: prints SOL, supply, decimals, whether the authority matches, and the
  metadata's name, symbol and uri (Borsh-decoded from the metadata account) if present.

Every subcommand takes `--url`, defaulting to `DEFAULT_RPC_URL`, and goes through
`Devnet::connect`.

- [ ] **Step 1: Write the failing tests.**
  - `mint_transaction_matches_the_fixture`: equals `vectors.mint_tx`.
  - `check_mint_names_each_mismatch`:
    - `None` gives `"absent"`;
    - 10 bytes give `"not a mint"`;
    - init 0 gives `"not initialized"`;
    - decimals 2 gives `"decimals"`;
    - another authority gives `"authority"`;
    - the good bytes give `Ok`.
  - `keypair_file_is_0600_and_never_overwritten` (in `std::env::temp_dir()` under a unique name):
    the mode is `0o600`, and a second write is `Err`.
  - `the_metadata_json_names_the_coin`: `assets/crackcoin.json`, parsed into a typed struct, has
    name `NAME`, symbol `SYMBOL` and image equal to the Global Constraints image URL.
  - `tests/live.rs`, every test `#[ignore]`:
    - `connect_refuses_mainnet`: `Devnet::connect("https://api.mainnet-beta.solana.com")` gives
      `Err(Mainnet)`;
    - `devnet_connects`;
    - `mint_round_trip`: reads `CRACK_COIN_KEYPAIR` and `CRACK_COIN_MINT`, mints 1 to a fresh
      `from_seed` owner, and `wait_for` returns `Landed`.
- [ ] **Step 2:** Run the gate. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **The logo:** Python PIL, run with `-I`, on
    `/home/lothrop/Pictures/cracktunes/segments/circular-emblem.png` (980×990 RGBA):
    - pad it centered onto a transparent 990×990;
    - resize to 512×512 with LANCZOS;
    - save, then `pngquant --quality 80-95`. The target is under 400 KB; report the size.
  - **The JSON:** `{"name","symbol","description","image"}`. The description is "A worthless-by-
    design Solana devnet token, earned by hanging out with CrackTunes."
- [ ] **Step 4:** Run the gate. Expected: PASS, with the live tests ignored. Sabotage `check_mint`'s
  decimals check and see its test fail.
- [ ] **Step 5: Commit**, `feat: the coin, the CLI and the CrackCoin logo`.

### Task A6: Publish v0.1.0 and set up devnet

Outward-facing. The owner approved the public repo, so no further ask is needed.

- [ ] **Step 1:** Create the repo with `gh repo create cycle-five/crack-coin --public --description
  "CrackCoin: a worthless-by-design Solana devnet token for CrackTunes" --source . --remote origin`.
  Push `master` (A1), then push `feat/v0.1.0`.
- [ ] **Step 2:** Open the PR from `feat/v0.1.0`; its body covers design, change and the
  fixture-regeneration recipe. Wait for CI to go green. Triage any review by applying it,
  deferring it with a reason, or pushing back with evidence. Squash-merge it.
- [ ] **Step 3:** Run `git checkout master && git pull --ff-only`, then `git tag -s v0.1.0 -m
  "crack-coin v0.1.0"` and `git push origin v0.1.0`. Nothing workflow-owned creates the release,
  so run `gh release create v0.1.0 --verify-tag --generate-notes` (global cadence, case B).
- [ ] **Step 4: Set up devnet,** using the CLI built with `--features cli`.
  1. Run `install -d -m 700 ~/.config/crack-coin`, then `crack-coin keygen
     ~/.config/crack-coin/devnet-authority.json`. That prints the public authority key.
  2. Run `crack-coin airdrop … 2`; on failure, retry once with 1. **If both fail,** stop and ask
     the owner to fund the printed authority from faucet.solana.com. That is the one owner call.
  3. Run `create-mint`, then `set-metadata`, then `info`. Expected: decimals 0, authority matches,
     name CrackCoin, symbol CRACK.
  4. Run the live tests:
     `CRACK_COIN_KEYPAIR="$(< ~/.config/crack-coin/devnet-authority.json)" CRACK_COIN_MINT=<mint> cargo test --all-features --test live -- --ignored`.
     Expected: 3 passed.
- [ ] **Step 5: Record and hand off.**
  - Record the mint and authority (public) in the arc's memory.
  - Add an owner to-do: vault `~/.config/crack-coin/devnet-authority.json` in Vaultwarden.

---

# Part B — crack-core (`/home/lothrop/projects/cracktunes-economy`, branch `feat/crack-coin`)

### Task B1: The schema and the money (ungated)

**Files:**
- Create `migrations/20261021180000_crack_coin.sql` and its identical copy in
  `crack-core/test_migrations/`. The DDL is spec section 5 verbatim, including `sponsored`, with
  `IF NOT EXISTS` everywhere, plus `CREATE INDEX IF NOT EXISTS coin_withdrawals_state ON
  coin_withdrawals (state)`.
- Create `crack-core/src/db/economy/coin.rs`, a child module, so it uses the private
  `lock_wallet`, `set_balance` and `Locked`.
- Modify:
  - `crack-core/src/db/economy.rs` (`pub mod coin;`);
  - `crack-core/src/economy/wallet.rs`: `LedgerKind::CoinWithdraw` `"coin_withdraw"` and
    `LedgerKind::CoinRefund` `"coin_refund"`;
  - `crack-core/src/commands/economy/credits.rs` (`ledger_label` arms);
  - `crack-core/src/messaging/messages.rs`: `ECONOMY_LEDGER_COIN_WITHDRAW = "withdrawn as
    CrackCoin"` and `ECONOMY_LEDGER_COIN_REFUND = "CrackCoin refund"`;
  - `.sqlx/`.

**Interfaces (produces):**
```rust
pub const ORPHAN_AFTER_SECS: i64 = 120;
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum WithdrawalState { Pending, Done, Refunded } // as_str/parse
pub struct NewWithdrawal<'a> { pub guild: GuildId, pub user: UserId, pub address: &'a str, pub mint: &'a str, pub amount: i64 }
pub struct Started { pub id: i64, pub balance: i64 }
#[derive(Debug)] pub enum StartError { Money(MoveError), AlreadyPending, Db(sqlx::Error) }
pub async fn start_withdrawal(pool: &PgPool, w: &NewWithdrawal<'_>) -> Result<Started, StartError>;
pub struct Recording<'a> { pub signature: &'a str, pub tx: &'a [u8], pub last_valid_block_height: i64, pub genesis_hash: &'a str, pub sponsored: bool }
pub struct Recorded { /* private: id, signature, tx */ }  // pub fn id(&self)->i64; signature(&self)->&str; tx(&self)->&[u8]
pub async fn record_sent(pool: &PgPool, id: i64, r: &Recording<'_>) -> sqlx::Result<Option<Recorded>>;
pub async fn finish_withdrawal(pool: &PgPool, id: i64) -> sqlx::Result<bool>;
pub async fn refund_withdrawal(pool: &PgPool, id: i64, reason: &str) -> sqlx::Result<Option<i64>>; // Some(balance) iff this call moved the row
pub struct PendingWithdrawal { pub id: i64, pub guild: GuildId, pub user: UserId, pub address: String, pub amount: i64,
    pub sent: Option<Recorded>, pub last_valid_block_height: Option<i64>, pub genesis_hash: Option<String>, pub orphan: bool }
pub async fn pending_withdrawals(pool: &PgPool, process_start: Option<DateTime<Utc>>) -> sqlx::Result<Vec<PendingWithdrawal>>;
pub async fn pending_withdrawal(pool: &PgPool, id: i64, process_start: Option<DateTime<Utc>>) -> sqlx::Result<Option<PendingWithdrawal>>;
pub async fn withdrawal_state(pool: &PgPool, id: i64) -> sqlx::Result<Option<(WithdrawalState, Option<String>)>>; // (state, reason)
pub async fn link_address(pool: &PgPool, user: UserId, address: &str) -> sqlx::Result<()>;
pub async fn unlink_address(pool: &PgPool, user: UserId) -> sqlx::Result<bool>;
pub async fn linked_address(pool: &PgPool, user: UserId) -> sqlx::Result<Option<String>>;
pub async fn sponsored_accounts(pool: &PgPool, user: UserId, mint: &str) -> sqlx::Result<i64>;
pub struct LastWithdrawal { pub amount: i64, pub state: WithdrawalState, pub signature: Option<String>, pub reason: Option<String>, pub created_at: DateTime<Utc> }
pub struct WalletOverview { pub total_withdrawn: i64, pub last: Option<LastWithdrawal> }
pub async fn wallet_overview(pool: &PgPool, user: UserId) -> sqlx::Result<WalletOverview>;
```
- `Recorded` has **no public constructor.** Only `record_sent` and the two `pending_*` functions
  make one. The latter make one only for rows with a signature and a tx.
- `orphan` is computed in SQL on the DB clock:
  `created_at < $process_start OR created_at < now() - make_interval(secs => ORPHAN_AFTER_SECS)`.
- The unique-violation code `23505` on index `coin_withdrawals_one_pending` maps to
  `AlreadyPending`.
- An amount ≤ 0 gives `Money(Invalid)`, with nothing written.
- The ledger rows set `coin_withdrawal_id`, through a private `write_coin_ledger` in the child
  module.

- [ ] **Step 1: Write the failing db-tests** (gated like their neighbours with
  `#[sqlx::test(migrator = "MIGRATOR")]` and the `db-tests` `cfg_attr`; seed with the
  `credit_earnings`-based `fund` helper):
  - `start_debits_and_writes_a_pending_row_and_a_ledger_row`: balance 100 − 30 = 70, the row is
    `pending`, and the ledger holds `coin_withdraw −30` with `coin_withdrawal_id` set.
  - `start_refuses_more_than_the_balance_and_writes_nothing`: covers `i64::MAX` as well →
    `Money(Insufficient { balance: 100 })`, with no row and no ledger row.
  - `start_refuses_a_non_positive_amount`.
  - `a_second_pending_withdrawal_is_refused_even_in_another_server` → `AlreadyPending`, and the
    balance is untouched.
  - `done_then_refund_moves_nothing`: `finish` gives `true`, then `refund` gives `None`, and the
    balance is still 70.
  - `refund_restores_the_balance_once`: the first refund gives `Some(100)`, the second `None`;
    there is one `coin_refund +30` row, and the state is `refunded` with its reason.
  - `racing_refunds_credit_once`: two `refund_withdrawal` calls under `tokio::join!` → exactly
    one `Some`, and the balance is 100.
  - `record_on_a_refunded_row_returns_none`.
  - `record_sets_signature_tx_and_sponsored` gives `Some(Recorded)`, whose `tx()` equals the
    input.
  - `pending_rows_mark_orphans_by_process_start_and_age`:
    - a row created now with `process_start = now + 1 s` is an orphan;
    - with `process_start = now − 1 h`, it isn't;
    - built with `Utc::now()`.
  - `sponsored_counts_only_done_rows_that_created_an_account`.
  - `link_replaces_and_unlink_deletes`.
  - `overview_sums_done_amounts_and_shows_the_last`.
  - In `credits.rs`, extend `ledger_labels_are_named_and_unknown_kinds_show_raw` with the two new
    kinds.
- [ ] **Step 2:** Migrate a fresh local DB and run the db gate. Expected: FAIL (not compiled).
- [ ] **Step 3:** Implement. Then regenerate `.sqlx` with `scripts/sqlx_sync.py`.
- [ ] **Step 4:** Run the gate, without the coin flags. Expected: PASS. Then sabotage the refund's
  "only if moved" guard and see `refund_restores_the_balance_once` or the racing test fail.
- [ ] **Step 5: Commit**, `feat(coin): withdrawals schema and money`.

### Task B2: The feature, the chain seam and the config

**Files:**
- Modify:
  - `crack-core/Cargo.toml`: dependency `crack-coin = { git =
    "https://github.com/cycle-five/crack-coin", tag = "v0.1.0", optional = true }` and feature
    `crack-coin = ["dep:crack-coin"]`;
  - `crack-cli/Cargo.toml`: `crack-coin = ["crack-core/crack-coin"]`;
  - `Cargo.lock`;
  - `crack-core/src/lib.rs`: `pub const COIN_BUILD: bool = cfg!(feature = "crack-coin");` and
    `#[cfg(feature = "crack-coin")] pub mod coin;`;
  - `.github/workflows/lint.yml`: the seam step becomes `cargo check -p crack-core --features
    crack-bf,crack-osint,crack-coin --locked`, plus a step `cargo clippy -p crack-core --features
    crack-coin --locked -- -D warnings`, with a comment saying why;
  - `.github/workflows/build.yml`: both test runs add `crack-core/crack-coin` to `--features`.
- Create `crack-core/src/coin/mod.rs`, `chain.rs` and `config.rs`.

**Interfaces (produces):**
```rust
// chain.rs
pub enum ChainStatus { Landed, Failed(String), Processed, NotFound }   // From<crack_coin::TxStatus>
#[derive(Debug)] pub struct ChainError(pub String);
pub struct Signed { pub signature: String, pub tx: Vec<u8>, pub last_valid_block_height: u64, pub genesis: String }
#[async_trait] pub trait CoinChain: Send + Sync {
    fn mint(&self) -> &str;
    fn authority(&self) -> String;
    async fn genesis(&self) -> Result<String, ChainError>;
    async fn finalized_height(&self) -> Result<u64, ChainError>;
    async fn tap_lamports(&self) -> Result<u64, ChainError>;
    async fn has_token_account(&self, owner: &str) -> Result<bool, ChainError>;
    async fn prepare(&self, owner: &str, amount: u64) -> Result<Signed, ChainError>; // fresh genesis + blockhash
    async fn send(&self, tx: &Recorded) -> Result<(), ChainError>;
    async fn status(&self, signature: &str) -> Result<ChainStatus, ChainError>;
}
pub struct LiveChain { /* crack_coin::Coin, mint string */ }
pub enum OpenError { Unreachable(String), Refused(CoinOff) }
impl LiveChain { pub async fn open(cfg: CoinConfig) -> Result<LiveChain, OpenError> }
// config.rs
pub struct CoinConfig { pub keypair: crack_coin::Keypair, pub mint: crack_coin::Pubkey, pub url: String }
#[derive(Debug, Clone, PartialEq, Eq)] pub enum CoinOff {
    NotConfigured(&'static str), BadKeypair, BadMint, Mainnet, MintMismatch(&'static str) }
impl std::fmt::Display for CoinOff   // "CRACK_COIN_MINT is not set", "the keypair does not parse", …; never the value
impl CoinConfig { pub fn from_values(keypair: Option<&str>, mint: Option<&str>, url: Option<&str>) -> Result<CoinConfig, CoinOff>;
                  pub fn from_env() -> Result<CoinConfig, CoinOff> }
```
- `CoinConfig`'s `Debug` must not show the key: rely on `Keypair`'s.
- `OpenError` mapping: `crack_coin::Error::Mainnet` → `Refused(Mainnet)`; `MintMismatch(w)` →
  `Refused(MintMismatch(w))`; `Http`/`Rpc` → `Unreachable`.
- An empty string counts as unset.

- [ ] **Step 1: Write the failing tests** in `coin/config.rs` and `coin/chain.rs`:
  - `each_missing_variable_is_named`: the keypair gives `NotConfigured("CRACK_COIN_KEYPAIR")`, and
    likewise the mint. The URL is optional; it defaults to `DEFAULT_RPC_URL`.
  - `a_keypair_with_a_trailing_newline_is_read`.
  - `a_bad_keypair_is_refused_without_echoing_it`: the input contains `"7777"`;
    `CoinOff::BadKeypair.to_string()` and the `Debug` don't.
  - `a_bad_mint_is_refused`.
  - `statuses_map_one_to_one`: each `crack_coin::TxStatus` maps to its `ChainStatus`.
  - `coin_build_is_true_here`: `assert!(crate::COIN_BUILD)`.
- [ ] **Step 2:** Run the gate with the coin flags. Expected: FAIL.
- [ ] **Step 3:** Implement. `LiveChain::send` sends `recorded.tx()`. `status` parses the signature;
  a parse failure is a `ChainError`.
- [ ] **Step 4:** Run the gate, now with the coin flags. Expected: PASS. Also run `cargo tree -p
  cracktunes -e normal | grep -c crack-coin`; the expected count is 0 without the feature, which
  proves a lean build compiles none of it.
- [ ] **Step 5: Commit**, `feat(coin): the crack-coin feature and the chain seam`.

### Task B3: The resolver, the withdrawal flow and the sweeper

**Files:** create `crack-core/src/coin/resolve.rs`, `flow.rs`, `sweep.rs` and `fake.rs`
(`#[cfg(test)]`).

**Interfaces:**
- Consumes B1 (`db::economy::coin::*`) and B2 (`CoinChain`, `ChainStatus`, `Signed`).
- Produces:
```rust
// resolve.rs (pure)
pub struct RowFacts<'a> { pub sent: bool, pub orphan: bool, pub last_valid_block_height: Option<i64>, pub genesis_hash: Option<&'a str> }
pub enum Observation { Unreachable, Seen { status: ChainStatus, finalized_height: u64, genesis: String } }
#[derive(Debug, Clone, PartialEq, Eq)] pub enum RefundReason { NeverSent, FailedOnChain(String), DevnetReset, Expired }
impl RefundReason { pub fn as_db_text(&self) -> String }   // "never sent", "failed on chain: …", "devnet was reset", "expired"
#[derive(Debug, Clone, PartialEq, Eq)] pub enum Decision { Done, Refund(RefundReason), Wait { resend: bool } }
pub fn decide(row: &RowFacts<'_>, obs: &Observation) -> Decision;
pub async fn observe(chain: &dyn CoinChain, signature: &str) -> Observation;   // any ChainError → Unreachable
// flow.rs
pub struct Timing { pub poll_every: Duration, pub poll_for: Duration }   // Timing::LIVE = 2 s / 180 s (amended: R18)
pub struct WithdrawRequest<'a> { pub guild: GuildId, pub user: UserId, pub address: &'a str, pub amount: i64, pub sponsored: bool }
pub async fn begin(pool: &PgPool, chain: &dyn CoinChain, req: &WithdrawRequest<'_>) -> Result<Started, StartError>; // snapshots address + chain.mint()
#[derive(Debug, PartialEq, Eq)] pub enum Outcome { Minted { signature: String }, Refunded { reason: RefundReason, balance: Option<i64> }, StillSettling }
pub async fn complete(pool: &PgPool, chain: &dyn CoinChain, started: &Started, req: &WithdrawRequest<'_>, timing: &Timing) -> Outcome;
pub async fn resolve_one(pool: &PgPool, chain: &dyn CoinChain, row: &PendingWithdrawal) -> Option<Outcome>; // None = still waiting
// sweep.rs
pub fn start_sweeper(pool: PgPool, chain: Arc<dyn CoinChain>, process_start: Option<DateTime<Utc>>); // effects::spawn_supervised("coin-sweeper", …), SWEEP_EVERY
```
- **`decide` is spec 6.2's table, exactly:**
  - not sent: `Refund(NeverSent)` if an orphan, else `Wait { resend: false }`;
  - sent and `Unreachable`: `Wait { resend: false }`;
  - `Landed`: `Done`;
  - `Failed(e)`: `Refund(FailedOnChain(e))`;
  - `Processed`: `Wait { resend: false }`;
  - `NotFound`, then in order:
    1. `genesis != row.genesis_hash` → `Refund(DevnetReset)`;
    2. `finalized_height > last_valid + EXPIRY_MARGIN` (`EXPIRY_MARGIN = 150`) →
       `Refund(Expired)`;
    3. otherwise `Wait { resend: true }`.
  - `observe` reads `finalized_height` **before** `status` (Ruling R9).
  - `ChainStatus::Failed` comes from the crate only for an error at confirmed or finalized
    (Ruling R7). An error seen only at processed arrives as `Processed`.
  - A sent row missing `last_valid` or `genesis` gets `Wait { resend: false }`.
- **`complete`:**
  1. `chain.prepare`. On an error, `refund_unsent` (amended: R12; the reason is `NeverSent`):
     this process's own row was never sent.
  2. `record_sent`. `None` means the row was resolved elsewhere: return its state via
     `withdrawal_state`, **sending nothing**.
  3. `chain.send(&recorded)`. Log an error and continue.
  4. Every `poll_every` until `poll_for`: `observe`, `decide`, apply. `Wait { resend: true }`
     resends.
  5. If applying finds the row already resolved, report `withdrawal_state`.
- `resolve_one` is the sweeper's single step: build `RowFacts` from the row, `observe` only if
  sent, then `decide`, apply, and resend on `Wait { resend: true }`.
- `fake.rs`, `FakeChain`:
  - `prepare` returns a scripted `Result<Signed>`;
  - `status` pops a scripted `VecDeque<Result<ChainStatus, ChainError>>`, repeating the last
    entry;
  - `finalized_height`, `genesis` and `tap_lamports` are settable;
  - it records every `prepare` (owner, amount) and every `send` (tx bytes) for assertions.

- [ ] **Step 1: Write the failing tests.**
  - **Pure,** one test per row of the table: `never_sent_orphan_refunds`,
    `never_sent_fresh_waits`, `landed_is_done`, `failed_refunds_with_the_error`,
    `processed_waits`, `not_found_after_reset_refunds`, `not_found_after_expiry_refunds`,
    `not_found_before_expiry_waits_and_resends`, `unreachable_waits`,
    `expiry_needs_the_margin`: `last_valid + 150` waits and `last_valid + 151` refunds.
  - `observe_reads_the_height_before_the_status`: the fake records its call order.
  - **db-tests with `FakeChain`, `#[tokio::test(start_paused = true)]` style timing,** each
    asserting the outcome, the sends and the final balance:
    - `lands_on_the_first_poll`: `Minted`, 1 send, balance 70;
    - `fails_on_chain_refunds`: `Refunded`, balance 100;
    - `never_seen_refunds_only_after_expiry`: heights rise past `last_valid`; there are resends
      before the refund;
    - `unreachable_mid_settle_never_refunds` (Review Focus 3): `status` errs for the whole window
      → `StillSettling`, the balance stays 70 and the row stays pending;
    - `unreachable_then_landed_is_done`;
    - `devnet_reset_refunds`;
    - `prepare_failure_refunds_never_sent` with 0 sends;
    - `a_row_refunded_by_the_sweeper_is_never_sent`: `begin`, then the row is made orphan (a
      `process_start` in the future) and swept → refunded. `complete` then sends **0** times and
      reports the refund;
    - `crash_after_record_is_settled_by_the_sweeper`: `begin` and `record_sent` by hand with no
      send, then `resolve_one` → resend, then `Landed` → done;
    - `relink_while_pending_mints_to_the_snapshot` (Review Focus 4): `begin` with address A, then
      `link_address(B)`, then `complete`. The fake's `prepare` owner is A.
- [ ] **Step 2:** Run the gate. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the gate. Expected: PASS. Then sabotage two places and see each caught:
  - swap the reset and expiry order in `decide`;
  - make `complete` send when `record_sent` returns `None`.
- [ ] **Step 5: Commit**, `feat(coin): the resolver, the withdrawal flow and the sweeper`.

### Task B4: Startup and readiness

**Files:**
- Create `crack-core/src/coin/registry.rs`.
- Modify:
  - `crack-core/src/coin/mod.rs` (`start`);
  - `crack-core/src/economy/registry.rs`: `#[cfg(feature = "crack-coin")] pub coin:
    crate::coin::registry::CoinRegistry` on `EconomyRegistry`, Default;
  - `crack-core/src/handlers/serenity.rs`: right after `start_sweeper`,
    `#[cfg(feature = "crack-coin")] crate::coin::start(self.data.clone());`.

**Interfaces (produces):**
```rust
#[derive(Clone)] pub enum CoinState { Starting, Off(CoinOff), Ready(Arc<dyn CoinChain>) }
#[derive(Default)] pub struct CoinRegistry { /* state: RwLock<CoinState> (initially Starting), started: AtomicBool */ }
impl CoinRegistry { pub fn state(&self) -> CoinState; pub fn set(&self, s: CoinState); pub fn start_once(&self) -> bool }
pub fn ready_line(mint: &str, authority: &str, lamports: u64) -> String;  // exact Global Constraints text, 2 decimals
pub fn off_line(off: &CoinOff) -> String;
pub fn startup_backoff(attempt: u32) -> Duration;                          // 5 s · 2^n, ≤ 300 s
pub fn start(data: Data);
```
- **`start`:**
  - it runs once, through `start_once`, and only with a DB pool;
  - it spawns `spawn_supervised("coin-start", …)`;
  - `CoinConfig::from_env()`: on `Err(off)`, `set(Off)` and log `off_line` at info;
  - otherwise it loops `LiveChain::open`:
    - `Unreachable` logs a warning and sleeps `startup_backoff(n)`;
    - `Refused(off)` sets `Off` and logs `off_line` at error for `Mainnet`, and at warn
      otherwise;
  - on success: read `tap_lamports` (0 if that fails), `set(Ready)`, log `ready_line` at info, and
    `start_sweeper(pool, chain, data.economy.process_start())`.

- [ ] **Step 1: Write the failing tests:**
  - `ready_line_is_exact`: `ready_line("M", "A", 1_234_000_000)` gives `"crack-coin: ready (mint
    M, authority A, 1.23 devnet SOL)"`;
  - `off_line_names_the_reason`;
  - `backoff_doubles_and_caps`: 5, 10, 20, …, 300, 300;
  - `the_state_starts_as_starting_and_start_runs_once`.
- [ ] **Step 2:** Run the gate. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the gate. Expected: PASS. Then sabotage the cap and see the backoff test
  fail.
- [ ] **Step 5: Commit**, `feat(coin): startup checks and the ready line`.

### Task B5: The per-server switch, the dashboard and `/economy`

**Files:**
- `crack-core/src/economy/settings.rs`: `pub coin_withdrawals: bool` on `EconomyConfig`, default
  `false`.
- `crack-core/src/commands/economy/admin.rs`: `economy_status_text` gains `coin:
  Option<&CoinLine>`. All callers pass `None` in lean builds; the `/economy status` command builds
  it under cfg from the registry state and a tap read.
- `crack-core/src/messaging/messages.rs`:
  - `ECONOMY_STATUS_COIN = "CrackCoin withdrawals"`;
  - `ECONOMY_STATUS_TAP = "tap"`;
  - `DASH_ECONOMY_COIN = "CrackCoin withdrawals"`;
  - `DASH_ECONOMY_COIN_NOTE`: one line, saying members can turn credits into a worthless
    devnet token, 1:1.
- `crack-web/src/settings.rs`, `crack-web/src/page.rs`, `crack-web/assets/settings.js` and
  `crack-web/src/routes.rs`.

**Interfaces:**
```rust
pub struct CoinLine { pub withdrawals: bool, pub tap_lamports: Option<u64> }   // admin.rs, ungated
// crack-web
EconomySettingsView { …, pub coin_build: bool, pub coin_withdrawals: bool }
EconomySettingsView::from_settings(s: &EconomySettings, coin_build: bool) -> Self
EconomyForm { …, #[serde(default)] pub coin_withdrawals: Option<bool> }   // None keeps the stored value
```
- `routes.rs` passes `crack_core::COIN_BUILD`.
- `page.rs` renders `<input type="checkbox" id="e-coin">` plus its note **only when
  `coin_build`**.
- `settings.js` reads and writes `e-coin` only if the element exists. It sends
  `coin_withdrawals` only then.

- [ ] **Step 1: Write the failing tests:**
  - `a_stored_config_without_coin_withdrawals_reads_false`;
  - `status_shows_the_coin_line_only_when_given`: `None` gives no `"CrackCoin"`; `Some` shows
    on/off and the tap as `0.05 SOL`;
  - `the_coin_checkbox_renders_only_in_a_coin_build`: `settings_page` with `coin_build`
    false/true;
  - `a_form_without_coin_withdrawals_keeps_the_stored_value`: stored `true`, the form omits it,
    and the result is still `true`;
  - `a_form_with_coin_withdrawals_false_turns_it_off`.

  Existing routes tests (`ECONOMY_FORM`) must still pass unchanged.
- [ ] **Step 2:** Run the gate. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the gate. Expected: PASS. Then sabotage `None` → `false` in the form apply
  and see the keep-test fail.
- [ ] **Step 5: Commit**, `feat(coin): the per-server switch on the dashboard and in /economy`.

### Task B6: `/wallet`

**Files:**
- Create `crack-core/src/coin/checks.rs` and `crack-core/src/commands/economy/wallet.rs` (both
  gated).
- Modify:
  - `crack-core/src/commands/economy/mod.rs`: `commands()` gains `#[cfg(feature =
    "crack-coin")] wallet::wallet()`, through a `let mut v` plus a cfg push;
  - `crack-core/src/messaging/messages.rs`: the `COIN_*` copy. The wording is the implementer's,
    within the spec's conventions. The constants:
    - `COIN_LINKED`, `COIN_UNLINKED`, `COIN_NOT_LINKED`;
    - `COIN_MINTING`, `COIN_MINTED`, `COIN_REFUNDED`, `COIN_STILL_SETTLING`;
    - `COIN_ALREADY_PENDING`, `COIN_FAILED`, `COIN_DEVNET_HINT`;
    - one `COIN_REFUSE_*` per `WithdrawRefusal`, and one `COIN_ADDRESS_*` per `AddressRefusal`.

**Interfaces:**
```rust
// checks.rs (pure)
#[derive(Debug, PartialEq, Eq)] pub enum AddressRefusal { NotBase58, WrongLength, OffCurve, IsTheMint }
pub fn check_address(raw: &str, mint: Option<&str>) -> Result<String, AddressRefusal>;  // trims; returns canonical base58
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Readiness { Ready, Starting, Off }   // from CoinState
#[derive(Debug, PartialEq, Eq)] pub enum WithdrawRefusal { Starting, Unavailable, SwitchOff, OptedOut, NotLinked, BadAmount, TapDry, SponsorCapReached }
pub fn check_withdraw_local(readiness: Readiness, switch_on: bool, opted_out: bool, linked: bool, amount: i64) -> Result<(), WithdrawRefusal>; // in that order; Starting → Starting, Off → Unavailable
pub fn check_withdraw_chain(tap_lamports: u64, has_account: bool, sponsored_used: i64) -> Result<bool /* sponsored */, WithdrawRefusal>;
pub fn short_address(a: &str) -> String;                                  // "ABCD…WXYZ"
pub fn explorer_tx(sig: &str) -> String;     // https://explorer.solana.com/tx/{sig}?cluster=devnet
pub fn explorer_address(a: &str) -> String;  // https://explorer.solana.com/address/{a}?cluster=devnet
pub fn refusal_text(r: &WithdrawRefusal) -> &'static str; pub fn address_refusal_text(r: &AddressRefusal) -> &'static str;
pub fn outcome_text(o: &Outcome, amount: i64, address: &str) -> String;
pub fn show_text(address: Option<&str>, mint: Option<&str>, overview: &WalletOverview) -> String;
```
**`/wallet`** is `#[poise::command(category = "Economy", slash_command, guild_only, subcommands(...))]`
with subcommands `show`, `link <address>`, `unlink` and `withdraw <amount>` (`#[min = 1]`). Every
reply goes through `say(ctx, …, true)` or `courier::reply_rendered(…, true)`.

**`withdraw`:**
1. `gate(ctx)`, which needs the economy on and a DB.
2. `check_withdraw_local`, with `Readiness` from `data.economy.coin.state()`.
3. `linked_address`.
4. `tap_lamports`, `has_token_account` and `sponsored_accounts` → `check_withdraw_chain`.
5. `flow::begin`:
   - `Money(Insufficient)` → the economy's existing can't-afford text with the balance;
   - `AlreadyPending` → `COIN_ALREADY_PENDING`;
   - `Db` → `COIN_FAILED`.
6. Reply with a placeholder: `COIN_MINTING`, the amount and the short address.
7. `flow::complete(.., &Timing::LIVE)`, then `courier::edit_rendered` with `outcome_text`.

`link` validates with `check_address` (mint from the ready state, else `None`), stores, and
replies with `COIN_LINKED`. That text explains devnet, recommends a fresh devnet-only address, and
says the holder list is public on-chain.

- [ ] **Step 1: Write the failing tests:**
  - `check_address`:
    - a fixture owner key is accepted;
    - the same with `" \n"` around it is accepted and comes back trimmed (Review Focus 1);
    - `"0OIl"` → `NotBase58`;
    - 31 bytes → `WrongLength`;
    - an ATA (from `crack_coin::associated_token_address`) → `OffCurve`;
    - the mint → `IsTheMint`;
  - `local_checks_refuse_in_order`: each refusal appears when it is the only false input. That
    includes `Starting` vs `Off` → `Starting` vs `Unavailable`, and an amount of 0 or negative →
    `BadAmount` (Review Focus 5);
  - `chain_checks`:
    - tap `9_999_999` → `TapDry`;
    - no account with 3 used → `SponsorCapReached`;
    - no account with 2 used → `Ok(true)`;
    - an account with 3 used → `Ok(false)`;
  - `short_address_keeps_four_and_four`;
  - `outcome_text_links_the_devnet_explorer`;
  - `outcome_texts_never_show_the_full_address`: for each `Outcome`, the text doesn't contain
    the full address;
  - `show_text_without_a_link_says_how_to_link`.
- [ ] **Step 2:** Run the gate. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the gate. Expected: PASS. The registration tests still pass, and `/wallet`
  appears only with the feature. Then sabotage the order of the tap check and see a test fail.
- [ ] **Step 5: Commit**, `feat(coin): /wallet show, link, unlink and withdraw`.

### Task B7: The coin image, the docs and v0.35.0

**Files:**
- `Dockerfile`: `ARG CARGO_FEATURES=""` in the builder stage, before the cook. Both the cook and
  the build append `${CARGO_FEATURES:+--features $CARGO_FEATURES}`, identically.
- `.github/workflows/docker.yml`: three steps after the runner build, each with `if:
  startsWith(github.ref, 'refs/tags/') || github.event_name == 'workflow_dispatch'`:
  - "Extract Git Metadata (coin image)", using `flavor: suffix=-coin`;
  - "Build & Push Coin Image": `target: runner`, `build-args: CARGO_FEATURES=crack-coin`, `push:
    ${{ github.event_name != 'pull_request' }}`, `cache-from`/`cache-to` scope `runner-coin`;
  - a comment explaining why the image builds only on tags and dispatch (TuneTitan alone pulls
    it).
  - **Plan ruling (beyond the spec's "tags only"):** `workflow_dispatch` builds it too, so Task B8
    can prove the coin image builds on the branch before any tag exists. A PR still reaches this
    file only through `docker-pr.yml`, whose event is `pull_request`, so the coin steps skip
    there.
- Create `docs/crack-coin.md`, the runbook:
  - what it is;
  - the env vars;
  - setup with the CLI;
  - topping up the tap;
  - the mint and authority (public);
  - the rollback SQL check;
  - why nothing announces withdrawals.
- `docs/economy.md`: a short CrackCoin section that points to the runbook.
- `docs/module-development.md`: a `crack-coin` row (`v0.1.0`) and its `--config` patch example.
- `CHANGELOG.md`: a v0.35.0 entry in the existing voice.
- `Cargo.toml`: `[workspace.package] version = "0.35.0"`, plus `Cargo.lock`.

- [ ] **Step 1:** Make the edits. Verify `cargo metadata --no-deps --format-version 1 | jq -r
  '.packages[] | select(.name=="cracktunes") | .version'`; expected: `0.35.0`.
- [ ] **Step 2:** Run the full gate. Expected: PASS.
- [ ] **Step 3: Commit**, `chore: the -coin image, the CrackCoin runbook, v0.35.0`.

### Task B8: Ship and deploy to TuneTitan

These steps are run by the controller, not a subagent. The owner's GO covers all of them; the only
stops are their genuine calls.

- [ ] **Step 1:** Push `feat/crack-coin` and open the PR. Its body covers:
  - the design and the change;
  - the money rule;
  - the rollback check: `SELECT count(*) FROM coin_withdrawals WHERE state = 'pending'` must be 0
    before going below v0.35.0 or to a lean image;
  - the live checklist from spec section 10;
  - `Closes` nothing.
- [ ] **Step 2:** Wait for CI to go green, then dispatch `docker.yml` on the branch (`gh workflow
  run docker.yml --ref feat/crack-coin`). This proves the coin image builds before any tag exists.
  Triage the review. Address findings as fix-forward commits; force-push nothing.
- [ ] **Step 3:** Squash-merge, then `git checkout master && git pull --ff-only` in the main
  checkout. Tag with `git tag -s v0.35.0 -m "v0.35.0: CrackCoin"` and `git push origin v0.35.0`.
  **Never run `gh release create`:** cargo-dist owns the release. Watch for 14 assets, plus images
  `v0.35.0`, `v0.35.0-coin` and the migrate image `v0.35.0`.
- [ ] **Step 4: The homelab PR.**
  - `tunetitan/docker-compose.yml`: add `CRACK_COIN_KEYPAIR: ${CRACK_COIN_KEYPAIR:-}`,
    `CRACK_COIN_MINT: ${CRACK_COIN_MINT:-}` and `CRACK_COIN_RPC_URL: ${CRACK_COIN_RPC_URL:-}` to
    `cracktunes.environment`.
  - `tunetitan/verify.sh`: when the running image tag ends in `-coin`, require `crack-coin: ready`
    in the startup log, using the file's `case` idiom (never `grep -q` under pipefail).
  - Merge it, then pull.
- [ ] **Step 5: Wire `.env.tunetitan` by key name, printing nothing:**
  - a `bash` heredoc that strips any old `CRACK_COIN_*` lines;
  - appends `CRACK_COIN_KEYPAIR=$(< ~/.config/crack-coin/devnet-authority.json | tr -d '\n ')`
    and `CRACK_COIN_MINT=<mint>`;
  - sets `TUNETITAN_CRACKTUNES_IMAGE=ghcr.io/cycle-five/cracktunes:v0.35.0-coin` and
    `TUNETITAN_CRACKTUNES_MIGRATE_IMAGE=ghcr.io/cycle-five/cracktunes-migrate:v0.35.0`;
  - verifies with `grep -c '^CRACK_COIN_KEYPAIR=\[' .env.tunetitan`, which should print `1`.
- [ ] **Step 6:** Run `./homelab.sh up tunetitan`, then `./homelab.sh verify tunetitan`; re-run
  verify once on a missing startup line. Confirm `crack-coin: ready` in the log.
- [ ] **Step 7: Close out.**
  - Update the memory pointer.
  - Report to the owner:
    - the live checks owed, for someone with Discord: turn `coin_withdrawals` on in a test
      server, link, withdraw 1, and see CrackCoin with the emblem in Phantom on devnet;
    - the vault to-do.
