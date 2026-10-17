//! Every room action, from a button, a modal or `/room`, goes through [`act`],
//! in spec 1.6's order: authority, guard, the new state (on a copy), Discord,
//! persist, commit, redraw the panel. A Discord failure stops before anything
//! is persisted or committed, so memory never runs ahead of Discord.

use super::authority::{is_moderator, may, standing, target_ok, RoomAction, TargetStanding};
use super::card::{PanelView, RoomCard, RoomDone, RoomRefusal};
use super::discord::{DiscordError, RoomsDiscord};
use super::facts::GuildFacts;
use super::guard::role_can_connect;
use super::overwrites::{diff, room_overwrites, OverwriteChange};
use super::registry::LiveRoom;
use super::settings::{RoomSettings, SettingsError, ROOM_NAME_MAX};
use super::state::{OverwriteTarget, RoomState, StoredOverwrite};
use crate::messaging::courier::{edit_rendered_message, post_message};
use crate::messaging::message::CrackedMessage;
use crate::messaging::render::{render, RenderCx, Rendered};
use crate::messaging::transport::{Transport, TransportError};
use crate::Data;
use serenity::all::{ChannelId, MessageId, RoleId, UserId};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What an action needs besides its inputs.
pub struct RoomCx<'a> {
    pub data: &'a Data,
    pub discord: &'a dyn RoomsDiscord,
    pub transport: &'a dyn Transport,
}

/// Do `action` in `room` on behalf of `actor`, or say why not.
pub async fn act(
    cx: &RoomCx<'_>,
    facts: &GuildFacts,
    room: ChannelId,
    actor: UserId,
    action: RoomAction,
    now: Instant,
) -> Result<RoomDone, RoomRefusal> {
    // 1. The room, under its lock: two presses on one room run in turn.
    let handle = cx.data.rooms.get(room).ok_or(RoomRefusal::NotInRoom)?;
    let mut live = handle.lock().await;
    // `RoomRegistry::remove` runs under this lock. A room removed while we
    // waited is gone: acting on it would write to a dead channel, and a
    // transfer would index its new owner under it again.
    let registered = cx.data.rooms.get(room);
    if !registered.is_some_and(|r| Arc::ptr_eq(&r, &handle)) || live.guild != facts.guild {
        return Err(RoomRefusal::NotInRoom);
    }
    let guild = live.guild;

    // 2. Authority, then the input itself.
    authorize(facts, &live.state, room, actor, &action)?;
    let action = validate(action)?;
    // One room per member per server.
    let new_owner = match &action {
        RoomAction::Transfer { target } => Some(*target),
        RoomAction::Claim => Some(actor),
        _ => None,
    };
    if new_owner
        .and_then(|u| cx.data.rooms.owned_by(guild, u))
        .is_some_and(|other| other != room)
    {
        return Err(RoomRefusal::OwnsAnotherRoom);
    }
    owner_kept_in(facts, &live.state, &action)?;

    // 3. Never widen: whoever is let in must already be able to get in.
    guard(facts, &live.baseline, &action)?;

    // 4. The new state, on a copy.
    let mut next = live.state.clone();
    let done = apply(&mut next, &action, actor);

    // 5-6. Discord. Any failure returns here, with nothing persisted.
    let before = room_overwrites(guild, facts.bot, &live.baseline, &live.state);
    let after = room_overwrites(guild, facts.bot, &live.baseline, &next);
    write_overwrites(cx.discord, room, &before, &after).await?;
    // A kick shuts them out first (above), then disconnects them: a refused
    // deny has done nothing, and a refused disconnect still leaves the kick
    // standing, since they cannot come back.
    if let RoomAction::Kick { target, .. } = &action {
        if facts.in_room(*target, room) {
            match cx.discord.move_member(guild, *target, None).await {
                // Left voice since the snapshot: nobody to disconnect.
                Ok(()) | Err(DiscordError::NotConnected) => {},
                Err(err) => tracing::warn!(
                    "rooms: {target} is kicked from {room} but disconnecting them failed; they \
                     stay blocked: {err:?}"
                ),
            }
        }
    }
    match &action {
        RoomAction::Limit(n) => cx
            .discord
            .set_user_limit(room, *n)
            .await
            .map_err(|e| refusal(room, e))?,
        RoomAction::Rename(name) => {
            // Counted once the attempt is made, even if Discord refuses it:
            // a slot wasted is a wait; a slot short is serenity sitting out
            // Discord's 429 with this room's lock held.
            live.renames
                .try_take(now)
                .map_err(|wait| RoomRefusal::RenameLimited {
                    wait_secs: whole_secs(wait),
                })?;
            cx.discord
                .rename(room, name)
                .await
                .map_err(|e| refusal(room, e))?;
        },
        RoomAction::Panel => {
            post_panel(cx, &mut live)
                .await
                .ok_or(RoomRefusal::DiscordFailed)?;
            next.panel = live.state.panel;
        },
        _ => {},
    }

    // 7. Persist. A failure does not undo Discord: memory follows Discord,
    // and the next save writes the whole state again.
    persist(cx.data, room, &next).await;

    // 8. Commit.
    let old_owner = live.state.owner;
    live.state = next;
    match &action {
        RoomAction::Limit(n) => live.limit = *n,
        RoomAction::Rename(name) => live.name.clone_from(name),
        _ => {},
    }
    if live.state.owner != old_owner {
        cx.data
            .rooms
            .reindex_owner(guild, old_owner, live.state.owner, room);
    }
    // A moderator kicked the owner: the room is no longer theirs to come
    // back to through the hub, and it can be claimed.
    if matches!(&action, RoomAction::Kick { target, .. } if *target == old_owner) {
        cx.data.rooms.unindex_owner(guild, old_owner, room);
    }

    // 9. Redraw; the panel just posted is already current.
    if !matches!(action, RoomAction::Panel) && redraw_panel(cx, &mut live).await {
        persist(cx.data, room, &live.state).await;
    }
    Ok(done)
}

/// Post the panel at the bottom of the room's chat, try to pin it, and
/// delete the one it replaces. `None`, with `state.panel` untouched, when the
/// send failed.
pub async fn post_panel(cx: &RoomCx<'_>, room: &mut LiveRoom) -> Option<MessageId> {
    let channel = room.channel.widen();
    let id = match post_message(cx.transport, channel, &panel(room)).await {
        Ok(id) => id,
        Err(err) => {
            tracing::warn!("rooms: posting the panel in {} failed: {err}", room.channel);
            return None;
        },
    };
    // Discord's support for pins in a voice channel's chat is unclear.
    if let Err(err) = cx.discord.pin(room.channel, id).await {
        tracing::debug!(
            "rooms: pinning the panel in {} failed: {err:?}",
            room.channel
        );
    }
    if let Some(old) = room.state.panel.replace(id) {
        match cx.transport.delete(channel, old).await {
            Ok(()) | Err(TransportError::UnknownMessage) => {},
            Err(err) => tracing::warn!(
                "rooms: deleting the old panel in {} failed: {err}",
                room.channel
            ),
        }
    }
    Some(id)
}

