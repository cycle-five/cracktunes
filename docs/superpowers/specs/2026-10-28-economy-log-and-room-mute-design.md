# Economy log, hidden buyers, and a room mute that works

Date: 2026-10-28. Release: **v0.46.0** (a new command and a new setting, so minor).
Supersedes parts of `2026-10-20-economy-design.md` (§ owner decisions, §5.4) and
`2026-10-16-voice-rooms-and-mod-log-design.md` (§1 mute row, §5 unverified table).

## Why

The owner found three things wrong on production (v0.45.3):

1. **The mod log unmasks anonymous buyers.** Every unsigned prank is logged as
   "@Mika used Rename on @Jo (anonymous)". The economy sells "reveal who did
   it", so wherever the log channel is readable that purchase is worthless.
   This was built as specified: the 2026-10-20 spec records "the mod log
   always names the buyer, signed or not" as the owner's decision. **The owner
   reversed it on 2026-10-28.**
2. **One channel carries two audiences.** Economy events and moderation events
   share the mod log channel. The owner's intent is that the economy's log is
   public, "like a public ledger". Many Discord communities, free-speech ones
   especially, send *all* their logs to a globally readable channel.
3. **Room mute does nothing to someone already talking.** It only denies Speak
   on the room. Discord applies a channel Speak change when the member next
   connects, so the mute bites only after a rejoin. The owner: "otherwise it's
   basically useless as a method of control in what is supposed to be that
   person's 'private' vc".

## The rule behind all of it (owner, 2026-10-28)

**Every log line is written as if the whole server reads it.** That includes the
mod log, the economy log and the old event-log channels. A fact meant to be
hidden, such as an unsigned buyer, appears in no channel post, log line or
card. It appears only in a private (ephemeral) reply to someone entitled to it.
Moderators who need to make it public get an explicit, logged action.

## Goals

- A new **economy log**: its own channel, public by intent, carrying the
  economy's story. It never names an unsigned, unrevealed buyer.
- The **mod log** stays independent of the economy and records what CrackTunes
  literally did to members ("CrackTunes server-muted @Jo (shop: Mute)"),
  without the buyer. It records moderators' economy actions naming the
  moderator.
- **`/economy reveal`**: a moderator unmasks an unsigned prank publicly, free.
- **Unsigned cards** never post in a way that labels the buyer.
- **Room mute** server-mutes the member while they are in the room, and undoes
  it when they leave.

## Non-goals

- Tracking moderators' nickname changes in the mod log. The mod log reads only
  mutes, deafens, moves and disconnects from Discord's audit log, and still
  does. The shop's own nickname changes are logged because the shop knows
  exactly what it did.
- Hiding moderators' names from the mod log.
- Any change to the music log (`queue_audit`, `/auditlog`, the History page).
  Checked: themes do not write to it, and dedicate/jump are always signed.
- Any change to the old event-log channels. Checked: a shop rename reaches them
  only as a nickname change, with no buyer.

## Anti-goals

- **Any public surface naming an unsigned buyer** before a reveal: log lines,
  cards, or an interaction message Discord labels "<user> used /command".
- **A room mute that outlives the room.** A member must never stay
  server-muted elsewhere because a room muted them, including across a bot
  restart, a disconnect, or the room being deleted.
- **Undoing someone else's mute.** A room never lifts a server mute it did not
  apply: not a moderator's, and not the shop's.

## 1. The economy log

**Setting.** `EconomyConfig` gains `log_channel_id: Option<ChannelId>`, stored
as a string like `announce_channel_id`. It lives in the config JSON, so no
migration; old rows read as `None`. The dashboard's economy settings get an
"Economy log" text-channel picker beside the announce channel. The economy
permission report checks it like the announce channel: View Channel, Send
Messages, and Embed Links if the lines are embeds. `/economy status` shows it.

**Off until set.** No channel, no economy log lines. The economy itself runs
either way. After upgrading, economy lines stop going to the mod log channel,
so the server picks its public channel here (and points the mod log at a
moderators-only one). The guide says so.

**What it carries.** Every economy event that is posted today, moved out of
the mod log, plus unmaskings:

