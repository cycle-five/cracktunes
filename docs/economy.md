# The economy: credits, the prank shop, immunity and a coinflip

CrackTunes can run a small economy in your server. Members earn **credits** by
spending time in voice and by chatting, check their balance, gift credits to
each other, and spend them in a shop. The shop sells **immunity**, a few
**defences**, a catalogue of **pranks** the bot performs on another member
for a little while, and **music and group play**: walk-on themes, dedications,
queue jumps, bounties and crowdfunds. There is also an optional **coinflip**.

The economy is **off by default** in every server, and free: no premium needed.
It needs the bot to have a database. If yours doesn't, it can't be turned on.

**Credits have no cash value.** They can't be bought with money, and they can't
be cashed out. They exist only inside your server.

- [For admins](#for-admins)
- [How members earn credits](#how-members-earn-credits)
- [Commands for members](#commands-for-members)
- [Commands for moderators](#commands-for-moderators)
- [Immunity](#immunity)
- [The prank shop](#the-prank-shop)
- [Music](#music)
- [Group play](#group-play)
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
| Shop prices | The price of each item, and whether it is for sale. A bounty's price is its **minimum stake**, a crowdfund's its **minimum pledge** | see [the catalogue](#the-catalogue), [Music](#music) and [Group play](#group-play), all for sale | 1 to 1,000,000 |
| Members may opt out for immunity | Whether `/credits optout` works | on | |
| Members may buy immunity | Whether the shop sells immunity | on | |
| Moderators may grant immunity | Whether `/economy grant` works | on | |
| Days between opting out and back in | How soon a member can switch again | 7 | 0 to 90 |
| Hours of bought immunity | How long one purchase lasts | 24 | 1 to 720 |
| Gambling (coinflip) is on | Whether `/coinflip` works | off | |
| Largest bet | The most one coinflip can stake | 500 | 1 to 1,000,000 |
| Hours a rename, lock or clown lasts | How long those three last | 24 | 1 to 168 |
| Minutes a mute, deafen or jail lasts | How long those three last | 5 | 1 to 60 |
| Jail channel | The voice channel jailed members are moved to. Without one, jail isn't sold | none | a voice channel of the server |
| Announcement channel | Where prank cards are posted, when they aren't posted in a voice channel's chat | none (cards go where the command was run) | a text channel of the server |

Setting a rate to 0 turns that kind of earning off. A value outside its range
is refused when you save, and nothing is changed.

Under the bounty, crowdfund and queue jump prices the dashboard adds a short
hint: "Minimum stake.", "Minimum pledge. A fund's goal is its prank's price.",
and, for jumping, how to make it a perk (below).

**Making queue jumping a paid perk.** `/shop jump` queues a song to play next,
like `/playnext`. If you want jumping the queue to be something members only get
by paying, restrict `/playnext` for members in Server Settings → Integrations
(Discord's own command permissions). Then `/shop jump` is the way in.

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
| `/shop menu` | Lists what is for sale and the prices, in sections (pranks, Music, Group play), with a link to this page. A bounty and a crowdfund read "from" their minimum. A prank CrackTunes can't do right now (a missing permission, no jail channel) is marked as not sold here, with why. |
| `/shop immunity` | Buys immunity. A public card says who is immune until when, and you privately see what you have left. Refused if you already hold a moderator grant that lasts until lifted. |
| `/shop rename\|lock\|clown\|drag\|mute\|deafen\|jail <member>` | Buys a [prank](#the-prank-shop) against another member. `rename` also takes the new nickname. Each takes an optional `note` and `signed`. |
| `/shop mirror`, `/shop cleanse`, `/shop reveal` | The [defences and the unmasking](#mirror-cleanse-and-reveal). |
| `/shop theme <member> <link> [start] [note] [signed]`, `/shop dedicate <member> <song> [note] [signed]`, `/shop jump <song> [note] [signed]` | The [music items](#music). |
| `/bounty place <member> <amount> [note] [signed]`, `/bounty list` | [Bounties](#bounties). |
| `/crowdfund start`, `/crowdfund pledge`, `/crowdfund list` | [Crowdfunds](#crowdfunds). |
| `/coinflip <bet> <heads\|tails>` | Bets on a coin toss, if the server allows it. |

A balance is never negative. You can't gift to yourself or to a bot, and you
must have the credits. A gift or a purchase that you can't afford is refused
with how much you have, or how many more you need.

An **opted-out** member keeps their balance, but can't earn, buy, gamble, gift
or be gifted.

## Commands for moderators

`/economy` is visible only to **Moderate Members** by default (Discord's
Integrations settings can change who sees it). Every `/economy` command except
`cancel` and `status` also refuses, privately, while the economy is off. Those
two need only the database, so moderators can still see and end pranks that
are running when the economy is turned off.

| Command | Who may run it | What it does |
|---|---|---|
| `/economy status` | **Moderate Members** | Shows the currency, how many wallets the server has (zero balances and opted-out members count) and how many credits are held in total, the earning rates, the three immunity switches and the gambling settings. Then what CrackTunes needs for each prank, in the dashboard's words, and the last 10 shop actions (fewer, saying how many were left out, if they don't fit in one message), newest first: what, who bought it (signed or not), on whom, its state, and when it ends or why it ended. Only you see it. |
| `/economy grant <member> [hours] [reason]` | **Moderate Members** | Makes a member immune for 1 to 8760 hours, or until lifted if you leave the hours out. Refused if the server has turned off moderator grants. |
| `/economy lift <member>` | **Moderate Members** | Removes a moderator grant. Always allowed, even if grants are switched off. It doesn't touch immunity the member bought. |
| `/economy cancel <member>` | **Moderate Members** | Ends every prank running on a member now, each undone as if it had expired. It says how many it ended, or that nothing was running. |
| `/economy adjust <member> <amount> <reason>` | **Manage Server**, on top of Moderate Members | Adds credits, or removes them with a negative amount. The reason is required. Refused if it would take the member below zero (it tells you what they hold), or if the amount is 0 or larger than 1,000,000,000 either way. |

Bots can't be granted immunity or have their credits adjusted.

If you have [the mod log](rooms-and-mod-log.md#mod-log) turned on, `grant`,
`lift`, `cancel` and `adjust` are posted to it, with who did it and to whom (and
why, for `grant` and `adjust`). So is every prank that changes a member, which
always names the buyer, signed or not (see [The prank shop](#the-prank-shop)),
and so is every walk-on theme, dedication and bounty, and every crowdfunded
prank that runs, which names all its contributors (see [Music](#music) and
[Group play](#group-play)).

## Immunity

Immunity is protection from [the shop's pranks](#the-prank-shop). It exists so
that nobody ever has to take part in something they didn't choose. A server can allow any mix of
three sources:

1. **Opting out.** `/credits optout` puts a member outside the economy. They
   keep their balance, which sits frozen, but they can't earn, buy, gamble, gift
   or be gifted, and they can't be targeted. Switching either way is refused
   until the cooldown since the last switch has passed (7 days by default),
   which stops "opt out when pranked, opt back in to retaliate". If the
   cooldown is set to 0, there is no wait. Opting out also ends any prank
   running on you, undone as if it had expired. Pranks you bought that are
   running on others carry on.
2. **Buying it.** `/shop immunity` costs 200 credits by default and lasts 24
   hours. Buying again before it ends adds the time on, counting from whichever
   is later: now, or the current end.
3. **A moderator grant.** `/economy grant`, for some hours or until lifted.
   Granting also ends any prank running on that member now.

**Turning a source off stops new grants only.** If you switch off buying, nobody
can buy immunity from then on, but immunity already bought runs out its full
term, so nobody loses what they paid for. The same goes for moderator grants
and for opting out: members who are already opted out stay opted out until they
choose to come back (switching back in is always allowed, subject to the
cooldown). The three switches decide what can be *created*, never what already
exists.

Immunity also blocks bounties on you. If you become immune or opt out, every
walk-on theme waiting for you is refunded to its buyer.

Immunity only blocks new pranks. Bought immunity does not end a prank that is
already running; a [cleanse](#mirror-cleanse-and-reveal) does.

Gifts still reach a member who is immune by a purchase or a grant. Only an
opted-out member can't be gifted.

## The prank shop

Credits buy things the bot does itself. They never buy roles or permissions.
Members use `/shop <prank> <member>`, and CrackTunes makes the change in
Discord, then posts a public **card** saying what happened, to whom, until when,
and the buyer's note if there is one. The card says "someone" unless the buyer
chose `signed`; **the mod log always names the buyer** (see below).

Every prank takes an optional `note` (up to 200 characters, no mentions or
links) and a `signed` choice (default off). The buyer's own reply is private and
says what is left of their balance. A purchase that is refused is not charged,
and says why in plain words, for example that the target is immune until a
given time, that you need more credits, or that the target must be in a voice
channel.

### The catalogue

Prices are the defaults; set your own under **Shop prices** on the dashboard,
and turn any item off there. All of these need the economy to be on.

| Prank | Price | What it does | How long |
|---|---|---|---|
| `rename` | 120 | Sets the target's nickname to what the buyer typed (1 to 32 characters; emoji count as 2 or more; no mentions or links). | The server's hours for renames (24 by default), then the old nickname comes back |
| `lock` | 60 | Locks the target's nickname as it is. If they change it, it is changed back. | Until the running rename or clown ends; if none is running, the server's hours for renames |
| `clown` | 30 | Adds a clown to the end of the target's current name. | The server's hours for renames |
| `drag` | 60 | Moves the target, who must be in voice, into the buyer's voice channel. The buyer must be in voice too, and the target not already there. | Instant; nothing to undo |
| `mute` | 90 | Server-mutes the target in voice. Refused if they are already server-muted (by a moderator, say): the shop never takes over someone else's mute, so its end can never lift it. | The server's minutes for short effects (5 by default) |
| `deafen` | 90 | Server-deafens the target in voice. Refused if they are already server-deafened, likewise. | The server's minutes for short effects |
| `jail` | 150 | Moves the target, who must be in voice, into the server's jail channel, and moves them back if they move to another channel. | The server's minutes for short effects; then they are free to go |

- **Rename and clown share one slot.** Only one name prank runs on a member at a
  time, and buying a second of the same kind while one runs is refused. A lock
  rides on top of a name prank.
- **Who can be pranked.** Not bots, not yourself, not the server owner, and
  not anyone whose highest role is at or above CrackTunes' own highest role
  (Discord doesn't let the bot change them). Not members who have opted out of
  the economy, and not immune members. If CrackTunes can't place someone's
  roles right now (rare, just after a restart), it says so and charges
  nothing; try again in a moment.
- **Drag never widens anyone's access.** It is refused unless the target is
  allowed to see and join the buyer's channel by their own roles.
- **Jail is the exception to that.** The admin picked the jail channel, and
  that choice is the server's consent to members being moved there, even if
  they can't normally see it. The dashboard says so under the picker. A jailed
  member who moves to any other channel is moved back (at most once every 10
  seconds; one who moves out again inside those 10 seconds is moved back when
  they are up), so a moderator who wants a jail over sooner uses `/economy cancel`
  or an immunity grant. Disconnecting is allowed. Jail isn't sold until a jail
  channel is set.

### What the bot needs

Each prank is sold only while CrackTunes has what it needs. If something is
missing, the purchase is refused before charging, in words, and the dashboard's
Economy section lists the gaps.

| Prank | CrackTunes needs |
|---|---|
| rename, clown | Manage Nicknames |
| lock | Manage Nicknames and View Audit Log (the audit log is how it tells the member's own change from a moderator's) |
| drag | Move Members; View Channel and Connect in both channels |
| mute | Mute Members |
| deafen | Deafen Members |
| jail | Move Members; View Channel and Connect in the jail channel |
| the cards | View Channel, Send Messages and Embed Links in the channel they post to |

Immunity, mirror, cleanse and reveal need no permissions. If the card can't be
posted where it should go, it is posted in the channel the command was run in
instead.

Cards for drag, jail, mute and deafen go to the chat of the voice channel the
target is in; every other card goes to the announcement channel, or to where
the command was run if none is set.

### Mirror, cleanse and reveal

- **`/shop mirror`** (150) is a one-time shield. The next prank anyone buys
  directly against you lands on **them** instead, with their own settings: their
  chosen nickname goes on them. Nobody can see it before it fires, and the
  buyer is charged either way, which is the mirror's sting. You can't hold two.
  If the bounced prank can't land on the buyer (they are immune, out of the bot's
  reach, or Discord refuses it), it is **blocked** and they are still charged.
  A mirror holder who is also immune still bounces the prank: the mirror is
  checked first.
  A bounced drag is always blocked, since it would move the buyer into their
  own channel. A blocked prank changes nobody, so it isn't posted to the mod log. The card says
  what happened: a bounce names whose mirror sent it back.
- **`/shop cleanse`** (80) ends every prank running on you now, each undone as
  if it had expired. A standing mirror is kept. If nothing is running on you
  (a mute already waiting for you to rejoin voice doesn't count) it says so and
  doesn't charge.
- **`/shop reveal`** (100) names the buyer of a prank, walk-on theme, dedication
  or bounty aimed at you that was bought unsigned, and the starter of a
  crowdfunded prank, publicly, in the channel where you ran it. By default it is
  the most recent one; the optional `action` lets you pick from your last ten.
  It covers only pranks aimed at you, including ones a mirror blocked, and its
  line says which kind it unmasked. A prank can't be revealed twice, and if
  nothing is eligible it says so before charging.
- **Rename them back.** The card for a rename or clown carries two buttons,
  **Rename them back** and **Reveal**, that only the target can use. Rename them
  back opens a form for a new nickname aimed at whoever renamed you, and it is
  an ordinary rename purchase: the same price, the same checks, and a mirror can
  bounce it. It works only once the buyer is known (they signed it, or you have
  revealed it). Until then it says "Reveal who did it first", so that a revenge
  rename can't unmask an anonymous buyer for the price of a rename.

### How pranks end

- **By the clock.** A sweeper checks every 15 seconds and undoes whatever has
  run out. A rename or clown puts back the nickname from before the first name
  prank. A mute or deafen is lifted.
- **A member who is not in voice** can't be unmuted or undeafened (Discord
  refuses). Their mute or deafen is lifted the next time they join a voice
  channel.
- **Restarts.** Prank records are kept in the database. After a restart or a
  reconnect, CrackTunes finishes anything that ran out while it was away, and a
  purchase interrupted by a restart is settled when the bot reconnects (a prank
  that didn't land is refunded). A nickname changed while CrackTunes was away
  stands: who changed it can't be known then, so the rename or clown (and a
  lock on it) ends there and nothing is restored over it.
- **A crowdfund** that fills is a prank like any other once it runs; see
  [Group play](#group-play) for how it differs.
- **A member who leaves the server** ends their prank; nothing is restored.
- **Moderators win.** If anyone other than the target or the bot changes a
  prank's nickname, or changes a server mute or deafen on a member while the shop's
  is running, the prank ends there and nothing is restored over the moderator's change.
- **The member's own change.** While a lock runs, a nickname they set themselves
  is changed back at once. With no lock, their own change ends the name prank and
  the new name stays: that is what the lock is for.
- **A lock ends with the name prank** it rode on.
- **Cleanse, opting out, a moderator's immunity grant** and **`/economy cancel`**
  end whatever is running on the member, undone as if it had expired. They also
  end a [walk-on theme](#music) still waiting for the member, refunded to its
  buyer. Running bounties and open crowdfunds are not ended; they refund when
  their time runs out.

If undoing something fails (for example the bot has lost a permission), the bot
logs it and retries, waiting longer after each failure (15 seconds at first, at
most 5 minutes). After an hour it gives up: the prank ends as `undo_failed` and
the bot logs it as an error, for a moderator to put right by hand. A mute or
deafen waiting for the member to rejoin voice keeps waiting.

### The mod log

With [the mod log](rooms-and-mod-log.md#mod-log) on, every prank that changes
a member is posted with the buyer named, signed or not, and a bounce names the
original attacker. The shop's own nickname, mute and move changes are
recorded as the shop's, not as CrackTunes acting alone. With the mod log
off, nothing is posted, and the prank records stay in the database:
`/economy status` lists the last 10, with their buyers.

## Music

Credits also buy things that happen in the music. The shop asks nothing extra
of CrackTunes up front: a theme needs Connect and Speak in the channel where it
plays, and checks them there.

| Item | Command | Price |
|---|---|---|
| Walk-on theme | `/shop theme <member> <link> [start] [note] [signed]` | 100 |
| Dedication | `/shop dedicate <member> <song> [note] [signed]` | 40 |
| Queue jump | `/shop jump <song> [note] [signed]` | 40 |

### Walk-on theme

A theme plays **the next time the target joins voice**, within 7 days: up to 10
seconds of the link, from `start` if you give one (`83`, `1:23` or `1:02:03`, at
most `12:00:00`).

- The link must be an http(s) link to a song or video. It is never shown on a
  card.
- A member can have **one** theme waiting at a time.
- **Where it plays.** If CrackTunes is already in that channel, the theme is
  mixed over the music. If CrackTunes is idle in the server, it joins, plays,
  and leaves about 30 seconds later, unless someone started music or a `/gp`
  game in the meantime. If CrackTunes is busy in another channel, or a `/gp`
  game is running, the theme waits for a later join.
- **Volume.** It plays at the server's music volume.
- **The target must still be in the channel** when it plays. Through a
  voice-rooms hub, the theme plays in the room the member is moved into, not in
  the hub.
- **When the economy is off,** themes don't play. They wait, and are refunded
  at 7 days.
- **The card.** When a theme plays, a card ("🎺 @target's walk-on theme", with
  the note, and the buyer if signed) is posted in the chat of that voice
  channel.
- **Refunded in full** if it never plays within 7 days, if the link can't be
  played, if the target becomes immune or opts out, or if it is ended by a
  cleanse, `/economy cancel`, an opt-out or a moderator's immunity grant.

### Dedicate and jump

- **`/shop dedicate`** queues a song like `/play` and posts "🎵 This one goes
  out to @target", with the note.
- **`/shop jump`** queues a song to play next, like `/playnext`, and posts "⏭️
  Someone paid to play this next".
- **The music rules apply:** the music channel and the music role. Both are
  blocked while a `/gp` game is running.
- **Charging.** If nothing is queued, nothing is charged, and a blank song is
  refused. If the song was queued but Discord failed afterwards, the charge
  stands. A dedication or jump interrupted by a restart is refunded.
- **Making jumping a perk:** see [For admins](#for-admins).

None of these is stopped or bounced by a mirror: a mirror only answers pranks.
The mod log names the buyer of every dedication and theme, signed or not. A jump
is aimed at nobody and posts nothing there.

## Group play

### Bounties

`/bounty place <member> <amount> [note] [signed]` puts credits on a member's
head. `/bounty list` shows the bounties running in the server.

- **The price in the dashboard is the minimum stake** (50 by default). The stake
  is held for 7 days, and bounties from different members stack.
- **Collecting.** Whoever next **lands** a prank on that member collects every
  bounty on them, except their own. A prank that bounced off a mirror or was
  blocked collects nothing, and neither does a crowdfunded prank.
- **Refunds.** A bounty nobody collects in 7 days is refunded to its placer.
- **Immunity** blocks bounties on you.
- **Cards.** A public card goes out when a bounty is placed ("💰 … on
  @target's head"). When it is collected, the prank's card says a bounty was
  collected, without naming who collected it.
- The mod log names the buyer of every bounty.

### Crowdfunds

Members can pool credits for a prank against someone.

- **`/crowdfund start`** starts a fund for rename, clown, lock, mute, deafen or
  jail against a member (drag can't be crowdfunded: it depends on where the
  buyer is when it runs). There is one open fund per member and prank.
- **`/crowdfund pledge`** adds credits. **`/crowdfund list`** shows the open
  funds.
- **The goal is that prank's price.** The dashboard's crowdfund price is the
  **minimum pledge** (10 by default), except for a last top-up that finishes the
  fund.
- **At least two people.** Nobody can pledge more than half of a fund's goal
  (rounded up), so it takes at least two members to fill one.
- **When it fills,** the prank runs like a bought one, with the member checked
  again at that moment: still reachable, not immune and not opted out, in voice
  for mute, deafen and jail, and the prank still switched on. If it can't land,
  everyone is refunded.
- **Mirrors and bounties.** A crowdfund ignores mirrors and collects no bounty.
- **Expiry.** A fund that doesn't fill within 24 hours refunds everyone.
- **Cards.** A public card goes out when a fund starts ("🎉 … started a
  crowdfund to mute @target: 30/90"), naming the starter only if signed. Another
  goes out when the prank lands, naming only the contributors who signed ("a
  crowd" otherwise). **The mod log names every contributor.**
- **Restarts.** A crowdfund interrupted between filling and running is refunded
  to its pledgers.

### Reveal and the rest

[Reveal](#mirror-cleanse-and-reveal) covers themes that played, dedications,
bounties and crowdfunded pranks (it reveals the starter). A fund that is still
filling can't be revealed yet.

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

- **A test-network coin you can see in a crypto wallet: worthless by design, later.**