/// Redraw the panel in place. A panel that is gone (or was never posted) is
/// posted again. Whether `state.panel` changed, so the caller can save it.
pub async fn redraw_panel(cx: &RoomCx<'_>, room: &mut LiveRoom) -> bool {
    let before = room.state.panel;
    match before {
        None => {
            post_panel(cx, room).await;
        },
        Some(id) => {
            let out = panel(room);
            match edit_rendered_message(cx.transport, room.channel.widen(), id, out).await {
                Ok(()) => {},
                Err(TransportError::UnknownMessage) => {
                    room.state.panel = None;
                    post_panel(cx, room).await;
                },
                Err(err) => tracing::warn!(
                    "rooms: redrawing the panel in {} failed: {err}",
                    room.channel
                ),
            }
        },
    }
    room.state.panel != before
}

fn panel(room: &LiveRoom) -> Rendered {
    let view = PanelView {
        room: room.channel,
        name: room.name.clone(),
        owner: room.state.owner,
        helpers: room.state.helpers.iter().copied().collect(),
        locked: room.state.locked,
        chat_locked: room.state.chat_locked,
        limit: room.limit,
    };
    render(
        &CrackedMessage::from(RoomCard::Panel(view)),
        &RenderCx::now(),
    )
}

/// The member an action is aimed at, if any.
pub(crate) fn user_target(action: &RoomAction) -> Option<UserId> {
    match action {
        RoomAction::Kick { target, .. }
        | RoomAction::LetIn { target }
        | RoomAction::Mute { target, .. }
        | RoomAction::Unmute { target }
        | RoomAction::Transfer { target }
        | RoomAction::HelperAdd { target }
        | RoomAction::HelperRemove { target }
        | RoomAction::AllowUser { target } => Some(*target),
        _ => None,
    }
}

/// What `target` is to `actor` in this room, most protected first.
pub(crate) fn target_standing(
    facts: &GuildFacts,
    state: &RoomState,
    actor: UserId,
    target: UserId,
) -> TargetStanding {
    if target == actor {
        TargetStanding::Itself
    } else if target == facts.bot {
        TargetStanding::TheBot
    } else if is_moderator(facts.guild_perms(target)) {
        TargetStanding::Moderator
    } else if target == state.owner {
        TargetStanding::Owner
    } else if state.helpers.contains(&target) {
        TargetStanding::Helper
    } else {
        TargetStanding::Plain
    }
}

/// Who may do this, to whom, and is the target where the action needs them.
fn authorize(
    facts: &GuildFacts,
    state: &RoomState,
    room: ChannelId,
    actor: UserId,
    action: &RoomAction,
) -> Result<(), RoomRefusal> {
    let who = standing(
        actor,
        state,
        is_moderator(facts.guild_perms(actor)),
        facts.in_room(actor, room),
    );
    may(who, action, facts.in_room(state.owner, room))?;
    let Some(target) = user_target(action) else {
        return Ok(());
    };
    target_ok(who, action, target_standing(facts, state, actor, target))?;
    match action {
        RoomAction::Kick { .. } | RoomAction::Mute { .. } | RoomAction::Transfer { .. }
            if !facts.in_room(target, room) =>
        {
            Err(RoomRefusal::TargetNotInRoom)
        },
        RoomAction::LetIn { .. } if !state.blocked.contains(&target) => {
            Err(RoomRefusal::NotBlocked)
        },
        _ => Ok(()),
    }
}

/// Check the action's own input; a rename comes back with its name trimmed.
/// `interact::action_for` runs it on a modal's input, so both say the same.
pub(crate) fn validate(action: RoomAction) -> Result<RoomAction, RoomRefusal> {
    match action {
        RoomAction::Limit(n) if n > 99 => Err(RoomRefusal::BadLimit),
        RoomAction::Rename(raw) => {
            let name = raw.trim();
            let len = name.chars().count();
            if len == 0 || len > ROOM_NAME_MAX || name.chars().any(char::is_control) {
                Err(RoomRefusal::BadName)
            } else {
                Ok(RoomAction::Rename(name.to_owned()))
            }
        },
        other => Ok(other),
    }
}

/// Blocking a role must not lock the room's owner out: not `@everyone`, and
/// not a role the owner holds (as far as `facts` knows their roles).
fn owner_kept_in(
    facts: &GuildFacts,
    state: &RoomState,
    action: &RoomAction,
) -> Result<(), RoomRefusal> {
    let RoomAction::BlockRole { role } = action else {
        return Ok(());
    };
    let everyone = RoleId::new(facts.guild.get());
    let owners = facts
        .roles
        .get(&state.owner)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if *role == everyone || owners.contains(role) {
        Err(RoomRefusal::CannotBlockRole)
    } else {
        Ok(())
    }
}

/// Every member or role the action would let connect must already be able to.
fn guard(
    facts: &GuildFacts,
    baseline: &[StoredOverwrite],
    action: &RoomAction,
) -> Result<(), RoomRefusal> {
    let ok = match action {
        RoomAction::HelperAdd { target }
        | RoomAction::AllowUser { target }
        | RoomAction::Transfer { target } => facts.member_can_join(*target, baseline),
        RoomAction::AllowRole { role } => role_can_connect(&facts.baseline(baseline), *role),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(RoomRefusal::CannotJoinHere)
    }
}

/// The action's effect on the room's state, and what to tell the presser.
fn apply(s: &mut RoomState, action: &RoomAction, actor: UserId) -> RoomDone {
    match action {
        RoomAction::Kick { target, .. } => {
            s.blocked.insert(*target);
            s.helpers.remove(target);
            s.permitted_users.remove(target);
            RoomDone::Kicked(*target)
        },
        RoomAction::LetIn { target } => {
            s.blocked.remove(target);
            RoomDone::LetIn(*target)
        },
        RoomAction::Mute { target, .. } => {
            s.muted.insert(*target);
            RoomDone::Muted(*target)
        },
        RoomAction::Unmute { target } => {
            s.muted.remove(target);
            RoomDone::Unmuted(*target)
        },
        RoomAction::Lock => {
            s.locked = true;
            RoomDone::Locked
        },
        RoomAction::Unlock => {
            s.locked = false;
            RoomDone::Unlocked
        },
        RoomAction::ChatLock => {
            s.chat_locked = true;
            RoomDone::ChatLocked
        },
        RoomAction::ChatUnlock => {
            s.chat_locked = false;
            RoomDone::ChatUnlocked
        },
        RoomAction::Limit(n) => RoomDone::LimitSet(*n),
        RoomAction::Rename(name) => RoomDone::Renamed(name.clone()),
        // The old owner becomes a plain member; helpers stay.
        RoomAction::Transfer { target } => {
            s.helpers.remove(target);
            s.owner = *target;
            RoomDone::Transferred(*target)
        },
        RoomAction::HelperAdd { target } => {
            s.helpers.insert(*target);
            RoomDone::HelperAdded(*target)
        },
        RoomAction::HelperRemove { target } => {
            s.helpers.remove(target);
            RoomDone::HelperRemoved(*target)
        },
        RoomAction::AllowUser { target } => {
            s.permitted_users.insert(*target);
            RoomDone::AllowedUser(*target)
        },
        RoomAction::AllowRole { role } => {
            s.permitted_roles.insert(*role);
            RoomDone::AllowedRole(*role)
        },
        RoomAction::BlockRole { role } => {
            s.blocked_roles.insert(*role);
            RoomDone::RoleBlocked(*role)
        },
        RoomAction::UnblockRole { role } => {
            s.blocked_roles.remove(role);
            RoomDone::RoleUnblocked(*role)
        },
        RoomAction::Claim => {
            s.helpers.remove(&actor);
            s.owner = actor;
            RoomDone::Claimed
        },
        RoomAction::Panel => RoomDone::PanelPosted,
    }
}

