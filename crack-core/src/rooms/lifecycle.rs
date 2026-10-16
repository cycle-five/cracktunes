//! A room's life: made when a member joins the hub, deleted once no human is
//! left in it (after the server's grace), forgotten when deleted by hand, and
//! put back in order at every `GuildCreate`.
//!
//! The rule everything here serves: a room never outlives its last human.

use super::discord::{DiscordError, LiveRoomsDiscord, NewRoom, RoomsDiscord};
use super::facts::{gather, GuildFacts};
use super::ops::{post_panel, RoomCx};
use super::overwrites::{diff, room_overwrites, OverwriteChange};
use super::registry::{LiveRoom, RenameWindow};
use super::settings::{render_room_name, RoomSettings};
use super::state::{RoomState, StoredOverwrite};
use crate::db::rooms::StoredRoom;
use crate::messaging::messages::ROOM_NAME_FALLBACK;
use crate::messaging::transport::DiscordTransport;
use crate::Data;
use dashmap::DashSet;
use serenity::all::{Cache, ChannelId, Context, Guild, GuildId, UserId, VoiceState};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// At most this many live rooms per server.
pub const MAX_ROOMS: usize = 50;

// Why a hub join made no room. Logged, never shown to anyone.
const MOVE_TO_OWN_FAILED: &str = "moving them into their own room failed";
const KEPT_OUT: &str = "the hub's category keeps them out";
const AT_CAP: &str = "the server has the most rooms allowed";
const MISSING_PERMISSIONS: &str = "CrackTunes is missing a permission";
const CREATE_FAILED: &str = "creating the channel failed";
const COPY_FAILED: &str = "category overwrites could not be copied";
const STORE_FAILED: &str = "the room could not be stored";
const OVERWRITES_FAILED: &str = "the room's overwrites could not be set";
const MOVE_FAILED: &str = "they could not be moved into the room";
const ROOMS_OFF: &str = "rooms are off or have no hub";
const HUB_UNKNOWN: &str = "the hub or its category is not in the cache";
const GUILD_UNKNOWN: &str = "the server is not in the cache";

/// One member's voice channel change; a mute or deafen in place is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceChange {
    Joined(ChannelId),
    Left(ChannelId),
    Moved { from: ChannelId, to: ChannelId },
}

impl VoiceChange {
    fn left(self) -> Option<ChannelId> {
        match self {
            Self::Left(from) | Self::Moved { from, .. } => Some(from),
            Self::Joined(_) => None,
        }
    }

    fn joined(self) -> Option<ChannelId> {
        match self {
            Self::Joined(to) | Self::Moved { to, .. } => Some(to),
            Self::Left(_) => None,
        }
    }
}

pub fn classify(old: Option<ChannelId>, new: Option<ChannelId>) -> Option<VoiceChange> {
    match (old, new) {
        (None, None) => None,
        (None, Some(to)) => Some(VoiceChange::Joined(to)),
        (Some(from), None) => Some(VoiceChange::Left(from)),
        (Some(from), Some(to)) if from == to => None,
        (Some(from), Some(to)) => Some(VoiceChange::Moved { from, to }),
    }
}

/// The hub, as the glue reads it from the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hub {
    pub channel: ChannelId,
    /// The hub's category, where rooms are made.
    pub parent: Option<ChannelId>,
    /// That category's overwrites.
    pub category_overwrites: Vec<StoredOverwrite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubOutcome {
    MovedToOwn(ChannelId),
    Created(ChannelId),
    /// A room was made for them too recently; try again after this long.
    CoolingDown(Duration),
    /// No room, and why, for the log.
    Refused(&'static str),
}

/// How many humans are in a voice channel right now. Live: the cache.
pub trait Occupancy: Send + Sync {
    fn humans_in(&self, guild: GuildId, room: ChannelId) -> usize;
}

/// What a deletion timer holds on to, since it outlives the event that
/// armed it.
#[derive(Clone)]
pub struct Sweep {
    pub discord: Arc<dyn RoomsDiscord>,
    pub occupancy: Arc<dyn Occupancy>,
}

/// A channel the server has, as reconcile needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingChannel {
    pub name: String,
    pub limit: u8,
}

/// Spec 1.3's hub join, steps 1 to 9, with the sync check after step 5.
pub async fn hub_join(
    cx: &RoomCx<'_>,
    facts: &GuildFacts,
    settings: &RoomSettings,
    hub: &Hub,
    user: UserId,
    now: Instant,
) -> HubOutcome {
    let data = cx.data;
    let guild = facts.guild;

    // 1. Their own room, if they have one.
    if let Some(room) = data.rooms.owned_by(guild, user) {
        // So the deletion cannot fire while they are on the way.
        data.rooms.disarm_timer(room);
        match cx.discord.move_member(guild, user, Some(room)).await {
            Ok(()) => return HubOutcome::MovedToOwn(room),
            // Deleted while nobody was watching: forget it, make a new one.
            Err(DiscordError::UnknownChannel) => forget(data, room).await,
            Err(err) => {
                tracing::warn!("rooms: moving {user} into their room {room} failed: {err:?}");
                return HubOutcome::Refused(MOVE_TO_OWN_FAILED);
            },
        }
    }

    // 2. One new room per member per ten seconds.
    let key = (guild, user);
    if !data.rooms.cooldown.allow(key, now) {
        let wait = data.rooms.cooldown.remaining(&key, now);
        return HubOutcome::CoolingDown(wait.unwrap_or_default());
    }
    // A room still being made for them (Discord can be slower than the
    // cooldown): wait, and the re-check finds the room or makes one.
    let Some(_creating) = Creating::claim(&data.rooms.hub_creating, key) else {
        let wait = data.rooms.cooldown.remaining(&key, now);
        return HubOutcome::CoolingDown(wait.unwrap_or_default());
    };

    // 3. The joiner guard: no room for someone the category keeps out.
    if !facts.member_can_join(user, &hub.category_overwrites) {
        return HubOutcome::Refused(KEPT_OUT);
    }

    // 4. The cap.
    if data.rooms.count_in(guild) >= MAX_ROOMS {
        tracing::warn!("rooms: {guild} has {MAX_ROOMS} rooms, the most allowed; none for {user}");
        return HubOutcome::Refused(AT_CAP);
    }

    // 5. The channel, in the hub's category, with no overwrites of its own.
    let name = match facts.display.get(&user) {
        Some(display) => render_room_name(&settings.name_template, display),
        None => ROOM_NAME_FALLBACK.to_string(),
    };
    let new = NewRoom {
        guild,
        name: name.clone(),
        parent: hub.parent,
        user_limit: settings.default_limit,
    };
    let created = match cx.discord.create_voice_channel(&new).await {
        Ok(created) => created,
        Err(err) => return refused(guild, "creating a room", err, CREATE_FAILED),
    };
    let room = created.channel;
    // The sync check: never a room more open than its category.
    let Some(baseline) = sync_check(cx.discord, room, created.overwrites, hub).await else {
        if let Err(err) = cx.discord.delete_channel(room).await {
            tracing::error!(
                "rooms: room {room} in {guild} lacks its category's overwrites and could not be \
                 deleted; delete it by hand: {err:?}"
            );
        }
        return HubOutcome::Refused(COPY_FAILED);
    };

    // 6. The row.
    let state = RoomState::new(user);
    if let Some(pool) = data.database_pool.as_ref() {
        let row = StoredRoom {
            channel: room,
            guild,
            owner: user,
            state: state.clone(),
            baseline: baseline.clone(),
        };
        if let Err(err) = crate::db::rooms::insert_room(pool, &row).await {
            tracing::warn!("rooms: storing room {room} in {guild} failed: {err}");
            if let Err(err) = cx.discord.delete_channel(room).await {
                tracing::warn!("rooms: deleting unstored room {room} failed: {err:?}");
            }
            return HubOutcome::Refused(STORE_FAILED);
        }
    }

    // 7. The managed layer.
    for change in diff(&baseline, &room_overwrites(guild, &baseline, &state)) {
        let written = match &change {
            OverwriteChange::Put(o) => cx.discord.put_overwrite(room, o).await,
            OverwriteChange::Delete(t) => cx.discord.delete_overwrite(room, *t).await,
        };
        if let Err(err) = written {
            tracing::warn!("rooms: setting up room {room}'s overwrites failed: {err:?}");
            delete_room(data, cx.discord, room).await;
            return HubOutcome::Refused(OVERWRITES_FAILED);
        }
    }

    // 8. Into the room. If they are gone, so is the room.
    if let Err(err) = cx.discord.move_member(guild, user, Some(room)).await {
        delete_room(data, cx.discord, room).await;
        return refused(
            guild,
            "moving a member into their new room",
            err,
            MOVE_FAILED,
        );
    }

    // 9. The panel, then the registry.
    let mut live = LiveRoom {
        guild,
        channel: room,
        name,
        state,
        baseline,
        limit: settings.default_limit,
        renames: RenameWindow::default(),
    };
    if post_panel(cx, &mut live).await.is_some() {
        if let Some(pool) = data.database_pool.as_ref() {
            let saved = crate::db::rooms::save_state(pool, room, user, &live.state).await;
            if let Err(err) = saved {
                tracing::warn!("rooms: saving room {room}'s panel failed: {err}");
            }
        }
    }
    data.rooms.insert(live);
    HubOutcome::Created(room)
}

