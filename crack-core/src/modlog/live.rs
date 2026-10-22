//! The mod log's glue: serenity's events in, the log channel out. Audit
//! entries for server mutes and deafens post at once; voice leaves and moves
//! go to [`batch`] for attribution; a server whose log is on has its audit
//! snapshots seeded at `GuildCreate` and after a save turns the log on.
//!
//! Every entry point returns before building anything for a server with the
//! log off.
use super::attribution::Change;
use super::audit::{nonzero, AuditSource, LiveAudit};
use super::batch::{self, ModLogSink};
use super::event::{Actor, ModLogEvent};
use crate::economy::discord::SHOP_AUDIT_REASON_PREFIX;
use crate::messaging::courier::{self, Destination};
use crate::messaging::message::CrackedMessage;
use crate::messaging::render::RenderCx;
use crate::messaging::transport::{DiscordTransport, Transport};
use crate::rooms::lifecycle::{classify, VoiceChange};
use crate::Data;
use futures::future::BoxFuture;
use serenity::all::{AuditLogEntry, ChannelId, Context, GuildId, Http, UserId, VoiceState};
use serenity::async_trait;
use serenity::model::guild::audit_log::{Action, Change as AuditChange, MemberAction};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// Posts each event to the server's log channel, read at post time: nothing
/// while the log is off or has no channel (M3).
pub struct ChannelSink {
    pub data: Data,
    pub transport: Arc<dyn Transport>,
}

#[async_trait]
impl ModLogSink for ChannelSink {
    async fn post(&self, guild: GuildId, e: ModLogEvent) {
        let settings = self.data.modlog.settings(guild);
        let Some(channel) = settings.channel.filter(|_| settings.enabled) else {
            tracing::debug!("modlog: {guild}: {e} not posted: the log is off or has no channel");
            return;
        };
        let kind = e.to_string();
        let msg = CrackedMessage::from(e);
        let dest = Destination::Channel(channel.widen());
        let landed = courier::post(
            &self.data,
            self.transport.as_ref(),
            dest,
            &msg,
            &RenderCx::now(),
        )
        .await;
        if landed.is_none() {
            tracing::warn!("modlog: {guild}: posting {kind} to {channel} failed");
        }
    }
}

/// The server mutes and deafens one audit entry records, in its order. None
/// for another action, for an entry without an actor or a target (or with a
/// zero id), and for CrackTunes muting or deafening itself (its self-deafen:
/// M21). A moderator muting CrackTunes is logged, and so is CrackTunes
/// muting someone else (an admin command run through the bot). Another
/// bot's mute or deafen is logged too, marked by `actor_is_bot`.
fn events_of(entry: &AuditLogEntry, bot: UserId, actor_is_bot: bool) -> Vec<ModLogEvent> {
    if !matches!(entry.action, Action::Member(MemberAction::Update)) {
        return Vec::new();
    }
    let user = |id: Option<u64>| id.and_then(nonzero).map(UserId::new);
    let actor = user(entry.user_id.map(|u| u.get()));
    let target = user(entry.target_id.map(|t| t.get()));
    let (Some(actor), Some(target)) = (actor, target) else {
        return Vec::new();
    };
    if actor == bot && target == bot {
        return Vec::new();
    }
    // The shop's own changes are logged by the shop, with the buyer named.
    if actor == bot
        && entry
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with(SHOP_AUDIT_REASON_PREFIX))
    {
        return Vec::new();
    }
    let actor = Actor {
        id: actor,
        bot: actor_is_bot,
    };
    let reason = entry.reason.as_deref().map(str::to_owned);
    entry
        .changes
        .iter()
        .filter_map(|c| {
            let (old, new, mute) = match c {
                AuditChange::Mute { old, new } => (*old, *new, true),
                AuditChange::Deaf { old, new } => (*old, *new, false),
                _ => return None,
            };
            // No new value says nothing; the same value changed nothing.
            let on = new.filter(|n| old != Some(*n))?;
            let reason = reason.clone();
            Some(if mute {
                ModLogEvent::ServerMute {
                    target,
                    actor,
                    on,
                    reason,
                }
            } else {
                ModLogEvent::ServerDeafen {
                    target,
                    actor,
                    on,
                    reason,
                }
            })
        })
        .collect()
}