| Event | Line (unsigned) | Line (signed or revealed) |
|---|---|---|
| A purchase aimed at a member (every prank, theme, dedicate, jump, bounty) | "Someone used Rename on @Jo until …" | "@Mika used Rename on @Jo until …" |
| A bounced prank | "Someone's rename bounced off @Jo's mirror" | names the buyer |
| A crowdfund opened / landed | "A crowdfund …": starter and backers not named | names them, each by their own choice (below) |
| Pranks ended early | by the target's cleanse: names the target. By a moderator: "a moderator" | |
| Immunity granted / lifted by a moderator | "a moderator" | |
| Credits adjusted by a moderator | "a moderator", with the reason | |
| **New:** a prank unmasked | bought: "@Jo unmasked the rename: @Mika"; by a moderator: "A moderator unmasked the rename on @Jo: @Mika" | |

**Crowdfunds name each person by their own choice.** The starter is named
only if the crowdfund was signed. Each backer is named only if their own
pledge was signed (`economy_pledges.signed`). The "+N others" count includes
everyone hidden. Today `CrowdfundExecuted` names every contributor, signed or
not (`crack-rooms/src/modlog/event.rs`, around line 156). That ends here.

**Mentions never ping**, in either log or the reveal line. The mod log and the
cards render with no allowed mentions today; keep it that way (my call).

The note travels with the line as today. **Enforce the buyer rule with the
type, not with care:** the economy log's event carries the buyer as something
like `Shown(UserRef) | Hidden`. It is built from `signed` (and `revealed_by`)
in one constructor, so no render path can reach a hidden buyer. That follows
the owner's standing preference for types over source scans.

**Ownership.** The economy log belongs to the economy. It must not need the
mod log to be on. Whether its events stay `ModLogEvent` variants or become an
economy-owned type is the builder's call, given the rule above and the
crate graph: `crack-rooms` depends on `crack-economy`, not the reverse.

## 2. The mod log

**Stays independent of the economy.** It needs no economy setting, and it is
on or off exactly as today.

**Literal lines for the shop's changes to members.** Every change the shop
makes goes through the `EconomyDiscord` trait (`crack-economy/src/discord.rs`):
`set_nick`, `set_mute`, `set_deaf`, `move_member`. That covers applying a
prank, the sweeper's undo, the voice-join finish, reconcile, a lock's revert,
a crowdfund landing and a mirror's bounce. After each *successful* call the
mod log gets one line:

- "CrackTunes server-muted @Jo (shop: Mute)" / "CrackTunes unmuted @Jo (shop: Mute)"
- "CrackTunes server-deafened …" / "… undeafened …"
- "CrackTunes moved @Jo to #jail (shop: Jail)"
- "CrackTunes changed @Jo's nickname to "x" (shop: Rename)" / "… cleared @Jo's nickname …"

These lines carry **no buyer field at all**. The actor is CrackTunes, as in
Discord's own audit log. After this change, **no mod log line names a buyer,
a crowdfund's starter or a backer**. The variants that did (`ShopAction`,
`CrowdfundStarted`, `CrowdfundExecuted`) leave `ModLogEvent` with their
renderers and tests. Their doc comments call "always names the buyer" an
owner ruling; that ruling is the one reversed here.

The literal lines obey the mod log's own switch. With the mod log off or
without a channel, they are dropped silently, as every mod log event is today
(`ChannelSink::post`).

**Changing `LiveEconomyDiscord` and every place that builds it is in scope.**
There are eight call sites: `crack-core` `shop.rs`, `crowdfund.rs`,
`credits.rs`, four in `handlers/serenity.rs`, plus the two inside
`crack-economy`. Tests and fakes get a no-op hook. `crack-economy` cannot depend on `crack-rooms`, so
`crack-economy` defines a small hook it calls after each successful change.
`crack-core` implements the hook by posting to the mod log, and every place
that builds a `LiveEconomyDiscord` gets it, including the two inside
`crack-economy` (the sweeper in `effects.rs`, and `lock.rs`). The builder
chooses the hook's shape: a field on `LiveEconomyDiscord`, something held in
`EconomyData`, or a wrapper.

