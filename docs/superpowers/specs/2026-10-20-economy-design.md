# The economy: credits, a prank shop, immunity, and a coinflip — design

**Date:** 2026-10-20 · **Status:** approved in conversation. The owner answered
the brainstorm and said "you have a go to carry it all the way through
deployment on tunetitan". Anything not marked **(owner)** is my call, and is
listed as such so it can be checked.

## Why

Saucey's 2026-10-16 bot-suggestion list ended with a server economy whose
credits buy **roles** that grant or remove permissions: change nicknames,
"unable to change nickname", "VC move perms to drag people". She worried
herself about abuse, and roles that carry permissions are exactly that risk.
The owner redesigned it **(owner)**: credits buy **actions the bot performs**,
for a limited time, and never a role or a permission.

The moment it exists for **(owner)**: *someone pays to rename a member for a
day, locks the nickname, and then has the bot drag them into the voice channel
where everyone is, so the whole room sees the great new name.* The target can
answer: rename the prankster back, buy immunity, and mute them for 5 minutes.

Saucey's server is **21+** **(owner)**, and its members have already asked for
gambling, so the economy starts with a simple coinflip **(owner)**, with room
to grow (section 9).

## Owner decisions

| Question | Decision |
|---|---|
| What credits buy | Bot-performed, temporary actions. Never roles or permissions |
| Notes | Every action can carry a message. Signing it is optional (anonymous by default) |
| Accountability | The mod log **always** names the buyer, signed or not. *Superseded 2026-10-28 by `2026-10-28-economy-log-and-room-mute-design.md`: no log names an unsigned buyer.* |
| Unmasking | "Reveal who did it" is itself something you can buy |
| Earning | Time in voice pays out directly. Messages sent count too (a count, never the content) |
| The catalogue | All of section 4, including every action I proposed: "keep them all" |
| Gambling | A simple coinflip now, with a note on how to expand |
| Immunity | First-class. Per server, any combination of: opt out of the economy, buy it, or have a mod grant it |
| Scope of the go-ahead | Design, plans, build, PRs, merge, tag, and deploy on **TuneTitan**. Production waits for the voice-rooms soak (~2026-10-25) and the owner |

## Goals

- A server turns the economy on from the dashboard, sets prices, and decides
  which actions are for sale and how immunity works. Everything is **off by
  default**, as with rooms and the mod log.
- Members earn credits by spending time in voice with other people and by
  chatting, and can see their balance, the leaderboard, and their history.
- Members spend credits on the actions in section 4. Every action is temporary,
  undoes itself, and survives a bot restart without leaving anyone stuck.
- Every action against a member can carry a note, signed or anonymous. The
  target can pay to unmask an anonymous one. Moderators always see who did it.
- Immunity protects a member from every action aimed at them, from any of the
  three sources the server enables.
- A coinflip with credits.

## Non-goals

- **Roles of any kind.** No role is created, granted or removed, and no Discord
  permission changes hands. That was the abuse risk in the original idea.
- **Real money.** Credits can't be bought with money (premium included),
  can't be cashed out, and aren't tradeable outside a server. That keeps the
  coinflip a game played with play money, not real-money gambling, which
  Discord's policies and the law treat differently.
- **Cross-server wallets.** Credits belong to one server.
- Kicks, bans and Discord timeouts as shop items. Mute is voice only (a server
  mute), so a muted member can still type.
- Earning from message *content*. There is no MESSAGE_CONTENT intent and none
  is needed: the bot counts messages, it never reads them.
- Premium gating. The economy is free on every server, like rooms. The owner
  can gate it later.
- More games than the coinflip (section 9 lists where to go next).

## Anti-goals (would make this a failure even if it works)

- **A prank that outlives its time.** A rename, mute, deafen or jail must end
  on time even across a bot restart, or when the target was offline at the
  moment it ended.
- **Undoing a moderator.** When a mod mutes, renames or moves someone during a
  prank, the mod's action wins, and the bot never reverses it.
- **Anonymous harassment that mods can't see.** The mod log always names the
  buyer, and mods can end any prank and grant immunity.
- **Moving someone where the server keeps them out.** The bot never drags a
  member into a channel their own roles couldn't join. Saucey's server gates by
  age, so this matters.
- **Charging for nothing.** A purchase that can't happen (an immune target, a
  target above the bot, a missing permission, someone not in voice) is refused
  *before* the charge. One that fails at Discord is refunded.