/// A member's claim on a room being made, let go however the join ends.
struct Creating<'a> {
    set: &'a DashSet<(GuildId, UserId)>,
    key: (GuildId, UserId),
}

impl<'a> Creating<'a> {
    fn claim(set: &'a DashSet<(GuildId, UserId)>, key: (GuildId, UserId)) -> Option<Self> {
        // `then`, not `then_some`: a guard built for a lost claim would let go
        // of the winner's on drop.
        set.insert(key).then(|| Self { set, key })
    }
}

impl Drop for Creating<'_> {
    fn drop(&mut self) {
        self.set.remove(&self.key);
    }
}

/// Spec 5 guesses that Discord gives a new channel its category's overwrites.
/// When the channel came back with others, the category's are copied onto
/// it. The room's baseline, or `None` when a copy was refused.
async fn sync_check(
    discord: &dyn RoomsDiscord,
    room: ChannelId,
    created: Vec<StoredOverwrite>,
    hub: &Hub,
) -> Option<Vec<StoredOverwrite>> {
    let sorted = |v: &[StoredOverwrite]| {
        let mut v = v.to_vec();
        v.sort_by_key(|o| o.target);
        v
    };
    if sorted(&created) == sorted(&hub.category_overwrites) {
        return Some(created);
    }
    static WARNED: AtomicBool = AtomicBool::new(false);
    if WARNED.swap(true, Ordering::Relaxed) {
        tracing::debug!("rooms: room {room} was not synced to its category; copying");
    } else {
        tracing::warn!(
            "rooms: Discord did not sync new room {room} to its category ({} overwrites, the \
             category has {}); copying the category's. Logged once.",
            created.len(),
            hub.category_overwrites.len()
        );
    }
    for o in &hub.category_overwrites {
        if let Err(err) = discord.put_overwrite(room, o).await {
            tracing::warn!("rooms: copying the category's overwrites onto {room} failed: {err:?}");
            return None;
        }
    }
    Some(hub.category_overwrites.clone())
}

/// Log a failed create or move, and say why for the outcome.
fn refused(guild: GuildId, what: &str, err: DiscordError, otherwise: &'static str) -> HubOutcome {
    tracing::warn!("rooms: {what} in {guild} failed: {err:?}");
    match err {
        DiscordError::MissingPermissions => HubOutcome::Refused(MISSING_PERMISSIONS),
        _ => HubOutcome::Refused(otherwise),
    }
}

/// If no human is in `room`, delete it after `grace`, unless one joins first.
pub fn schedule_if_empty(
    data: &Data,
    discord: Arc<dyn RoomsDiscord>,
    occupancy: Arc<dyn Occupancy>,
    guild: GuildId,
    room: ChannelId,
    grace: Duration,
) {
    if occupancy.humans_in(guild, room) > 0 {
        return;
    }
    // Already counting down: the earlier deadline stands.
    if data.rooms.timer_pending(room) {
        return;
    }
    // From now, not from whenever the task first runs.
    let deadline = tokio::time::Instant::now() + grace;
    let (armed, is_armed) = tokio::sync::oneshot::channel::<()>();
    let data_for_timer = data.clone();
    let timer = tokio::spawn(async move {
        let data = data_for_timer;
        // Not a step before `arm_timer` below holds this task's handle, so
        // the release further down always finds it.
        if is_armed.await.is_err() {
            return;
        }
        tokio::time::sleep_until(deadline).await;
        // Out of the map unaborted: `delete_room` removes the room, and that
        // would abort this very task at its next await.
        data.rooms.release_timer(room, tokio::task::id());
        if occupancy.humans_in(guild, room) > 0 {
            return;
        }
        delete_room(&data, &*discord, room).await;
    });
    data.rooms.arm_timer(room, timer.abort_handle());
    let _ = armed.send(());
}

/// Delete the room's channel, then forget it and drop its row. A channel
/// already gone counts as deleted. Any other failure keeps all of it, for
/// reconcile to finish at the next `GuildCreate`. Works on a room the
/// registry does not hold (yet) too.
pub async fn delete_room(data: &Data, discord: &dyn RoomsDiscord, room: ChannelId) {
    let handle = data.rooms.get(room);
    // `RoomRegistry::remove` is called under the room's lock.
    let _held = match &handle {
        Some(h) => Some(h.lock().await),
        None => None,
    };
    match discord.delete_channel(room).await {
        Ok(()) | Err(DiscordError::UnknownChannel) => {
            data.rooms.remove(room);
            drop_row(data, room).await;
        },
        Err(err) => tracing::warn!(
            "rooms: deleting room {room} failed; it stays until the next reconcile: {err:?}"
        ),
    }
}

/// A channel was deleted, by anyone. A room's registry entry, timer and row go.
pub async fn on_channel_delete(data: &Data, channel: ChannelId) {
    if data.rooms.get(channel).is_some() {
        forget(data, channel).await;
    }
}

/// Forget a room whose channel is gone: registry, owner, timer, row.
async fn forget(data: &Data, room: ChannelId) {
    if let Some(handle) = data.rooms.get(room) {
        let _held = handle.lock().await;
        data.rooms.remove(room);
    }
    drop_row(data, room).await;
}

async fn drop_row(data: &Data, room: ChannelId) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    if let Err(err) = crate::db::rooms::delete_room(pool, room).await {
        tracing::warn!("rooms: dropping room {room}'s row failed: {err}");
    }
}

fn grace_of(settings: &RoomSettings) -> Duration {
    Duration::from_secs(settings.empty_grace_secs.into())
}

/// Sweep `room` if it is empty, with its server's grace.
fn sweep_if_empty(data: &Data, sweep: &Sweep, guild: GuildId, room: ChannelId) {
    let grace = grace_of(&data.rooms.settings(guild));
    schedule_if_empty(
        data,
        sweep.discord.clone(),
        sweep.occupancy.clone(),
        guild,
        room,
        grace,
    );
}

/// The rooms side of a human's voice change: a room they left is swept if
/// now empty, a room they joined is no longer. The channel they joined, for
/// the hub check.
pub fn room_voice_change(
    data: &Data,
    sweep: &Sweep,
    guild: GuildId,
    change: VoiceChange,
) -> Option<ChannelId> {
    // Whether or not rooms are on: turning them off leaves live rooms to
    // empty out.
    if let Some(from) = change.left() {
        if data.rooms.get(from).is_some() {
            sweep_if_empty(data, sweep, guild, from);
        }
    }
    let to = change.joined()?;
    data.rooms.disarm_timer(to);
    Some(to)
}

/// What a hub join leaves to sweep. A room made for someone who left before
/// it was registered saw no leave; a failed move into their own room left it
/// as empty as it was.
fn after_hub_join(data: &Data, sweep: &Sweep, guild: GuildId, user: UserId, outcome: HubOutcome) {
    match outcome {
        HubOutcome::Created(room) => sweep_if_empty(data, sweep, guild, room),
        HubOutcome::Refused(why) => {
            tracing::debug!("rooms: no room for {user} in {guild}: {why}");
            if let Some(room) = data.rooms.owned_by(guild, user) {
                sweep_if_empty(data, sweep, guild, room);
            }
        },
        HubOutcome::MovedToOwn(_) | HubOutcome::CoolingDown(_) => {},
    }
}

