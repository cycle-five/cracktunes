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
- [Mod log: coming in v0.29.0](#mod-log--coming-in-v0290)

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
  old one back.
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
  not sent to the member; it will appear in the mod log once that ships
  (v0.29.0). The member picker lists only people it can be done to. A moderator who kicks
  the room's owner frees the room: anyone left in it can claim it, and the
  hub gives the old owner a new room rather than moving them back.
- **Mute** is for this room only. It does not server-mute the person anywhere
  else. If someone is muted and leaves, the mute stays with the room.
- **Lock** blocks newcomers, not the people already inside: people already
  inside can rejoin if they drop out. The owner, helpers and anyone you
  allowed can still join, and so can CrackTunes, to play music. Allow a person
  or role first, then lock, to let only them in.
- **Block role** keeps a role out. It is refused for `@everyone` and for any role
  the owner holds, because it would lock the owner out. Use Lock instead.
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

## Why there is no deafen

Discord has no way to deafen someone in one channel. Server-deafen applies
to a person everywhere in the server, and it can only be done by a bot or
a moderator, so a room owner pressing "Deafen" would silence someone in every
voice channel they join, long after they left the room. CrackTunes does not
offer a control that would do that. Mute works because Discord can deny the
Speak permission in a single channel. There is no such permission for
hearing.

Kick, mute and lock cover what deafening someone in your room would be for.

## Mod log — coming in v0.29.0

Not in this version. The mod log will be a channel the server chooses where
CrackTunes posts who server-muted, deafened, disconnected or moved whom, and who
kicked whom from a voice room. It ships in v0.29.0 and this section will
describe it then.