- **Credits for money**, in either direction.

## 1. Settings (per server)

New table `economy_settings` (its own table, as `room_settings` is: the
`guild_settings` row is saved whole from memory).

| Setting | Default | Meaning |
|---|---|---|
| `enabled` | false | The whole economy |
| `currency_name` / `currency_emoji` | `credits` / 🪙 | Shown everywhere a balance is |
| `voice_credits_per_minute` | 1 | 0 turns voice earning off |
| `message_credits` | 1 | Credits for a counted message; 0 turns it off |
| `message_cooldown_seconds` | 60 | At most one counted message per member per this many seconds |
| `announce_channel_id` | none | Where public cards go when there's no better place (section 5.3) |
| `jail_channel_id` | none | The voice channel `jail` uses. Jail isn't sold without one |
| `prices` | section 4's defaults | Per action: price, and whether it's for sale |
| `rename_hours` | 24 | How long a rename lasts **(owner: "for a day")** |
| `short_effect_minutes` | 5 | Mute, deafen and jail length **(owner: "mute them for 5 minutes")** |
| `immunity_optout` | true | Members may opt out of the economy |
| `immunity_shop` | true | Immunity is for sale |
| `immunity_mods` | true | Mods may grant immunity |
| `optout_cooldown_days` | 7 | How long before a member can switch opt-out again |
| `bought_immunity_hours` | 24 | How long bought immunity lasts |
| `gambling_enabled` | false | The coinflip, separately from `enabled` |
| `max_bet` | 500 | The most one coinflip can stake |

- Typed: `prices` is a typed map `ActionKind → ActionPrice { price: u32, on: bool }`,
  stored as JSONB through serde (never `serde_json::Value`). An action missing
  from a stored map takes its default, so a later release can add actions.
- Enabling needs a database. The dashboard refuses to save otherwise.
- Loaded at `GuildCreate`, held in memory, written through on save, as rooms do.

## 2. Credits

### 2.1 Wallets and the ledger

- `economy_wallets (guild_id, user_id)`: `balance` (BIGINT, never below 0),
  `earned_voice` and `earned_messages` (lifetime counters),
  `opted_out_at` and `optout_changed_at` (section 6.1).
- `economy_ledger`: one row for each movement that isn't routine earning. That
  covers purchases, refunds, gifts, coinflip stakes and wins, bounty and
  crowdfund escrow, and admin adjustments. Each row has `guild_id`, `user_id`,
  `delta`, a typed `kind`, an optional `action_id`, `created_at`, and for
  adjustments, who made it and why.
- **Earning is not in the ledger** (a row per member per minute is noise). It
  goes into the wallet's counters.
- Every debit is one transaction that locks the wallet row (`SELECT … FOR
  UPDATE`), checks the balance, and writes the ledger row. A balance can't go
  negative, even with two purchases at once.

### 2.2 Earning in voice

A once-a-minute tick, for each server with the economy on. It reads voice
states from the cache, so there are no sessions to rebuild after a restart.
A member earns `voice_credits_per_minute` for that minute when:

- they are in a voice channel that isn't the server's AFK channel;
- at least one **other** non-bot member is in that channel. Sitting alone
  doesn't pay, which stops overnight farming;
- they aren't self-deafened. Deafened is how Discord says "not here";
- they haven't opted out (section 6.1).

Each server's credits for the minute are written as one batched update.

### 2.3 Earning from messages

A new `MessageCreate` route. For a non-bot author in a server channel, the
message earns `message_credits` if their last counted message was at least
`message_cooldown_seconds` ago. The cooldown lives in memory, and losing it on
a restart costs at most one extra credit. The bot reads no content.

### 2.4 Commands

`/credits` (members):
- `balance [member]`;
- `top`: the ten richest in the server;
- `gift <member> <amount>`;
- `history`: your last 15 ledger rows, private;
- `optout` / `optin` (section 6.1).

Gifts refuse self-gifts, bots and opted-out members, and need a positive amount
you have.

## 3. Buying

Every purchase runs the same steps, in this order, and stops at the first that
fails. Nothing is charged until step 5:

1. **The economy and the action are on** in this server.
2. **The buyer may play.** They have a wallet and haven't opted out.
3. **The target may be targeted** (if the action has a target):
   - not a bot, and not the buyer, except for self-actions such as cleanse;
   - not out of reach. Their top role must sit below the bot's, and they
     mustn't be the server owner, for any action that changes them in Discord;
   - for a voice action, connected to voice;
   - not opted out, and not immune (section 6). A **mirror shield** is checked
     before immunity (section 4.2).