/// Put a server's rooms in order: see spec 1.3, "Reconcile at GuildCreate".
pub async fn reconcile(
    cx: &RoomCx<'_>,
    sweep: &Sweep,
    facts: &GuildFacts,
    stored: Vec<StoredRoom>,
    existing: &HashMap<ChannelId, ExistingChannel>,
    settings: &RoomSettings,
    hub: Option<&Hub>,
) {
    let data = cx.data;
    let guild = facts.guild;
    // Run again on every gateway reconnect: a room still in the registry is
    // live, and its state wins over its row.
    let mut rooms = data.rooms.rooms_in(guild);
    for row in stored {
        let ch = row.channel;
        if data.rooms.get(ch).is_some() {
            continue;
        }
        match existing.get(&ch) {
            None => drop_row(data, ch).await,
            Some(channel) => {
                data.rooms.insert(LiveRoom {
                    guild,
                    channel: ch,
                    name: channel.name.clone(),
                    state: row.state,
                    baseline: row.baseline,
                    limit: channel.limit,
                    renames: RenameWindow::default(),
                });
                rooms.push(ch);
            },
        }
    }
    for ch in rooms {
        if existing.contains_key(&ch) {
            schedule_if_empty(
                data,
                sweep.discord.clone(),
                sweep.occupancy.clone(),
                guild,
                ch,
                grace_of(settings),
            );
        } else {
            forget(data, ch).await;
        }
    }

    // Members found sitting in the hub are fresh joins.
    let Some(hub) = hub.filter(|h| settings.enabled && settings.hub == Some(h.channel)) else {
        return;
    };
    for user in facts.humans_in(hub.channel) {
        let outcome = hub_join(cx, facts, settings, hub, user, Instant::now()).await;
        after_hub_join(data, sweep, guild, user, outcome);
    }
}

/// The humans among `states` (user, channel, known to be a bot) in `room`.
/// CrackTunes is never one; a member not known to be a bot is.
fn count_humans(
    room: ChannelId,
    me: UserId,
    states: impl IntoIterator<Item = (UserId, Option<ChannelId>, bool)>,
) -> usize {
    states
        .into_iter()
        .filter(|(user, ch, is_bot)| *ch == Some(room) && !is_bot && *user != me)
        .count()
}

/// The hub as `g` holds it. `None` when the hub is not there, or when its
/// category is not: an unread category could be hiding an 18+ gate.
fn hub_in(g: &Guild, hub: ChannelId) -> Option<Hub> {
    let parent = g.channels.get(&hub)?.parent_id;
    let category_overwrites = match parent {
        Some(category) => g
            .channels
            .get(&category)?
            .permission_overwrites
            .iter()
            .map(StoredOverwrite::from)
            .collect(),
        None => Vec::new(),
    };
    Some(Hub {
        channel: hub,
        parent,
        category_overwrites,
    })
}

fn existing_channels(g: &Guild) -> HashMap<ChannelId, ExistingChannel> {
    g.channels
        .iter()
        .map(|c| {
            let limit = c
                .user_limit
                .map_or(0, |l| u8::try_from(l.get()).unwrap_or(u8::MAX));
            (
                c.id,
                ExistingChannel {
                    name: c.base.name.to_string(),
                    limit,
                },
            )
        })
        .collect()
}

// --- Glue: serenity events in, the pieces above out. Untested here; the
// TuneTitan checklist covers the routing.

/// The live occupancy: the cache's voice states.
pub struct CacheOccupancy {
    pub cache: Arc<Cache>,
}

impl Occupancy for CacheOccupancy {
    fn humans_in(&self, guild: GuildId, room: ChannelId) -> usize {
        let me = self.cache.current_user().id;
        let Some(g) = self.cache.guild(guild) else {
            // Cannot tell, so keep it: the server's next GuildCreate
            // reconciles it.
            return 1;
        };
        count_humans(
            room,
            me,
            g.voice_states.iter().map(|vs| {
                let member = vs.member.as_ref().or_else(|| g.members.get(&vs.user_id));
                (
                    vs.user_id,
                    vs.channel_id,
                    member.is_some_and(|m| m.user.bot()),
                )
            }),
        )
    }
}

impl Sweep {
    pub fn live(ctx: &Context) -> Self {
        Self {
            discord: Arc::new(LiveRoomsDiscord {
                http: ctx.http.clone(),
            }),
            occupancy: Arc::new(CacheOccupancy {
                cache: ctx.cache.clone(),
            }),
        }
    }
}

/// Every voice state update, before the bot-only handler.
pub async fn on_voice_state(
    data: &Data,
    ctx: &Context,
    old: Option<&VoiceState>,
    new: &VoiceState,
) {
    let Some(guild) = new.guild_id else {
        return;
    };
    let Some(change) = classify(old.and_then(|o| o.channel_id), new.channel_id) else {
        return;
    };
    // Bots, CrackTunes among them, neither get rooms nor keep one alive.
    if is_bot(&ctx.cache, guild, new) {
        return;
    }
    let sweep = Sweep::live(ctx);
    let Some(joined) = room_voice_change(data, &sweep, guild, change) else {
        return;
    };
    let settings = data.rooms.settings(guild);
    if !settings.enabled || settings.hub != Some(joined) {
        return;
    }
    let user = new.user_id;
    if let HubOutcome::CoolingDown(wait) = hub_join_live(data, ctx, &sweep, guild, user).await {
        recheck_after(data.clone(), ctx.clone(), sweep, guild, user, wait);
    }
}

fn is_bot(cache: &Cache, guild: GuildId, vs: &VoiceState) -> bool {
    if vs.user_id == cache.current_user().id {
        return true;
    }
    if let Some(m) = &vs.member {
        return m.user.bot();
    }
    cache
        .guild(guild)
        .and_then(|g| g.members.get(&vs.user_id).map(|m| m.user.bot()))
        .unwrap_or(false)
}

/// A hub join with what the cache says now.
async fn hub_join_live(
    data: &Data,
    ctx: &Context,
    sweep: &Sweep,
    guild: GuildId,
    user: UserId,
) -> HubOutcome {
    let settings = data.rooms.settings(guild);
    let Some(hub_id) = settings.hub.filter(|_| settings.enabled) else {
        return HubOutcome::Refused(ROOMS_OFF);
    };
    let hub = ctx.cache.guild(guild).and_then(|g| hub_in(&g, hub_id));
    let Some(hub) = hub else {
        tracing::debug!("rooms: hub {hub_id} of {guild} or its category is not cached");
        return HubOutcome::Refused(HUB_UNKNOWN);
    };
    let Some(facts) = gather(&ctx.cache, &ctx.http, guild, &[user]).await else {
        return HubOutcome::Refused(GUILD_UNKNOWN);
    };
    let transport = DiscordTransport::of(ctx);
    let cx = RoomCx {
        data,
        discord: &*sweep.discord,
        transport: &transport,
    };
    let outcome = hub_join(&cx, &facts, &settings, &hub, user, Instant::now()).await;
    if outcome == HubOutcome::Refused(MISSING_PERMISSIONS) {
        if let Some(findings) = super::preflight::for_guild(&ctx.cache, guild, hub_id) {
            tracing::warn!("rooms: what CrackTunes is missing in {guild}: {findings:?}");
        }
    }
    after_hub_join(data, sweep, guild, user, outcome);
    outcome
}

/// Spec 1.3 step 2: wait out the cooldown, then go again if they are still
/// in the hub.
fn recheck_after(
    data: Data,
    ctx: Context,
    sweep: Sweep,
    guild: GuildId,
    user: UserId,
    wait: Duration,
) {
    tokio::spawn(async move {
        let mut wait = wait;
        // Each round waits out a cooldown another attempt started; a few
        // are plenty.
        for _ in 0..3 {
            tokio::time::sleep(wait).await;
            let settings = data.rooms.settings(guild);
            let hub = settings.hub.filter(|_| settings.enabled);
            if hub.is_none() || voice_channel_of(&ctx.cache, guild, user) != hub {
                return;
            }
            match hub_join_live(&data, &ctx, &sweep, guild, user).await {
                HubOutcome::CoolingDown(next) => wait = next,
                _ => return,
            }
        }
    });
}

fn voice_channel_of(cache: &Cache, guild: GuildId, user: UserId) -> Option<ChannelId> {
    cache.guild(guild)?.voice_states.get(&user)?.channel_id
}

