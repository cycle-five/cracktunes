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

Read only by a coin build. If one is missing, or devnet can't be reached, the
bot starts with CrackCoin off and logs `crack-coin: off (reason)`. It retries in
the background, and music never depends on it. When ready it logs
`crack-coin: ready (mint ..., authority ..., N devnet SOL)`.

| Variable | Contents | Secret |
|---|---|---|
| `CRACK_COIN_KEYPAIR` | The authority's keypair, as the exact text of the Solana CLI JSON file | **yes** |
| `CRACK_COIN_MINT` | The mint address (base58) | no |
| `CRACK_COIN_RPC_URL` | Optional. Defaults to `https://api.devnet.solana.com` | no |

Load the key from its file without ever printing it, for example in a compose
`.env`:

```
CRACK_COIN_KEYPAIR="$(< path/to/key.json)"
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

The migration is additive, so rolling back is image-only. But a lean image, or
a version below v0.35.0, has no resolver for a withdrawal that is still pending,
and its credits would stay held until a coin build returns. They are not lost.
Before rolling back this must return 0:

```sql
SELECT count(*) FROM coin_withdrawals WHERE state = 'pending';
```

Coins already minted stay minted.