/// Write the overwrites that differ, in order. If one is refused, the ones
/// already written are put back, so a half-done lock does not leave the room
/// locked while its state says open.
async fn write_overwrites(
    discord: &dyn RoomsDiscord,
    room: ChannelId,
    before: &[StoredOverwrite],
    after: &[StoredOverwrite],
) -> Result<(), RoomRefusal> {
    let mut written: Vec<OverwriteTarget> = Vec::new();
    for change in diff(before, after) {
        let target = match &change {
            OverwriteChange::Put(o) => o.target,
            OverwriteChange::Delete(t) => *t,
        };
        if let Err(err) = write(discord, room, &change).await {
            let refused = refusal(room, err);
            undo(discord, room, before, &written).await;
            return Err(refused);
        }
        written.push(target);
    }
    Ok(())
}

async fn write(
    discord: &dyn RoomsDiscord,
    room: ChannelId,
    change: &OverwriteChange,
) -> Result<(), DiscordError> {
    match change {
        OverwriteChange::Put(o) => discord.put_overwrite(room, o).await,
        OverwriteChange::Delete(t) => discord.delete_overwrite(room, *t).await,
    }
}

/// Put back what `before` held for each written target, newest first. Best
/// effort: a failure is logged.
async fn undo(
    discord: &dyn RoomsDiscord,
    room: ChannelId,
    before: &[StoredOverwrite],
    written: &[OverwriteTarget],
) {
    for t in written.iter().rev() {
        let back = match before.iter().find(|o| o.target == *t) {
            Some(o) => OverwriteChange::Put(*o),
            None => OverwriteChange::Delete(*t),
        };
        if let Err(err) = write(discord, room, &back).await {
            tracing::warn!("rooms: putting back an overwrite on {room} failed: {err:?}");
        }
    }
}

fn refusal(room: ChannelId, err: DiscordError) -> RoomRefusal {
    tracing::warn!("rooms: a Discord call for room {room} failed: {err:?}");
    match err {
        DiscordError::MissingPermissions => RoomRefusal::MissingPermissions,
        DiscordError::UnknownChannel | DiscordError::NotConnected | DiscordError::Other(_) => {
            RoomRefusal::DiscordFailed
        },
    }
}

/// A wait in whole seconds, rounded up, at least one.
fn whole_secs(wait: Duration) -> u32 {
    let secs = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
    u32::try_from(secs).unwrap_or(u32::MAX).max(1)
}

/// Save the room's state when there is a database. A failure is logged.
async fn persist(data: &Data, room: ChannelId, state: &RoomState) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    if let Err(err) = crate::db::rooms::save_state(pool, room, state.owner, state).await {
        tracing::warn!("rooms: saving room {room} failed; memory follows Discord: {err}");
    }
}

/// Why [`save_settings`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSaveError {
    /// The settings failed validation.
    Invalid(SettingsError),
    /// The database refused the write.
    Db,
}