/// Reconcile a server's rooms as its `GuildCreate` arrives: at startup, after
/// an outage, and on every gateway reconnect.
pub async fn reconcile_guild(data: &Data, ctx: &Context, guild: &Guild) {
    let stored = match data.database_pool.as_ref() {
        Some(pool) => match crate::db::rooms::rooms_in(pool, guild.id).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!("rooms: loading the rooms of {} failed: {err}", guild.id);
                return;
            },
        },
        None => Vec::new(),
    };
    let Some(facts) = gather(&ctx.cache, &ctx.http, guild.id, &[]).await else {
        tracing::debug!("rooms: {} is not cached; no reconcile", guild.id);
        return;
    };
    let settings = data.rooms.settings(guild.id);
    // The channels as the cache has them now, after the rows were read, not
    // as the payload had them: a room a hub join made since the payload has
    // a row, and must not lose it for a channel the payload never listed.
    let read = ctx.cache.guild(guild.id).map(|g| {
        (
            existing_channels(&g),
            settings.hub.and_then(|h| hub_in(&g, h)),
        )
    });
    let Some((existing, hub)) = read else {
        return;
    };
    let sweep = Sweep::live(ctx);
    let transport = DiscordTransport::of(ctx);
    let cx = RoomCx {
        data,
        discord: &*sweep.discord,
        transport: &transport,
    };
    reconcile(
        &cx,
        &sweep,
        &facts,
        stored,
        &existing,
        &settings,
        hub.as_ref(),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::test_support::{FakeTransport, Op};
    use crate::rooms::facts::tests::empty;
    use crate::rooms::registry::NEW_ROOM_COOLDOWN;
    use crate::rooms::state::OverwriteTarget;
    use crate::rooms::test_support::{Call, CallKind, FakeRoomsDiscord};
    use crate::DataInner;
    use serenity::all::{GuildChannel, MessageId, Permissions, RoleId};
    use serenity::nonmax::NonMaxU16;
    use serenity::small_fixed_array::{FixedArray, FixedString};
    use std::sync::Mutex;

    const G: GuildId = GuildId::new(100);
    const HUB: ChannelId = ChannelId::new(300);
    const CATEGORY: ChannelId = ChannelId::new(301);
    /// A room that already lives.
    const ROOM: ChannelId = ChannelId::new(600);
    /// The fake's first created channel.
    const NEW: ChannelId = ChannelId::new(5000);
    /// The fake transport's first message.
    const PANEL: MessageId = MessageId::new(1000);
    const JOINER: UserId = UserId::new(1);
    const OTHER: UserId = UserId::new(2);
    const OWNER2: UserId = UserId::new(3);
    const GUILD_OWNER: UserId = UserId::new(77);
    const BOT: UserId = UserId::new(99);
    const OTHER_BOT: UserId = UserId::new(98);
    const ADULT: RoleId = RoleId::new(18);
    const V: Permissions = Permissions::VIEW_CHANNEL;
    const C: Permissions = Permissions::CONNECT;
    const NONE: Permissions = Permissions::empty();
    const SEC: Duration = Duration::from_secs(1);

    fn ow(target: OverwriteTarget, allow: Permissions, deny: Permissions) -> StoredOverwrite {
        StoredOverwrite {
            target,
            allow,
            deny,
        }
    }
    fn everyone() -> OverwriteTarget {
        OverwriteTarget::Role {
            id: RoleId::new(G.get()),
        }
    }

    /// An 18+ category: hidden from `@everyone`, open to the adult role.
    fn gate() -> Vec<StoredOverwrite> {
        vec![
            ow(everyone(), NONE, V),
            ow(OverwriteTarget::Role { id: ADULT }, V | C, NONE),
        ]
    }

    fn hub() -> Hub {
        Hub {
            channel: HUB,
            parent: Some(CATEGORY),
            category_overwrites: gate(),
        }
    }

    fn settings() -> RoomSettings {
        RoomSettings {
            enabled: true,
            hub: Some(HUB),
            default_limit: 4,
            ..RoomSettings::off(G)
        }
    }

    fn new_room() -> NewRoom {
        NewRoom {
            guild: G,
            name: "Saucey's room".into(),
            parent: Some(CATEGORY),
            user_limit: 4,
        }
    }

    /// The joiner holds the adult role and sits in the hub; another member,
    /// without it, too.
    fn facts() -> GuildFacts {
        let mut f = empty(G);
        f.bot = BOT;
        f.role_perms = [(ADULT, NONE)].into();
        f.roles = [
            (JOINER, vec![ADULT]),
            (OTHER, vec![]),
            (OWNER2, vec![ADULT]),
        ]
        .into();
        f.display = [
            (JOINER, "Saucey".to_string()),
            (OTHER, "Kid".to_string()),
            (OWNER2, "Owner".to_string()),
            (GUILD_OWNER, "Boss".to_string()),
        ]
        .into();
        f.voice = [(JOINER, (HUB, false))].into();
        f
    }

    fn live(channel: ChannelId, owner: UserId) -> LiveRoom {
        LiveRoom {
            guild: G,
            channel,
            name: "room".into(),
            state: RoomState::new(owner),
            baseline: gate(),
            limit: 0,
            renames: RenameWindow::default(),
        }
    }

    #[derive(Default)]
    struct FakeOccupancy(Mutex<HashMap<ChannelId, usize>>);

    impl FakeOccupancy {
        fn set(&self, room: ChannelId, n: usize) {
            self.0.lock().unwrap().insert(room, n);
        }
    }

    impl Occupancy for FakeOccupancy {
        fn humans_in(&self, _guild: GuildId, room: ChannelId) -> usize {
            self.0.lock().unwrap().get(&room).copied().unwrap_or(0)
        }
    }

    struct World {
        data: Data,
        discord: Arc<FakeRoomsDiscord>,
        transport: FakeTransport,
        occupancy: Arc<FakeOccupancy>,
    }

    impl World {
        /// Discord syncs a new room to its category, as spec 5 assumes.
        fn new() -> Self {
            Self::with(FakeRoomsDiscord::default().with_created_overwrites(gate()))
        }

        fn with(discord: FakeRoomsDiscord) -> Self {
            Self::build(discord, None)
        }

        fn build(discord: FakeRoomsDiscord, pool: Option<sqlx::PgPool>) -> Self {
            let data = Data(Arc::new(DataInner {
                database_pool: pool,
                ..Default::default()
            }));
            data.rooms.set_settings(settings());
            Self {
                data,
                discord: Arc::new(discord),
                transport: FakeTransport::default(),
                occupancy: Arc::new(FakeOccupancy::default()),
            }
        }

        fn cx(&self) -> RoomCx<'_> {
            RoomCx {
                data: &self.data,
                discord: &*self.discord,
                transport: &self.transport,
            }
        }

        fn sweep(&self) -> Sweep {
            Sweep {
                discord: self.discord.clone(),
                occupancy: self.occupancy.clone(),
            }
        }

        async fn join(&self, facts: &GuildFacts, user: UserId, now: Instant) -> HubOutcome {
            hub_join(&self.cx(), facts, &settings(), &hub(), user, now).await
        }

        async fn room(&self, ch: ChannelId) -> Option<LiveRoom> {
            match self.data.rooms.get(ch) {
                Some(r) => Some(r.lock().await.clone()),
                None => None,
            }
        }

        fn voice(&self, change: VoiceChange) -> Option<ChannelId> {
            room_voice_change(&self.data, &self.sweep(), G, change)
        }

        async fn reconcile(
            &self,
            facts: &GuildFacts,
            stored: Vec<StoredRoom>,
            existing: &HashMap<ChannelId, ExistingChannel>,
            settings: &RoomSettings,
        ) {
            let hub = hub();
            reconcile(
                &self.cx(),
                &self.sweep(),
                facts,
                stored,
                existing,
                settings,
                Some(&hub),
            )
            .await;
        }
    }

    /// Let spawned timers run as far as they can.
    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    /// Move the paused clock on, then let the timers run.
    async fn after(d: Duration) {
        tokio::time::advance(d).await;
        settle().await;
    }

    fn stored(channel: ChannelId, owner: UserId) -> StoredRoom {
        StoredRoom {
            channel,
            guild: G,
            owner,
            state: RoomState::new(owner),
            baseline: gate(),
        }
    }

    fn existing(channels: &[ChannelId]) -> HashMap<ChannelId, ExistingChannel> {
        channels
            .iter()
            .map(|c| {
                (
                    *c,
                    ExistingChannel {
                        name: format!("room {c}"),
                        limit: 0,
                    },
                )
            })
            .collect()
    }

    // --- classify

    #[test]
    fn classify_join() {
        assert_eq!(classify(None, Some(HUB)), Some(VoiceChange::Joined(HUB)));
    }

    #[test]
    fn classify_leave() {
        assert_eq!(classify(Some(ROOM), None), Some(VoiceChange::Left(ROOM)));
    }

    #[test]
    fn classify_move() {
        assert_eq!(
            classify(Some(HUB), Some(ROOM)),
            Some(VoiceChange::Moved {
                from: HUB,
                to: ROOM
            })
        );
    }

    #[test]
    fn classify_same_channel_is_none() {
        // A mute, deafen or stream toggle in place.
        assert_eq!(classify(Some(ROOM), Some(ROOM)), None);
        assert_eq!(classify(None, None), None);
    }

    // --- the hub join

    #[tokio::test]
    async fn hub_join_creates_persists_overwrites_moves_and_posts_in_order() {
        let w = World::new();
        let got = w.join(&facts(), JOINER, Instant::now()).await;
        assert_eq!(got, HubOutcome::Created(NEW));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Create(new_room()),
                Call::Move(JOINER.get(), Some(NEW.get())),
                Call::Pin(NEW.get(), PANEL.get()),
            ]
        );
        assert_eq!(w.transport.ops(), vec![Op::Send(NEW.get())]);
        let room = w.room(NEW).await.expect("registered");
        let mut want = RoomState::new(JOINER);
        want.panel = Some(PANEL);
        assert_eq!(room.state, want);
        assert_eq!(room.baseline, gate(), "what Discord gave the channel");
        assert_eq!(room.name, "Saucey's room");
        assert_eq!(room.limit, 4);
        assert_eq!(w.data.rooms.owned_by(G, JOINER), Some(NEW));
        assert!(w.data.rooms.hub_creating.is_empty(), "the guard is let go");
    }

    #[tokio::test]
    async fn create_sends_no_permission_overwrites() {
        let w = World::new();
        w.join(&facts(), JOINER, Instant::now()).await;
        // `NewRoom` has no overwrites to send: the channel inherits its
        // category's. Every field it does carry is pinned here.
        assert_eq!(w.discord.calls()[0], Call::Create(new_room()));

        // A joiner whose name the facts lack gets the plain fallback name.
        let w = World::new();
        let mut f = facts();
        f.display.remove(&JOINER);
        w.join(&f, JOINER, Instant::now()).await;
        assert_eq!(
            w.discord.calls()[0],
            Call::Create(NewRoom {
                name: ROOM_NAME_FALLBACK.into(),
                ..new_room()
            })
        );
    }

    #[tokio::test]
    async fn unsynced_creation_copies_category_overwrites() {
        // Discord hands the channel back with no overwrites at all.
        let w = World::with(FakeRoomsDiscord::default());
        let got = w.join(&facts(), JOINER, Instant::now()).await;
        assert_eq!(got, HubOutcome::Created(NEW));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Create(new_room()),
                Call::Put(NEW.get(), ow(everyone(), NONE, V)),
                Call::Put(
                    NEW.get(),
                    ow(OverwriteTarget::Role { id: ADULT }, V | C, NONE)
                ),
                Call::Move(JOINER.get(), Some(NEW.get())),
                Call::Pin(NEW.get(), PANEL.get()),
            ]
        );
        let room = w.room(NEW).await.expect("registered");
        assert_eq!(room.baseline, gate(), "the category's, not the empty set");
    }

    #[tokio::test]
    async fn unsynced_creation_that_cannot_copy_deletes_the_room() {
        let w = World::with(FakeRoomsDiscord::default());
        w.discord
            .fail_next(CallKind::Put, DiscordError::MissingPermissions);
        let got = w.join(&facts(), JOINER, Instant::now()).await;
        assert_eq!(got, HubOutcome::Refused(COPY_FAILED));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Create(new_room()),
                Call::Put(NEW.get(), ow(everyone(), NONE, V)),
                Call::Delete(NEW.get()),
            ],
            "deleted, and nobody moved into a room open to everyone"
        );
        assert_eq!(w.transport.ops(), vec![]);
        assert!(w.data.rooms.get(NEW).is_none());
        assert_eq!(w.data.rooms.owned_by(G, JOINER), None);
    }

    #[tokio::test]
    async fn a_synced_room_in_another_order_is_not_copied() {
        let mut reversed = gate();
        reversed.reverse();
        let w = World::with(FakeRoomsDiscord::default().with_created_overwrites(reversed.clone()));
        assert_eq!(
            w.join(&facts(), JOINER, Instant::now()).await,
            HubOutcome::Created(NEW)
        );
        assert_eq!(
            w.discord.calls().iter().map(Call::kind).collect::<Vec<_>>(),
            [CallKind::Create, CallKind::Move, CallKind::Pin],
            "the same overwrites, listed differently, are in sync"
        );
        assert_eq!(w.room(NEW).await.unwrap().baseline, reversed);
    }

    #[tokio::test]
    async fn the_managed_layer_is_applied_before_the_move() {
        // A neutral entry in the category (a role added, nothing set) is not
        // part of the managed set, so the diff clears it on the room.
        let neutral = OverwriteTarget::Role {
            id: RoleId::new(50),
        };
        let mut category = gate();
        category.push(ow(neutral, NONE, NONE));
        let w = World::with(FakeRoomsDiscord::default().with_created_overwrites(category.clone()));
        let got = hub_join(
            &w.cx(),
            &facts(),
            &settings(),
            &Hub {
                category_overwrites: category,
                ..hub()
            },
            JOINER,
            Instant::now(),
        )
        .await;
        assert_eq!(got, HubOutcome::Created(NEW));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Create(new_room()),
                Call::DeleteOverwrite(NEW.get(), neutral),
                Call::Move(JOINER.get(), Some(NEW.get())),
                Call::Pin(NEW.get(), PANEL.get()),
            ]
        );
    }

    #[tokio::test]
    async fn a_room_whose_overwrites_cannot_be_set_is_deleted() {
        let neutral = OverwriteTarget::Role {
            id: RoleId::new(50),
        };
        let mut category = gate();
        category.push(ow(neutral, NONE, NONE));
        let w = World::with(FakeRoomsDiscord::default().with_created_overwrites(category.clone()));
        w.discord
            .fail_next(CallKind::DeleteOverwrite, DiscordError::MissingPermissions);
        let hub = Hub {
            category_overwrites: category,
            ..hub()
        };
        let got = hub_join(&w.cx(), &facts(), &settings(), &hub, JOINER, Instant::now()).await;
        assert_eq!(got, HubOutcome::Refused(OVERWRITES_FAILED));
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Create(new_room()),
                Call::DeleteOverwrite(NEW.get(), neutral),
                Call::Delete(NEW.get()),
            ]
        );
        assert!(w.data.rooms.get(NEW).is_none());
    }

    #[tokio::test]
    async fn owner_rejoining_hub_is_moved_to_existing_room() {
        let w = World::new();
        let t0 = Instant::now();
        assert_eq!(w.join(&facts(), JOINER, t0).await, HubOutcome::Created(NEW));
        // Their room emptied while they went back to the hub.
        let timer = tokio::spawn(std::future::pending::<()>());
        w.data.rooms.arm_timer(NEW, timer.abort_handle());

        let got = w.join(&facts(), JOINER, t0 + 2 * SEC).await;
        assert_eq!(got, HubOutcome::MovedToOwn(NEW));
        assert_eq!(
            w.discord.calls()[3..],
            [Call::Move(JOINER.get(), Some(NEW.get()))],
            "moved, and nothing created"
        );
        assert!(
            !w.data.rooms.timer_pending(NEW),
            "the pending deletion is cancelled"
        );
        assert_eq!(w.data.rooms.count_in(G), 1);
        assert_eq!(w.transport.ops(), vec![Op::Send(NEW.get())]);
    }

    #[tokio::test]
    async fn hub_rejoin_inside_cooldown_creates_one_room() {
        let creates = |w: &World| {
            w.discord
                .calls()
                .iter()
                .filter(|c| c.kind() == CallKind::Create)
                .count()
        };

        // Hub, out, hub again within seconds, with the room made.
        let w = World::new();
        let t0 = Instant::now();
        assert_eq!(w.join(&facts(), JOINER, t0).await, HubOutcome::Created(NEW));
        assert_eq!(
            w.join(&facts(), JOINER, t0 + 2 * SEC).await,
            HubOutcome::MovedToOwn(NEW)
        );
        assert_eq!(creates(&w), 1);
        assert_eq!(w.data.rooms.count_in(G), 1);

        // The first room failed (they hopped out before the move) and was
        // deleted; back in the hub two seconds later, they wait out the rest.
        let w = World::new();
        w.discord
            .fail_next(CallKind::Move, DiscordError::NotConnected);
        assert_eq!(
            w.join(&facts(), JOINER, t0).await,
            HubOutcome::Refused(MOVE_FAILED)
        );
        assert_eq!(
            w.join(&facts(), JOINER, t0 + 2 * SEC).await,
            HubOutcome::CoolingDown(NEW_ROOM_COOLDOWN - 2 * SEC)
        );
        assert_eq!(creates(&w), 1, "no second channel inside the window");
        assert_eq!(
            w.join(&facts(), JOINER, t0 + NEW_ROOM_COOLDOWN).await,
            HubOutcome::Created(ChannelId::new(5001))
        );
        assert_eq!(w.data.rooms.count_in(G), 1);
        assert_eq!(creates(&w), 2, "one deleted, one live");
    }

    #[tokio::test]
    async fn a_creation_in_flight_makes_no_second_room() {
        let w = World::new();
        w.data.rooms.hub_creating.insert((G, JOINER));
        let got = w.join(&facts(), JOINER, Instant::now()).await;
        assert_eq!(
            got,
            HubOutcome::CoolingDown(NEW_ROOM_COOLDOWN),
            "waits, so a re-check follows if the first one fails"
        );
        assert_eq!(w.discord.calls(), vec![]);
        assert!(
            w.data.rooms.hub_creating.contains(&(G, JOINER)),
            "the other creation's claim is left alone"
        );
    }

    #[tokio::test]
    async fn joiner_the_category_keeps_out_gets_no_room() {
        let w = World::new();
        let mut f = facts();
        f.voice.insert(OTHER, (HUB, false));
        let got = w.join(&f, OTHER, Instant::now()).await;
        assert_eq!(got, HubOutcome::Refused(KEPT_OUT));
        assert_eq!(w.discord.calls(), vec![]);
        assert_eq!(w.transport.ops(), vec![]);
        assert_eq!(w.data.rooms.count_in(G), 0);

        // The server's owner gets in whatever their roles (R10).
        let got = w.join(&f, GUILD_OWNER, Instant::now()).await;
        assert_eq!(got, HubOutcome::Created(NEW));
    }

    #[tokio::test]
    async fn fifty_rooms_is_the_cap() {
        let w = World::new();
        for i in 0..MAX_ROOMS as u64 {
            w.data
                .rooms
                .insert(live(ChannelId::new(700 + i), UserId::new(1000 + i)));
        }
        // Another server's rooms do not count.
        let mut elsewhere = live(ChannelId::new(900), UserId::new(900));
        elsewhere.guild = GuildId::new(200);
        w.data.rooms.insert(elsewhere);

        let t0 = Instant::now();
        let got = w.join(&facts(), JOINER, t0).await;
        assert_eq!(got, HubOutcome::Refused(AT_CAP));
        assert_eq!(w.discord.calls(), vec![]);

        w.data.rooms.remove(ChannelId::new(700));
        let got = w.join(&facts(), JOINER, t0 + NEW_ROOM_COOLDOWN).await;
        assert_eq!(got, HubOutcome::Created(NEW), "the 50th room is allowed");
    }

    // --- emptying

    #[tokio::test(start_paused = true)]
    async fn empty_room_is_deleted_after_grace() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        assert_eq!(w.voice(VoiceChange::Left(ROOM)), None);
        assert!(w.data.rooms.timer_pending(ROOM));

        after(29 * SEC).await;
        assert_eq!(w.discord.calls(), vec![], "still inside the grace");
        assert!(w.data.rooms.get(ROOM).is_some());

        after(2 * SEC).await;
        assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
        assert!(w.data.rooms.get(ROOM).is_none());
        assert_eq!(w.data.rooms.owned_by(G, JOINER), None);
        assert!(!w.data.rooms.timer_pending(ROOM));
    }

    #[tokio::test(start_paused = true)]
    async fn moving_out_of_a_room_counts_as_leaving_it() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        let to = ChannelId::new(42);
        assert_eq!(w.voice(VoiceChange::Moved { from: ROOM, to }), Some(to));
        after(31 * SEC).await;
        assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
    }

    #[tokio::test(start_paused = true)]
    async fn rejoin_inside_grace_cancels_deletion() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        w.voice(VoiceChange::Left(ROOM));

        // Somebody, not necessarily the owner, comes back inside the grace.
        after(10 * SEC).await;
        w.occupancy.set(ROOM, 1);
        assert_eq!(w.voice(VoiceChange::Joined(ROOM)), Some(ROOM));
        assert!(!w.data.rooms.timer_pending(ROOM), "the deletion is aborted");

        after(60 * SEC).await;
        assert_eq!(w.discord.calls(), vec![]);
        let room = w.room(ROOM).await.expect("the room survives");
        assert_eq!(room.state.owner, JOINER, "and keeps its owner");
        assert_eq!(w.data.rooms.owned_by(G, JOINER), Some(ROOM));
    }

    #[tokio::test(start_paused = true)]
    async fn a_second_emptying_gets_a_whole_new_grace() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        w.voice(VoiceChange::Left(ROOM)); // t0: deadline t30
        after(10 * SEC).await;
        w.occupancy.set(ROOM, 1);
        w.voice(VoiceChange::Joined(ROOM)); // t10
        after(10 * SEC).await;
        w.occupancy.set(ROOM, 0);
        w.voice(VoiceChange::Left(ROOM)); // t20: deadline t50

        after(11 * SEC).await; // t31
        assert_eq!(
            w.discord.calls(),
            vec![],
            "the first timer was cancelled, not left to fire at t30"
        );
        after(20 * SEC).await; // t51
        assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_second_sweep_keeps_the_first_deadline() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        w.voice(VoiceChange::Left(ROOM)); // t0: deadline t30
        after(20 * SEC).await;
        // Swept again while already counting down, as a reconcile does.
        w.voice(VoiceChange::Left(ROOM));
        after(11 * SEC).await; // t31
        assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_room_occupied_when_the_timer_fires_is_kept() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        w.voice(VoiceChange::Left(ROOM));
        // Someone is there by the deadline, though no join was seen.
        w.occupancy.set(ROOM, 1);
        after(31 * SEC).await;
        assert_eq!(w.discord.calls(), vec![]);
        assert!(w.data.rooms.get(ROOM).is_some());
        assert!(!w.data.rooms.timer_pending(ROOM));
    }

    #[tokio::test(start_paused = true)]
    async fn an_occupied_room_left_by_one_is_not_scheduled() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        w.occupancy.set(ROOM, 2);
        w.voice(VoiceChange::Left(ROOM));
        assert!(!w.data.rooms.timer_pending(ROOM));
        // Leaving a channel that is no room schedules nothing either.
        w.voice(VoiceChange::Left(ChannelId::new(42)));
        assert!(!w.data.rooms.timer_pending(ChannelId::new(42)));
        after(301 * SEC).await;
        assert_eq!(w.discord.calls(), vec![]);
    }

    #[tokio::test(start_paused = true)]
    async fn the_servers_grace_applies_even_with_rooms_off() {
        let w = World::new();
        w.data.rooms.set_settings(RoomSettings {
            enabled: false,
            empty_grace_secs: 120,
            ..settings()
        });
        w.data.rooms.insert(live(ROOM, JOINER));
        w.voice(VoiceChange::Left(ROOM));
        after(119 * SEC).await;
        assert_eq!(w.discord.calls(), vec![]);
        after(2 * SEC).await;
        assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
    }

    #[test]
    fn bots_alone_count_as_empty() {
        // CrackTunes playing music, even with a voice state not marked bot,
        // and another bot.
        let bots = [(BOT, Some(ROOM), false), (OTHER_BOT, Some(ROOM), true)];
        assert_eq!(count_humans(ROOM, BOT, bots), 0);
        let mixed = [
            (BOT, Some(ROOM), false),
            (JOINER, Some(ROOM), false),
            (OTHER, Some(HUB), false),
            (OWNER2, None, false),
        ];
        assert_eq!(count_humans(ROOM, BOT, mixed), 1);
    }

    #[tokio::test]
    async fn a_failed_delete_keeps_the_room() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        w.discord
            .fail_next(CallKind::Delete, DiscordError::MissingPermissions);
        delete_room(&w.data, &*w.discord, ROOM).await;
        assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
        assert!(w.data.rooms.get(ROOM).is_some(), "kept for reconcile");
    }

    #[tokio::test(start_paused = true)]
    async fn channel_deleted_by_hand_drops_room_and_timer() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, JOINER));
        w.voice(VoiceChange::Left(ROOM));
        assert!(w.data.rooms.timer_pending(ROOM));

        on_channel_delete(&w.data, ROOM).await;
        assert!(w.data.rooms.get(ROOM).is_none());
        assert_eq!(w.data.rooms.owned_by(G, JOINER), None);
        assert!(!w.data.rooms.timer_pending(ROOM));
        after(60 * SEC).await;
        assert_eq!(w.discord.calls(), vec![], "no deletion of a gone channel");

        // Any other channel's deletion touches nothing.
        w.data.rooms.insert(live(ROOM, JOINER));
        on_channel_delete(&w.data, ChannelId::new(42)).await;
        assert!(w.data.rooms.get(ROOM).is_some());
    }

    // --- reconcile

    #[tokio::test(start_paused = true)]
    async fn reconcile_schedules_empty_rooms() {
        let w = World::new();
        // Rooms since turned off: an empty one is still swept.
        w.reconcile(
            &facts(),
            vec![stored(ROOM, OWNER2)],
            &existing(&[ROOM]),
            &RoomSettings::off(G),
        )
        .await;
        assert!(w.data.rooms.get(ROOM).is_some(), "back in the registry");
        assert!(w.data.rooms.timer_pending(ROOM));
        after(29 * SEC).await;
        assert_eq!(w.discord.calls(), vec![]);
        after(2 * SEC).await;
        assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
        assert!(w.data.rooms.get(ROOM).is_none());
        assert_eq!(w.data.rooms.owned_by(G, OWNER2), None);
    }

    #[tokio::test(start_paused = true)]
    async fn reconcile_keeps_occupied_rooms_and_owners() {
        let w = World::new();
        let mut row = stored(ROOM, OWNER2);
        row.state.helpers.insert(OTHER);
        row.state.locked = true;
        row.state.panel = Some(MessageId::new(77));
        let channels = HashMap::from([(
            ROOM,
            ExistingChannel {
                name: "Owner's room".into(),
                limit: 5,
            },
        )]);
        w.occupancy.set(ROOM, 1);
        let off = RoomSettings::off(G);
        w.reconcile(&facts(), vec![row.clone()], &channels, &off)
            .await;

        let room = w.room(ROOM).await.expect("registered");
        assert_eq!(room.state, row.state);
        assert_eq!(room.baseline, row.baseline);
        assert_eq!(room.name, "Owner's room");
        assert_eq!(room.limit, 5);
        assert_eq!(w.data.rooms.owned_by(G, OWNER2), Some(ROOM));
        assert!(!w.data.rooms.timer_pending(ROOM));
        after(301 * SEC).await;
        assert_eq!(w.discord.calls(), vec![]);

        // Again, as on a gateway reconnect: the live room is kept as it is,
        // not reloaded from its row.
        let handle = w.data.rooms.get(ROOM).unwrap();
        handle.lock().await.state.locked = false;
        w.reconcile(&facts(), vec![row], &channels, &off).await;
        let again = w.data.rooms.get(ROOM).unwrap();
        assert!(Arc::ptr_eq(&handle, &again), "the same live room");
        assert!(!again.lock().await.state.locked);
    }

    #[tokio::test]
    async fn reconcile_gives_hub_occupants_rooms() {
        let w = World::new();
        let mut f = facts();
        // The joiner, a bot and the owner of a stored room sit in the hub.
        f.voice.insert(OTHER_BOT, (HUB, true));
        f.voice.insert(OWNER2, (HUB, false));
        w.occupancy.set(NEW, 1);
        w.reconcile(
            &f,
            vec![stored(ROOM, OWNER2)],
            &existing(&[ROOM]),
            &settings(),
        )
        .await;
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Create(new_room()),
                Call::Move(JOINER.get(), Some(NEW.get())),
                Call::Pin(NEW.get(), PANEL.get()),
                Call::Move(OWNER2.get(), Some(ROOM.get())),
            ]
        );
        assert_eq!(w.data.rooms.owned_by(G, JOINER), Some(NEW));
        assert_eq!(w.data.rooms.owned_by(G, OWNER2), Some(ROOM));
        assert!(!w.data.rooms.timer_pending(ROOM), "its owner is back in it");
        assert!(!w.data.rooms.timer_pending(NEW));

        // With rooms off (the hub still set), nobody in the hub gets one.
        let w = World::new();
        let off = RoomSettings {
            enabled: false,
            ..settings()
        };
        w.reconcile(&f, vec![], &existing(&[]), &off).await;
        assert_eq!(w.discord.calls(), vec![]);
    }

    #[tokio::test(start_paused = true)]
    async fn rooms_a_hub_join_leaves_empty_are_swept() {
        // The joiner left before their new room was registered: no leave
        // for it is ever seen, and its occupancy stays 0.
        let w = World::new();
        w.reconcile(&facts(), vec![], &existing(&[]), &settings())
            .await;
        assert_eq!(w.data.rooms.owned_by(G, JOINER), Some(NEW));
        assert!(w.data.rooms.timer_pending(NEW), "nobody ever arrived");
        after(31 * SEC).await;
        assert_eq!(w.discord.calls()[3..], [Call::Delete(NEW.get())]);

        // OWNER2 sits in the hub, and moving them into their room fails.
        let w = World::new();
        let mut f = facts();
        f.voice = [(OWNER2, (HUB, false))].into();
        w.discord
            .fail_next(CallKind::Move, DiscordError::MissingPermissions);
        let rows = vec![stored(ROOM, OWNER2)];
        w.reconcile(&f, rows, &existing(&[ROOM]), &settings()).await;
        assert_eq!(
            w.discord.calls(),
            vec![Call::Move(OWNER2.get(), Some(ROOM.get()))]
        );
        assert!(
            w.data.rooms.timer_pending(ROOM),
            "re-armed after the failed move"
        );
        after(31 * SEC).await;
        assert!(w.data.rooms.get(ROOM).is_none());
    }

    #[tokio::test]
    async fn reconcile_forgets_a_live_room_whose_channel_went() {
        let w = World::new();
        w.data.rooms.insert(live(ROOM, OWNER2));
        w.reconcile(
            &facts(),
            vec![stored(ROOM, OWNER2), stored(ChannelId::new(601), OTHER)],
            &existing(&[]),
            &RoomSettings::off(G),
        )
        .await;
        assert!(w.data.rooms.get(ROOM).is_none());
        assert!(w.data.rooms.get(ChannelId::new(601)).is_none());
        assert_eq!(w.data.rooms.owned_by(G, OWNER2), None);
        assert_eq!(w.discord.calls(), vec![], "nothing to delete on Discord");
    }

    // --- the glue's cache reads

    fn channel(id: ChannelId, parent: Option<ChannelId>) -> GuildChannel {
        let mut c = GuildChannel::default();
        c.id = id;
        c.parent_id = parent;
        c
    }

    #[test]
    fn a_hub_whose_category_is_not_cached_is_unknown() {
        let mut g = Guild::default();
        assert_eq!(hub_in(&g, HUB), None, "no hub");
        g.channels.insert(channel(HUB, Some(CATEGORY)));
        assert_eq!(
            hub_in(&g, HUB),
            None,
            "an unread category is not an empty one"
        );
        let mut cat = channel(CATEGORY, None);
        cat.permission_overwrites =
            FixedArray::from_vec_trunc(gate().iter().map(StoredOverwrite::to_serenity).collect());
        g.channels.insert(cat);
        assert_eq!(hub_in(&g, HUB), Some(hub()));

        // A hub outside any category has no category overwrites.
        g.channels.insert(channel(ChannelId::new(302), None));
        assert_eq!(
            hub_in(&g, ChannelId::new(302)),
            Some(Hub {
                channel: ChannelId::new(302),
                parent: None,
                category_overwrites: vec![],
            })
        );
    }

    #[test]
    fn existing_channels_reads_names_and_limits() {
        let mut g = Guild::default();
        let mut a = channel(ROOM, Some(CATEGORY));
        a.base.name = FixedString::from_str_trunc("Saucey's room");
        a.user_limit = NonMaxU16::new(5);
        g.channels.insert(a);
        g.channels.insert(channel(HUB, None));
        let got = existing_channels(&g);
        assert_eq!(
            got.get(&ROOM),
            Some(&ExistingChannel {
                name: "Saucey's room".into(),
                limit: 5
            })
        );
        assert_eq!(got.get(&HUB).map(|c| c.limit), Some(0), "no limit");
        assert_eq!(got.len(), 2);
    }

    mod db {
        use super::*;
        use crate::db::rooms::{insert_room, rooms_in};
        use sqlx::PgPool;

        pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

        fn world(pool: &PgPool, discord: FakeRoomsDiscord) -> World {
            World::build(discord, Some(pool.clone()))
        }

        fn synced() -> FakeRoomsDiscord {
            FakeRoomsDiscord::default().with_created_overwrites(gate())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn hub_join_stores_the_room_row(pool: PgPool) -> sqlx::Result<()> {
            let w = world(&pool, synced());
            let got = w.join(&facts(), JOINER, Instant::now()).await;
            assert_eq!(got, HubOutcome::Created(NEW));
            let mut state = RoomState::new(JOINER);
            state.panel = Some(PANEL);
            assert_eq!(
                rooms_in(&pool, G).await?,
                vec![StoredRoom {
                    channel: NEW,
                    guild: G,
                    owner: JOINER,
                    state,
                    baseline: gate(),
                }]
            );
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_room_that_cannot_be_stored_is_deleted_before_anyone_moves(
            pool: PgPool,
        ) -> sqlx::Result<()> {
            // A row already holds the channel id the fake will hand out.
            let mut clash = stored(NEW, OTHER);
            clash.guild = GuildId::new(999);
            insert_room(&pool, &clash).await?;
            let w = world(&pool, synced());
            let got = w.join(&facts(), JOINER, Instant::now()).await;
            assert_eq!(got, HubOutcome::Refused(STORE_FAILED));
            assert_eq!(
                w.discord.calls(),
                vec![Call::Create(new_room()), Call::Delete(NEW.get()),]
            );
            assert_eq!(w.transport.ops(), vec![]);
            assert!(w.data.rooms.get(NEW).is_none());
            assert!(rooms_in(&pool, G).await?.is_empty());
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn failed_move_deletes_channel_and_row(pool: PgPool) -> sqlx::Result<()> {
            let w = world(&pool, synced());
            w.discord
                .fail_next(CallKind::Move, DiscordError::NotConnected);
            let got = w.join(&facts(), JOINER, Instant::now()).await;
            assert_eq!(got, HubOutcome::Refused(MOVE_FAILED));
            assert_eq!(
                w.discord.calls(),
                vec![
                    Call::Create(new_room()),
                    Call::Move(JOINER.get(), Some(NEW.get())),
                    Call::Delete(NEW.get()),
                ]
            );
            assert!(rooms_in(&pool, G).await?.is_empty(), "the row went too");
            assert!(w.data.rooms.get(NEW).is_none());
            assert_eq!(w.data.rooms.owned_by(G, JOINER), None);
            assert_eq!(w.transport.ops(), vec![], "no panel");
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn an_uncopyable_room_leaves_no_row(pool: PgPool) -> sqlx::Result<()> {
            let w = world(&pool, FakeRoomsDiscord::default());
            w.discord
                .fail_next(CallKind::Put, DiscordError::MissingPermissions);
            let got = w.join(&facts(), JOINER, Instant::now()).await;
            assert_eq!(got, HubOutcome::Refused(COPY_FAILED));
            assert!(rooms_in(&pool, G).await?.is_empty());
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn unknown_channel_on_delete_drops_row_anyway(pool: PgPool) -> sqlx::Result<()> {
            insert_room(&pool, &stored(ROOM, JOINER)).await?;
            let w = world(&pool, synced());
            w.data.rooms.insert(live(ROOM, JOINER));
            w.discord
                .fail_next(CallKind::Delete, DiscordError::UnknownChannel);
            delete_room(&w.data, &*w.discord, ROOM).await;
            assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
            assert!(w.data.rooms.get(ROOM).is_none());
            assert!(rooms_in(&pool, G).await?.is_empty());
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_failed_delete_keeps_the_row(pool: PgPool) -> sqlx::Result<()> {
            insert_room(&pool, &stored(ROOM, JOINER)).await?;
            let w = world(&pool, synced());
            w.data.rooms.insert(live(ROOM, JOINER));
            w.discord
                .fail_next(CallKind::Delete, DiscordError::MissingPermissions);
            delete_room(&w.data, &*w.discord, ROOM).await;
            assert!(w.data.rooms.get(ROOM).is_some());
            assert_eq!(rooms_in(&pool, G).await?, vec![stored(ROOM, JOINER)]);
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_row_without_a_live_room_is_deleted_with_its_channel(
            pool: PgPool,
        ) -> sqlx::Result<()> {
            insert_room(&pool, &stored(ROOM, JOINER)).await?;
            let w = world(&pool, synced());
            delete_room(&w.data, &*w.discord, ROOM).await;
            assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
            assert!(rooms_in(&pool, G).await?.is_empty());
            Ok(())
        }

        /// Real time: the timer's delete must not be cut short by its own
        /// removal from the registry while the row is being dropped.
        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn an_emptied_room_loses_its_row_when_the_timer_fires(
            pool: PgPool,
        ) -> sqlx::Result<()> {
            insert_room(&pool, &stored(ROOM, JOINER)).await?;
            let w = world(&pool, synced());
            w.data.rooms.set_settings(RoomSettings {
                empty_grace_secs: 0,
                ..settings()
            });
            w.data.rooms.insert(live(ROOM, JOINER));
            w.voice(VoiceChange::Left(ROOM));
            for _ in 0..500 {
                if rooms_in(&pool, G).await?.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(w.discord.calls(), vec![Call::Delete(ROOM.get())]);
            assert!(rooms_in(&pool, G).await?.is_empty(), "the row went too");
            assert!(w.data.rooms.get(ROOM).is_none());
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_room_deleted_by_hand_loses_its_row(pool: PgPool) -> sqlx::Result<()> {
            insert_room(&pool, &stored(ROOM, JOINER)).await?;
            let w = world(&pool, synced());
            w.data.rooms.insert(live(ROOM, JOINER));
            on_channel_delete(&w.data, ROOM).await;
            assert!(rooms_in(&pool, G).await?.is_empty());
            assert_eq!(w.discord.calls(), vec![]);
            Ok(())
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn reconcile_drops_rows_for_vanished_channels(pool: PgPool) -> sqlx::Result<()> {
            let gone = ChannelId::new(601);
            let live_gone = ChannelId::new(602);
            for row in [
                stored(ROOM, JOINER),
                stored(gone, OTHER),
                stored(live_gone, OWNER2),
            ] {
                insert_room(&pool, &row).await?;
            }
            let w = world(&pool, synced());
            // A room the bot held through a reconnect whose channel went meanwhile.
            w.data.rooms.insert(live(live_gone, OWNER2));
            w.occupancy.set(ROOM, 1);
            let rows = rooms_in(&pool, G).await?;
            w.reconcile(&facts(), rows, &existing(&[ROOM]), &RoomSettings::off(G))
                .await;
            assert_eq!(rooms_in(&pool, G).await?, vec![stored(ROOM, JOINER)]);
            assert!(w.data.rooms.get(ROOM).is_some());
            assert!(w.data.rooms.get(gone).is_none());
            assert!(w.data.rooms.get(live_gone).is_none());
            assert_eq!(w.data.rooms.owned_by(G, OWNER2), None);
            assert_eq!(w.discord.calls(), vec![]);
            Ok(())
        }
    }
}