4. **The bot can do it.** That needs the per-action permission preflight
   (section 7), and the action's own conditions (for example, the buyer is in
   voice for drag). A running prank of the same kind on the same target refuses
   a second one, saying when the first ends.
5. **Charge**, in one transaction that writes the ledger row and an
   `economy_actions` row in state `pending`.
6. **Act** through the Discord seam (section 8). On success the row goes
   `active`, or `done` for instant actions, and the public card posts. On
   failure the row goes `failed`, the price is refunded with a ledger row, and
   the buyer is told why in plain words.

`economy_actions` is the source of truth. It records:
- `kind`, `buyer_id`, `target_id`, the price paid, the note, `signed`;
- `payload`: typed per kind, through serde. For example, the nickname set and
  the nickname to restore;
- `state`, `created_at`, `expires_at`, `ended_at`, `end_reason` (expired,
  cleansed, mod cancelled, immunity, opt-out, overridden by a mod, target left
  the server);
- `revealed_at` and `revealed_by`.

It is what the sweeper, reconcile, reveal, `/credits history` and the mod log
read.

## 4. The catalogue

All prices are defaults, set per server on the dashboard. Earning runs at about
60 an hour in voice with company, plus up to 60 an hour from chat, so 120 is
"two hours in voice". Every action except gift, cleanse and the coinflip takes
an optional `note` (up to 200 characters) and a `signed` flag (default off).

### 4.1 Pranks (aimed at a member, temporary)

| Action | Default price | What happens | Undoes itself |
|---|---|---|---|
| **rename** **(owner)** | 120 | Sets the target's nickname (1-32 characters, no mentions or links) | After `rename_hours`, the nickname from before the prank comes back |
| **lock** **(owner)** | 60 | Locks the target's nickname as it is. Their own changes are reverted at once (6.4) | At the end of the running rename, or `rename_hours` from now if none is running |
| **clown** | 30 | Adds 🤡 to the end of the target's current name (within 32 characters) | After `rename_hours` |
| **drag** **(owner)** | 60 | Moves the target, who must be in voice, into the buyer's voice channel | Instant; nothing to undo |
| **mute** **(owner)** | 90 | Server-mutes the target in voice | After `short_effect_minutes` |
| **deafen** | 90 | Server-deafens the target | After `short_effect_minutes` |
| **jail** | 150 | Moves the target into the jail channel and moves them back if they wander off | After `short_effect_minutes`; they're then free to go |

- **Rename and clown are the same slot.** Only one name prank runs on a
  target at a time.
- **The name to restore** is captured when the first name prank starts. Lock
  rides on top.
- **Drag never widens** (anti-goal). The target must be allowed View Channel
  and Connect in the destination by their own roles and the channel's
  overwrites, computed as rooms' never-widen check does. A destination that's
  full is refused unless Discord lets Move Members through (section 10).
- **Jail is exempt from never-widen** (my call, builder check Q4). The admin
  chose the jail channel, and that choice is the server's consent to members
  being moved there. The dashboard says so under the jail picker: "Members are
  moved here even if they can't see it."
- **When a timed effect ends, the bot restores only what it changed.** Two
  examples:
  - a mute ends with an unmute unless a moderator changed the mute meanwhile;
  - a rename ends by restoring the old nickname unless someone other than
    the target or the bot renamed them meanwhile (section 6.4).

### 4.2 Defence (bought for yourself)

| Action | Default price | What happens |
|---|---|---|
| **immunity** **(owner)** | 200 | Immune for `bought_immunity_hours`. Blocks new actions only; running ones carry on (cleanse ends those) |
| **mirror** | 150 | A one-time shield: the next **4.1 prank** someone buys directly against you lands on **them** instead, with their own parameters (their chosen name goes on them). Dedications, themes, bounties and crowdfunds don't trigger or use it up (my call, builder check Q5). A bounced drag would move the attacker into their own channel, so it's blocked. Not visible to anyone before it fires. If the bounced action can't land on the attacker (they're immune, or out of the bot's reach), it's simply blocked. The attacker is charged either way: that's the mirror's sting. The card says what happened |
| **cleanse** | 80 | Ends every running prank on you now, with each one undone as if it had expired |
| **rename back** **(owner)** | as rename | Not a separate item. It's a rename aimed at whoever renamed you. The reply to a rename offers a "Rename them back" button that opens the rename form with the target filled in |