/// R4: the one way a server's room settings change (`/rooms`, the dashboard).
/// Validates, writes the database (when there is one), then the registry, so
/// memory never runs ahead of what a restart would load.
pub async fn save_settings(data: &Data, s: RoomSettings) -> Result<(), SettingsSaveError> {
    s.validate(data.database_pool.is_some())
        .map_err(SettingsSaveError::Invalid)?;
    if let Some(pool) = &data.database_pool {
        if let Err(err) = crate::db::rooms::save_settings(pool, &s).await {
            tracing::warn!("rooms: saving settings of {} failed: {err}", s.guild);
            return Err(SettingsSaveError::Db);
        }
    }
    data.rooms.set_settings(s);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::test_support::{FakeTransport, Op};
    use crate::messaging::transport::TransportError;
    use crate::rooms::authority::Reason;
    use crate::rooms::discord::DiscordError;
    use crate::rooms::facts::tests::empty;
    use crate::rooms::registry::RenameWindow;
    use crate::rooms::state::{OverwriteTarget, RoomState, StoredOverwrite};
    use crate::rooms::test_support::{Call, CallKind, FakeRoomsDiscord};
    use crate::DataInner;
    use serenity::all::{GuildId, Permissions, RoleId};
    use std::sync::Arc;
    use std::time::Duration;

    const G: GuildId = GuildId::new(100);
    const ROOM: ChannelId = ChannelId::new(500);
    const PANEL: MessageId = MessageId::new(900);
    const OWNER: UserId = UserId::new(1);
    const HELPER: UserId = UserId::new(2);
    const TARGET: UserId = UserId::new(3);
    const OUTSIDE: UserId = UserId::new(4);
    const MODERATOR: UserId = UserId::new(5);
    const BOT: UserId = UserId::new(99);
    const MOD_ROLE: RoleId = RoleId::new(7);
    const V: Permissions = Permissions::VIEW_CHANNEL;
    const C: Permissions = Permissions::CONNECT;
    const NONE: Permissions = Permissions::empty();

    fn member(id: UserId) -> OverwriteTarget {
        OverwriteTarget::Member { id }
    }
    fn role(id: u64) -> OverwriteTarget {
        OverwriteTarget::Role {
            id: RoleId::new(id),
        }
    }
    fn ow(target: OverwriteTarget, allow: Permissions, deny: Permissions) -> StoredOverwrite {
        StoredOverwrite {
            target,
            allow,
            deny,
        }
    }

    /// Owner, helper, target and the bot in the room; one member outside it;
    /// a moderator (Move Members) who is not in voice.
    fn facts() -> GuildFacts {
        let mut f = empty(G);
        f.bot = BOT;
        f.role_perms = [(MOD_ROLE, Permissions::MOVE_MEMBERS)].into();
        f.roles = [
            (OWNER, vec![]),
            (HELPER, vec![]),
            (TARGET, vec![]),
            (OUTSIDE, vec![]),
            (MODERATOR, vec![MOD_ROLE]),
        ]
        .into();
        f.voice = [
            (OWNER, (ROOM, false)),
            (HELPER, (ROOM, false)),
            (TARGET, (ROOM, false)),
            (BOT, (ROOM, true)),
            (OUTSIDE, (ChannelId::new(501), false)),
        ]
        .into();
        f
    }

    fn live(baseline: Vec<StoredOverwrite>) -> LiveRoom {
        let mut state = RoomState::new(OWNER);
        state.helpers.insert(HELPER);
        state.panel = Some(PANEL);
        LiveRoom {
            guild: G,
            channel: ROOM,
            name: "Saucey's room".into(),
            state,
            baseline,
            limit: 0,
            renames: RenameWindow::default(),
        }
    }

    struct World {
        data: Data,
        discord: FakeRoomsDiscord,
        transport: FakeTransport,
    }

    impl World {
        fn new(room: LiveRoom) -> Self {
            let data = Data(Arc::new(DataInner {
                ..Default::default()
            }));
            data.rooms.insert(room);
            Self {
                data,
                discord: FakeRoomsDiscord::default(),
                transport: FakeTransport::default(),
            }
        }

        fn cx(&self) -> RoomCx<'_> {
            RoomCx {
                data: &self.data,
                discord: &self.discord,
                transport: &self.transport,
            }
        }

        async fn act(
            &self,
            facts: &GuildFacts,
            actor: UserId,
            action: RoomAction,
        ) -> Result<RoomDone, RoomRefusal> {
            self.act_at(facts, actor, action, Instant::now()).await
        }

        async fn act_at(
            &self,
            facts: &GuildFacts,
            actor: UserId,
            action: RoomAction,
            now: Instant,
        ) -> Result<RoomDone, RoomRefusal> {
            act(&self.cx(), facts, ROOM, actor, action, now).await
        }

        async fn room(&self) -> LiveRoom {
            self.data
                .rooms
                .get(ROOM)
                .expect("the room is registered")
                .lock()
                .await
                .clone()
        }
    }

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn on_with_hub() -> RoomSettings {
        RoomSettings {
            enabled: true,
            hub: Some(ChannelId::new(55)),
            ..RoomSettings::off(G)
        }
    }

    #[tokio::test]
    async fn save_settings_refuses_invalid_and_leaves_the_registry_alone() {
        let data = Data(Arc::new(DataInner::default()));
        let before = data.rooms.settings(G);
        // Enabling needs a database.
        assert_eq!(
            save_settings(&data, on_with_hub()).await,
            Err(SettingsSaveError::Invalid(SettingsError::NoDatabase))
        );
        // A bad template, whatever the database.
        let bad = RoomSettings {
            name_template: "  ".into(),
            ..RoomSettings::off(G)
        };
        assert_eq!(
            save_settings(&data, bad).await,
            Err(SettingsSaveError::Invalid(SettingsError::BadTemplate))
        );
        assert_eq!(data.rooms.settings(G), before);
    }

    #[tokio::test]
    async fn save_settings_without_a_pool_still_turns_rooms_off() {
        let data = Data(Arc::new(DataInner::default()));
        let off = RoomSettings {
            default_limit: 4,
            ..RoomSettings::off(G)
        };
        assert_eq!(save_settings(&data, off.clone()).await, Ok(()));
        assert_eq!(data.rooms.settings(G), off);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn save_settings_writes_the_database_then_the_registry(pool: sqlx::PgPool) {
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        let s = on_with_hub();
        assert_eq!(save_settings(&data, s.clone()).await, Ok(()));
        assert_eq!(data.rooms.settings(G), s);
        assert_eq!(
            crate::db::rooms::load_settings(&pool, G).await.unwrap(),
            Some(s)
        );
    }

    fn kick(target: UserId) -> RoomAction {
        RoomAction::Kick {
            target,
            reason: Reason::parse("spamming").unwrap(),
        }
    }

    #[tokio::test]
    async fn kick_denies_then_disconnects_and_returns_kicked() {
        let mut room = live(vec![]);
        room.state.helpers.insert(TARGET);
        room.state.permitted_users.insert(TARGET);
        let w = World::new(room);
        let got = w.act(&facts(), OWNER, kick(TARGET)).await;
        assert_eq!(got, Ok(RoomDone::Kicked(TARGET)));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(ROOM.get(), ow(member(TARGET), NONE, V | C)),
                Call::Move(TARGET.get(), None),
            ],
            "shut out first, so a refused disconnect still leaves them out"
        );
        let state = w.room().await.state;
        assert!(state.blocked.contains(&TARGET));
        assert!(
            !state.helpers.contains(&TARGET),
            "a kicked helper is not one"
        );
        assert!(!state.permitted_users.contains(&TARGET), "nor permitted");
        assert!(state.helpers.contains(&HELPER));
        assert_eq!(w.transport.ops(), vec![Op::Edit(ROOM.get(), PANEL.get())]);
    }

    #[tokio::test]
    async fn kick_of_someone_already_gone_from_voice_still_blocks_them() {
        let w = World::new(live(vec![]));
        w.discord
            .fail_next(CallKind::Move, DiscordError::NotConnected);
        assert_eq!(
            w.act(&facts(), OWNER, kick(TARGET)).await,
            Ok(RoomDone::Kicked(TARGET))
        );
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(ROOM.get(), ow(member(TARGET), NONE, V | C)),
                Call::Move(TARGET.get(), None),
            ]
        );
        assert!(w.room().await.state.blocked.contains(&TARGET));

        // A disconnect refused for any other reason, once they are denied:
        // the kick stands (they are blocked), and the failure is logged.
        let w = World::new(live(vec![]));
        w.discord
            .fail_next(CallKind::Move, DiscordError::MissingPermissions);
        assert_eq!(
            w.act(&facts(), OWNER, kick(TARGET)).await,
            Ok(RoomDone::Kicked(TARGET))
        );
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(ROOM.get(), ow(member(TARGET), NONE, V | C)),
                Call::Move(TARGET.get(), None),
            ]
        );
        assert!(w.room().await.state.blocked.contains(&TARGET));
        assert_eq!(w.transport.ops(), vec![Op::Edit(ROOM.get(), PANEL.get())]);
    }

    #[tokio::test]
    async fn a_kick_whose_deny_is_refused_does_nothing() {
        let w = World::new(live(vec![]));
        w.discord
            .fail_next(CallKind::Put, DiscordError::MissingPermissions);
        assert_eq!(
            w.act(&facts(), OWNER, kick(TARGET)).await,
            Err(RoomRefusal::MissingPermissions)
        );
        assert_eq!(
            w.discord.calls(),
            vec![Call::Put(ROOM.get(), ow(member(TARGET), NONE, V | C))],
            "not disconnected"
        );
        assert!(w.room().await.state.blocked.is_empty());
        assert_eq!(w.transport.ops(), vec![], "no panel edit");
    }

    #[tokio::test]
    async fn kick_of_someone_not_in_room_refuses_and_sends_nothing() {
        let w = World::new(live(vec![]));
        let got = w.act(&facts(), OWNER, kick(OUTSIDE)).await;
        assert_eq!(got, Err(RoomRefusal::TargetNotInRoom));
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(w.transport.ops(), vec![]);
        assert!(w.room().await.state.blocked.is_empty());
    }

    #[tokio::test]
    async fn mute_and_transfer_of_someone_not_in_room_refuse() {
        let w = World::new(live(vec![]));
        let mute = RoomAction::Mute {
            target: OUTSIDE,
            reason: None,
        };
        let transfer = RoomAction::Transfer { target: OUTSIDE };
        for a in [mute, transfer] {
            assert_eq!(
                w.act(&facts(), OWNER, a).await,
                Err(RoomRefusal::TargetNotInRoom)
            );
        }
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(w.room().await.state, live(vec![]).state);
    }

    #[tokio::test]
    async fn let_in_of_someone_not_blocked_refuses() {
        let w = World::new(live(vec![]));
        let got = w
            .act(&facts(), OWNER, RoomAction::LetIn { target: OUTSIDE })
            .await;
        assert_eq!(got, Err(RoomRefusal::NotBlocked));
        assert_eq!(w.discord.calls(), vec![]);
    }

    #[tokio::test]
    async fn let_in_after_kick_deletes_the_member_overwrite() {
        let w = World::new(live(vec![]));
        w.act(&facts(), OWNER, kick(TARGET)).await.unwrap();
        let got = w
            .act(&facts(), HELPER, RoomAction::LetIn { target: TARGET })
            .await;
        assert_eq!(got, Ok(RoomDone::LetIn(TARGET)));
        assert_eq!(
            w.discord.calls()[2..],
            [Call::DeleteOverwrite(ROOM.get(), member(TARGET))]
        );
        assert!(w.room().await.state.blocked.is_empty());
    }

    #[tokio::test]
    async fn a_refused_overwrite_leaves_state_unchanged() {
        let w = World::new(live(vec![]));
        w.discord
            .fail_next(CallKind::Put, DiscordError::MissingPermissions);
        let got = w.act(&facts(), OWNER, RoomAction::Lock).await;
        assert_eq!(got, Err(RoomRefusal::MissingPermissions));
        assert!(!w.room().await.state.locked);
        assert_eq!(w.transport.ops(), vec![], "no panel edit");
    }

    #[tokio::test]
    async fn any_other_discord_failure_is_discord_failed() {
        let w = World::new(live(vec![]));
        w.discord
            .fail_next(CallKind::Limit, DiscordError::Other("boom".into()));
        let got = w.act(&facts(), OWNER, RoomAction::Limit(4)).await;
        assert_eq!(got, Err(RoomRefusal::DiscordFailed));
        assert_eq!(w.room().await.limit, 0);
        assert_eq!(w.transport.ops(), vec![]);
    }

    #[tokio::test]
    async fn a_partly_applied_change_is_rolled_back() {
        let mut room = live(vec![]);
        room.state.locked = true;
        let w = World::new(room);
        // Transfer under lock: the old owner's allow goes, the new owner's
        // comes. The second write fails, so the first is undone.
        w.discord
            .fail_next(CallKind::Put, DiscordError::MissingPermissions);
        let got = w
            .act(&facts(), OWNER, RoomAction::Transfer { target: TARGET })
            .await;
        assert_eq!(got, Err(RoomRefusal::MissingPermissions));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::DeleteOverwrite(ROOM.get(), member(OWNER)),
                Call::Put(ROOM.get(), ow(member(TARGET), C, NONE)),
                Call::Put(ROOM.get(), ow(member(OWNER), C, NONE)),
            ]
        );
        assert_eq!(w.room().await.state.owner, OWNER);
        assert_eq!(w.data.rooms.owned_by(G, OWNER), Some(ROOM));
        assert_eq!(w.data.rooms.owned_by(G, TARGET), None);
    }

    #[tokio::test]
    async fn lock_puts_only_the_changed_targets() {
        let baseline = vec![
            // Allows Connect: moved to deny.
            ow(role(18), V | C, NONE),
            // Does not allow Connect: left alone.
            ow(role(19), V, NONE),
            // A member overwrite of the category's: left alone.
            ow(member(UserId::new(42)), Permissions::SPEAK, NONE),
        ];
        let w = World::new(live(baseline));
        let got = w.act(&facts(), OWNER, RoomAction::Lock).await;
        assert_eq!(got, Ok(RoomDone::Locked));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(ROOM.get(), ow(member(OWNER), C, NONE)),
                Call::Put(ROOM.get(), ow(member(HELPER), C, NONE)),
                Call::Put(ROOM.get(), ow(member(BOT), C, NONE)),
                Call::Put(ROOM.get(), ow(role(18), V, C)),
                Call::Put(ROOM.get(), ow(role(G.get()), NONE, C)),
            ]
        );
        assert!(w.room().await.state.locked);
        assert_eq!(w.transport.ops(), vec![Op::Edit(ROOM.get(), PANEL.get())]);
    }

    #[tokio::test]
    async fn unlock_puts_the_baseline_back() {
        let w = World::new(live(vec![ow(role(18), V | C, NONE)]));
        w.act(&facts(), OWNER, RoomAction::Lock).await.unwrap();
        let locked = w.discord.calls().len();
        assert_eq!(
            w.act(&facts(), HELPER, RoomAction::Unlock).await,
            Ok(RoomDone::Unlocked)
        );
        assert_eq!(
            w.discord.calls()[locked..],
            [
                Call::DeleteOverwrite(ROOM.get(), member(OWNER)),
                Call::DeleteOverwrite(ROOM.get(), member(HELPER)),
                Call::DeleteOverwrite(ROOM.get(), member(BOT)),
                Call::Put(ROOM.get(), ow(role(18), V | C, NONE)),
                Call::DeleteOverwrite(ROOM.get(), role(G.get())),
            ]
        );
        assert!(!w.room().await.state.locked);
    }

    #[tokio::test]
    async fn allow_while_locked_lets_the_member_and_role_connect() {
        let mut room = live(vec![]);
        room.state.locked = true;
        let w = World::new(room);
        let friends = RoleId::new(30);
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::AllowUser { target: OUTSIDE })
                .await,
            Ok(RoomDone::AllowedUser(OUTSIDE))
        );
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::AllowRole { role: friends })
                .await,
            Ok(RoomDone::AllowedRole(friends))
        );
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(ROOM.get(), ow(member(OUTSIDE), C, NONE)),
                Call::Put(ROOM.get(), ow(role(30), C, NONE)),
            ]
        );
        let state = w.room().await.state;
        assert!(state.permitted_users.contains(&OUTSIDE));
        assert!(state.permitted_roles.contains(&friends));
    }

    #[tokio::test]
    async fn helper_remove_drops_the_helper() {
        let w = World::new(live(vec![]));
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::HelperRemove { target: HELPER })
                .await,
            Ok(RoomDone::HelperRemoved(HELPER))
        );
        assert!(w.room().await.state.helpers.is_empty());
        assert_eq!(
            w.act(&facts(), HELPER, RoomAction::Lock).await,
            Err(RoomRefusal::NotAllowed),
            "no longer a helper"
        );
    }

    #[tokio::test]
    async fn rename_third_inside_ten_minutes_is_refused_with_wait() {
        let w = World::new(live(vec![]));
        let t0 = Instant::now();
        let rename = |s: &str| RoomAction::Rename(s.to_string());
        assert_eq!(
            w.act_at(&facts(), OWNER, rename(" first "), t0).await,
            Ok(RoomDone::Renamed("first".into()))
        );
        assert_eq!(
            w.act_at(
                &facts(),
                OWNER,
                rename("second"),
                t0 + Duration::from_secs(1)
            )
            .await,
            Ok(RoomDone::Renamed("second".into()))
        );
        let third = w
            .act_at(
                &facts(),
                OWNER,
                rename("third"),
                t0 + Duration::from_secs(2),
            )
            .await;
        match third {
            Err(RoomRefusal::RenameLimited { wait_secs }) => {
                assert!(wait_secs > 0 && wait_secs <= 600, "{wait_secs}");
            },
            other => panic!("expected RenameLimited, got {other:?}"),
        }
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Rename(ROOM.get(), "first".into()),
                Call::Rename(ROOM.get(), "second".into()),
            ]
        );
        assert_eq!(w.room().await.name, "second");
        assert_eq!(w.transport.ops().len(), 2, "one redraw per rename");
    }

    #[tokio::test]
    async fn rename_refuses_bad_names_before_discord() {
        let w = World::new(live(vec![]));
        for bad in ["", "   ", &"x".repeat(101), "bell\u{7}"] {
            assert_eq!(
                w.act(&facts(), OWNER, RoomAction::Rename(bad.to_string()))
                    .await,
                Err(RoomRefusal::BadName),
                "{bad:?}"
            );
        }
        assert_eq!(w.discord.calls(), vec![]);
        let edge = "é".repeat(100);
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::Rename(edge.clone()))
                .await,
            Ok(RoomDone::Renamed(edge))
        );
    }

    #[tokio::test]
    async fn transfer_moves_ownership_and_owner_index() {
        let mut room = live(vec![]);
        room.state.locked = true;
        let w = World::new(room);
        let got = w
            .act(&facts(), OWNER, RoomAction::Transfer { target: TARGET })
            .await;
        assert_eq!(got, Ok(RoomDone::Transferred(TARGET)));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::DeleteOverwrite(ROOM.get(), member(OWNER)),
                Call::Put(ROOM.get(), ow(member(TARGET), C, NONE)),
            ]
        );
        let state = w.room().await.state;
        assert_eq!(state.owner, TARGET);
        assert!(state.helpers.contains(&HELPER), "helpers stay");
        assert!(!state.helpers.contains(&OWNER), "the old owner is plain");
        assert_eq!(w.data.rooms.owned_by(G, TARGET), Some(ROOM));
        assert_eq!(w.data.rooms.owned_by(G, OWNER), None);
    }

    #[tokio::test]
    async fn transfer_to_a_helper_drops_them_from_helpers() {
        let mut room = live(vec![]);
        room.state.helpers.insert(TARGET);
        let w = World::new(room);
        w.act(&facts(), OWNER, RoomAction::Transfer { target: TARGET })
            .await
            .unwrap();
        let state = w.room().await.state;
        assert_eq!(state.owner, TARGET);
        assert_eq!(state.helpers, [HELPER].into());
    }

    #[tokio::test]
    async fn claim_only_when_owner_absent() {
        let w = World::new(live(vec![]));
        let mut f = facts();
        assert_eq!(
            w.act(&f, TARGET, RoomAction::Claim).await,
            Err(RoomRefusal::OwnerPresent)
        );
        assert_eq!(w.room().await.state.owner, OWNER);
        f.voice.remove(&OWNER);
        assert_eq!(
            w.act(&f, TARGET, RoomAction::Claim).await,
            Ok(RoomDone::Claimed)
        );
        assert_eq!(w.room().await.state.owner, TARGET);
        assert_eq!(w.data.rooms.owned_by(G, TARGET), Some(ROOM));
        assert_eq!(w.data.rooms.owned_by(G, OWNER), None);
        assert_eq!(
            w.act(&f, OUTSIDE, RoomAction::Claim).await,
            Err(RoomRefusal::NotAllowed),
            "only someone in the room may claim it"
        );
    }

    /// An 18+ category: `@everyone` cannot see it, the adult role can.
    fn gated() -> (Vec<StoredOverwrite>, RoleId) {
        let adult = RoleId::new(18);
        (
            vec![
                ow(role(G.get()), NONE, V),
                ow(OverwriteTarget::Role { id: adult }, V | C, NONE),
            ],
            adult,
        )
    }

    #[tokio::test]
    async fn helper_add_refuses_member_the_baseline_keeps_out() {
        let (baseline, adult) = gated();
        let w = World::new(live(baseline));
        let mut f = facts();
        let got = w
            .act(&f, OWNER, RoomAction::HelperAdd { target: TARGET })
            .await;
        assert_eq!(got, Err(RoomRefusal::CannotJoinHere));
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(w.transport.ops(), vec![]);
        assert!(!w.room().await.state.helpers.contains(&TARGET));

        f.roles.insert(TARGET, vec![adult]);
        assert_eq!(
            w.act(&f, OWNER, RoomAction::HelperAdd { target: TARGET })
                .await,
            Ok(RoomDone::HelperAdded(TARGET))
        );
        assert!(w.room().await.state.helpers.contains(&TARGET));
    }

    #[tokio::test]
    async fn guards_cover_allow_user_allow_role_and_transfer() {
        let (baseline, adult) = gated();
        let w = World::new(live(baseline));
        let f = facts();
        for a in [
            RoomAction::AllowUser { target: OUTSIDE },
            RoomAction::Transfer { target: TARGET },
            RoomAction::AllowRole {
                role: RoleId::new(30),
            },
        ] {
            assert_eq!(
                w.act(&f, OWNER, a.clone()).await,
                Err(RoomRefusal::CannotJoinHere),
                "{a:?}"
            );
        }
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(
            w.act(&f, OWNER, RoomAction::AllowRole { role: adult })
                .await,
            Ok(RoomDone::AllowedRole(adult))
        );
    }

    #[tokio::test]
    async fn the_server_owner_passes_the_guard() {
        let (baseline, _) = gated();
        let w = World::new(live(baseline));
        let mut f = facts();
        f.owner = OUTSIDE;
        assert_eq!(
            w.act(&f, OWNER, RoomAction::AllowUser { target: OUTSIDE })
                .await,
            Ok(RoomDone::AllowedUser(OUTSIDE))
        );
    }

    #[tokio::test]
    async fn protected_targets_are_derived_from_facts() {
        let w = World::new(live(vec![]));
        let mut f = facts();
        f.voice.insert(MODERATOR, (ROOM, false));
        for (actor, target) in [
            (OWNER, OWNER),     // itself
            (OWNER, BOT),       // the bot
            (OWNER, MODERATOR), // a moderator
            (HELPER, OWNER),    // the owner, by a non-moderator
            (HELPER, HELPER),   // itself, again
        ] {
            assert_eq!(
                w.act(&f, actor, kick(target)).await,
                Err(RoomRefusal::ProtectedTarget),
                "{actor} kicking {target}"
            );
        }
        let mut two_helpers = live(vec![]);
        two_helpers.state.helpers.insert(TARGET);
        let w2 = World::new(two_helpers);
        assert_eq!(
            w2.act(&f, HELPER, kick(TARGET)).await,
            Err(RoomRefusal::ProtectedTarget),
            "a helper cannot kick a helper"
        );
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(w2.discord.calls(), vec![]);
    }

    #[test]
    fn target_standing_priority() {
        let mut f = facts();
        let state = live(vec![]).state;
        let ts = |f: &GuildFacts, actor, target| target_standing(f, &state, actor, target);
        assert_eq!(ts(&f, HELPER, TARGET), TargetStanding::Plain);
        assert_eq!(ts(&f, OWNER, HELPER), TargetStanding::Helper);
        assert_eq!(ts(&f, HELPER, OWNER), TargetStanding::Owner);
        assert_eq!(ts(&f, OWNER, MODERATOR), TargetStanding::Moderator);
        assert_eq!(ts(&f, OWNER, BOT), TargetStanding::TheBot);
        assert_eq!(ts(&f, OWNER, OWNER), TargetStanding::Itself);
        // A room owner with server powers is a moderator first.
        f.roles.insert(OWNER, vec![MOD_ROLE]);
        assert_eq!(ts(&f, MODERATOR, OWNER), TargetStanding::Moderator);
        // The bot is the bot even with server powers.
        f.roles.insert(BOT, vec![MOD_ROLE]);
        assert_eq!(ts(&f, OWNER, BOT), TargetStanding::TheBot);
        // Oneself comes before everything.
        assert_eq!(ts(&f, BOT, BOT), TargetStanding::Itself);
    }

    #[tokio::test]
    async fn a_muted_helper_cannot_unmute_themselves() {
        let mut room = live(vec![]);
        room.state.muted.insert(HELPER);
        let w = World::new(room);
        assert_eq!(
            w.act(&facts(), HELPER, RoomAction::Unmute { target: HELPER })
                .await,
            Err(RoomRefusal::ProtectedTarget)
        );
        assert_eq!(w.discord.calls(), vec![]);
        assert!(w.room().await.state.muted.contains(&HELPER));
        // The owner can.
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::Unmute { target: HELPER })
                .await,
            Ok(RoomDone::Unmuted(HELPER))
        );
    }

    #[tokio::test]
    async fn mute_denies_speak_and_unmute_takes_it_back() {
        let w = World::new(live(vec![]));
        let mute = RoomAction::Mute {
            target: TARGET,
            reason: None,
        };
        assert_eq!(
            w.act(&facts(), HELPER, mute).await,
            Ok(RoomDone::Muted(TARGET))
        );
        assert!(w.room().await.state.muted.contains(&TARGET));
        assert_eq!(
            w.act(&facts(), HELPER, RoomAction::Unmute { target: TARGET })
                .await,
            Ok(RoomDone::Unmuted(TARGET))
        );
        assert!(w.room().await.state.muted.is_empty());
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(ROOM.get(), ow(member(TARGET), NONE, Permissions::SPEAK)),
                Call::DeleteOverwrite(ROOM.get(), member(TARGET)),
            ]
        );
    }

    #[tokio::test]
    async fn chat_lock_and_block_role_write_their_overwrites() {
        let w = World::new(live(vec![]));
        let s = Permissions::SEND_MESSAGES;
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::ChatLock).await,
            Ok(RoomDone::ChatLocked)
        );
        let blocked = RoleId::new(30);
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::BlockRole { role: blocked })
                .await,
            Ok(RoomDone::RoleBlocked(blocked))
        );
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::UnblockRole { role: blocked })
                .await,
            Ok(RoomDone::RoleUnblocked(blocked))
        );
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::ChatUnlock).await,
            Ok(RoomDone::ChatUnlocked)
        );
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(ROOM.get(), ow(member(OWNER), s, NONE)),
                Call::Put(ROOM.get(), ow(member(HELPER), s, NONE)),
                // CrackTunes can still post the panel.
                Call::Put(ROOM.get(), ow(member(BOT), s, NONE)),
                Call::Put(ROOM.get(), ow(role(G.get()), NONE, s)),
                Call::Put(ROOM.get(), ow(role(30), NONE, V | C)),
                Call::DeleteOverwrite(ROOM.get(), role(30)),
                Call::DeleteOverwrite(ROOM.get(), member(OWNER)),
                Call::DeleteOverwrite(ROOM.get(), member(HELPER)),
                Call::DeleteOverwrite(ROOM.get(), member(BOT)),
                Call::DeleteOverwrite(ROOM.get(), role(G.get())),
            ]
        );
        assert_eq!(w.room().await.state, live(vec![]).state);
    }

    /// A moderator kicks the room's owner: the room is no longer indexed as
    /// theirs, so the hub gives them a new one and the room can be claimed.
    #[tokio::test]
    async fn kicking_the_owner_frees_the_room() {
        let w = World::new(live(vec![]));
        assert_eq!(
            w.act(&facts(), MODERATOR, kick(OWNER)).await,
            Ok(RoomDone::Kicked(OWNER))
        );
        assert_eq!(w.data.rooms.owned_by(G, OWNER), None);
        let mut f = facts();
        f.voice.remove(&OWNER);
        assert_eq!(
            w.act(&f, TARGET, RoomAction::Claim).await,
            Ok(RoomDone::Claimed)
        );
        assert_eq!(w.data.rooms.owned_by(G, TARGET), Some(ROOM));

        // Kicking anyone else leaves the owner's entry alone.
        let w = World::new(live(vec![]));
        w.act(&facts(), MODERATOR, kick(TARGET)).await.unwrap();
        assert_eq!(w.data.rooms.owned_by(G, OWNER), Some(ROOM));
    }

    #[tokio::test]
    async fn a_moderator_outside_the_room_acts_as_owner() {
        let w = World::new(live(vec![]));
        assert_eq!(
            w.act(&facts(), MODERATOR, kick(OWNER)).await,
            Ok(RoomDone::Kicked(OWNER))
        );
        assert_eq!(
            w.act(&facts(), OUTSIDE, RoomAction::Lock).await,
            Err(RoomRefusal::NotAllowed)
        );
    }

    #[tokio::test]
    async fn limit_sets_user_limit() {
        let w = World::new(live(vec![]));
        assert_eq!(
            w.act(&facts(), HELPER, RoomAction::Limit(5)).await,
            Ok(RoomDone::LimitSet(5))
        );
        assert_eq!(w.discord.calls(), vec![Call::Limit(ROOM.get(), 5)]);
        assert_eq!(w.room().await.limit, 5);
        assert_eq!(w.transport.ops(), vec![Op::Edit(ROOM.get(), PANEL.get())]);
        assert_eq!(
            w.act(&facts(), HELPER, RoomAction::Limit(100)).await,
            Err(RoomRefusal::BadLimit)
        );
        assert_eq!(w.discord.calls().len(), 1);
    }

    #[tokio::test]
    async fn panel_reposts_and_records_the_new_id() {
        let w = World::new(live(vec![]));
        assert_eq!(
            w.act(&facts(), HELPER, RoomAction::Panel).await,
            Ok(RoomDone::PanelPosted)
        );
        assert_eq!(
            w.transport.ops(),
            vec![Op::Send(ROOM.get()), Op::Delete(ROOM.get(), PANEL.get())]
        );
        assert_eq!(w.discord.calls(), vec![Call::Pin(ROOM.get(), 1000)]);
        assert_eq!(w.room().await.state.panel, Some(MessageId::new(1000)));
    }

    #[tokio::test]
    async fn a_failed_pin_or_delete_still_posts_the_panel() {
        let w = World::new(live(vec![]));
        w.discord
            .fail_next(CallKind::Pin, DiscordError::MissingPermissions);
        *w.transport.delete_error.lock().unwrap() = Some(TransportError::UnknownMessage);
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::Panel).await,
            Ok(RoomDone::PanelPosted)
        );
        assert_eq!(w.room().await.state.panel, Some(MessageId::new(1000)));
    }

    #[tokio::test]
    async fn a_panel_that_cannot_be_sent_is_discord_failed() {
        let w = World::new(live(vec![]));
        *w.transport.send_error.lock().unwrap() = Some(TransportError::Other("nope".into()));
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::Panel).await,
            Err(RoomRefusal::DiscordFailed)
        );
        assert_eq!(w.room().await.state.panel, Some(PANEL));
        assert_eq!(w.discord.calls(), vec![], "nothing to pin");
    }

    #[tokio::test]
    async fn a_deleted_panel_is_posted_again_on_redraw() {
        let w = World::new(live(vec![]));
        *w.transport.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::Lock).await,
            Ok(RoomDone::Locked)
        );
        assert_eq!(
            w.transport.ops(),
            vec![Op::Edit(ROOM.get(), PANEL.get()), Op::Send(ROOM.get())],
            "the gone panel is not deleted again"
        );
        assert_eq!(w.room().await.state.panel, Some(MessageId::new(1000)));
    }

    #[tokio::test]
    async fn redraw_without_a_panel_posts_one() {
        let mut room = live(vec![]);
        room.state.panel = None;
        let w = World::new(room.clone());
        assert!(redraw_panel(&w.cx(), &mut room).await);
        assert_eq!(room.state.panel, Some(MessageId::new(1000)));
        assert_eq!(w.transport.ops(), vec![Op::Send(ROOM.get())]);
        assert!(!redraw_panel(&w.cx(), &mut room).await, "edited in place");
    }

    #[tokio::test]
    async fn facts_for_another_server_refuse() {
        let w = World::new(live(vec![]));
        let mut f = facts();
        f.guild = GuildId::new(101);
        assert_eq!(
            w.act(&f, OWNER, RoomAction::Lock).await,
            Err(RoomRefusal::NotInRoom)
        );
        assert_eq!(w.discord.calls(), vec![]);
    }

    #[tokio::test]
    async fn a_room_removed_while_an_action_waits_for_its_lock_refuses() {
        let w = World::new(live(vec![]));
        let handle = w.data.rooms.get(ROOM).expect("registered");
        let held = handle.lock().await;
        let f = facts();
        let action = w.act(&f, OWNER, RoomAction::Transfer { target: TARGET });
        let remover = async {
            // The action reaches the lock first, then the room goes, under it.
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            w.data.rooms.remove(ROOM);
            drop(held);
        };
        let (got, ()) = tokio::join!(action, remover);
        assert_eq!(got, Err(RoomRefusal::NotInRoom));
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(w.transport.ops(), vec![]);
        assert_eq!(w.data.rooms.owned_by(G, TARGET), None, "not indexed again");
        assert_eq!(w.data.rooms.owned_by(G, OWNER), None);
    }

    /// Another room of `owner`'s in the same server.
    fn other_room_of(owner: UserId) -> LiveRoom {
        LiveRoom {
            channel: ChannelId::new(600),
            state: RoomState::new(owner),
            ..live(vec![])
        }
    }

    #[tokio::test]
    async fn transfer_to_someone_with_a_room_of_their_own_refuses() {
        let w = World::new(live(vec![]));
        w.data.rooms.insert(other_room_of(TARGET));
        assert_eq!(
            w.act(&facts(), OWNER, RoomAction::Transfer { target: TARGET })
                .await,
            Err(RoomRefusal::OwnsAnotherRoom)
        );
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(w.room().await.state.owner, OWNER);
        assert_eq!(w.data.rooms.owned_by(G, TARGET), Some(ChannelId::new(600)));
        assert_eq!(w.data.rooms.owned_by(G, OWNER), Some(ROOM));
    }

    #[tokio::test]
    async fn claim_by_someone_with_a_room_of_their_own_refuses() {
        let w = World::new(live(vec![]));
        w.data.rooms.insert(other_room_of(TARGET));
        let mut f = facts();
        f.voice.remove(&OWNER);
        assert_eq!(
            w.act(&f, TARGET, RoomAction::Claim).await,
            Err(RoomRefusal::OwnsAnotherRoom)
        );
        assert_eq!(w.room().await.state.owner, OWNER);
        assert_eq!(w.data.rooms.owned_by(G, TARGET), Some(ChannelId::new(600)));
        assert_eq!(w.transport.ops(), vec![]);
    }

    #[tokio::test]
    async fn block_role_refuses_everyone_and_the_owners_roles() {
        let w = World::new(live(vec![]));
        let mut f = facts();
        let owners_role = RoleId::new(40);
        f.roles.insert(OWNER, vec![owners_role]);
        for role in [RoleId::new(G.get()), owners_role] {
            assert_eq!(
                w.act(&f, OWNER, RoomAction::BlockRole { role }).await,
                Err(RoomRefusal::CannotBlockRole),
                "{role}"
            );
        }
        assert_eq!(w.discord.calls(), vec![]);
        assert!(w.room().await.state.blocked_roles.is_empty());
        let other = RoleId::new(41);
        assert_eq!(
            w.act(&f, OWNER, RoomAction::BlockRole { role: other })
                .await,
            Ok(RoomDone::RoleBlocked(other))
        );
    }

    #[tokio::test]
    async fn an_unknown_room_is_not_in_room() {
        let w = World::new(live(vec![]));
        let got = act(
            &w.cx(),
            &facts(),
            ChannelId::new(501),
            OWNER,
            RoomAction::Lock,
            Instant::now(),
        )
        .await;
        assert_eq!(got, Err(RoomRefusal::NotInRoom));
    }

    mod db {
        use super::*;
        use crate::db::rooms::{insert_room, rooms_in, StoredRoom};
        use sqlx::PgPool;

        pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

        async fn world(pool: PgPool) -> sqlx::Result<World> {
            let room = live(vec![]);
            insert_room(
                &pool,
                &StoredRoom {
                    channel: room.channel,
                    guild: room.guild,
                    owner: room.state.owner,
                    state: room.state.clone(),
                    baseline: room.baseline.clone(),
                },
            )
            .await?;
            let data = Data(Arc::new(DataInner {
                database_pool: Some(pool),
                ..Default::default()
            }));
            data.rooms.insert(room);
            Ok(World {
                data,
                discord: FakeRoomsDiscord::default(),
                transport: FakeTransport::default(),
            })
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn an_action_is_persisted(pool: PgPool) -> sqlx::Result<()> {
            let w = world(pool.clone()).await?;
            w.act(&facts(), OWNER, RoomAction::Transfer { target: TARGET })
                .await
                .unwrap();
            let stored = rooms_in(&pool, G).await?;
            assert_eq!(stored[0].owner, TARGET);
            assert_eq!(stored[0].state, w.room().await.state);
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_refused_action_is_not_persisted(pool: PgPool) -> sqlx::Result<()> {
            let w = world(pool.clone()).await?;
            w.discord
                .fail_next(CallKind::Put, DiscordError::MissingPermissions);
            let got = w.act(&facts(), OWNER, RoomAction::Lock).await;
            assert_eq!(got, Err(RoomRefusal::MissingPermissions));
            let stored = rooms_in(&pool, G).await?;
            assert!(!stored[0].state.locked);
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_reposted_panel_id_is_persisted(pool: PgPool) -> sqlx::Result<()> {
            let w = world(pool.clone()).await?;
            *w.transport.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
            w.act(&facts(), OWNER, RoomAction::Lock).await.unwrap();
            let stored = rooms_in(&pool, G).await?;
            assert_eq!(stored[0].state.panel, Some(MessageId::new(1000)));
            assert!(stored[0].state.locked);
            Ok(())
        }
    }
}
