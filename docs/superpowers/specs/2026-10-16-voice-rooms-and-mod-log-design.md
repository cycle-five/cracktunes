# Voice rooms and the mod log — design

**Date:** 2026-10-16 · **Status:** approved in conversation. The owner confirmed
the order (rooms and the mod log together, economy later) and said "run through
this autonomously", so the rest of this design is my call unless marked as the
owner's.

## Why

Saucey, a long-time CrackTunes champion, sent a list of bot ideas. Two of them are
this arc:

1. **Temporary voice channels whose controls are written down in the channel.**
   Her server uses Maki for this today. Its three failures are this design's
   requirements:
   - it has next to no documentation, so people "try once, can't figure out
     the command to kick somebody, and never use the feature again";
   - it does not delete a temporary channel when everyone leaves;
   - its dashboard shows none of the options.
2. **A "who modded me?" log**: a channel, visible to whoever the server chooses
   (her server: the 18+ role), where the bot posts who server-muted, deafened,
   disconnected or moved whom, and who kicked whom from a temporary channel.
   Her ask: a kick from a temporary channel **must carry a reason**.

The rest of her list (an economy, the coffee) is out of scope. The economy
gets its own arc, redesigned to sell bot-performed actions instead of
permission roles.

## Owner decisions

| Question | Decision |
|---|---|
| Who it is for | Saucey's server first. Built so any server can turn it on if word spreads |
| Where it lives | Inside CrackTunes. Every feature is **off by default**; a server's admins turn it on |
| Order | Rooms and mod log together, as one arc: two stacked PRs (v0.28.0 rooms, v0.29.0 mod log) |
| Discord review | None needed. No new privileged intent. Everything else is per-server permissions an admin grants |

## Goals

- A member joins a "hub" voice channel and is moved into a new voice room named
  after them. They own it.
- The room's chat opens with a **control panel**: an embed that lists every
  control in plain words, with buttons for each. The same controls exist as
  `/room …` slash commands. The panel is the documentation.
- A room is **always** deleted once no human is left in it. This holds across
  bot restarts and channel deletions made by hand.
- Every option is set on the dashboard (`dash.cracktun.es`, Settings), and the
  dashboard says which permissions the bot is missing and where to grant them.
- Owners can kick (with a required reason), mute and unmute, let a kicked
  person back in, lock and unlock the room and its chat, set a member limit,
  rename the room, transfer it, name helpers, and allow or block people and
  roles. Anyone in a room whose owner left can claim it.
- The mod log posts server mutes and deafens (exact, from the audit log),
  forced disconnects and moves (attributed from the audit log, labelled when
  that attribution is a match by timing), and every room kick, mute, unmute
  and unblock (exact, with the reason).

## Non-goals

- **Deafen inside a room.** Discord has no per-channel "can hear" permission.
  A room deafen would have to be a server-wide deafen that the bot undoes when
  the person leaves; if the bot is down at that moment, the person stays
  deafened server-wide. Mute has a per-channel form (deny Speak), so mute is
  in and deafen is out. The panel's help says so.
- Giving owners native Discord permissions on their room. Every owner action
  goes through the bot. That is what makes the kick reason enforceable, and
  what keeps an owner from opening the room past the server's own gating
  (see "Never widen").
- Logging server kicks, bans and timeouts. Saucey's list is about voice. Those
  are cheap follow-ups on the same mod log.
- An economy, nickname games, anything paid. Rooms and the mod log are free on
  every server.
- Choosing a room category separate from the hub's. Rooms are created in the
  hub's category, so whoever can see the hub can see the rooms. One setting
  fewer, and one way to strand a member in a room they cannot see is gone.
- Several hubs per server. One hub per server for now.

## Anti-goals (would make this a failure even if it works)

- A room outliving its last member. That is Maki's bug.
- A room owner able to let in someone the server's own permissions keep out,
  such as a non-18+ member into an 18+ category.