### 4.3 Unmasking

| Action | Default price | What happens |
|---|---|---|
| **reveal** **(owner)** | 100 | Names the buyer of the most recent unsigned action aimed at you (or one you pick from your last ten), publicly, in the channel where you ran it. A revealed action can't be revealed again; the command tells you so before charging |

### 4.4 Music (CrackTunes' own turf)

| Action | Default price | What happens |
|---|---|---|
| **theme** | 100 | A walk-on theme: the next time the target joins voice (within 7 days), CrackTunes plays a clip of up to 10 seconds, from a link and start time the buyer gives. It plays only where the bot can: mixed over the music if the bot is already in that channel, or by joining, playing and leaving if the bot is idle in the server. If the bot is busy in another channel, the theme waits for a later join |
| **dedicate** | 40 | Queues a song like `/play`, and the announcement says "This one goes out to @target", with the note |
| **jump** | 40 | Queues a song to play next, like `/playnext`. A server that wants this as a paid perk restricts `/playnext` for members in Discord's Integrations settings. The dashboard says so |

The theme's link goes through the same guard every link does
(`crack_types::ytdl_for_url`, http(s) only, no private addresses). `dedicate`
and `jump` follow the music commands' existing checks (the buyer is in the
music channel), and are blocked during a `/gp` game (`GP_BLOCKED_COMMANDS`).

### 4.5 Group play

| Action | What happens |
|---|---|
| **gift** | `/credits gift` (2.4). No price |
| **bounty** | Put any amount (minimum 50) on a member's head. It's escrowed. Whoever next lands a prank (4.1) on them collects it all, except the person who placed it. A bounty unclaimed after 7 days is refunded. Immunity blocks bounties on you |
| **crowdfund** | Start a fund for any 4.1 action against a target, with its parameters and note. Others pledge. When pledges reach the price it executes, with the target re-checked at that moment; if it can't land, everyone is refunded. Unfunded after 24 hours, everyone is refunded. The mod log names every contributor; the card names those who signed |

## 5. What people see

### 5.1 Commands

- **`/shop`** (members): `menu` shows every action that's on, with its price,
  in an embed, the economy's own documentation. Then one subcommand per
  action: `rename <member> <name>`, `lock <member>`, `clown <member>`,
  `drag <member>`, `mute <member>`, `deafen <member>`, `jail <member>`,
  `immunity`, `mirror`, `cleanse`, `reveal`, `theme <member> <link> [start]`,
  `dedicate <member> <song>` and `jump <song>`. Each takes `note` and `signed`
  where 4 says so.
- **`/bounty`**: `place <member> <amount> [note] [signed]`, `list`.
- **`/crowdfund`**: `start <action> <member> …`, `pledge <fund> <amount>`, `list`.
- **`/coinflip <bet> <heads|tails>`** (section 9).
- **`/economy`** (mods: Moderate Members):
  - `status`: settings, the permission report, and what's running;
  - `grant <member> [hours] [reason]` and `lift <member>`, for immunity
    (section 6);
  - `cancel <member>`: ends every running prank on them, undone as if
    expired;
  - `adjust <member> <amount> <reason>`: needs Manage Server, and is logged.
- New commands join the registration lists. Opted-in servers receive them at
  the next startup refresh (manual registration, v0.30.0).

### 5.2 Replies

The buyer's reply is private: the outcome, the balance left, and for a rename
a "Rename them back" button sent to the **target's** card instead (5.3). A
refusal names the step that failed in plain words, such as "Mika is immune
until 21:40" or "You need 40 more credits", and nothing is charged.

### 5.3 Public cards

Every action that lands posts a card (an embed):
- what happened, to whom, and until when;
- the note;
- either "— @buyer" when signed, or "— someone" when not.

Where the card goes:
- **Drag, jail, mute, deafen and a theme:** the chat of the voice channel the
  target is in after the action. That's where the room is.
- **Everything else:** the channel the command ran in, or `announce_channel_id`
  when that's set.

The target's card for a rename carries **Rename them back** and **Reveal**
buttons. Rename them back opens the rename form against the buyer, but only
once the buyer is known: signed, or revealed. While the buyer is anonymous,
pressing it says "Reveal who did it first" (my call, builder check Q2).
Otherwise a revenge rename would unmask the hidden buyer for the price of a
rename, and nobody would buy a reveal.