/// A server mute or deafen from Discord's audit log: logged exactly, with
/// the actor, the new value and the reason. Nothing for a server with the
/// log off.
pub async fn on_audit_entry(
    data: &Data,
    sink: &dyn ModLogSink,
    guild: GuildId,
    entry: &AuditLogEntry,
    bot: UserId,
    actor_is_bot: bool,
) {
    if !data.modlog.is_enabled(guild) {
        return;
    }
    for e in events_of(entry, bot, actor_is_bot) {
        sink.post(guild, e).await;
    }
}

/// The change the mod log batches from one voice update: a leave or a move.
/// A join, or a mute or deafen in place, is none.
pub fn voice_change(
    old: Option<ChannelId>,
    new: Option<ChannelId>,
    user: UserId,
) -> Option<Change> {
    match classify(old, new)? {
        VoiceChange::Left(from) => Some(Change::Left { target: user, from }),
        VoiceChange::Moved { from, to } => Some(Change::Moved {
            target: user,
            from,
            to,
        }),
        VoiceChange::Joined(_) => None,
    }
}

/// What a voice change is attributed with: built only for a server with the
/// log on.
struct Wire {
    audit: Arc<dyn AuditSource>,
    sink: Arc<dyn ModLogSink>,
    bot: UserId,
}

/// [`on_voice_state`] once serenity's types are unpacked: a leave or move
/// in a server with the log on is batched, whoever made it (M8a).
fn feed(
    data: &Data,
    guild: GuildId,
    user: UserId,
    old: Option<ChannelId>,
    new: Option<ChannelId>,
    now: Instant,
    wire: impl FnOnce() -> Wire,
) {
    // Off: nothing looked at further, nothing built, nothing held.
    if !data.modlog.is_enabled(guild) {
        return;
    }
    let Some(change) = voice_change(old, new, user) else {
        return;
    };
    let Wire { audit, sink, bot } = wire();
    batch::record_change(data, audit, sink, guild, change, now, bot);
}

/// Seed the server's snapshots in the background, if its log is on.
fn spawn_seed(data: &Data, audit: Arc<dyn AuditSource>, guild: GuildId) -> Option<JoinHandle<()>> {
    if !data.modlog.is_enabled(guild) {
        return None;
    }
    let data = data.clone();
    Some(tokio::spawn(async move {
        batch::seed(&data, audit.as_ref(), guild).await;
    }))
}

/// How long a failed read of a server's settings at `GuildCreate` waits
/// before its one retry (rooms' reconcile waits as long).
pub const LOAD_RETRY: Duration = Duration::from_secs(60);

/// Reads a server's stored settings into the registry; false when the read
/// failed.
type Load = Arc<dyn Fn(Data, GuildId) -> BoxFuture<'static, bool> + Send + Sync>;

/// The real read: [`super::load_settings`].
fn stored_settings() -> Load {
    Arc::new(|data, guild| Box::pin(async move { super::load_settings(&data, guild).await }))
}

/// Read the server's settings, then seed if the log is on. A failed read is
/// tried once more after [`LOAD_RETRY`] (M22), on tokio's clock; the retry's
/// handle is returned. Until a read succeeds the server keeps what was read
/// before (off, if nothing was) and refuses saves (`NotLoaded`).
async fn load_and_seed(
    data: &Data,
    audit: Arc<dyn AuditSource>,
    guild: GuildId,
    load: Load,
) -> Option<JoinHandle<()>> {
    if load(data.clone(), guild).await {
        spawn_seed(data, audit, guild);
        return None;
    }
    // Timed from the failure, not from when the task first runs.
    let at = Instant::now() + LOAD_RETRY;
    let data = data.clone();
    Some(tokio::spawn(async move {
        tokio::time::sleep_until(at).await;
        if !load(data.clone(), guild).await {
            tracing::warn!(
                "modlog: {guild}: the settings could not be read again; they wait for the \
                 next GuildCreate"
            );
            return;
        }
        if let Some(seed) = spawn_seed(&data, audit, guild) {
            let _ = seed.await;
        }
    }))
}

// --- Glue: serenity in, the pieces above out. Untested here beyond what
// they hand on; the TuneTitan checklist covers the routing.

