# CrackCoin

CrackCoin is an optional feature: members turn their server credits into a
token on Solana's **devnet** test network, 1 credit for 1 coin. The token is
worthless by design. It exists so a member can see something in a crypto
wallet, and nothing about it is money.

It is compiled in only with the `crack-coin` cargo feature, which the `-coin`
image carries (`ghcr.io/cycle-five/cracktunes:vX.Y.Z-coin`). The plain image
never has it. The chain code lives in
[`cycle-five/crack-coin`](https://github.com/cycle-five/crack-coin), pinned by
tag in `crack-core/Cargo.toml` (see `docs/module-development.md`).

## The token

| | |
|---|---|
| Name / symbol | `CrackCoin` / `CRACK` |
| Decimals | 0 (one credit is one whole coin) |
| Network | Solana devnet only. CrackCoin refuses a mainnet RPC at startup |
| Mint | `C5yd4LTK4YsXByXUgPxqFL6rhx2X8kB9hgVtyXRdoCEU` |
| Mint authority | `BRpU6hL36Vm78vREgYrKPdK5uSeJEvbcBT2HBb2xD7Ke` |
| Freeze authority | none |

Both addresses are public on-chain.

## What members and admins see

- `/wallet link <address>` stores a devnet address (one per member, used in
  every server). It must be a wallet's own address. A fresh address made just
  for devnet is best, because the mint's holder list is public.
- `/wallet withdraw <amount>` debits credits and mints that many coins to the
  linked address. One withdrawal is in flight per member.
- `/wallet show` and `/wallet unlink` show and delete the link.
- Admins turn it on per server: "CrackCoin withdrawals" on the dashboard's
  economy page, on top of the economy itself. A lean build never shows it.

CrackCoin pays for a member's first three token accounts itself (the rent for
creating them). After that a member's address must already hold one.

## Settings

Read only by a coin build. Music never depends on CrackCoin. At boot:

- A configuration error (a missing or unparsable variable, a mainnet RPC URL, a
  mint that doesn't match) turns CrackCoin off with `crack-coin: off (reason)`.
  There is no retry: fix the setting and restart. A coin build with no database
  is off too: `crack-coin: off (there is no database)`.
- Devnet being unreachable is not off. CrackCoin stays "starting" and logs a
  warning, `crack-coin: devnet unreachable (attempt N, retrying in Ns): ...`,
  on each attempt. The retry waits 5 s, doubling, capped at 300 s, until devnet
  answers.
- When ready it logs
  `crack-coin: ready (mint ..., authority ..., N devnet SOL)`.

| Variable | Contents | Secret |
|---|---|---|
| `CRACK_COIN_KEYPAIR` | The authority's keypair, as the exact text of the Solana CLI JSON file | **yes** |
| `CRACK_COIN_MINT` | The mint address (base58) | no |
| `CRACK_COIN_RPC_URL` | Optional. Defaults to `https://api.devnet.solana.com` | no |

Compose env files don't run commands, so write the line into the file with a
shell command that never prints the key:

```
printf 'CRACK_COIN_KEYPAIR=%s\n' "$(tr -d '\n ' < path/to/devnet-authority.json)" >> .env
```

Never `echo` it, `cat` the key file, or run `set -x` or `bash -n` over a file
that holds it. Keep the key file mode 0600 in a mode 0700 directory, outside
any repository, and keep a copy in a secret store.

## Setup with the CLI

The `crack-coin` repository ships an operator CLI that prints only public
data. Do this once:

```
crack-coin keygen <path>                      # makes the key (0600), prints the public key
crack-coin airdrop <keypair> [sol=1]          # devnet SOL for fees and rent
crack-coin create-mint <keypair>              # prints the mint address
crack-coin set-metadata <keypair> <mint> [uri]
crack-coin info <keypair> <mint>              # checks the mint, its metadata and the balance
```

Each also takes `--url <rpc>`. Then set the three variables above on the
deployment that runs the `-coin` image. A deployment must also pull the
plain-tagged migrate image of the same version.

## Topping up the tap

The authority pays every transaction fee and the rent for new token accounts
in devnet SOL. When it holds less than 0.01 SOL, `/wallet withdraw` refuses
with "the tap is dry" and the bot logs a warning. Top up with
`crack-coin airdrop <keypair> [sol]`. Devnet's faucet rate-limits, so repeat it
or use another devnet faucet to send SOL to the authority address. `crack-coin
info` shows the balance.

## Why nothing announces a withdrawal

Wallets such as Phantom use the same address on mainnet and devnet, so a linked
address is usually someone's real wallet. The holder list of the mint is public,
so an announcement like "X cashed out 50" would let anyone match a Discord name
to an address by amount and time. Every `/wallet` reply is private, an address
is shown only to its owner (shortened), and nothing is posted publicly.

## Rolling back

The migration is additive, so rolling back is image-only for the bot. The
migrate image is the exception: it STAYS at v0.35.0. The v0.34.0 migrate image
stops with `VersionMissing` when it meets a migration it doesn't know. Rolling
back the bot image is fine. A lean image, or a version below v0.35.0, has no
resolver for a withdrawal that is still pending, and its credits would stay
held until a coin build returns. They are not lost.
Before rolling back this must return 0:

```sql
SELECT count(*) FROM coin_withdrawals WHERE state = 'pending';
```

Coins already minted stay minted.

## Devnet reset

Devnet is occasionally wiped. The mint then no longer exists, so a coin build
refuses to start on it (`crack-coin: off (the mint does not match: ...)`).

1. Refill the authority: a reset wipes its devnet SOL too, and the next two
   commands pay rent and fees. `crack-coin airdrop <keypair> [sol]`, repeated
   or through the web faucet if devnet rate-limits it.
2. Create a new mint with the CLI: `crack-coin create-mint <keypair>`, then
   `crack-coin set-metadata <keypair> <mint> [uri]`.
3. Set the new `CRACK_COIN_MINT` and restart the bot.
4. Once the coin is ready on the new mint, the sweeper settles every pending
   withdrawal, whatever its mint. A signed row whose transaction is not found
   on the chain, and which was signed against a different genesis hash, is
   refunded as "devnet was reset". A pending row that was never sent is
   refunded as "never sent". Credits stay held until then; a row missing its
   expiry height or genesis hash just waits.