### 5.4 The mod log

> **Superseded 2026-10-28 by `2026-10-28-economy-log-and-room-mute-design.md`.** The economy's events moved to a separate economy log, the mod log carries literal shop lines with no buyer, and "the buyer is always named" below was reversed.

New `ModLogEvent` variants, posted directly (exact, like room kicks):
- `ShopAction { action, buyer, target, note, signed, until }`: every action
  that changes a member in Discord, mirrored ones included, which carry the
  original attacker;
- `ImmunityGranted { by, target, until, reason }` and `ImmunityLifted { by, target }`;
- `ShopCancelled { by, target }`;
- `CreditsAdjusted { by, target, amount, reason }`;
- `CrowdfundExecuted { action, target, contributors }`.

**The buyer is always named here**, signed or not (owner). The audit-derived
`ServerMute`/`ServerDeafen` and move rows for the bot's own shop actions are
**not** posted as CrackTunes' own. The shop event replaces them, so the log
reads "Mika (via the shop) muted Jo", never "CrackTunes muted Jo". The mod
log's existing "CrackTunes' own rows stay consumed" rule already does this for
moves. For mutes and deafens, the shop marks an *expected* audit change, as
rooms do for kicks. If the mod log is off, nothing posts, and the
`economy_actions` rows remain the record (`/economy status` shows recent ones).

## 6. Immunity (first-class)

A member is **protected** when any enabled source covers them. A protected
member can't be the target of any action in section 4 (pranks, drag, theme,
dedicate, a bounty or a crowdfund aimed at them). Gifts still reach them.
`is_protected(member, now)` is one pure function over the three sources, and
every purchase and every bounty claim calls it.

**Switching a source off stops new grants only** (my call, builder check Q3),
the same rule 6.1 gives opt-out. Immunity already bought or granted runs out
its term, so nobody loses what they paid for. So `is_protected` reads the
stored grants and never the switches, and the switches only gate creating a
grant.

### 6.1 Opting out (`immunity_optout`)

- `/credits optout`: you're out of the economy. Out means the following.
  - Nobody can target you.
  - You can't buy, earn or gamble. Your balance is kept, frozen.
  - Running pranks on you end now, undone.
  - Pranks you bought that are running on others carry on. They were paid for.
- `/credits optin` brings you back.
- Either switch is refused within `optout_cooldown_days` of the last one,
  which stops "opt out when pranked, opt back in to retaliate".
- A server that turns `immunity_optout` off stops new opt-outs. Existing ones
  stay until the member opts back in.

### 6.2 Bought immunity (`immunity_shop`)

The shop item in 4.2, for `bought_immunity_hours`. It doesn't end running
pranks. Buying more extends it from whichever is later: now, or its current
end.

### 6.3 Granted by a mod (`immunity_mods`)

- `/economy grant <member> [hours] [reason]`, by anyone with Moderate Members.
  With no hours it lasts until lifted.
- A grant ends running pranks on the member now, undone.
- `/economy lift` ends a mod's grant. It doesn't touch bought immunity,
  which only ever blocks shop actions.
- Both post to the mod log.

### 6.4 The nickname lock and moderators

The lock and the rename restore read who changed a nickname from the audit log
(`GuildAuditLogEntryCreate`, `MEMBER_UPDATE` with a `nick` change, which the
mod log already receives). The executor decides:
- the target themselves: reverted at once, while a lock runs;
- the bot: ignored;
- anyone else, such as a mod: the mod wins. The lock and the name prank end
  with `end_reason = overridden`, and nothing is restored later.

This needs View Audit Log. Without it, `lock` isn't sold (the preflight says
why). A rename then simply restores at the end; a mod who wants it gone sooner
uses `/economy cancel`.

## 7. Permission preflight

The same shape as rooms' and the mod log's: a pure function over the bot's
computed permissions. It returns `(where, missing)` in words, shown by
`/economy status` and the dashboard. Each action is sold only while its needs
are met; `/shop menu` marks the others as unavailable and says why.

| Action | Needs |
|---|---|
| rename, clown | Manage Nicknames |
| lock | Manage Nicknames, View Audit Log |
| drag | Move Members; View Channel + Connect on both channels |
| mute | Mute Members |
| deafen | Deafen Members |
| jail | Move Members; View Channel + Connect on the jail channel |
| theme | Connect + Speak where it plays |
| cards | View Channel, Send Messages, Embed Links where they post |