/// Every voice state update, CrackTunes' own included (M8a). Synchronous:
/// it runs before the rooms hook, which can wait on Discord, so a change's
/// arrival time is when the gateway delivered it.
pub fn on_voice_state(data: &Data, ctx: &Context, old: Option<&VoiceState>, new: &VoiceState) {
    let Some(guild) = new.guild_id else {
        return;
    };
    feed(
        data,
        guild,
        new.user_id,
        old.and_then(|o| o.channel_id),
        new.channel_id,
        Instant::now(),
        || Wire {
            audit: Arc::new(LiveAudit {
                http: ctx.http.clone(),
            }),
            sink: Arc::new(ChannelSink {
                data: data.clone(),
                transport: Arc::new(DiscordTransport::of(ctx)),
            }),
            bot: ctx.cache.current_user().id,
        },
    );
}

/// Every audit log entry Discord pushes (it needs View Audit Log).
pub async fn on_audit_event(data: &Data, ctx: &Context, guild: GuildId, entry: &AuditLogEntry) {
    if !data.modlog.is_enabled(guild) {
        return;
    }
    let sink = ChannelSink {
        data: data.clone(),
        transport: Arc::new(DiscordTransport::of(ctx)),
    };
    // Read out first: the cache guard must not be held across the await.
    let bot = ctx.cache.current_user().id;
    // Look the actor up only for an entry that will post something.
    let actor_is_bot = match entry.user_id {
        Some(actor) if !events_of(entry, bot, false).is_empty() => {
            is_bot_account(ctx, guild, UserId::new(actor.get())).await
        },
        _ => false,
    };
    on_audit_entry(data, &sink, guild, entry, bot, actor_is_bot).await;
}

/// Whether `user` is a bot account: the cached member first, else one
/// bounded fetch from Discord. Unknown counts as a person, which only drops
/// the bot mark.
async fn is_bot_account(ctx: &Context, guild: GuildId, user: UserId) -> bool {
    let cached = ctx
        .cache
        .guild(guild)
        .and_then(|g| g.members.get(&user).map(|m| m.user.bot()));
    if let Some(bot) = cached {
        return bot;
    }
    match tokio::time::timeout(Duration::from_secs(3), ctx.http.get_user(user)).await {
        Ok(Ok(u)) => u.bot(),
        _ => false,
    }
}

/// A save went through: if the server's log is now on, seed its snapshots
/// in the background, so the first forced action is attributable. `/modlog`
/// and the dashboard call this after a successful save (M20). At most once
/// per server per minute (M26): a save inside that minute was stored, but
/// starts no fetch; the next flush then seeds what it reads.
pub fn after_save(data: &Data, http: Arc<Http>, guild: GuildId) {
    seed_after_save(data, Arc::new(LiveAudit { http }), guild);
}

/// [`after_save`] with the audit log passed in.
fn seed_after_save(
    data: &Data,
    audit: Arc<dyn AuditSource>,
    guild: GuildId,
) -> Option<JoinHandle<()>> {
    if !data.modlog.take_seed_owed(guild) {
        tracing::debug!(
            "modlog: {guild}: not seeded after the save: one ran less than a minute ago"
        );
        return None;
    }
    spawn_seed(data, audit, guild)
}

/// A `GuildCreate` for a server that already has a batch life is a gateway
/// re-identify: mark both kinds for a re-read (M31). Called at the very start
/// of `GuildCreate` handling, before any await: the handler's database reads
/// take time, during which a flush can already run, and it must find the
/// marks. [`ModLogRegistry::mark_reconnected`] asks for a read from `SKEW`
/// after now, so a fetch that began before cannot clear them.
pub fn mark_reconnected(data: &Data, guild: GuildId) {
    data.modlog.mark_reconnected(guild, Instant::now());
}