- A panel that needs its own documentation. Every control is named in words on
  the panel.
- The mod log naming the wrong person as the moderator. Where the audit log
  cannot tell, the entry says so, or says nothing.

## 1. Rooms

### 1.1 Settings (per server)

New table `room_settings` (its own table, not more columns on `guild_settings`:
that row is saved whole from memory, which is the clobber family
`toggle_now_playing_buttons` works around):

| Column | Type | Default | Meaning |
|---|---|---|---|
| `guild_id` | BIGINT PK | | |
| `enabled` | BOOLEAN | false | |
| `hub_channel_id` | BIGINT NULL | | The voice channel to join to get a room |
| `name_template` | TEXT | `{name}'s room` | `{name}` = the owner's display name |
| `default_limit` | SMALLINT | 0 | 0 = no limit, 1..=99 |
| `empty_grace_seconds` | INTEGER | 30 | One of 0, 10, 30, 60, 120, 300 |
| `updated_at` | TIMESTAMPTZ | now() | |

- Enabling needs a database and a hub. Without either, the save is refused with
  a message saying which is missing.
- Loaded per guild at `GuildCreate`, held in memory, written through on save.
- `name_template`: 1..=100 characters after trimming, no control characters.
  A template without `{name}` is allowed.

### 1.2 Room state

New table `rooms`, one row per live room:

| Column | Type |
|---|---|
| `channel_id` | BIGINT PK |
| `guild_id` | BIGINT, indexed |
| `owner_id` | BIGINT |
| `created_at` | TIMESTAMPTZ |
| `state` | JSONB: a typed `RoomState` (serde), never `serde_json::Value` |
| `baseline` | JSONB: the overwrites Discord gave the channel at creation, as typed `Vec<StoredOverwrite>` |

```rust
pub struct RoomState {
    pub owner: UserId,
    pub helpers: BTreeSet<UserId>,
    pub blocked: BTreeSet<UserId>,          // kicked: cannot see or join
    pub muted: BTreeSet<UserId>,            // cannot speak in this room
    pub permitted_users: BTreeSet<UserId>,  // may join while locked
    pub permitted_roles: BTreeSet<RoleId>,
    pub blocked_roles: BTreeSet<RoleId>,
    pub locked: bool,
    pub chat_locked: bool,
    pub panel: Option<MessageId>,
}
```

- In memory: a registry `DashMap<ChannelId, Arc<Mutex<Room>>>` on `Data`, plus
  an owner index (one room per member per server). Every action takes that
  room's mutex, so two presses on one room run one after the other.
- Rename timestamps (for Discord's limit) live in memory only.
- A room's row is deleted with the room. Kick reasons are never stored. They go
  to the mod log and nowhere else.

### 1.3 Lifecycle

**Joining the hub** (a non-bot member's voice state moves into the hub, rooms
enabled):

1. If they already own a room in this server, move them into it. Done.
2. One new room per member per 10 seconds. Inside that window, wait out the
   remainder, then re-check that they are still in the hub.
3. **Joiner guard.** Compute from the cache whether the joiner has View Channel
   and Connect in the hub's category (their roles, the category's overwrites).
   If not, create nothing. A cache miss fails open, as `music::perms` does.
4. Cap: at most 50 live rooms per server. Past it, nothing is created and a
   warning is logged.
5. Create the voice channel in the hub's category with **no**
   `permission_overwrites`, so Discord syncs the category's (see 1.5). Name:
   the template rendered and capped at 100 characters. `user_limit`: the
   default.
   **Sync check:** if the new channel's overwrites differ from the
   category's, copy the category's onto it. If that fails, delete the channel.
   A wrong guess about Discord's syncing must never leave a room open to
   everyone in an 18+ category.
6. Insert the `rooms` row. If that fails, delete the channel and stop.
7. Apply the managed layer (1.5): the owner's member overwrite.
8. Move the member into the room. If that fails (they left the hub), delete
   the channel and the row.