## 8. Timed effects: the sweeper and reconcile

- **A sweeper** runs every 15 seconds. For each `active` row past `expires_at`,
  it undoes the effect and marks the row `ended`.
- **Undo for voice state.** Discord refuses to change mute or deafen for a
  member who isn't connected. Such a row goes `ending` and is finished by the
  `VoiceStateUpdate` of that member's next join, before anything else reacts
  to it.
- **Reconcile at `GuildCreate`** (startup and reconnects) finishes every
  overdue row for the guild the same way, and re-arms the lock watcher for
  running locks.
- **A target who left the server** ends their rows with `end_reason = left`,
  and nothing is restored.
- **Jail** re-moves a jailed member who moves to another voice channel while
  the sentence runs, at most once per 10 seconds per member. Disconnecting is
  allowed.
  - Discord's move audit entries name no target, and a member moving
    themselves writes none, so the bot can't tell "wandered off" from "a mod
    moved them".
  - **So jail moves back every move** (my call, builder check Q1). A mod ends a
    jail with `/economy cancel` or a grant, the same fallback the lock uses
    without View Audit Log. The anti-goal "never undo a moderator" holds for
    every action whose author the bot *can* read (mute, deafen and nickname).
    For jail it's honoured through the cancel and grant levers, and the
    sentence is only `short_effect_minutes` long anyway.
- **The Discord seam** is a `trait EconomyDiscord` (async) with a real impl
  over `Http` and a fake that **records every call** for tests. It covers
  `set_nick`, `set_mute`, `set_deaf`, `move_member` and `post_card`. Cache
  reads (voice states, member roles, the channel overwrites never-widen
  needs) are gathered by thin glue into plain snapshot structs, so the
  decisions are pure functions.

## 9. Gambling: the coinflip, and where it goes next

- **`/coinflip <bet> <heads|tails>`**, when `gambling_enabled`.
  - The bet must be between 1 and `max_bet`, and within your balance.
  - 50/50, from the operating system's RNG through `rand`, with no house edge.
  - A win pays the bet back plus the same again. A loss takes the bet.
  - Both are ledger rows. The reply is public, so the table can cheer.
- Opted-out members can't play.
- The dashboard's gambling switch carries one line: "Credits have no cash
  value. Only turn this on where your members are adults." Saucey's server is
  21+.
- **Where to go next** (not built now; the note the owner asked for):
  - **duels:** a coinflip between two members for a shared pot;
  - **dice** and **higher-or-lower**;
  - **slots**, with a payout table;
  - **blackjack** against the bot, with buttons;
  - a **daily spin** for a small free amount;
  - a **lottery** whose pot is drawn weekly.

  Each needs its own house edge decision. As long as the edge is zero, credits
  don't drain; anything above zero is a credit sink. Keep each game behind
  `gambling_enabled`, and keep the ledger as the record.

## 10. Unverified Discord behaviour (check on TuneTitan)

| Assumption | If wrong |
|---|---|
| A member changing their own nickname writes a `MEMBER_UPDATE` audit entry with themselves as executor | The lock can't tell self from mod. Then it reverts every change not made by the bot, and mods use `/economy cancel`. The preflight note changes to say so |
| PATCHing mute or deaf for a member not in voice fails (400) | The `ending` path is then just never needed; nothing breaks |
| Move Members lets the bot drag someone into a channel at its user limit | Drag and jail into a full channel are refused before the charge |
| songbird mixes a second track over the queue in the same Call (the theme) | The theme plays only when the bot is idle |
| A bot nickname change is refused for the server owner and for members at or above the bot's top role | Already refused before the charge, from the role check |

## 11. Testing

Per [[cracktunes-standing-rules]]: sabotage every test, assert on what is sent,
and no source scans.

- **Pure, exhaustively:**
  - the purchase steps in section 3, for every refusal and its message;
  - `is_protected` for every source combination and expiry;
  - the mirror and immunity ordering;
  - the voice-earning eligibility in 2.2;
  - the message cooldown;
  - nickname validation and the clown cap at 32 characters;
  - never-widen for drag;
  - the preflight table;
  - the lock's executor rule in 6.4;
  - bounty claim and refund, and crowdfund execution and refunds;
  - the coinflip settlement, with the RNG injected.