/// `GuildCreate`: read the server's stored settings, then seed if the log is
/// on; a failed read is retried once a minute later (see `load_and_seed`).
pub async fn on_guild_create(data: &Data, http: Arc<Http>, guild: GuildId) {
    let audit = Arc::new(LiveAudit { http });
    load_and_seed(data, audit, guild, stored_settings()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::messages::{MODLOG_DEAFENED, MODLOG_MUTED};
    use crate::messaging::render::Mentions;
    use crate::messaging::test_support::{FakeTransport, Op};
    use crate::modlog::attribution::AuditKind::{Disconnect, Move};
    use crate::modlog::batch::{DEBOUNCE, MAX_WAIT, SKEW};
    use crate::modlog::settings::ModLogSettings;
    use crate::modlog::test_support::{FakeAudit, FakeSink};
    use crate::DataInner;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::Mutex;

    const G: GuildId = GuildId::new(1);
    const OTHER: GuildId = GuildId::new(2);
    const BOT: UserId = UserId::new(99);
    const MOD: UserId = UserId::new(7);
    const T: UserId = UserId::new(8);
    const C1: ChannelId = ChannelId::new(101);
    const C2: ChannelId = ChannelId::new(102);
    const LOG: ChannelId = ChannelId::new(555);
    const LOG2: ChannelId = ChannelId::new(556);

    fn on() -> ModLogSettings {
        ModLogSettings {
            enabled: true,
            channel: Some(LOG),
            ..ModLogSettings::off(G)
        }
    }

    fn data_off() -> Data {
        Data(Arc::new(DataInner::default()))
    }

    /// The log on, set straight into the registry (enabling through
    /// `save_settings` needs a database).
    fn data_on() -> Data {
        let data = data_off();
        data.modlog.set_settings(on());
        data
    }

    fn entry(json: &str) -> AuditLogEntry {
        serde_json::from_str::<AuditLogEntry>(json).expect("an audit log entry")
    }

    /// A MEMBER_UPDATE by MOD on T with `changes`, and `reason` if any.
    fn member_update(changes: &str, reason: Option<&str>) -> AuditLogEntry {
        let reason = reason.map_or(String::new(), |r| format!(r#""reason": "{r}","#));
        entry(&format!(
            r#"{{"id": "900", "action_type": 24, "user_id": "{MOD}", "target_id": "{T}",
                {reason} "changes": [{changes}]}}"#
        ))
    }

    const MUTE_ON: &str = r#"{"key": "mute", "old_value": false, "new_value": true}"#;
    const DEAF_OFF: &str = r#"{"key": "deaf", "old_value": true, "new_value": false}"#;

    async fn posted(data: &Data, e: &AuditLogEntry) -> Vec<(GuildId, ModLogEvent)> {
        let sink = FakeSink::default();
        on_audit_entry(data, &sink, G, e, BOT, false).await;
        sink.posted()
    }

    fn mute(on: bool, reason: Option<&str>) -> ModLogEvent {
        ModLogEvent::ServerMute {
            target: T,
            actor: Actor::person(MOD),
            on,
            reason: reason.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn server_mute_entry_logs_exact_actor_target_and_reason() {
        let data = data_on();
        let e = member_update(MUTE_ON, Some("too loud"));
        assert_eq!(
            posted(&data, &e).await,
            vec![(G, mute(true, Some("too loud")))]
        );
    }

    /// Another bot's mute is logged like a moderator's, marked as a bot's.
    #[tokio::test]
    async fn a_bot_actor_is_carried_into_the_event() {
        let data = data_on();
        let sink = FakeSink::default();
        on_audit_entry(&data, &sink, G, &member_update(MUTE_ON, None), BOT, true).await;
        let posted = sink.posted();
        let [(_, ModLogEvent::ServerMute { actor, .. })] = posted.as_slice() else {
            panic!("{posted:?}");
        };
        assert_eq!(*actor, Actor::bot(MOD));
    }

    /// The bot's own entry for a mute, with `reason`.
    fn bot_mute(reason: &str) -> AuditLogEntry {
        entry(&format!(
            r#"{{"id": "901", "action_type": 24, "user_id": "{BOT}", "target_id": "{T}",
                "reason": "{reason}", "changes": [{MUTE_ON}]}}"#
        ))
    }

    #[tokio::test]
    async fn events_of_skips_the_shops_own_mute() {
        let data = data_on();
        assert!(posted(&data, &bot_mute("CrackTunes shop: mute"))
            .await
            .is_empty());
        // The same bot-authored entry with another reason is still logged.
        let other = posted(&data, &bot_mute("an admin command")).await;
        let [(_, ModLogEvent::ServerMute { actor, reason, .. })] = other.as_slice() else {
            panic!("{other:?}");
        };
        assert_eq!(actor.id, BOT);
        assert_eq!(reason.as_deref(), Some("an admin command"));
        // A moderator quoting the prefix is not the shop.
        let spoof = member_update(MUTE_ON, Some("CrackTunes shop: mute"));
        assert_eq!(posted(&data, &spoof).await.len(), 1);
    }

    #[tokio::test]
    async fn deafen_off_entry_logs_undeafen() {
        let data = data_on();
        let e = member_update(DEAF_OFF, None);
        assert_eq!(
            posted(&data, &e).await,
            vec![(
                G,
                ModLogEvent::ServerDeafen {
                    target: T,
                    actor: Actor::person(MOD),
                    on: false,
                    reason: None,
                }
            )]
        );
    }

    #[tokio::test]
    async fn one_entry_changing_mute_and_deaf_logs_both_in_order() {
        let data = data_on();
        let nick = r#"{"key": "nick", "old_value": "a", "new_value": "b"}"#;
        let e = member_update(&format!("{nick}, {MUTE_ON}, {DEAF_OFF}"), Some("r"));
        assert_eq!(
            posted(&data, &e).await,
            vec![
                (G, mute(true, Some("r"))),
                (
                    G,
                    ModLogEvent::ServerDeafen {
                        target: T,
                        actor: Actor::person(MOD),
                        on: false,
                        reason: Some("r".into()),
                    }
                ),
            ]
        );
    }

    #[tokio::test]
    async fn entries_for_a_disabled_guild_log_nothing() {
        let e = member_update(MUTE_ON, Some("r"));
        assert_eq!(posted(&data_off(), &e).await, vec![], "never configured");
        let configured_off = data_off();
        configure_off(&configured_off);
        assert_eq!(posted(&configured_off, &e).await, vec![], "configured off");
    }

    /// Configured, with a channel, but off.
    fn configure_off(data: &Data) {
        data.modlog.set_settings(ModLogSettings {
            channel: Some(LOG),
            ..ModLogSettings::off(G)
        });
    }

    #[tokio::test]
    async fn an_entry_without_actor_or_target_is_skipped() {
        let data = data_on();
        let without = [
            r#"{"id": "900", "action_type": 24, "target_id": "8", "changes": [CH]}"#,
            r#"{"id": "900", "action_type": 24, "user_id": "7", "changes": [CH]}"#,
            r#"{"id": "900", "action_type": 24, "user_id": "0", "target_id": "8", "changes": [CH]}"#,
            r#"{"id": "900", "action_type": 24, "user_id": "7", "target_id": "0", "changes": [CH]}"#,
        ];
        for json in without {
            let e = entry(&json.replace("CH", MUTE_ON));
            assert_eq!(posted(&data, &e).await, vec![], "{json}");
        }
        // Both present: logged, so the cases above are skipped for what
        // they lack.
        let both = r#"{"id": "900", "action_type": 24, "user_id": "7", "target_id": "8", "changes": [CH]}"#;
        assert_eq!(
            posted(&data, &entry(&both.replace("CH", MUTE_ON))).await,
            vec![(G, mute(true, None))]
        );
    }

    #[tokio::test]
    async fn other_actions_and_other_changes_log_nothing() {
        let data = data_on();
        // A role update carrying a mute change is no server mute.
        let role_update = entry(&format!(
            r#"{{"id": "900", "action_type": 25, "user_id": "{MOD}", "target_id": "{T}",
                "changes": [{MUTE_ON}]}}"#
        ));
        assert_eq!(posted(&data, &role_update).await, vec![]);
        for changes in [
            r#"{"key": "nick", "old_value": "a", "new_value": "b"}"#,
            r#"{"key": "mute", "old_value": true, "new_value": true}"#,
            r#"{"key": "deaf", "old_value": true}"#,
        ] {
            assert_eq!(
                posted(&data, &member_update(changes, None)).await,
                vec![],
                "{changes}"
            );
        }
        // No old value (Discord can omit it): the new one is the change.
        assert_eq!(
            posted(
                &data,
                &member_update(r#"{"key": "mute", "new_value": true}"#, None)
            )
            .await,
            vec![(G, mute(true, None))]
        );
    }

    /// A MEMBER_UPDATE by `actor` on `target` with one change.
    fn update_by(actor: UserId, target: UserId, change: &str) -> AuditLogEntry {
        entry(&format!(
            r#"{{"id": "900", "action_type": 24, "user_id": "{actor}", "target_id": "{target}",
                "changes": [{change}]}}"#
        ))
    }

    #[tokio::test]
    async fn only_crack_tunes_muting_itself_is_skipped() {
        let data = data_on();
        let deaf_on = r#"{"key": "deaf", "old_value": false, "new_value": true}"#;
        assert_eq!(
            posted(&data, &update_by(BOT, BOT, deaf_on)).await,
            vec![],
            "its own self-deafen (M21)"
        );
        assert_eq!(
            posted(&data, &update_by(MOD, BOT, deaf_on)).await,
            vec![(
                G,
                ModLogEvent::ServerDeafen {
                    target: BOT,
                    actor: Actor::person(MOD),
                    on: true,
                    reason: None,
                }
            )],
            "a moderator deafening CrackTunes"
        );
        assert_eq!(
            posted(&data, &update_by(BOT, T, MUTE_ON)).await,
            vec![(
                G,
                ModLogEvent::ServerMute {
                    target: T,
                    actor: Actor::person(BOT),
                    on: true,
                    reason: None,
                }
            )],
            "an admin command through the bot"
        );
    }

    #[test]
    fn only_a_leave_or_a_move_is_a_change() {
        assert_eq!(voice_change(None, None, T), None);
        assert_eq!(voice_change(None, Some(C1), T), None, "a join");
        assert_eq!(voice_change(Some(C1), Some(C1), T), None, "a mute in place");
        assert_eq!(
            voice_change(Some(C1), None, T),
            Some(Change::Left {
                target: T,
                from: C1
            })
        );
        assert_eq!(
            voice_change(Some(C1), Some(C2), T),
            Some(Change::Moved {
                target: T,
                from: C1,
                to: C2
            })
        );
    }

    /// `on_voice_state`'s path with fakes in place of the live audit log
    /// and channel.
    struct V {
        data: Data,
        audit: Arc<FakeAudit>,
        sink: Arc<FakeSink>,
        /// How many times the wiring was built.
        built: Cell<usize>,
    }

    impl V {
        fn new(data: Data) -> Self {
            Self {
                data,
                audit: Arc::default(),
                sink: Arc::default(),
                built: Cell::new(0),
            }
        }

        fn voice(&self, user: UserId, old: Option<ChannelId>, new: Option<ChannelId>) {
            feed(&self.data, G, user, old, new, Instant::now(), || {
                self.built.set(self.built.get() + 1);
                Wire {
                    audit: self.audit.clone(),
                    sink: self.sink.clone(),
                    bot: BOT,
                }
            });
        }

        fn pending(&self) -> Vec<Change> {
            self.data
                .modlog
                .batch_existing(G, |b| b.pending.iter().map(|(c, _)| c.clone()).collect())
                .unwrap_or_default()
        }

        /// Every kind of update.
        fn all_kinds(&self) {
            self.voice(T, Some(C1), None);
            self.voice(T, Some(C1), Some(C2));
            self.voice(T, Some(C1), Some(C1));
            self.voice(T, None, Some(C1));
            self.voice(BOT, Some(C1), None);
        }
    }

    async fn after(d: Duration) {
        tokio::time::advance(d).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_guild_records_nothing_and_fetches_nothing() {
        for (what, data) in [
            ("never configured", data_off()),
            ("configured off", {
                let d = data_off();
                configure_off(&d);
                d
            }),
        ] {
            let v = V::new(data);
            v.all_kinds();
            assert_eq!(v.built.get(), 0, "{what}: nothing built");
            assert!(!v.data.modlog.has_state(G), "{what}: nothing held");
            after(MAX_WAIT + Duration::from_secs(1)).await;
            assert_eq!(v.audit.calls(), vec![], "{what}: nothing fetched");
            assert_eq!(v.sink.posted(), vec![], "{what}: nothing posted");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn leaves_and_moves_are_batched_the_bots_too_and_nothing_else() {
        let v = V::new(data_on());
        v.all_kinds();
        assert_eq!(
            v.pending(),
            vec![
                Change::Left {
                    target: T,
                    from: C1
                },
                Change::Moved {
                    target: T,
                    from: C1,
                    to: C2
                },
                // CrackTunes' own leave consumes its audit unit (M8a).
                Change::Left {
                    target: BOT,
                    from: C1
                },
            ]
        );
        assert_eq!(v.built.get(), 3, "built only for the changes batched");
        after(DEBOUNCE).await;
        assert_eq!(v.audit.kinds(), vec![Disconnect, Move]);
    }

    #[tokio::test]
    async fn channel_sink_posts_only_while_enabled_and_to_the_configured_channel() {
        let data = data_off();
        let transport = Arc::new(FakeTransport::default());
        let sink = ChannelSink {
            data: data.clone(),
            transport: transport.clone(),
        };
        let e = mute(true, Some("loud"));

        sink.post(G, e.clone()).await;
        assert_eq!(transport.ops(), vec![], "never configured");
        configure_off(&data);
        sink.post(G, e.clone()).await;
        assert_eq!(transport.ops(), vec![], "configured off");
        // M3: on, but with no usable channel stored.
        data.modlog.set_settings(ModLogSettings {
            channel: None,
            ..on()
        });
        sink.post(G, e.clone()).await;
        assert_eq!(transport.ops(), vec![], "on without a channel");

        data.modlog.set_settings(on());
        sink.post(G, e.clone()).await;
        assert_eq!(transport.ops(), vec![Op::Send(LOG.get())]);
        assert_eq!(
            transport.texts(),
            vec![format!("<@{MOD}> {MODLOG_MUTED} <@{T}>")]
        );
        assert_eq!(
            transport.sent.lock().unwrap()[0].mentions,
            Mentions::None,
            "nobody pinged"
        );

        sink.post(OTHER, e.clone()).await;
        assert_eq!(transport.ops().len(), 1, "another server's log is off");

        // The channel is read at post time.
        data.modlog.set_settings(ModLogSettings {
            channel: Some(LOG2),
            ..on()
        });
        sink.post(
            G,
            ModLogEvent::ServerDeafen {
                target: T,
                actor: Actor::person(MOD),
                on: true,
                reason: None,
            },
        )
        .await;
        assert_eq!(
            transport.ops(),
            vec![Op::Send(LOG.get()), Op::Send(LOG2.get())]
        );
        assert_eq!(
            transport.texts()[1],
            format!("<@{MOD}> {MODLOG_DEAFENED} <@{T}>")
        );
    }

    /// M26: what `save_settings` does with the log on, then what its
    /// callers do (`after_save`): one seed per server per minute at most.
    #[tokio::test(start_paused = true)]
    async fn a_save_seeds_at_most_once_a_minute() {
        use crate::modlog::registry::RESUME_EVERY;
        let data = data_on();
        let audit = Arc::new(FakeAudit::default());
        let t0 = Instant::now();
        let save = |at: Instant| {
            data.modlog.settings_saved(G, at);
            seed_after_save(&data, audit.clone(), G)
        };
        save(t0).expect("the first save seeds").await.unwrap();
        assert_eq!(audit.kinds(), vec![Disconnect, Move]);
        let almost = t0 + RESUME_EVERY - Duration::from_millis(1);
        assert!(save(almost).is_none(), "a second save inside the minute");
        data.modlog.set_settings(ModLogSettings::off(G));
        data.modlog.set_settings(on());
        assert!(save(almost).is_none(), "off and on again inside it");
        assert_eq!(audit.calls().len(), 2);
        save(t0 + RESUME_EVERY).expect("a minute on").await.unwrap();
        assert_eq!(audit.calls().len(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn a_seed_is_spawned_only_for_a_server_with_the_log_on() {
        let audit = Arc::new(FakeAudit::default());
        let off = data_off();
        configure_off(&off);
        assert!(spawn_seed(&off, audit.clone(), G).is_none());
        assert_eq!(audit.calls(), vec![], "off: nothing fetched");

        let on = data_on();
        spawn_seed(&on, audit.clone(), G)
            .expect("a seed for a server with the log on")
            .await
            .unwrap();
        assert_eq!(audit.kinds(), vec![Disconnect, Move], "both kinds read");
        assert!(on.modlog.snapshots(G).is_some(), "the snapshots are held");
    }

    /// A settings read answering `script` in turn (a read past its end
    /// fails); a good read stores `then`. The second value counts reads.
    fn scripted(script: &[bool], then: ModLogSettings) -> (Load, Arc<AtomicUsize>) {
        let script = Arc::new(Mutex::new(script.iter().copied().collect::<VecDeque<_>>()));
        let reads = Arc::new(AtomicUsize::new(0));
        let counted = reads.clone();
        let load: Load = Arc::new(move |data: Data, guild: GuildId| {
            counted.fetch_add(1, SeqCst);
            let ok = script.lock().unwrap().pop_front().unwrap_or(false);
            let then = then.clone();
            Box::pin(async move {
                if ok {
                    data.modlog.set_settings(then);
                    data.modlog.mark_loaded(guild);
                }
                ok
            })
        });
        (load, reads)
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_settings_read_is_retried_a_minute_later_then_seeds() {
        let data = data_off();
        let audit = Arc::new(FakeAudit::default());
        let (load, reads) = scripted(&[false, true], on());
        let retry = load_and_seed(&data, audit.clone(), G, load)
            .await
            .expect("a failed read is retried");
        assert_eq!(reads.load(SeqCst), 1);
        after(LOAD_RETRY - Duration::from_millis(1)).await;
        assert_eq!(reads.load(SeqCst), 1, "not before a minute");
        assert_eq!(audit.calls(), vec![], "nothing seeded while unread");
        after(Duration::from_millis(1)).await;
        // Before awaiting the handle: a paused clock jumps ahead to any
        // sleeper once everything else is idle.
        assert_eq!(reads.load(SeqCst), 2, "read again at a minute");
        retry.await.unwrap();
        assert!(data.modlog.is_enabled(G));
        assert_eq!(audit.kinds(), vec![Disconnect, Move], "seeded once read");
    }

    #[tokio::test(start_paused = true)]
    async fn a_settings_read_is_retried_only_once() {
        let data = data_off();
        let audit = Arc::new(FakeAudit::default());
        let (load, reads) = scripted(&[false, false, true], on());
        let retry = load_and_seed(&data, audit.clone(), G, load)
            .await
            .expect("a failed read is retried");
        after(LOAD_RETRY).await;
        retry.await.unwrap();
        after(LOAD_RETRY * 10).await;
        assert_eq!(reads.load(SeqCst), 2, "one retry, no more");
        assert!(!data.modlog.is_enabled(G));
        assert_eq!(audit.calls(), vec![]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_read_seeds_nothing_nor_does_a_retry_that_reads_the_log_off() {
        // On from an earlier read: a seed now would fetch.
        let data = data_on();
        let audit = Arc::new(FakeAudit::default());
        let (load, reads) = scripted(&[false, true], ModLogSettings::off(G));
        let retry = load_and_seed(&data, audit.clone(), G, load)
            .await
            .expect("a failed read is retried");
        // Long enough for a seed to have fetched (it reads SKEW after it is
        // called).
        after(Duration::ZERO).await;
        after(SKEW).await;
        assert_eq!(audit.calls(), vec![], "a failed read seeds nothing");
        after(LOAD_RETRY).await;
        retry.await.unwrap();
        assert_eq!(reads.load(SeqCst), 2);
        assert!(!data.modlog.is_enabled(G));
        assert_eq!(audit.calls(), vec![], "read off: nothing seeded");
    }

    #[tokio::test(start_paused = true)]
    async fn a_good_read_seeds_after_the_skew_and_is_not_retried() {
        let data = data_off();
        let audit = Arc::new(FakeAudit::default());
        let (load, reads) = scripted(&[true], on());
        assert!(
            load_and_seed(&data, audit.clone(), G, load).await.is_none(),
            "no retry"
        );
        // A seed reads SKEW after it is called (M28a).
        after(Duration::ZERO).await;
        after(SKEW).await;
        assert_eq!(audit.kinds(), vec![Disconnect, Move]);
        after(LOAD_RETRY * 2).await;
        assert_eq!(reads.load(SeqCst), 1);
    }

    #[tokio::test]
    async fn without_a_database_the_stored_read_is_not_retried() {
        let retry = load_and_seed(
            &data_off(),
            Arc::new(FakeAudit::default()),
            G,
            stored_settings(),
        )
        .await;
        assert!(
            retry.is_none(),
            "no database: nothing to read, nothing failed"
        );
    }

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_failed_stored_read_schedules_a_retry(pool: sqlx::PgPool) {
        // A closed pool fails every read.
        pool.close().await;
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool),
            ..Default::default()
        }));
        let retry = load_and_seed(&data, Arc::new(FakeAudit::default()), G, stored_settings())
            .await
            .expect("a failed read is retried");
        assert!(!data.modlog.loaded(G));
        retry.abort();
    }
}
