//! Voice leaves and moves, batched per server, then matched against
//! Discord's audit log by [`attribute`] (spec 2.3). Only servers with the
//! mod log on hold any of this; for the rest every call returns at once.
//!
//! A change (re)arms the server's debounce: [`DEBOUNCE`] after the last
//! change, at most [`MAX_WAIT`] after the first. The debounce [`flush`]es:
//! it fetches the kinds the pending changes need, then takes the changes
//! (so those that arrived during the fetch join the batch: M8b), attributes
//! them against the snapshots, posts the events, and advances the snapshots
//! by what it fetched (M9/M10). One fetch runs per server at a time.
//!
//! What a fetch cannot vouch for is never diffed (M8b, M15): a kind whose
//! fetch failed, or both kinds after a 403 suspension, are re-read on the
//! next flush first (a seed) and attribute nothing until then.

use super::attribution::{advance, attribute, AuditKind, AuditRow, Change};
use super::audit::{AuditError, AuditSource};
use super::event::ModLogEvent;
use super::registry::{next_fetch, GuildBatch, ModLogRegistry};
use crate::Data;
use serenity::all::{GuildId, UserId};
use serenity::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

/// A flush waits this long after the last change of a burst.
pub const DEBOUNCE: Duration = Duration::from_secs(2);
/// ...and at most this long after the first.
pub const MAX_WAIT: Duration = Duration::from_secs(5);
/// A 403 pauses fetching for this long, or until the settings are saved.
pub const SUSPEND: Duration = Duration::from_secs(600);
/// Entries per fetch, per action type.
pub const FETCH_LIMIT: u8 = 10;
/// A fetch that takes longer counts as failed (serenity waits out a 429
/// rather than failing, and the server's one fetch slot must not stay taken).
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Both kinds, in the order they are fetched.
const BOTH: [AuditKind; 2] = [AuditKind::Disconnect, AuditKind::Move];

/// Where the events go: the server's log channel (`modlog::live`), or a
/// recording fake in tests.
#[async_trait]
pub trait ModLogSink: Send + Sync {
    async fn post(&self, guild: GuildId, e: ModLogEvent);
}

/// A sink that drops everything, for callers that never log.
pub struct NoopSink;

#[async_trait]
impl ModLogSink for NoopSink {
    async fn post(&self, _guild: GuildId, _e: ModLogEvent) {}
}

fn kind_of(c: &Change) -> AuditKind {
    match c {
        Change::Left { .. } => AuditKind::Disconnect,
        Change::Moved { .. } => AuditKind::Move,
    }
}

fn add_kind(kinds: &mut Vec<AuditKind>, k: AuditKind) {
    if !kinds.contains(&k) {
        kinds.push(k);
    }
}

/// Queue a voice change for attribution and (re)arm the debounce. `now` is
/// on tokio's clock. CrackTunes' own changes are queued too (M8a): they
/// consume their audit units and are never logged. Dropped for a server
/// with the log off (nothing is held), for a suspended one, and for a leave
/// the bot made itself ([`super::registry::ModLogRegistry::expect_disconnect`];
/// only a leave consumes that, never a move: M16).
pub fn record_change(
    data: &Data,
    audit: Arc<dyn AuditSource>,
    sink: Arc<dyn ModLogSink>,
    guild: GuildId,
    change: Change,
    now: Instant,
    bot: UserId,
) {
    let reg = &data.modlog;
    // Off: return before anything is looked up or held.
    if !reg.is_enabled(guild) {
        return;
    }
    if let Change::Left { target, .. } = &change {
        if reg.take_expected(guild, *target, now) {
            tracing::debug!(
                "modlog: {guild}: the bot's own disconnect of {target} is not attributed"
            );
            return;
        }
    }
    if reg.is_suspended(guild, now) {
        return;
    }
    let deadline = reg.batch_if_enabled(guild, |b| {
        b.pending.push((change, now));
        let first = b.pending.iter().map(|(_, at)| *at).min().unwrap_or(now);
        (now + DEBOUNCE).min(first + MAX_WAIT)
    });
    if let Some(deadline) = deadline {
        arm(data, audit, sink, guild, bot, deadline);
    }
}

