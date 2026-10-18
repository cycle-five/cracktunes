# Voice rooms and the mod log

CrackTunes can give each member a voice room of their own. A member joins one
"hub" voice channel and is moved into a new room, named after them, that they
control. Every control is a button on a panel posted in the room's chat, and
each button has a `/room` command that does the same thing. When the last
person leaves, the room is deleted.

Voice rooms are **off by default** in every server, and free: no premium needed.

- [Voice rooms: for admins](#voice-rooms--for-admins)
- [Voice rooms: for room owners](#voice-rooms--for-room-owners)
- [What gets deleted and when](#what-gets-deleted-and-when)
- [Why there is no deafen](#why-there-is-no-deafen)
- [Mod log](#mod-log)

## Voice rooms — for admins

You need **Manage Server** (or to be the server's owner) to set rooms up, change them or look at them.

### Turning rooms on

Pick the voice channel members will join to get a room. This is the **hub**.
Rooms are created in the hub's category, so put the hub in the category you
want the rooms to appear in. A hub with no category works, but then CrackTunes
needs its permissions server-wide.

There are two ways, and they change the same settings:

- **Dashboard.** Open dash.cracktun.es, pick the server, then **Settings**.
  Tick "Rooms are on", pick the hub channel, and press Save. The page also
  shows which permissions CrackTunes is still missing.
- **Discord.** Run `/rooms setup hub:<voice channel>`. It turns rooms on with
  that hub and answers with the status below.

The settings page has these options:

| Option | What it does | Default |
|---|---|---|
| Rooms are on | The on/off switch. Turning on needs a hub. | off |
| Hub channel | The voice channel members join to get a room. | none |
| Room name | The name given to new rooms. `{name}` becomes the member's server name. 1 to 100 characters. The page shows a preview. | `{name}'s room` |
| Default member limit | How many people a new room holds. 0 means no limit. 0 to 99. | 0 |
| Delete an empty room after | How long an empty room is kept: 0, 10, 30, 60, 120 or 300 seconds. | 30 seconds |

`/rooms setup` sets the enabled switch and the hub and leaves the other options
as they were. Change the name, limit and delay on the dashboard.

Rooms need CrackTunes' database. A CrackTunes without one cannot turn rooms on.
If the database could not be read when CrackTunes joined or rejoined the
server, rooms stay off there and saving is refused with "Room settings
couldn't be read from the database yet — try again in a minute." rather than
overwriting your settings with defaults. CrackTunes reads them again a minute
later.

### Permissions CrackTunes needs

CrackTunes needs these on the hub's **category**, because that is where it
creates and manages rooms. Grant them to CrackTunes' role (or to CrackTunes)
in the category's permission settings. Granting them server-wide also works.

| Permission | Why |
|---|---|
| View Channel | See the category and its rooms. |
| Connect | Be able to reach the rooms it manages. |
| Manage Channels | Create a room, rename it, set its member limit, delete it. |
| Manage Permissions (Manage Roles) | Lock a room, lock its chat, kick, mute and let people in. All of these are channel permission overwrites. |
| Move Members | Move a member from the hub into their new room, and disconnect someone who is kicked. |
| Send Messages | Post the panel in the room's chat. |
| Embed Links | Show the panel, which is an embed. |

On the **hub channel** itself CrackTunes needs View Channel, Connect and Move
Members. Normally these come from the category; a permission set directly on
the hub can take them away.

One more is optional: **Pin Messages**. With it the panel is pinned in the
room's chat. Without it rooms still work and the panel is just a normal
message.

Manage Permissions deserves a note. It is what lets CrackTunes lock rooms. A
lock rewrites the room's permission overwrites, and Discord only lets a bot
change permission bits it holds itself. CrackTunes copies the category's
overwrites into each new room, so if the category's rules mention a permission
CrackTunes does not have (for example Manage Messages), `/rooms status` warns
"Locking a room may fail". Grant CrackTunes that permission too, or lock may
be refused in those rooms.

### Checking the setup

Run `/rooms status` (it answers only to you). It shows whether rooms are on,
the hub, the room name, the default limit and the delay, then either "CrackTunes
has everything it needs." or one line per problem:

- "Give CrackTunes **Move Members** on #category": a required permission is
  missing there. The line says where: the category, the hub, or server-wide.
- "Optional: give CrackTunes **Pin Messages** on #category".
- "Locking a room may fail: CrackTunes lacks ...": see the note above.
- "Couldn't check permissions ...": CrackTunes cannot see the hub or its
  category (give it View Channel there) or it has only just started. Try again
  in a minute.

The status also links to the server's dashboard settings page when the bot
knows its dashboard address. A member who hits a missing permission is told
"CrackTunes is missing a permission on this room's category. An admin can run
`/rooms status` to see which."

### Turning rooms off

Run `/rooms off`, or untick "Rooms are on" on the dashboard. The hub stops
making new rooms. **Rooms that already exist stay**, with their panels and
controls, until they empty and are deleted as usual. Nothing is deleted at the
moment you turn rooms off.

### Who may run `/rooms`

Only members with Manage Server (and the server owner). Discord hides the command from everyone else.

### Limits worth knowing

- One room per member at a time. A member who owns a room and joins the hub
  again is moved back into their own room, not given a second.
- A member waits 10 seconds between new rooms.
- At most 50 rooms per server at once. At the limit, the hub stops making
  rooms until one is deleted.
- Rooms are never more open than their category. CrackTunes copies the
  category's permissions into each new room, and a room owner can only narrow
  who gets in, never open the room past the category's own rules. A member the
  category keeps out of the hub gets no room. If you tighten the category
  while a room exists and Discord carries the change into the room, the
  room's controls keep the tighter rule: locking or unlocking never puts the
  old one back. The same goes for a permission you set by hand in a room on
  its owner, helpers, allowed people or CrackTunes, such as denying a helper
  Connect or giving CrackTunes Manage Channels: the room's controls keep it.
  The exception is an allow of Connect or Send Messages on one of them, which
  Lock and Lock chat set and clear themselves. Allowing someone never lets
  them past a deny the room already holds for them.
- Moderators (anyone with Administrator, Manage Channels or Move Members) can
  use every control in every room.

## Voice rooms — for room owners

Join the hub and you are moved into your own room. You are its **owner**. A
panel is posted in the room's chat. It shows the owner, the helpers, whether
the room is locked and its chat locked, and the member limit, and it lists
every control. The buttons and the commands are the same list.

Run `/room` commands from the room's voice channel or its chat. Buttons that
need a choice open a small form.

### The controls

| Button | Command | What it does |
|---|---|---|
| 🚫 Kick | `/room kick member reason` | Disconnect someone and keep them out of the room. A reason is required, 1 to 200 characters. |
| 🔇 Mute | `/room mute member [reason]` | Stop someone speaking in this room. They stay connected. The reason is optional. |
| 🔊 Unmute | `/room unmute member` | Let a muted person speak again. |
| ↩️ Let back in | `/room letin member` | Lift a kick so the person can rejoin. |
| 🔒 Lock / 🔓 Unlock | `/room lock`, `/room unlock` | Close the room to newcomers, or open it again. |
| 🤐 Lock chat / 💬 Unlock chat | `/room chatlock`, `/room chatunlock` | Stop or allow typing in the room's chat. |
| 👥 Limit | `/room limit people` | How many people fit. 0 to 99; 0 means no limit. |
| ✏️ Rename | `/room rename name` | Change the room's name. 1 to 100 characters. |
| 🔑 Transfer | `/room transfer member` | Hand the room to someone in it. You become a plain member. |
| 🛡️ Helpers | `/room helper add member`, `/room helper remove member` | Let people run the moderation controls with you. |
| 🚪 Who can join | `/room allow member`, `/room allow role`, `/room block role`, `/room unblock role` | Let a person or a role in (useful with a locked room), or keep a role out. |
| 👑 Claim | `/room claim` | Take over a room whose owner has left. |
| ❓ Help | | A private, longer guide. |
| | `/room panel` | Post the panel again, at the bottom of the chat. |

The Lock and Lock chat buttons become Unlock and Unlock chat while the room is
locked.

### Who may do what

| | Owner | Helper | Moderator | Someone else in the room |
|---|---|---|---|---|
| Kick, mute, unmute, let back in | yes | yes | yes | no |
| Lock, unlock, lock chat, unlock chat | yes | yes | yes | no |
| Limit | yes | yes | yes | no |
| Post the panel again | yes | yes | yes | no |
| Rename | yes | no | yes | no |
| Transfer | yes | no | yes | no |
| Add or remove helpers | yes | no | yes | no |
| Allow, block, unblock | yes | no | yes | no |
| Claim | no | only when the owner has left | only when the owner has left | only when the owner has left |

- **Moderators** are server members with Administrator, Manage Channels or Move
  Members. They can act in any room, whether or not they are in it.
- **Helpers** are chosen by the owner. A helper who tries an owner-only control
  is told only the owner can do that.
- Someone who is not in the room and is not a moderator cannot do anything.
- **Claim** works only when the owner has left the room. While the owner is
  still in it, nobody can claim it. A claimer becomes the owner, and stops
  being a helper if they were one. A member who already owns another room
  cannot be handed a second.
- **Transfer**: the old owner becomes a plain member; the helpers stay.

### Who you cannot act on

You cannot kick or mute CrackTunes, a moderator, the room's owner (only a
moderator can act on the owner), or yourself. A helper also cannot kick or
mute another helper. You cannot transfer the room or add a helper to yourself
or to CrackTunes, and you cannot unmute yourself or let yourself back in.
Someone you kick is also removed from the helpers and from the people let in.

### Notes on each control

- **Kick** keeps the person out until you use Let back in, even if the room is
  open. The reason is required, so you cannot kick without giving one. It is
  not sent to the member; it appears in the [mod log](#mod-log) when the
  server has one on. The member picker lists only people it can be done to. A
  moderator who kicks the room's owner frees the room: anyone left in it can
  claim it, and the hub gives the old owner a new room rather than moving
  them back.
- **Mute** is for this room only. It does not server-mute the person anywhere
  else. If someone is muted and leaves, the mute stays with the room.
- **Lock** blocks newcomers, not the people already inside: they can rejoin
  if they drop out. The owner, helpers and anyone you allowed can still join,
  and so can CrackTunes, to play music. Allow a person or role first, then
  lock, to let only them in.
- **Block role** keeps a role out. It is refused for `@everyone`, for any role
  the owner holds and for any role CrackTunes holds, because it would lock the
  owner or CrackTunes out (CrackTunes needs to see a room to delete it). Use
  Lock instead.
  Allowing `@everyone` in is refused too: use Unlock instead.
- **Allow**, **Helpers** (add), **Transfer** and **Claim** are refused for a
  person or role the server's own permissions keep out ("This server's own
  permissions keep them out of this room"). A room can never be more open
  than its category.
- **Rename** is limited to twice every 10 minutes. Discord limits channel renames
  to that, and CrackTunes enforces it itself (the count starts over if the bot
  restarts). The reply says how long to wait.
- **Limit** needs a number from 0 to 99.

If you run a command while not in a room, CrackTunes says "You're not in a
room. Run this from your room's voice channel or its chat."

## What gets deleted and when

- **A room is deleted once no human is left in it**, after the server's delay
  (default 30 seconds; the admin picks 0, 10, 30, 60, 120 or 300). If someone
  joins before the delay runs out, the deletion is cancelled and the next
  emptying starts a fresh delay. A new room isn't deleted before CrackTunes has
  seen its owner arrive (it waits at least 10 seconds for that).
- Bots in the room do not count. A room with only bots in it is empty.
- **After a restart**: when CrackTunes starts, it checks every room it knows
  about. Empty rooms are deleted after the server's delay, not at once, and rooms
  with someone in them are kept.
- **A room someone deletes by hand** is forgotten. CrackTunes does not bring it
  back and does not keep trying to manage it. Running a `/room` command in it
  afterwards says you are not in a room.
- **Turning rooms off** deletes nothing at that moment. Rooms empty out and are
  deleted as usual.
- If Discord refuses a delete, CrackTunes tries again a minute later, up to
  three times in all. After that it waits for the next check (CrackTunes runs one whenever it
  starts or rejoins the server), or an admin can delete the channel by hand.
- A room's panel, owner, helpers, locks and mutes are stored in CrackTunes'
  database so they survive a restart. They are removed with the room.
- A room being created at the very moment CrackTunes crashes or is
  restarted may not have been stored yet; if one is left behind, delete it
  by hand.

## Why there is no deafen

Discord has no way to deafen someone in one channel. Server-deafen applies
to a person everywhere in the server, and it can only be done by a bot or
a moderator, so a room owner pressing "Deafen" would silence someone in every
voice channel they join, long after they left the room. CrackTunes does not
offer a control that would do that. Mute works because Discord can deny the
Speak permission in a single channel. There is no such permission for
hearing.

Kick, mute and lock cover what deafening someone in your room would be for.

## Mod log

The mod log is a channel the server picks where CrackTunes posts who muted,
deafened, disconnected or moved whom in voice, and who kicked, muted or let
back in whom in a voice room. Each entry is one embed with a timestamp.
Mentions sit inside the embed and nobody is pinged.

It is **off by default** in every server, free, and independent of voice
rooms: you can use either without the other. It needs CrackTunes' database;
without one it cannot be turned on.

### What it logs

| Entry | What it says | How sure it is |
|---|---|---|
| Server muted / unmuted | Who did it to whom, and the reason if the moderator gave one. | Exact: it comes straight from Discord's audit log. |
| Server deafened / undeafened | The same. | Exact. |
| Disconnected | Who was disconnected from which channel, and by whom. | Matched by timing (see below). |
| Moved | Who was moved from which channel to which, and by whom. | Matched by timing. |
| Disconnected several | Several disconnects at once, when they cannot be told apart. | Matched by timing. |
| Kicked from a room | Who kicked whom from which voice room, with the reason. | Exact: CrackTunes did it itself. |
| Muted / unmuted in a room | Who did it, to whom, in which room; a mute shows its reason if one was given. | Exact. |
| Let back into a room | Who lifted a room kick. | Exact. |

Server mutes and deafens appear as soon as Discord records them. A server mute
or deafen that CrackTunes applies to itself (it deafens itself when it joins a
channel) is not logged. A moderator muting CrackTunes is logged, and so is
CrackTunes muting someone else.

Another bot's moderation is logged the same way, with "(bot)" after its name:
its mutes and deafens, and its disconnects and moves (a temp-voice bot moving
people out of its hub, say). The action still happened, and the mark tells you
which bot to look at next.

### What it does not log, and why entries say "matched by timing"

Muting or deafening yourself is never logged. Neither is anything done to
CrackTunes' own voice connection: a moderator disconnecting or moving
CrackTunes is not logged (a moderator muting or deafening it is, as above).
Nor are CrackTunes' own moves, such as putting someone into the room it just
made for them; its room kicks are logged, with the kicker and their reason.

A member who leaves or switches channels by themselves isn't posted on their
own account. If it happens in the same few seconds as a forced disconnect or
move, the audit log can't tell them apart, so they may be listed in a
"disconnected N of" line or, rarely, credited to the moderator. That is why
those lines say "matched by timing".

Discord's audit log records that a moderator disconnected or moved *someone*,
but it does not say who. CrackTunes sees each member leave or move, and about 2
to 5 seconds after it happens it looks at the audit log, waits a second for
voice changes still on their way, and matches what changed there to the members
who left or moved. These entries therefore carry the footer "Matched from
Discord's audit log by timing", and they are the one place CrackTunes can be
unsure. When it cannot be sure:

- If several moderators acted around the same time, the entry says "by one of"
  and lists them.
- If several people were disconnected together and the audit log cannot say who
  did which, one line reads "<moderators> disconnected N of: <members>" (it
  names the first ten members and then "and N more"; with several moderators it
  starts "One or more of"). Several moves it cannot tell apart post nothing.
- If someone left or moved around the moment CrackTunes read the audit log, it
  cannot tell whether the log already held their entry, so it matches nothing
  of that kind (disconnects, or moves) from that look. At busy moments some
  forced disconnects and moves therefore go unlogged.
- If the audit log gives CrackTunes nothing it can match with confidence,
  **nothing is posted**. A missing line does not prove nobody forced the
  change.

Room kicks made from the panel or `/room kick` are logged once, as a room kick,
and are not also posted as a disconnect.

The kick reason shown in the log is for your moderators. It is never sent to
the member who was kicked.

### Turning it on

You need **Manage Server** (or to be the server's owner). There are two ways,
and they change the same settings:

- **Dashboard.** Open dash.cracktun.es, pick the server, then **Settings**.
  In the "Mod log" section tick "Mod log is on", pick the log channel and press
  Save. The page also shows which permissions CrackTunes is still missing. Only
  text channels can be picked; announcement channels cannot.
- **Discord.** Run `/modlog set channel:<text channel>`. It turns the log on
  and answers with the status below. `/modlog off` turns it off and keeps the
  channel for next time. Only members with Manage Server see these commands.

`/modlog status` (it answers only to you) shows whether the log is on and where,
then either "CrackTunes has everything it needs." or one line for each thing
still missing. The dashboard shows the same lines.

### Permissions CrackTunes needs

| Permission | Where | Why |
|---|---|---|
| View Audit Log | Server-wide (Server Settings, Roles) | To read who muted, deafened, disconnected or moved someone. The audit log is not a channel setting. |
| View Channel | The log channel | To see it. |
| Send Messages | The log channel | To post entries. |
| Embed Links | The log channel | Entries are embeds. |

Room kicks, room mutes and let-backs need only the log channel permissions;
they do not read the audit log.

If Discord refuses to show CrackTunes the audit log (View Audit Log missing),
CrackTunes **stops looking for about 10 minutes**, or until the mod log
settings are saved again (a save resumes it at most once a minute), and then
tries once more. While it is paused,
forced disconnects and moves are not logged, and they are not caught up
afterwards. Room actions are logged as usual. `/modlog status` and the
dashboard then show "Fetching is paused". Grant View Audit Log and save the
settings (press Save, or run `/modlog set` again) to resume at once; if a save
already resumed it less than a minute ago, wait a minute and save again.

Without View Audit Log, Discord also does not tell CrackTunes about server
mutes and deafens, so those are missed too.

### Who can read it

Whoever the log channel's own Discord permissions let see it. CrackTunes never
changes a channel's permissions. To keep the log private, make the channel
visible to only the roles you want: for example, give only your 18+ role View
Channel and leave it denied for @everyone. `/modlog status` reminds you that
anyone who can see the channel can read the log.

### Good to know

- Turning the log off stops new entries; entries already posted stay.
- If the log channel is deleted, or CrackTunes loses permission in it, entries
  are dropped and CrackTunes keeps working normally. `/modlog status` says what
  is missing.
- Only changes that happen while the log is on are recorded; nothing is
  reconstructed afterwards.