9. Post the panel in the room's chat, record its id, and try to pin it.
   Pinning is best effort: Discord's support for pins in voice-channel chats
   is unclear (see 5).

**Leaving.** Whenever a voice state leaves a room, count the humans left in it
(cache voice states, bots excluded). At zero, schedule deletion after
`empty_grace_seconds`. Anyone joining the room cancels it. When the timer
fires, re-check emptiness, delete the channel, then the row. If Discord says
the channel is already gone, delete the row. Any other failure keeps the row,
for reconcile to finish later, and logs a warning.

A room whose only occupants are bots (CrackTunes playing music in it,
included) is empty. Deleting the channel disconnects the bot through the
existing kicked/disconnected path.

**Deleted by hand.** `ChannelDelete` for a room drops its row and its timers.

**Reconcile at `GuildCreate`** (startup, outage recovery, a guild joined while
running). For each stored room in the guild:
- its channel no longer exists: drop the row;
- it exists but has no humans in it (from the payload's voice states):
  schedule deletion;
- it is occupied: re-arm it.

Members found sitting in the hub are handled as fresh joins.

### 1.4 Authority

A pure function decides who may do what:

| Who | May |
|---|---|
| Owner | everything below |
| Helper | kick, unblock, mute, unmute, lock, unlock, chat lock/unlock, limit |
| Server moderator (Administrator, Manage Channels or Move Members at server level) | everything, as if owner |
| Anyone in the room | claim, but only while the owner is not in the room |
| Anyone else | nothing; the press is answered privately with who can |

Protected targets: nobody targets themselves, the room's owner (unless the
actor is a moderator) or a server moderator. Helpers cannot target other
helpers. Nobody targets CrackTunes itself (`/leave` exists for that).

### 1.5 Permissions: the managed layer, and "never widen"

A room's overwrites are the category's (synced by Discord at creation) plus a
**managed layer** the bot computes from `RoomState`. The bot changes only these
bits: View Channel, Connect, Speak, Send Messages.

A pure function `room_overwrites(baseline, &RoomState) -> Vec<Overwrite>`
returns the whole wanted set. The glue diffs it against the channel's current
overwrites and PUTs or DELETEs only the targets that differ (Edit/Delete
Channel Permission). A target the bot has no reason to touch is never sent.

| State | Effect |
|---|---|
| owner, helpers | member allow Connect (so a lock cannot lock them out); while the chat is locked, also allow Send Messages |
| blocked user | member deny View Channel + Connect |
| muted user | member deny Speak |
| permitted user (only matters while locked) | member allow Connect |
| locked | Connect moves from allow to deny on `@everyone` and on every role overwrite that allows it; permitted roles keep allow Connect |
| chat locked | the same for Send Messages |
| blocked role | role deny View Channel + Connect |