- **Glue, against the fake seam:**
  - a rename sets the nickname, and the sweeper restores the old one;
  - a mod's rename in between ends the prank and is not restored;
  - a mute's undo for a member who left voice lands on their next join;
  - a failed Discord call refunds and leaves no running row;
  - reconcile after a "restart" finishes overdue rows;
  - jail re-moves a wanderer;
  - every shop action emits its mod log event naming the buyer, signed or not.
- **DB (`sqlx::test`):**
  - two simultaneous purchases can't take a balance below zero;
  - the ledger and wallet agree after purchase, refund, gift, coinflip and
    adjust;
  - settings round-trip with a partial `prices` map.
- **crack-web:** 403 for a non-manager, the exact settings the backend
  receives, and validation refusals.
- **The TuneTitan checklist** covers section 10 and whatever the fakes can't
  see.

## 12. Delivery

Three PRs, one after another. Each one is merged, tagged, released by
cargo-dist and deployed to TuneTitan before the next one starts. Production
waits for the owner, after the voice-rooms soak.

- **PR 1, `feat/economy-core` → v0.32.0:**
  - sections 1, 2 and 6, plus the purchase machinery of section 3 with
    **immunity** as its first item;
  - the coinflip (9);
  - `/economy` grant, lift, adjust and status, and their mod log events;
  - the dashboard's Economy section;
  - the docs page.
- **PR 2, `feat/economy-pranks` → v0.33.0:** 4.1, 4.2 (mirror, cleanse,
  rename back), 4.3, section 5's cards and buttons, the shop mod log events, 7,
  and 8.
- **PR 3, `feat/economy-music-and-groups` → v0.34.0:** 4.4 and 4.5.
- **Migrations:** each PR's own, in both `migrations/` and
  `crack-core/test_migrations/`, with `.sqlx` regenerated. All of them are
  additive. 🔙 Rolling back is image-only: an older image ignores the new
  tables.

## 13. Docs and dashboard

- **`docs/economy.md`:** what the economy is, how to turn it on, the
  catalogue with what each action does and how long, immunity, the permission
  table, and the "credits have no cash value" line. The dashboard and
  `/shop menu` link to it.
- **Dashboard `GET /g/{g}/settings`, a new Economy section** (Manage Server,
  as rooms):
  - on/off, the currency name and emoji;
  - the earning rates;
  - a table of actions, each with on/off and price;
  - the durations;
  - the three immunity switches, the opt-out cooldown and the bought-immunity
    length;
  - the announce and jail channels;
  - gambling on/off and the max bet;
  - the permission report.

  It saves through `POST /g/{g}/settings/economy` (JSON, server-side
  validation), the same shape as rooms.

## 14. Left to the builder

Settled here so the plan doesn't have to guess (my calls, from the builder
check):
- **No partial refunds** when a prank ends early (cleanse, cancel, a grant,
  an opt-out, a mod's override). It landed, so it was paid for.
- **A theme that never plays** in its 7 days is refunded, since charging for
  nothing is an anti-goal.
- **The sweeper and reconcile keep running** for a server that turns the
  economy off. Otherwise a rename would never be restored.
- **A row found `pending` at reconcile** (a crash between charge and act) is
  resolved by looking at Discord. If the effect landed (for a rename, the
  nickname is the one set), the row goes `active`. Otherwise it goes `failed`
  and is refunded.
- **The economy consumes audit entries itself**, whether or not the mod log is
  on. The lock and the mute/deafen undo rules need them either way.
- **"Expected audit change" for mutes and deafens** is new. Today mutes and
  deafens post straight from the audit entry (`modlog/live.rs`). Add a marker
  like rooms' `expect_disconnect`, so the shop's own mute posts as a
  `ShopAction` and never as CrackTunes'.
- **`/economy adjust` checks Manage Server in code.** The command's default
  permission is Moderate Members.

- Module layout. Suggested: `crack-core/src/economy/`, with these modules:
  - `settings`, `wallet` (with the ledger), `earn`;
  - `shop` (the purchase steps), `catalogue` (kinds, defaults, payloads);
  - `immunity`, `effects` (the sweeper and reconcile), `lock`;
  - `preflight`, `discord` (the seam), `cards`, `gamble`.

  Commands go under `crack-core/src/commands/economy/`.
- Exact copy and embed colours, within the strings-in-`messages.rs` rule.
- How the earning tick and sweeper are scheduled. One task each for the
  process, iterating over enabled guilds, is the obvious shape.
- The order of the per-PR task breakdown.