The audit-log reader keeps skipping CrackTunes' entries whose reason starts
`CrackTunes shop`, and keeps never crediting CrackTunes' own moves. The
literal lines replace both, exactly, without duplicates.

**Moderators' economy actions, naming the moderator:** immunity granted and
lifted, credits adjusted, pranks ended with `/economy cancel`, and the new
`/economy reveal`. These go to **both** logs: the economy log says "a
moderator", the mod log names them.

**What leaves the mod log:** `ShopAction`, a cleanse's `ShopCancelled`,
`CrowdfundStarted` and `CrowdfundExecuted`. They are economy events. The
member changes they cause still appear through the literal lines.

**`/economy status` is unchanged:** moderators only, private, and still names
the buyer of every recent action.

## 3. `/economy reveal` (new)

- **Who:** a subcommand of `/economy`, so moderators only
  (`MODERATE_MEMBERS`, plus the existing `mod_gate`).
- **Options:** `member` (required), then `action` with autocomplete. The
  autocomplete lists that member's unsigned, unrevealed pranks, labelled like
  `/shop reveal` ("Rename · 5 minutes ago"). It reuses
  `db::reveal_candidates(pool, guild, member, …)`; the autocomplete reads the
  chosen `member` from the interaction's other options. Leaving `action` empty
  takes the most recent.
- **What it does:** `mark_revealed(prank, by = moderator)`. It is free and
  creates no purchase row. Refusals reuse the shop's words: nothing to reveal,
  already revealed.
- **Effects are the same as a bought reveal:** the target can no longer buy a
  reveal of it, and the card's "Rename them back" button works.
- **Public line:** posted as a plain channel message in the channel the command
  ran in, **not** as an interaction follow-up, which Discord labels with the
  moderator's name. It reads "A moderator unmasked the rename on @Jo: @Mika".
  If CrackTunes can't post there, the moderator is told privately and the
  reveal still stands. The moderator always gets a private confirmation naming
  the buyer.
- **Logs:** the economy log gets the unmasking line ("a moderator"); the mod
  log names the moderator. The two posts are independent. A failed post is
  logged with `tracing::warn!` and doesn't undo the reveal, as `ChannelSink`
  does today.
- **Racing a paid reveal:** `mark_revealed` writes only where
  `revealed_at IS NULL`. A paid reveal that loses the race already refunds and
  says "already revealed" (`shop.rs` `reveal`). A moderator reveal that loses
  says the same.
- **Crowdfunds:** they appear in `reveal_candidates` today, so they can be
  unmasked; the starter (`buyer_id`) is named.

## 4. Unsigned cards never label the buyer

When a prank card can't be posted where it belongs, `buy_prank`
(`crack-core/src/commands/economy/shop.rs`, around line 450) falls back to
`answer.public`, a public follow-up to the buyer's interaction. Discord labels
that "<buyer> used /shop …". The button path (`PressAnswer`,
`crack-core/src/economy/buttons.rs`) does the same.

**Fix:** an unsigned card never falls back. It is skipped, and the buyer is told
privately that the card couldn't be posted (new string). A signed card keeps
the fallback. Look for any other unsigned public interaction reply in the shop
and its buttons, and give each the same rule.

## 5. Room mute

**Owner-approved behaviour (2026-10-28):**

- **Mute:** CrackTunes server-mutes the member at once, so it works mid-sentence.
  The room also keeps its Speak deny, as today, so the mute holds if they
  rejoin.
- **They leave the room:** CrackTunes lifts the server mute.
  - **Moving to another channel:** unmute at once.
  - **Disconnecting:** Discord refuses to unmute someone not in voice
    (40032, `EconDiscordError::NotConnected`). The debt is recorded and paid
    at their next voice join in that server, or at startup reconcile if they
    are in voice by then.
- **They come back while still room-muted:** server-muted again.
- **Unmute button:** removes the Speak deny, and lifts the server mute if the
  room holds it.

**Calls approved with it:**

- **Someone else's mute stays theirs.** If the member is already server-muted
  when the room mutes them (by a moderator, or by the shop), the room takes no
  hold and never lifts it. Same on a rejoin. This is the shop's "whoever muted
  them owns it" rule (`crack-economy/src/target.rs:126`).