**Never widen.** The bot never writes an allow for View Channel. A server that
gates rooms by visibility (Saucey's 18+ role) stays gated: Connect without View
Channel admits no one. For servers that gate by Connect instead, every
**member or role allow of Connect is guarded**. A user (helper, permit,
transfer target) or role (permit) is admitted to a state set only if the
category's baseline already lets them connect. Otherwise the action is refused
with a message saying they can't join rooms on this server.

`baseline` is the room's overwrites as Discord returned them at creation
(the `baseline` column). Changing
the category later affects new rooms, not live ones.

🪤 **Discord only lets a bot write overwrite bits it holds itself** ("only
permissions your bot has in the guild or parent channel … can be
allowed/denied"). The member overwrites contain only the four managed bits,
which the bot needs anyway. A lock re-PUTs role overwrites that also carry
baseline bits. If a baseline role overwrite holds a bit the bot lacks (say,
Manage Messages for a mod role), that PUT may be refused. The preflight (1.8)
flags this ahead of time. At runtime a refused lock leaves the state unchanged
and tells the presser which permission CrackTunes needs on the category.

### 1.6 Actions

Every action follows the same order: authority → guard → compute the new
state → Discord effects → persist → re-render the panel → mod log event (PR 2).
A Discord failure aborts before persisting, and the presser is told.

| Action | Input | Effect |
|---|---|---|
| Kick | member in the room; **reason, required, 1..=200 chars** | disconnect them if present, add to `blocked` |
| Let back in | a blocked member | remove from `blocked` |
| Mute / Unmute | member; reason optional | add/remove `muted` |
| Lock / Unlock | | `locked` |
| Lock chat / Unlock chat | | `chat_locked` |
| Limit | 0..=99 | channel `user_limit` |
| Rename | 1..=100 chars | channel name; refused past 2 renames per 10 minutes (Discord's own limit), saying when it can be tried again |
| Transfer | member in the room | `owner` changes; old owner becomes a plain member; helpers stay |
| Helper add / remove | any member (guarded) | `helpers` |
| Allow (permit) | user or role (guarded) | `permitted_*` |
| Block role / Unblock role | role | `blocked_roles` |
| Claim | | presser becomes owner (owner absent) |
| Panel | | re-post the panel at the bottom of the chat (old one deleted) |

### 1.7 The panel, buttons, modals, and `/room`

**Panel** (`CrackedMessage::RoomPanel(Box<RoomPanel>)`, one renderer, sent via
the courier and transport like every other message):
- Title: "🔊 {owner}'s room". Fields: owner, helpers, locked / chat locked,
  limit.
- A short reference: every control by name, what it does, and its slash
  command (`/room kick`, …). One line on deletion: "This room is deleted when
  everyone leaves."
- Buttons, labelled in words (emoji + text). Discoverability is the point, so
  this panel does not follow #588's icon-only now-playing row:
  - Row 1: Kick · Mute · Unmute · Let back in
  - Row 2: Lock/Unlock (drawn by state) · Lock chat/Unlock chat · Limit · Rename
  - Row 3: Transfer · Helpers · Who can join · Claim · Help
- Re-rendered (edited in place) after every state change.

**Custom ids** carry everything a press needs, like `NowPlayingButton`: a
typed enum, `room:<kind>:<channel_id>` for buttons and `roomm:<kind>:<channel_id>`
for modals, with a parser that never panics (0 and `u64::MAX` rejected before
`ChannelId::new`). Routed by prefix in `handlers/serenity.rs` beside `np:` and
`/gp`. Component and modal interactions both need routes.

**Modals.** serenity `next` has `CreateLabel::select_menu`, so a single modal
can hold a member picker and a text field. A button that needs input answers
with a modal **as its first response** (no defer first: a modal cannot follow
an acknowledgement):
- Kick: a string select of the room's current human occupants (excluding the
  presser, max 25), plus a **required** Reason text input. A room with more
  than 25 people points the presser to `/room kick`.
- Mute / Unmute / Transfer: the same select; Mute has an optional reason.
- Let back in: a select of the blocked members.
- Helpers, Who can join: user / role selects plus an add/remove choice.
- Limit, Rename: a text input, validated on submit.

Lock, chat lock, claim and help need no input. They acknowledge, act, and
re-render. Help answers privately with the full guide.

A press or submit by someone without authority is answered privately and
nothing happens. A modal submit re-checks authority and the target's presence:
the room may have changed while the modal was open.

**`/room`** (guild only, no `default_member_permissions`, authority checked at
run time). The room is the invoker's current voice channel if it is a room,
otherwise the channel the command was run in if that is a room's chat.
Otherwise: "You're not in a room." Subcommands: `kick member reason`
(reason required by Discord itself), `letin`, `mute member [reason]`, `unmute`,
`lock`, `unlock`, `chatlock`, `chatunlock`, `limit n`, `rename name`,
`transfer member`, `helper add|remove member`, `allow user-or-role`,
`block role`, `unblock role`, `claim`, `panel`. Buttons and commands call the
same `rooms::ops` functions.

**`/rooms`** (admin: `default_member_permissions = MANAGE_GUILD`): `setup hub`
(enables, sets the hub), `off`, `status` (settings, the permission report, and
a link to the dashboard Settings page). This keeps self-hosters without the
dashboard able to use rooms.

All user-facing strings live in `messaging/messages.rs`
([[localization-strings-in-one-file]]).

### 1.8 Permission preflight

A pure function over the bot's computed permissions in the hub's category and
the hub. Needed: View Channel, Connect, Manage Channels, Manage Permissions
(Manage Roles, granted on the category or the server), Move Members, Send
Messages, Embed Links. Optional: Pin Messages (for the panel pin). It also
flags baseline overwrite bits the bot lacks (1.5's 🪤). It returns a list of
`(where, missing permissions)`. `/rooms status` and the dashboard show it in
words ("Give CrackTunes **Move Members** on the **Rooms** category"). The
lifecycle logs it at warn when a create or move fails with Missing
Permissions.

### 1.9 The Discord seam

`trait RoomsDiscord` (async), real impl over `Http`, a fake that **records every
call** for tests ([[cracktunes-standing-rules]]: assert on what is sent):
`create_voice_channel`, `delete_channel`, `put_overwrite`, `delete_overwrite`,
`set_user_limit`, `rename`, `move_member(guild, user, Option<ChannelId>)`
(`None` disconnects), `pin`. The panel goes through the existing `Transport`.
Cache reads are gathered by thin glue into plain snapshot structs (occupants,
category overwrites, member roles, role permissions), so the decisions are
pure.

## 2. Mod log

### 2.1 Settings

New table `mod_log_settings` (`guild_id` PK, `enabled` false, `channel_id`
NULL, `updated_at`). Who can read the log is the log channel's own Discord
permissions. The dashboard says so in one line ("Anyone who can see this
channel can read the log. Saucey's server: give only the 18+ role View
Channel"). The bot never changes that channel's permissions.

### 2.2 Events

```rust
pub enum ModLogEvent {
    ServerMute   { target: UserId, actor: UserId, on: bool, reason: Option<String> },
    ServerDeafen { target: UserId, actor: UserId, on: bool, reason: Option<String> },
    Disconnect   { target: UserId, from: ChannelId, by: Attribution },
    Move         { target: UserId, from: ChannelId, to: ChannelId, by: Attribution },
    /// D forced disconnects among more leaves than that: who did it is known,
    /// whom is not.
    DisconnectBatch { actors: Vec<UserId>, count: u32, among: Vec<UserId> },
    RoomKick     { target: UserId, room: ChannelId, actor: UserId, reason: String },
    RoomUnblock  { target: UserId, room: ChannelId, actor: UserId },
    RoomMute     { target: UserId, room: ChannelId, actor: UserId, on: bool, reason: Option<String> },
}
pub enum Attribution { By(UserId), OneOf(Vec<UserId>) }
```

Rendered as one embed per event (`CrackedMessage::ModLog`), mentions in the
embed (embeds do not ping; allowed mentions none), timestamped. An attributed
disconnect or move is footed "Matched from Discord's audit log by timing".

### 2.3 Sources

**Server mute / deafen: exact.** `GuildAuditLogEntryCreate`, `MEMBER_UPDATE`
(24) with a `mute` or `deaf` change gives the actor, the target, the new value
and the audit reason. Self-mute is not a server mute and is never logged.
Muting someone just for yourself is invisible to bots.

**Room actions: exact.** `rooms::ops` emits the event with the actor and the
reason. Before the bot disconnects someone, it records an *expected*
`(guild, user)` for 10 s. That leave is excluded from attribution, and the
audit entries the bot itself authored are ignored as candidates.

**Forced disconnects and moves: attributed.** Discord's audit log records
`MEMBER_DISCONNECT` (27) and `MEMBER_MOVE` (26) with the moderator and a
`count`. By widespread report, with no confirmation in Discord's docs, the
target is not named, and repeated actions by one moderator raise the count on
one entry instead of creating new ones. The design works whether or not that
holds:

- A per-guild, per-action-type snapshot `entry id → count` is seeded when the
  mod log becomes active (startup for enabled guilds, or the moment it is
  enabled), so the first forced action is attributable.
- Voice leaves (old channel set, new channel none) and moves (channel changed)
  are batched per guild. 2 s after the last one in a burst (at most 5 s after
  the first), the bot fetches `GET /guilds/{g}/audit-logs?action_type=…&limit=10`
  for each type it needs and diffs against the snapshot.
- A pure `attribute(snapshot, entries, changes, bot)` decides:
  - an entry whose `target_id` names a changed member pairs with that change
    exactly;
  - otherwise, with D the summed positive count deltas (bot-authored entries
    excluded) and L the unexplained changes:
    - D = 0: nothing is logged (people left on their own);
    - D = L: each change is attributed `By(actor)`, or `OneOf(actors)` if
      several moderators acted;
    - D < L: one `DisconnectBatch` entry;
    - D > L: as D = L, the excess logged at debug.
  - Moves are matched per destination channel (`options.channel_id`).
  - **Amended during the build (rulings M1, M7, M8):** a row that names a
    target never counts toward D. It pairs only with that target's change of the
    same kind (moves: same destination), and only when the entry is new since the
    snapshot, since an aggregated entry may carry a stale target. D > L logs
    nothing, because a surplus means the batch isn't self-contained. Moves with
    D < L log nothing. CrackTunes' own leaves and moves are fed in so their
    audit units are consumed, but they are never logged.
  - **Further amendments (M11–M14):** a leaves batch claims only
    D − (CrackTunes' own changes + repeated targets), and logs nothing when that is 0.
    A batch with several moderators reads "One or more of …". A targeted row pairs
    only when it is new, has count 1 and exactly one candidate change. Rows are
    deduplicated by (kind, id). A stored count never goes down.
- Cost guard: only for guilds with the mod log enabled, one fetch in flight
  per guild, and a 403 marks the guild "missing View Audit Log". That shows on
  `/modlog status` and the dashboard, and suspends fetches until the settings
  are saved again or 10 minutes pass.

### 2.4 Permissions

View Audit Log (server-level only; without it, `GUILD_AUDIT_LOG_ENTRY_CREATE`
is never delivered and fetches 403). View Channel, Send Messages and Embed
Links in the log channel. The same preflight shape as 1.8.

### 2.5 `/modlog`

Admin (`MANAGE_GUILD`): `set channel`, `off`, `status`.

## 3. Dashboard

`GET /g/{g}/settings`: Manage Server only, the same rule (and 404/503/403
handling) as the history page. Both features are free, so there is no plan gate.

- **Voice rooms** section: on/off, hub (a select of the server's voice
  channels), name template (with a live preview), default member limit,
  "delete an empty room after" (the six choices), and the permission report
  from 1.8 with what to fix.
- **Mod log** section (PR 2): on/off, log channel (a select of text channels),
  the visibility note, and the permission report.
- Saves are `POST /g/{g}/settings/rooms` and `/g/{g}/settings/modlog` with
  JSON bodies (the existing `is_json` check is the CSRF guard). The answer is
  the saved view or a refusal message. Validation is server-side. The page
  renders its form with current values server-side, and a small
  `settings.js` (DOM APIs only, CSP Trusted Types hold) posts and shows the
  result.
- The guild page links to Settings next to History.
- `Backend` gains `settings_access`, `room_settings`, `save_room_settings` (and
  the mod log pair in PR 2). `LiveBackend` implements them over crack-core.

## 4. Docs

`docs/rooms-and-mod-log.md`: what each feature does, how to set it up, the
permission table, and a "for room owners" section that matches the panel. The
dashboard Settings page links to it.

## 5. Unverified Discord behaviour (check on TuneTitan before merge)

| Assumption | If wrong |
|---|---|
| A channel created in a category without overwrites is synced to it | The sync check (1.3 step 5) copies them; the room is deleted if it cannot |
| A member-level Speak deny mutes someone already connected, at once | Mute becomes a server mute plus a tracked undo on leave (stored in `RoomState`, reconciled at startup) |
| Pins work in a voice channel's chat | Nothing breaks: the panel is the chat's first message, and `/room panel` re-posts it |
| `MEMBER_DISCONNECT`/`MEMBER_MOVE` entries lack a target and aggregate | Attribution already pairs exactly when a target is present |
| Re-PUTting a role overwrite whose baseline bits the bot lacks is refused | The preflight warning is then just conservative |
| A modal can hold a string select (`CreateLabel::select_menu`) on desktop and mobile | Kick becomes two steps: a select message, then a reason-only modal |
| An aggregated `MEMBER_DISCONNECT`/`MEMBER_MOVE` entry keeps its id when its count rises | A re-created entry counts in full; D > L logs nothing, but D = L could misattribute |
| Moves to different channels never share one aggregated `MEMBER_MOVE` entry | A stale `channel_id` would send units to the wrong destination group |

## 6. Testing

Per [[cracktunes-standing-rules]]: sabotage every test, assert on what is sent,
and no source scans ([[enforce-with-types-not-source-scans]]).

- Pure, exhaustively:
  - `room_overwrites` for every state combination, including never-widen and
    lock over role allows;
  - the authority table and the protected targets;
  - the joiner and permit guards;
  - the preflight;
  - custom id round trips and malformed ids;
  - name template rendering and capping;
  - the rename throttle;
  - the empty decision (bots excluded);
  - `attribute` for every row of 2.3.
- Glue against `FakeRoomsDiscord` + `FakeTransport`:
  - a hub join creates, persists, overwrites, moves and posts, in that order;
  - a failed move deletes the channel and the row;
  - an emptied room is deleted after the grace period, and a rejoin cancels it;
  - a kick sends the disconnect and the deny overwrite, and emits the event
    carrying the reason;
  - a lock refused by Discord leaves the state unchanged.
- DB (`sqlx::test`): settings and room rows survive a save and a load;
  reconcile drops rows for missing channels.
- crack-web: 403 for a non-manager, the exact settings the backend receives,
  and validation refusals.
- The TuneTitan checklist covers section 5 and the glue tests cannot see:
  `handlers/serenity.rs` routes, modal rendering, real deletion timing.

## 7. Delivery

- **PR 1 `feat/voice-rooms` → v0.28.0:** sections 1, 3 (rooms), 4 (rooms part).
  Migration `room_settings` + `rooms`, in both `migrations/` and
  `crack-core/test_migrations/`. `.sqlx` regenerated by `scripts/sqlx_sync.py`.
- **PR 2 `feat/mod-log` → v0.29.0**, stacked on PR 1: sections 2, 3 (mod log),
  4 (mod log part), plus the room-action events.
- Both run on TuneTitan from branch images, with the owner's hand test,
  before merge. Production deploy only on the owner's go. Saucey's server then
  grants the permissions the preflight lists.

## 8. Left to the builder

- Module layout. Suggested: `crack-core/src/rooms/` (`state`, `overwrites`,
  `authority`, `preflight`, `ids`, `panel`, `discord`, `ops`, `lifecycle`),
  `crack-core/src/modlog/` (`event`, `attribution`, `poll`), commands under
  `crack-core/src/commands/rooms/`.
- Exact embed colours and copy, within the strings-in-`messages.rs` rule.
- How the deletion timers are held (a task per room with a cancellation
  token is the obvious shape).