/// Arm the server's debounce for `deadline`, replacing any armed one.
fn arm(
    data: &Data,
    audit: Arc<dyn AuditSource>,
    sink: Arc<dyn ModLogSink>,
    guild: GuildId,
    bot: UserId,
    deadline: Instant,
) {
    let (armed, is_armed) = tokio::sync::oneshot::channel::<()>();
    let for_timer = data.clone();
    let timer = tokio::spawn(async move {
        let data = for_timer;
        // Not a step before `set_debounce` below holds this task's handle,
        // so the release further down always finds it.
        if is_armed.await.is_err() {
            return;
        }
        tokio::time::sleep_until(deadline).await;
        // Out of the state unaborted: a change arriving during the flush
        // arms a new debounce, which would abort this one mid-flush.
        data.modlog.release_debounce(guild, tokio::task::id());
        let busy = flush_once(&data, audit.as_ref(), sink.as_ref(), guild, bot).await;
        // Not done (a fetch was running, or this one was outdated by a
        // save): the changes wait for the next debounce.
        if busy == Flushed::Busy && !data.modlog.debounce_armed(guild) {
            arm(&data, audit, sink, guild, bot, Instant::now() + DEBOUNCE);
        }
    });
    if data.modlog.set_debounce(guild, timer.abort_handle()) {
        let _ = armed.send(());
    } else {
        timer.abort();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flushed {
    /// Nothing was done: another fetch for the server was running, or this
    /// one was outdated by a settings save. Try again later.
    Busy,
    Done,
}

/// What a flush is about to fetch.
enum Claim {
    Busy,
    Idle,
    Go {
        epoch: u64,
        token: u64,
        kinds: Vec<AuditKind>,
        reseed: Vec<AuditKind>,
    },
}

/// What one round of fetches brought back.
#[derive(Default)]
struct Fetched {
    ok: Vec<AuditKind>,
    failed: Vec<AuditKind>,
    rows: Vec<AuditRow>,
    forbidden: bool,
    /// When each attempted kind's fetch started.
    started: Vec<(AuditKind, Instant)>,
    /// When each attempted kind's fetch returned (or gave up). A change
    /// after that may have a unit those rows cannot hold.
    returned: Vec<(AuditKind, Instant)>,
}

fn time_of(times: &[(AuditKind, Instant)], kind: AuditKind) -> Option<Instant> {
    times.iter().find(|(k, _)| *k == kind).map(|(_, at)| *at)
}

/// The server's fetch slot, taken. Dropped before [`Self::release`] (a
/// cancelled caller, a panic) it frees the slot, so the server is never
/// left waiting on a fetch that will not finish.
struct InFlight<'a> {
    reg: &'a ModLogRegistry,
    guild: GuildId,
    token: u64,
    held: bool,
}

impl<'a> InFlight<'a> {
    fn new(reg: &'a ModLogRegistry, guild: GuildId, token: u64) -> Self {
        Self {
            reg,
            guild,
            token,
            held: true,
        }
    }

    /// The slot was freed with the state update; nothing left to undo.
    fn release(mut self) {
        self.held = false;
    }
}

/// Free the slot if `token` still holds it.
fn free_slot(b: &mut GuildBatch, token: u64) {
    if b.in_flight == Some(token) {
        b.in_flight = None;
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if self.held {
            let token = self.token;
            self.reg.batch_existing(self.guild, |b| free_slot(b, token));
        }
    }
}

/// Fetch each kind in turn, [`FETCH_LIMIT`] entries. A 403 stops the round.
async fn fetch(audit: &dyn AuditSource, guild: GuildId, kinds: &[AuditKind]) -> Fetched {
    let mut f = Fetched::default();
    for &kind in kinds {
        f.started.push((kind, Instant::now()));
        let read = tokio::time::timeout(FETCH_TIMEOUT, audit.recent(guild, kind, FETCH_LIMIT))
            .await
            .unwrap_or_else(|_| Err(AuditError::Other("timed out".into())));
        f.returned.push((kind, Instant::now()));
        match read {
            Ok(rows) => {
                f.ok.push(kind);
                f.rows.extend(rows.into_iter().filter(|r| r.kind == kind));
            },
            Err(AuditError::Forbidden) => {
                f.forbidden = true;
                break;
            },
            Err(AuditError::Other(err)) => {
                tracing::warn!("modlog: {guild}: reading the {kind:?} audit log failed: {err}");
                f.failed.push(kind);
            },
        }
    }
    f
}

/// A 403: pause fetching for [`SUSPEND`], drop the batch, and re-read both
/// snapshots when fetching resumes.
fn suspend(b: &mut GuildBatch, guild: GuildId, now: Instant) {
    tracing::warn!(
        "modlog: {guild}: CrackTunes cannot read the audit log (View Audit Log); \
         fetching paused for {} s",
        SUSPEND.as_secs()
    );
    b.suspended_until = Some(now + SUSPEND);
    b.pending.clear();
    b.reseed = BOTH.to_vec();
    if let Some(h) = b.debounce.take() {
        h.abort();
    }
}

/// Fold a round's outcome into the snapshots: the kinds read are advanced
/// (seeded) and need no re-read; the kinds that failed do.
fn absorb(b: &mut GuildBatch, f: &Fetched) {
    advance(&mut b.snaps, &f.ok, &f.rows);
    b.reseed.retain(|k| !f.ok.contains(k));
    for k in &f.failed {
        add_kind(&mut b.reseed, *k);
    }
}

/// The debounced fire: attribute the pending changes and post the events.
/// Does nothing while another fetch for the server runs.
pub async fn flush(
    data: &Data,
    audit: &dyn AuditSource,
    sink: &dyn ModLogSink,
    guild: GuildId,
    bot: UserId,
) {
    flush_once(data, audit, sink, guild, bot).await;
}

async fn flush_once(
    data: &Data,
    audit: &dyn AuditSource,
    sink: &dyn ModLogSink,
    guild: GuildId,
    bot: UserId,
) -> Flushed {
    let reg = &data.modlog;
    let now = Instant::now();
    let claim = reg.batch_existing(guild, |b| {
        if b.in_flight.is_some() {
            return Claim::Busy;
        }
        if b.suspended(now) {
            b.pending.clear();
            return Claim::Idle;
        }
        // The kinds the changes need, and any awaiting a re-read: after a
        // suspension that is both, which makes this flush a seed (M8b).
        let kinds: Vec<AuditKind> = BOTH
            .into_iter()
            .filter(|k| b.reseed.contains(k) || b.pending.iter().any(|(c, _)| kind_of(c) == *k))
            .collect();
        if kinds.is_empty() {
            return Claim::Idle;
        }
        let token = next_fetch();
        b.in_flight = Some(token);
        Claim::Go {
            epoch: b.epoch,
            token,
            kinds,
            reseed: b.reseed.clone(),
        }
    });
    let (epoch, token, kinds, reseed) = match claim {
        None | Some(Claim::Idle) => return Flushed::Done,
        Some(Claim::Busy) => return Flushed::Busy,
        Some(Claim::Go {
            epoch,
            token,
            kinds,
            reseed,
        }) => (epoch, token, kinds, reseed),
    };

    let slot = InFlight::new(reg, guild, token);
    let fetched = fetch(audit, guild, &kinds).await;

    let now = Instant::now();
    let outcome = reg.batch_existing(guild, |b| {
        free_slot(b, token);
        if b.epoch != epoch {
            return None;
        }
        if fetched.forbidden {
            suspend(b, guild, now);
            return Some(Vec::new());
        }
        // Taken only now (M8b), so changes that arrived during the fetch
        // join this batch; but only those its rows can cover (M19): a change
        // of a kind fetched this round, that arrived before that kind's
        // fetch returned. The rest wait for the next round.
        let (taken, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut b.pending)
            .into_iter()
            .partition(|(c, at)| time_of(&fetched.returned, kind_of(c)).is_some_and(|r| *at <= r));
        b.pending = rest;
        // A kind attributes only when it was read and needs no re-read; the
        // changes of any other kind are dropped (M15).
        let attributable = |k: AuditKind| fetched.ok.contains(&k) && !reseed.contains(&k);
        let taken_count = taken.len();
        let changes: Vec<Change> = taken
            .into_iter()
            .map(|(c, _)| c)
            .filter(|c| attributable(kind_of(c)))
            .collect();
        if changes.len() < taken_count {
            tracing::debug!(
                "modlog: {guild}: {} changes dropped: their audit log was not read cleanly",
                taken_count - changes.len()
            );
        }
        let rows: Vec<AuditRow> = fetched
            .rows
            .iter()
            .filter(|r| attributable(r.kind))
            .cloned()
            .collect();
        let events = attribute(&b.snaps, &rows, &changes, bot);
        absorb(b, &fetched);
        Some(events)
    });
    slot.release();
    match outcome {
        None => {
            tracing::debug!("modlog: {guild}: state forgotten during a fetch; nothing posted");
            Flushed::Done
        },
        Some(None) => {
            tracing::debug!(
                "modlog: {guild}: a fetch was outdated (settings saved, or the log turned off \
                 and on); it is discarded"
            );
            Flushed::Busy
        },
        Some(Some(events)) => {
            for e in events {
                sink.post(guild, e).await;
            }
            Flushed::Done
        },
    }
}

/// Read both kinds into the snapshots, so the first forced action after the
/// log becomes active is attributable. For a server with the log on that is
/// not suspended and has no fetch running (a running flush keeps the
/// snapshots itself). Pending changes that arrived before their kind was
/// read are dropped: the seed took in their units.
pub async fn seed(data: &Data, audit: &dyn AuditSource, guild: GuildId) {
    let reg = &data.modlog;
    let now = Instant::now();
    let claim = reg.batch_if_enabled(guild, |b| {
        if b.in_flight.is_some() || b.suspended(now) {
            return None;
        }
        let token = next_fetch();
        b.in_flight = Some(token);
        Some((b.epoch, token))
    });
    let Some(Some((epoch, token))) = claim else {
        tracing::debug!(
            "modlog: {guild}: not seeded: the log is off, a fetch is running or fetching is paused"
        );
        return;
    };
    let slot = InFlight::new(reg, guild, token);
    let fetched = fetch(audit, guild, &BOTH).await;
    let now = Instant::now();
    reg.batch_existing(guild, |b| {
        free_slot(b, token);
        if b.epoch != epoch {
            return;
        }
        if fetched.forbidden {
            suspend(b, guild, now);
            return;
        }
        // A failed kind is marked for a re-read, which drops these anyway.
        b.pending
            .retain(|(c, at)| !time_of(&fetched.started, kind_of(c)).is_some_and(|s| *at < s));
        absorb(b, &fetched);
    });
    slot.release();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modlog::attribution::Snapshot;
    use crate::modlog::event::Attribution::{self, By};
    use crate::modlog::settings::ModLogSettings;
    use crate::modlog::test_support::{FakeAudit, FakeSink};
    use crate::modlog::{load_settings, save_settings};
    use crate::DataInner;
    use serenity::all::ChannelId;
    use AuditKind::{Disconnect, Move};

    const G: GuildId = GuildId::new(1);
    const BOT: UserId = UserId::new(99);
    const M1: UserId = UserId::new(1);
    const M2: UserId = UserId::new(2);
    const T1: UserId = UserId::new(11);
    const T2: UserId = UserId::new(12);
    const T3: UserId = UserId::new(13);
    const T4: UserId = UserId::new(14);
    const C1: ChannelId = ChannelId::new(101);
    const C2: ChannelId = ChannelId::new(102);
    const LOG: ChannelId = ChannelId::new(555);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn on() -> ModLogSettings {
        ModLogSettings {
            enabled: true,
            channel: Some(LOG),
            ..ModLogSettings::off(G)
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn dc(id: u64, actor: UserId, count: u32) -> AuditRow {
        AuditRow {
            id,
            kind: Disconnect,
            actor: Some(actor),
            target: None,
            count,
            channel: None,
        }
    }

    fn mv(id: u64, actor: UserId, count: u32, to: ChannelId) -> AuditRow {
        AuditRow {
            kind: Move,
            channel: Some(to),
            ..dc(id, actor, count)
        }
    }

    fn left(t: UserId) -> Change {
        Change::Left {
            target: t,
            from: C1,
        }
    }

    fn moved(t: UserId, to: ChannelId) -> Change {
        Change::Moved {
            target: t,
            from: C1,
            to,
        }
    }

    fn disc(t: UserId, by: Attribution) -> ModLogEvent {
        ModLogEvent::Disconnect {
            target: t,
            from: C1,
            by,
        }
    }

    fn mov(t: UserId, to: ChannelId, by: Attribution) -> ModLogEvent {
        ModLogEvent::Move {
            target: t,
            from: C1,
            to,
            by,
        }
    }

    /// Let spawned timers run as far as they can.
    async fn settle() {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    /// Move the paused clock on, then let the timers run.
    async fn after(d: Duration) {
        tokio::time::advance(d).await;
        settle().await;
    }

    struct W {
        data: Data,
        audit: Arc<FakeAudit>,
        sink: Arc<FakeSink>,
    }

    impl W {
        /// The mod log off.
        fn off() -> Self {
            Self::with(Data(Arc::new(DataInner::default())))
        }

        /// The mod log on (set straight into the registry: enabling through
        /// `save_settings` needs a database).
        fn new() -> Self {
            let w = Self::off();
            w.data.modlog.set_settings(on());
            w
        }

        fn with(data: Data) -> Self {
            Self {
                data,
                audit: Arc::default(),
                sink: Arc::default(),
            }
        }

        fn change(&self, c: Change) {
            record_change(
                &self.data,
                self.audit.clone(),
                self.sink.clone(),
                G,
                c,
                Instant::now(),
                BOT,
            );
        }

        async fn seed(&self) {
            seed(&self.data, self.audit.as_ref(), G).await;
        }

        async fn flush(&self) {
            flush(&self.data, self.audit.as_ref(), self.sink.as_ref(), G, BOT).await;
        }

        fn dc(&self, rows: Vec<AuditRow>) {
            self.audit.set(G, Disconnect, rows);
        }

        fn mv(&self, rows: Vec<AuditRow>) {
            self.audit.set(G, Move, rows);
        }

        /// The kinds fetched since `from` fetches had been made.
        fn kinds_since(&self, from: usize) -> Vec<AuditKind> {
            self.audit.kinds()[from..].to_vec()
        }

        fn fetches(&self) -> usize {
            self.audit.calls().len()
        }

        fn pending(&self) -> usize {
            self.data
                .modlog
                .batch_existing(G, |b| b.pending.len())
                .unwrap_or(0)
        }

        fn pending_changes(&self) -> Vec<Change> {
            self.data
                .modlog
                .batch_existing(G, |b| b.pending.iter().map(|(c, _)| c.clone()).collect())
                .unwrap_or_default()
        }

        fn reseed(&self) -> Vec<AuditKind> {
            self.data
                .modlog
                .batch_existing(G, |b| b.reseed.clone())
                .unwrap_or_default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_is_fetched_once_after_two_seconds() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T1));
        after(ms(1000)).await;
        w.change(left(T2));
        after(ms(1999)).await;
        assert_eq!(w.fetches(), s0, "nothing until 2 s after the last change");
        after(ms(1)).await;
        assert_eq!(
            w.audit.calls()[s0..],
            [(G, Disconnect, 10)],
            "one fetch, of the disconnect log, limit 10"
        );
        assert_eq!(
            w.sink.posted(),
            vec![(G, disc(T1, By(M1))), (G, disc(T2, By(M1)))]
        );
        after(secs(10)).await;
        assert_eq!(w.fetches(), s0 + 1, "the burst was fetched once");
    }

    #[tokio::test(start_paused = true)]
    async fn a_long_burst_flushes_by_five_seconds() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.change(left(T1));
        for t in [T2, T3, T4] {
            after(ms(1500)).await;
            w.change(left(t));
        }
        // 4.5 s in: the last change would wait until 6.5 s.
        after(ms(499)).await;
        assert_eq!(w.fetches(), s0, "not before 5 s after the first change");
        after(ms(1)).await;
        assert_eq!(w.kinds_since(s0), [Disconnect], "capped at 5 s");
        after(secs(10)).await;
        assert_eq!(w.fetches(), s0 + 1);
    }

    #[tokio::test(start_paused = true)]
    async fn only_the_kinds_present_are_fetched() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.change(moved(T1, C2));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Move]);
        w.change(left(T2));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0 + 1), [Disconnect]);
        w.change(left(T3));
        w.change(moved(T4, C2));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0 + 2), [Disconnect, Move]);
        assert!(w.audit.calls().iter().all(|(_, _, l)| *l == 10));
    }

    #[tokio::test(start_paused = true)]
    async fn seeded_snapshot_attributes_the_first_disconnect() {
        let w = W::new();
        // Long before: M1's aggregated entry already counts 3.
        w.dc(vec![dc(5, M1, 3)]);
        w.seed().await;
        assert_eq!(w.audit.kinds(), [Disconnect, Move], "a seed reads both");
        // M1 disconnects T1: the entry ticks up to 4.
        w.dc(vec![dc(5, M1, 4)]);
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert_eq!(w.sink.events(), vec![disc(T1, By(M1))]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unseeded_server_attributes_nothing_until_its_first_fetch() {
        let w = W::new();
        w.dc(vec![dc(5, M1, 3)]);
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert!(w.sink.events().is_empty(), "3 old units are not T1's");
        let snaps = w.data.modlog.snapshots(G).expect("held");
        assert!(snaps.disconnect.seeded, "the flush seeded what it read");
        w.dc(vec![dc(5, M1, 4)]);
        w.change(left(T2));
        after(DEBOUNCE).await;
        assert_eq!(w.sink.events(), vec![disc(T2, By(M1))]);
    }

    #[tokio::test(start_paused = true)]
    async fn forbidden_suspends_fetching() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        w.dc(vec![dc(1, M1, 1)]);
        w.change(left(T1));
        after(DEBOUNCE).await;
        let suspended_at = Instant::now();
        assert_eq!(w.fetches(), s0 + 1);
        assert!(w.sink.events().is_empty(), "the batch is dropped");
        assert!(w.data.modlog.is_suspended(G, suspended_at));
        // No fetch for ten minutes, whatever happens in voice.
        for _ in 0..9 {
            after(secs(60)).await;
            w.change(left(T2));
            w.change(moved(T3, C2));
            after(DEBOUNCE).await;
        }
        let last = suspended_at + SUSPEND - ms(1);
        tokio::time::advance(last - Instant::now()).await;
        w.change(left(T4));
        assert_eq!(w.pending(), 0, "dropped while suspended");
        after(secs(30)).await;
        assert_eq!(w.fetches(), s0 + 1, "nothing fetched for 10 min");
        // Ten minutes on: fetching resumes.
        assert!(!w.data.modlog.is_suspended(G, Instant::now()));
        w.change(left(T4));
        after(DEBOUNCE).await;
        assert!(w.fetches() > s0 + 1, "fetching resumed");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_settings_save_lifts_the_suspension(pool: sqlx::PgPool) {
        let w = W::with(Data(Arc::new(DataInner {
            database_pool: Some(pool),
            ..Default::default()
        })));
        assert!(load_settings(&w.data, G).await);
        assert_eq!(save_settings(&w.data, on()).await, Ok(()));
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        w.seed().await;
        assert!(w.data.modlog.is_suspended(G, Instant::now()));
        w.change(left(T1));
        assert_eq!(w.pending(), 0, "suspended: dropped");

        let epoch = |w: &W| w.data.modlog.batch_existing(G, |b| b.epoch);
        let before = epoch(&w);
        assert_eq!(save_settings(&w.data, on()).await, Ok(()));
        assert!(!w.data.modlog.is_suspended(G, Instant::now()), "lifted");
        assert_ne!(epoch(&w), before, "a new epoch: older fetches are void");
        assert_eq!(
            w.reseed(),
            [Disconnect, Move],
            "both snapshots are still re-read before anything is diffed"
        );
        w.change(left(T2));
        assert_eq!(w.pending(), 1, "changes are batched again");
    }

    #[tokio::test(start_paused = true)]
    async fn a_lapsed_suspension_reseeds_before_attributing() {
        let w = W::new();
        w.seed().await;
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        w.dc(vec![dc(1, M1, 1)]);
        w.change(left(T1));
        after(DEBOUNCE).await;
        after(SUSPEND).await;
        // T2 leaves on their own. The log still holds T1's unit, unseen: one
        // unit, one leave, and a diff would name M1.
        let s0 = w.fetches();
        w.change(left(T2));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Disconnect, Move], "a re-seed of both");
        assert!(w.sink.events().is_empty(), "T2 is not blamed for T1's unit");
        assert!(w.reseed().is_empty());
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T3));
        after(DEBOUNCE).await;
        assert_eq!(w.sink.events(), vec![disc(T3, By(M1))]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_fetch_drops_its_kind_and_reseeds_it() {
        let w = W::new();
        w.seed().await;
        w.dc(vec![dc(1, M1, 1)]);
        w.mv(vec![mv(2, M2, 1, C2)]);
        w.audit
            .fail_next(G, Disconnect, AuditError::Other("boom".into()));
        w.change(left(T1));
        w.change(moved(T2, C2));
        after(DEBOUNCE).await;
        assert_eq!(
            w.sink.events(),
            vec![mov(T2, C2, By(M2))],
            "the move is attributed; the leave is dropped"
        );
        assert_eq!(w.reseed(), [Disconnect]);
        // T3 leaves on their own; the log still holds T1's unseen unit.
        let s0 = w.fetches();
        w.change(left(T3));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert_eq!(w.sink.events().len(), 1, "T3 is not blamed for T1's unit");
        assert!(w.reseed().is_empty());
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T4));
        after(DEBOUNCE).await;
        assert_eq!(w.sink.events()[1..], [disc(T4, By(M1))]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_kind_awaiting_a_reseed_is_read_without_its_changes() {
        let w = W::new();
        w.seed().await;
        w.audit
            .fail_next(G, Disconnect, AuditError::Other("boom".into()));
        w.change(left(T1));
        after(DEBOUNCE).await;
        let s0 = w.fetches();
        w.mv(vec![mv(2, M2, 1, C2)]);
        w.change(moved(T2, C2));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Disconnect, Move]);
        assert_eq!(w.sink.events(), vec![mov(T2, C2, By(M2))]);
        assert!(w.reseed().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_seed_kind_waits_for_a_reseed() {
        let w = W::new();
        w.audit.fail_next(G, Move, AuditError::Other("boom".into()));
        w.seed().await;
        assert_eq!(w.reseed(), [Move]);
        let snaps = w.data.modlog.snapshots(G).expect("held");
        assert!(snaps.disconnect.seeded);
        assert_eq!(snaps.moves, Snapshot::default());
    }

    #[tokio::test(start_paused = true)]
    async fn an_expected_disconnect_is_consumed_not_logged() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.data.modlog.expect_disconnect(G, T1, Instant::now());
        w.dc(vec![dc(1, BOT, 1)]);
        w.change(left(T1));
        assert_eq!(w.pending(), 0, "the bot's own kick is not batched");
        after(DEBOUNCE * 2).await;
        assert_eq!(w.fetches(), s0, "nothing to fetch");
        assert!(w.sink.events().is_empty());
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert_eq!(
            w.kinds_since(s0),
            [Disconnect],
            "consumed: the next leave is batched"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_move_never_consumes_an_expected_disconnect() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.data.modlog.expect_disconnect(G, T1, Instant::now());
        w.change(moved(T1, C2));
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Move], "the leave consumed the marker");
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_guild_records_nothing() {
        let w = W::off();
        w.change(left(T1));
        w.change(moved(T2, C2));
        assert!(!w.data.modlog.has_state(G), "nothing held");
        w.seed().await;
        assert!(!w.data.modlog.has_state(G));
        after(secs(10)).await;
        assert!(w.audit.calls().is_empty(), "nothing fetched");
        assert!(w.sink.events().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn disabling_forgets_everything() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.data.modlog.expect_disconnect(G, T3, Instant::now());
        w.change(left(T1));
        assert_eq!(save_settings(&w.data, ModLogSettings::off(G)).await, Ok(()));
        assert!(!w.data.modlog.has_state(G));
        after(secs(10)).await;
        assert_eq!(w.fetches(), s0, "the armed debounce went with it");
    }

    #[tokio::test(start_paused = true)]
    async fn off_and_on_again_does_not_fire_the_old_debounce() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.change(left(T1)); // its debounce: t = 2
        after(ms(500)).await;
        assert_eq!(save_settings(&w.data, ModLogSettings::off(G)).await, Ok(()));
        w.data.modlog.set_settings(on());
        after(ms(500)).await;
        w.change(left(T2)); // t = 1: its debounce, t = 3
        after(ms(1999)).await;
        assert_eq!(w.fetches(), s0, "T1's debounce died with the disable");
        after(ms(1)).await;
        assert_eq!(w.kinds_since(s0), [Disconnect]);
    }

    #[tokio::test(start_paused = true)]
    async fn turning_the_log_off_by_any_path_forgets_its_state() {
        let w = W::new();
        w.change(left(T1));
        assert!(w.data.modlog.has_state(G));
        // Not through `save_settings`: `load_settings` reading an off row.
        w.data.modlog.set_settings(ModLogSettings::off(G));
        assert!(!w.data.modlog.has_state(G));
        after(DEBOUNCE).await;
        assert!(w.audit.calls().is_empty());
        w.seed().await;
        assert!(!w.data.modlog.has_state(G), "a seed holds nothing for it");
        assert!(w.audit.calls().is_empty());
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn loading_an_off_row_drops_the_state(pool: sqlx::PgPool) {
        let stored = ModLogSettings {
            channel: Some(LOG),
            ..ModLogSettings::off(G)
        };
        crate::db::modlog::save_settings(&pool, &stored)
            .await
            .unwrap();
        let w = W::with(Data(Arc::new(DataInner {
            database_pool: Some(pool),
            ..Default::default()
        })));
        w.data.modlog.set_settings(on());
        w.change(left(T1));
        assert!(w.data.modlog.has_state(G));
        assert!(load_settings(&w.data, G).await);
        assert!(!w.data.modlog.is_enabled(G));
        assert!(!w.data.modlog.has_state(G));
    }

    #[tokio::test(start_paused = true)]
    async fn a_fetch_from_before_a_disable_writes_nothing_after_it() {
        let w = W::new();
        w.seed().await;
        w.audit.set_delay(secs(1));
        w.dc(vec![dc(1, M1, 1)]);
        w.change(left(T1));
        after(DEBOUNCE).await;
        // Off and on again while that fetch runs.
        w.data.modlog.set_settings(ModLogSettings::off(G));
        w.data.modlog.set_settings(on());
        w.change(left(T2));
        after(ms(1000)).await;
        assert!(w.sink.events().is_empty());
        let snaps = w.data.modlog.snapshots(G).expect("the new state");
        assert!(!snaps.disconnect.seeded, "the old fetch did not seed it");
        assert_eq!(w.pending(), 1, "nor took its changes");
        assert!(!in_flight(&w), "nor holds its slot");
    }

    #[tokio::test(start_paused = true)]
    async fn an_outdated_fetch_never_frees_a_newer_ones_slot() {
        let w = W::new();
        w.seed().await;
        w.audit.set_delay(secs(3));
        w.change(left(T1));
        after(DEBOUNCE).await; // t = 2: the old fetch runs until 5
        after(ms(500)).await;
        w.data.modlog.set_settings(ModLogSettings::off(G));
        w.data.modlog.set_settings(on());
        w.change(left(T2)); // t = 2.5: its debounce, 4.5
        after(secs(2)).await; // t = 4.5: the new fetch runs until 7.5
        assert!(in_flight(&w));
        after(ms(500)).await; // t = 5: the old one returns
        assert!(in_flight(&w), "the new fetch still holds the slot");
    }

    #[tokio::test(start_paused = true)]
    async fn a_leave_after_its_kinds_fetch_returned_waits_for_the_next_round() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.dc(vec![dc(1, M1, 1)]);
        w.mv(vec![mv(2, M2, 1, C2)]);
        w.audit.set_delay(secs(1));
        w.change(left(T1));
        w.change(moved(T2, C2));
        after(DEBOUNCE).await; // t = 2: disconnects read until 3
        after(secs(1)).await; // t = 3: then moves, until 4
        after(ms(500)).await; // t = 3.5
                              // M1 disconnects T3 now, after the disconnect rows were read.
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T3));
        after(ms(500)).await; // t = 4: the round ends
        assert_eq!(w.kinds_since(s0), [Disconnect, Move]);
        assert_eq!(
            w.sink.events(),
            vec![disc(T1, By(M1)), mov(T2, C2, By(M2))],
            "T3 is not judged by rows read before it left"
        );
        assert_eq!(w.pending(), 1, "T3 waits");
        after(DEBOUNCE).await; // t = 6: T3's debounce fired at 5.5
        after(secs(1)).await; // t = 7
        assert_eq!(w.sink.events()[2..], [disc(T3, By(M1))]);
        // T4 leaves on their own: no unit is left over to pin on M1.
        w.change(left(T4));
        after(DEBOUNCE).await;
        after(secs(1)).await;
        assert_eq!(w.sink.events().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_seed_drops_the_changes_whose_units_it_read() {
        let w = W::new();
        // T1 and T3 were forced before the seed: their units are in the log.
        w.dc(vec![dc(1, M1, 1)]);
        w.mv(vec![mv(2, M1, 1, C2)]);
        w.change(left(T1));
        w.change(moved(T3, C2));
        after(ms(100)).await;
        w.audit.set_delay(secs(1));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let seeding = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await; // t = 0.1: disconnects are read until 1.1
        after(ms(500)).await; // t = 0.6
        w.change(left(T2)); // during that read: kept
        after(ms(500)).await; // t = 1.1: then moves, until 2.1
        after(secs(1)).await; // t = 2.1
        assert!(seeding.is_finished());
        assert_eq!(
            w.pending_changes(),
            vec![left(T2)],
            "T1 and T3 went into the seed"
        );
        // M2 had disconnected T2.
        w.audit.set_delay(Duration::ZERO);
        w.dc(vec![dc(1, M1, 1), dc(3, M2, 1)]);
        let s0 = w.fetches();
        after(ms(500)).await; // t = 2.6: T2's debounce
        assert_eq!(w.kinds_since(s0), [Disconnect], "nothing left of the move");
        assert_eq!(w.sink.events(), vec![disc(T2, By(M2))]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_save_outdates_a_running_fetch_and_its_403() {
        let w = W::new();
        w.seed().await;
        w.audit.set_delay(secs(1));
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        w.dc(vec![dc(1, M1, 1)]);
        w.change(left(T1));
        after(DEBOUNCE).await; // t = 2: the fetch runs until 3
        after(ms(500)).await;
        // What `save_settings` does when it enables the log.
        w.data.modlog.settings_saved(G);
        after(ms(500)).await; // t = 3: the 403, from before the save
        assert!(
            !w.data.modlog.is_suspended(G, Instant::now()),
            "a 403 from before the save does not suspend after it"
        );
        assert!(!in_flight(&w), "the slot is free");
        assert_eq!(w.pending(), 1, "T1 waits for a fetch after the save");
        after(DEBOUNCE).await; // t = 5: retried, read until 6
        after(secs(1)).await;
        assert_eq!(w.sink.events(), vec![disc(T1, By(M1))]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_save_outdates_a_running_seed_and_its_403() {
        let w = W::new();
        w.audit.set_delay(secs(1));
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let seeding = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await; // t = 0: the seed reads disconnects until 1
        after(ms(500)).await;
        w.data.modlog.settings_saved(G);
        after(ms(500)).await; // t = 1: the 403, from before the save
        assert!(seeding.is_finished());
        assert!(!w.data.modlog.is_suspended(G, Instant::now()));
        assert!(!in_flight(&w));
    }

    #[tokio::test(start_paused = true)]
    async fn a_seed_during_a_flush_fetches_nothing() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.audit.set_delay(secs(1));
        w.change(left(T1));
        after(DEBOUNCE).await;
        w.seed().await;
        assert_eq!(w.fetches(), s0 + 1, "only the flush's fetch");
        assert_eq!(w.audit.most_running(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_seed_while_suspended_fetches_nothing() {
        let w = W::new();
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        w.seed().await;
        assert!(w.data.modlog.is_suspended(G, Instant::now()));
        let s0 = w.fetches();
        w.seed().await;
        assert_eq!(w.fetches(), s0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_403_after_a_good_read_drops_the_whole_round() {
        let w = W::new();
        w.dc(vec![dc(1, M1, 1)]);
        w.seed().await;
        let before = w.data.modlog.snapshots(G).expect("seeded");
        let s0 = w.fetches();
        w.dc(vec![dc(1, M1, 2)]);
        w.mv(vec![mv(2, M2, 1, C2)]);
        w.audit.fail_next(G, Move, AuditError::Forbidden);
        w.change(left(T1));
        w.change(moved(T2, C2));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Disconnect, Move]);
        assert!(w.sink.events().is_empty(), "nothing from the good read");
        assert_eq!(
            w.data.modlog.snapshots(G).expect("held"),
            before,
            "nothing advanced"
        );
        assert_eq!(w.reseed(), [Disconnect, Move]);
        assert_eq!(w.pending(), 0);
        assert!(w.data.modlog.is_suspended(G, Instant::now()));
    }

    #[tokio::test(start_paused = true)]
    async fn the_bots_own_changes_are_batched() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        // A moderator disconnects CrackTunes.
        w.dc(vec![dc(1, M1, 1)]);
        w.change(left(BOT));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert!(w.sink.events().is_empty(), "never logged");
        // Its unit is spent: T1 leaving on their own is not blamed on M1.
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert!(w.sink.events().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn changes_arriving_during_the_fetch_join_its_batch() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.audit.set_delay(ms(500));
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Disconnect], "the fetch is running");
        // T2 is disconnected while it runs; the log it reads has both.
        w.change(left(T2));
        w.dc(vec![dc(1, M1, 2)]);
        after(ms(500)).await;
        assert_eq!(w.sink.events(), vec![disc(T1, By(M1)), disc(T2, By(M1))]);
        after(DEBOUNCE * 2).await;
        assert_eq!(w.fetches(), s0 + 1, "T2 went with that batch");
    }

    #[tokio::test(start_paused = true)]
    async fn one_fetch_in_flight_per_guild() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.audit.set_delay(secs(3));
        w.mv(vec![mv(2, M2, 1, C2)]);
        w.change(left(T1));
        after(DEBOUNCE).await; // t = 2: the disconnect fetch runs until 5
        w.change(moved(T2, C2));
        after(DEBOUNCE).await; // t = 4: T2's debounce finds it running
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert_eq!(w.audit.most_running(), 1);
        after(secs(1)).await; // t = 5: done; T2's kind was not fetched
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert_eq!(w.pending(), 1, "T2 waits");
        after(secs(1)).await; // t = 6: the next debounce
        assert_eq!(w.kinds_since(s0), [Disconnect, Move]);
        after(secs(3)).await;
        assert_eq!(w.audit.most_running(), 1, "never two at once");
        assert_eq!(w.sink.events(), vec![mov(T2, C2, By(M2))]);
    }

    fn in_flight(w: &W) -> bool {
        w.data
            .modlog
            .batch_existing(G, |b| b.in_flight.is_some())
            .unwrap_or(false)
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_flush_frees_the_fetch_slot() {
        let w = W::new();
        w.seed().await;
        w.audit.set_delay(secs(1));
        w.change(left(T1));
        let cut = tokio::time::timeout(ms(10), w.flush()).await;
        assert!(cut.is_err(), "the flush was dropped mid-fetch");
        assert!(!in_flight(&w), "the slot is free again");
        let seed_dropped = tokio::time::timeout(ms(10), w.seed()).await;
        assert!(seed_dropped.is_err());
        assert!(!in_flight(&w), "a dropped seed frees it too");
    }

    #[tokio::test(start_paused = true)]
    async fn a_hung_fetch_times_out_and_is_reseeded() {
        let w = W::new();
        w.seed().await;
        w.audit.set_delay(FETCH_TIMEOUT + secs(60));
        w.dc(vec![dc(1, M1, 1)]);
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert!(in_flight(&w));
        after(FETCH_TIMEOUT).await;
        assert!(!in_flight(&w), "given up");
        assert_eq!(w.reseed(), [Disconnect], "its kind is re-read next time");
        assert!(w.sink.events().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_direct_flush_while_one_runs_does_nothing() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.audit.set_delay(secs(3));
        w.change(left(T1));
        after(DEBOUNCE).await;
        w.flush().await;
        assert_eq!(w.fetches(), s0 + 1);
        assert_eq!(w.audit.most_running(), 1);
    }
}
