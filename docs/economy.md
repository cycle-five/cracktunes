# The economy: credits, immunity and a coinflip

CrackTunes can run a small economy in your server. Members earn **credits** by
spending time in voice and by chatting, check their balance, gift credits to
each other, and spend them in a shop. In this first release the shop sells one
thing, **immunity**, and there is an optional **coinflip**.

The economy is **off by default** in every server, and free: no premium needed.
It needs the bot to have a database. If yours doesn't, it can't be turned on.

**Credits have no cash value.** They can't be bought with money, and they can't
be cashed out. They exist only inside your server.

- [For admins](#for-admins)
- [How members earn credits](#how-members-earn-credits)
- [Commands for members](#commands-for-members)
- [Commands for moderators](#commands-for-moderators)
- [Immunity](#immunity)
- [The coinflip](#the-coinflip)
- [Coming next](#coming-next)

## For admins

You need **Manage Server** to change the economy's settings.

### Turning it on

Open dash.cracktun.es, pick the server, then **Settings**, then the **Economy**
section. Tick "Economy is on" and press Save. Until you do, every economy
command replies privately "The economy is off in this server." and does nothing.

### The settings

| Setting | What it does | Default | Allowed |
|---|---|---|---|
| Currency name | What the credits are called | credits | 1 to 24 characters, no control characters |
| Currency emoji | Shown next to amounts | 🪙 | 1 to 64 characters, no spaces |
| Credits per minute in voice | Paid each minute a member qualifies | 1 | 0 to 100 |
| Credits per message | Paid for each counted message | 1 | 0 to 100 |
| Seconds between paid messages | At most one counted message per member in this time | 60 | 10 to 3600 |
| Shop prices | The price of each item, and whether it is for sale | immunity: 200, for sale | 1 to 1,000,000 |
| Members may opt out for immunity | Whether `/credits optout` works | on | |
| Members may buy immunity | Whether the shop sells immunity | on | |
| Moderators may grant immunity | Whether `/economy grant` works | on | |
| Days between opting out and back in | How soon a member can switch again | 7 | 0 to 90 |
| Hours of bought immunity | How long one purchase lasts | 24 | 1 to 720 |
| Gambling (coinflip) is on | Whether `/coinflip` works | off | |
| Largest bet | The most one coinflip can stake | 500 | 1 to 1,000,000 |

Setting a rate to 0 turns that kind of earning off. A value outside its range
is refused when you save, and nothing is changed.

Turning the economy off keeps every balance. Turning it back on picks up where
it left off.

## How members earn credits

**In voice**, a member is paid every minute, as long as all of these hold:

- there is **at least one other person** in the channel with them (bots don't
  count as company);
- they are **not self-deafened**;
- they are **not in the server's AFK channel**;
- they haven't opted out of the economy.

Someone who is opted out, or deafened, still counts as company for the others.
Sitting alone, or alone with a bot, earns nothing.

**By chatting**, a member earns for one counted message per cooldown (60
seconds unless you change it). Messages sent inside the cooldown aren't counted.
CrackTunes only counts messages: it **never reads what they say**. Messages are
paid out together with the voice credits, so they arrive within about a minute.

Bots never earn, and a server whose settings haven't loaded yet pays nothing.

## Commands for members

All of these are slash commands, and work only in a server where the economy is on.

| Command | What it does |
|---|---|
| `/credits balance [member]` | Shows your credits, or another member's. Public. |
| `/credits top` | The 10 richest members, leaving out opted-out members. Public. |
| `/credits gift <member> <amount>` | Gives some of your credits to someone else. Public. |
| `/credits history` | Your last 15 movements of credits: gifts, purchases, refunds, bets and moderator adjustments. Earnings are not rows: they appear as the "Earned so far" totals (voice and messages) on the first line. Only you see it. |
| `/credits optout` | Opts you out of the economy (see [Immunity](#immunity)). It tells you when you can switch back. |
| `/credits optin` | Opts you back in. |
| `/shop menu` | Lists what is for sale and the prices, with a link to this page. |
| `/shop immunity` | Buys immunity. A public card says who is immune until when, and you privately see what you have left. Refused if you already hold a moderator grant that lasts until lifted. Pranks arrive in the next update. |
| `/coinflip <bet> <heads\|tails>` | Bets on a coin toss, if the server allows it. |

A balance is never negative. You can't gift to yourself or to a bot, and you
must have the credits. A gift or a purchase that you can't afford is refused
with how much you have, or how many more you need.

An **opted-out** member keeps their balance, but can't earn, buy, gamble, gift
or be gifted.

## Commands for moderators

`/economy` is visible only to **Moderate Members** by default (Discord's
Integrations settings can change who sees it). Every `/economy` command also
refuses, privately, while the economy is off.

| Command | Who may run it | What it does |
|---|---|---|
| `/economy status` | **Moderate Members** | Shows the currency, how many wallets the server has (zero balances and opted-out members count) and how many credits are held in total, the earning rates, the three immunity switches and the gambling settings. Only you see it. |
| `/economy grant <member> [hours] [reason]` | **Moderate Members** | Makes a member immune for 1 to 8760 hours, or until lifted if you leave the hours out. Refused if the server has turned off moderator grants. |
| `/economy lift <member>` | **Moderate Members** | Removes a moderator grant. Always allowed, even if grants are switched off. It doesn't touch immunity the member bought. |
| `/economy adjust <member> <amount> <reason>` | **Manage Server**, on top of Moderate Members | Adds credits, or removes them with a negative amount. The reason is required. Refused if it would take the member below zero (it tells you what they hold), or if the amount is 0 or larger than 1,000,000,000 either way. |

Bots can't be granted immunity or have their credits adjusted.

If you have [the mod log](rooms-and-mod-log.md#mod-log) turned on, `grant`,
`lift` and `adjust` are posted to it, with who did it, to whom and why.

## Immunity

Immunity is protection from the pranks the shop will sell in a later release
(see [Coming next](#coming-next)). It exists from the start so that nobody ever
has to take part in something they didn't choose. A server can allow any mix of
three sources:

1. **Opting out.** `/credits optout` puts a member outside the economy. They
   keep their balance, which sits frozen, but they can't earn, buy, gamble, gift
   or be gifted, and they can't be targeted. Switching either way is refused
   until the cooldown since the last switch has passed (7 days by default),
   which stops "opt out when pranked, opt back in to retaliate". If the
   cooldown is set to 0, there is no wait.
2. **Buying it.** `/shop immunity` costs 200 credits by default and lasts 24
   hours. Buying again before it ends adds the time on, counting from whichever
   is later: now, or the current end.
3. **A moderator grant.** `/economy grant`, for some hours or until lifted.

**Turning a source off stops new grants only.** If you switch off buying, nobody
can buy immunity from then on, but immunity already bought runs out its full
term, so nobody loses what they paid for. The same goes for moderator grants
and for opting out: members who are already opted out stay opted out until they
choose to come back (switching back in is always allowed, subject to the
cooldown). The three switches decide what can be *created*, never what already
exists.

Gifts still reach a member who is immune by a purchase or a grant. Only an
opted-out member can't be gifted.

In this release nothing can target anyone yet, so immunity protects against
nothing until the prank shop arrives.

## The coinflip

Off by default. Turn it on with "Gambling (coinflip) is on" in the settings,
and set the largest bet. The dashboard reminds you: credits have no cash
value, and you should only turn this on where your members are adults.

`/coinflip <bet> <heads|tails>` stakes some of your credits on a call. It is 50/50
with no house edge. If you call it right you get your stake back and the same
again; if you don't, the bet is gone. The result is posted publicly.

The bet must be at least 1, no more than the server's largest bet, and no more
than you hold. Opted-out members can't play.

## Coming next

- **The prank shop** (the next release): credits will buy actions the bot
  performs for a limited time, such as renaming a member, locking their
  nickname, dragging them into a voice channel, muting or deafening them for a
  few minutes, and more. Credits never buy roles or permissions, only things the
  bot does itself, and the mod log will always name the buyer. Immunity is what
  protects members from all of it.
- **A test-network coin you can see in a crypto wallet: worthless by design, later.**