- **Moderators win.** If the member is server-unmuted while in the room and
  the room holds their mute, the room drops its hold and leaves them unmuted
  until they rejoin the room.
- **One mod log line.** The room's server mute and its undo use their own audit
  reason, `CrackTunes room: mute`. The mod log's audit reader skips
  CrackTunes-authored entries with that prefix, as it does `CrackTunes shop`.
  The existing `RoomMute` line is the record.
- **Without Mute Members** in the room, mute falls back to the Speak deny and
  the reply says it takes effect when they rejoin. The rooms permission check
  (`crack-rooms/src/rooms/preflight.rs`) lists Mute Members as optional, beside
  Pin Messages, and the dashboard shows it.

**Storage.** A hold must outlive the room's row, which is deleted when the room
empties, typically right after the muted member leaves. So holds go in a new
table, for example `room_mute_holds (guild_id, user_id, channel_id,
created_at)`, primary key `(guild_id, user_id)`, in migration
`20261028120000_room_mute_holds.sql`.

**One rule covers every exit:** a hold row means CrackTunes server-muted this
member for room R.

- **The member is in voice somewhere other than R** (a move, a join, or seen
  at startup reconcile): unmute, then delete the row. That covers the room
  being deleted under them.
- **The member left voice entirely:** try to unmute. On `NotConnected`, keep
  the row for the next join.
- **The member is in R and server-unmuted while we hold:** a moderator did it,
  so delete the row and don't re-mute.
- **The member joins R while in `state.muted`, server-unmuted, with no row:**
  mute and insert.

Unmute errors other than `NotConnected` are logged and retried at the next
event or reconcile, not looped.

**How "a moderator unmuted them" is told apart from CrackTunes' own unmute**
(my call):

- CrackTunes unmutes a held member in only two cases: when they are outside R,
  and from the Unmute button.
- The button deletes the hold row *before* it unmutes.
- So "in R, server-unmuted, row present" can only be someone else's doing. It
  is read from the voice state Discord sends (`VoiceState::mute`), not from
  the cache's old copy, so no old-versus-new comparison is needed.

**Every channel that isn't R counts as "not R",** the hub included. A member
who leaves R for the hub is unmuted at once. A room deleted with the member
inside disconnects them, and their next join pays the debt.

**Where it runs:** the rooms voice-state handler is synchronous
(`crack-rooms/src/modlog/live.rs`, around line 249, and the lifecycle's
handler). Hold work is async (database and Discord), so it runs in a spawned
task, serialized per member with the room's existing lock or a per-guild one.
The builder chooses which.

**Known edge (my call, documented, not fixed).** The room mutes someone the
shop has already muted, so the room takes no hold. When the shop's mute ends,
they can speak in the room until they rejoin. The reverse can't happen: the
shop refuses `AlreadyMuted` on someone the room muted.

## 6. Docs

- `docs/economy.md`: the economy log, `/economy reveal`, the anonymity rule,
  and the upgrade note.
- `docs/rooms-and-mod-log.md`: mute is now a server mute while in the room
  (today it promises the opposite, around line 214); the mod log's literal
  shop lines; and that economy events moved out.
- `docs/web-dashboard.md`, if it lists economy settings.
- The two older specs get a dated note where they are superseded (the economy
  spec's "Accountability" row and §5.4; the rooms spec's mute row and §5
  table).

## 7. Testing

Unit and DB tests in the project's style. Sabotage each one once to see it
fail, and assert on what is sent. Most of all:

- No economy log render of an unsigned, unrevealed event contains the buyer's
  id. Cover every variant, bounce and crowdfund included.
- The hook fires once per successful `EconomyDiscord` call and never on a
  failed one, for apply and undo.
- Moving the moderators' economy actions sends them to both logs, with the
  right name in each.
- Room mute holds: every exit in the rule above, plus reconcile.

**TuneTitan by hand, before production:**

- Mute mid-speech.
- Move out; disconnect, then rejoin elsewhere.
- Rejoin the room.
- A moderator unmutes them in the room.
- The Unmute button.
- The room with Mute Members removed.
- An unsigned rename in a server whose economy log is set: check the economy
  log, the mod log and the card.
- `/economy reveal`.
