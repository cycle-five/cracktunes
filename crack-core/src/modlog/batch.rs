//! Voice leaves and moves, batched per server, then matched against
//! Discord's audit log by [`attribute`] (spec 2.3). Only servers with the
//! mod log on hold any of this; for the rest every call returns at once.
//!
//! A change (re)arms the server's debounce: [`DEBOUNCE`] after the last
//! change, at most [`MAX_WAIT`] after the first. The debounce [`flush`]es:
//! it fetches the kinds the pending changes need, waits [`GRACE`] (and
//! [`DECIDE_MARGIN`]), then attributes the changes against the snapshots,
//! posts the events, and advances the snapshots by what it fetched
//! (M9/M10). One fetch runs per server at a time.
//!
//! Discord reads its audit log somewhere inside a fetch, and a voice event
//! can land a little before its audit entry is written or a little after the
//! fetch's answer (M25). So a change that arrived from [`SKEW`] before a
//! kind's fetch started until [`GRACE`] after it returned may or may not be
//! in the rows: if a kind has any such change, that round attributes nothing
//! of the kind, absorbs its rows, drops its earlier changes and keeps the
//! overlapping ones for the next round. A kind with none is matched as
//! usual, from the changes that arrived before its window.
//!
//! What a fetch cannot vouch for is never diffed (M8b, M15, M28c): a kind
//! whose fetch failed, both kinds after a 403 suspension, and the
//! disconnects after a room kick's leave turned out to be a moderator's,
//! are re-read on the next flush first (a seed) and attribute nothing until
//! then.
//!
//! A [`seed`] reads [`SKEW`] after it is called (M28a). Every caller seeds
//! as recording (re)starts, and a change dropped just before then (the log
//! was off, or fetching paused) may have its unit written up to [`SKEW`]
//! later: an earlier read could miss it, and the unit would then wait for
//! whoever leaves next.

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
/// How long after a kind's fetch returns a voice event may still belong to a
/// unit those rows hold (the gateway can trail the REST answer). A flush
/// waits this long before it decides (M25).
pub const GRACE: Duration = Duration::from_secs(1);
/// How long before a kind's fetch starts a voice event may arrive whose
/// audit entry is not written yet (M25). A seed waits this long before it
/// reads (M28a).
pub const SKEW: Duration = Duration::from_millis(250);
/// How long after the window closes ([`GRACE`] after the return) the
/// decision runs (M28b). A voice event is stamped before its handler takes
/// the batch lock, so one stamped as the window closes may queue behind the
/// decision: this gives it time to land first. The window itself still
/// closes at [`GRACE`].
pub const DECIDE_MARGIN: Duration = Duration::from_millis(50);

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

fn target_of(c: &Change) -> UserId {
    match c {
        Change::Left { target, .. } | Change::Moved { target, .. } => *target,
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
        // save), or changes held back (M25): they wait for the next
        // debounce.
        let held = data
            .modlog
            .batch_existing(guild, |b| !b.pending.is_empty())
            .unwrap_or(false);
        if (busy == Flushed::Busy || held) && !data.modlog.debounce_armed(guild) {
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
    /// When each attempted kind's fetch returned (or gave up). Discord read
    /// the rows somewhere between the two (see [`Arrival`]).
    returned: Vec<(AuditKind, Instant)>,
}

fn time_of(times: &[(AuditKind, Instant)], kind: AuditKind) -> Option<Instant> {
    times.iter().find(|(k, _)| *k == kind).map(|(_, at)| *at)
}

/// When a change of a kind stands against that kind's fetch (M25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrival {
    /// Before the window: its unit, if it has one, is in the rows.
    Early,
    /// From [`SKEW`] before the fetch started to [`GRACE`] after it
    /// returned: its unit may or may not be in the rows.
    Overlap,
    /// After the window: its unit is not in the rows.
    Late,
    /// Its kind was not fetched this round.
    Unread,
}

/// Where the window of a fetch that started at `start` opens.
fn opens(start: Instant) -> Instant {
    start.checked_sub(SKEW).unwrap_or(start)
}

impl Fetched {
    fn arrival(&self, kind: AuditKind, at: Instant) -> Arrival {
        match (time_of(&self.started, kind), time_of(&self.returned, kind)) {
            (Some(start), Some(back)) => {
                if at < opens(start) {
                    Arrival::Early
                } else if at <= back + GRACE {
                    Arrival::Overlap
                } else {
                    Arrival::Late
                }
            },
            _ => Arrival::Unread,
        }
    }

    /// The instant the decision waits for: [`GRACE`] and [`DECIDE_MARGIN`]
    /// after the last kind that was read returned. None when nothing was
    /// read (nothing to match, so nothing to wait for).
    fn decide_at(&self) -> Option<Instant> {
        self.ok
            .iter()
            .filter_map(|k| time_of(&self.returned, *k))
            .max()
            .map(|back| back + GRACE + DECIDE_MARGIN)
    }
}

/// Signed milliseconds from `from` to `at`.
fn ms_from(at: Instant, from: Instant) -> i128 {
    if at >= from {
        (at - from).as_millis() as i128
    } else {
        -((from - at).as_millis() as i128)
    }
}

/// One debug line per round: each change against its kind's fetch, for
/// measuring the gateway's lag on a live server.
fn log_arrivals(guild: GuildId, f: &Fetched, pending: &[(Change, Instant)]) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let lines: Vec<String> = pending
        .iter()
        .filter_map(|(c, at)| {
            let k = kind_of(c);
            let start = time_of(&f.started, k)?;
            let back = time_of(&f.returned, k)?;
            Some(format!(
                "{k:?} {} {:+} ms from start, {:+} ms from return: {:?}",
                target_of(c),
                ms_from(*at, start),
                ms_from(*at, back),
                f.arrival(k, *at)
            ))
        })
        .collect();
    if !lines.is_empty() {
        tracing::debug!("modlog: {guild}: round arrivals: {}", lines.join("; "));
    }
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
    for k in BOTH {
        b.mark_reread(k, now);
    }
    if let Some(h) = b.debounce.take() {
        h.abort();
    }
}

/// Fold a round's outcome into the snapshots: the kinds read are advanced
/// (seeded), and a read clears a kind's re-read mark when its fetch started
/// no earlier than the mark asks (a mark set during the round outlives it:
/// M28c); the kinds that failed need a re-read, which any later read
/// satisfies.
fn absorb(b: &mut GuildBatch, f: &Fetched, now: Instant) {
    advance(&mut b.snaps, &f.ok, &f.rows);
    b.reseed.retain(|(k, from)| {
        !(f.ok.contains(k) && time_of(&f.started, *k).is_some_and(|s| s >= *from))
    });
    for k in &f.failed {
        b.mark_reread(*k, now);
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
            .filter(|k| b.awaits_reread(*k) || b.pending.iter().any(|(c, _)| kind_of(c) == *k))
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
        }
    });
    let (epoch, token, kinds) = match claim {
        None | Some(Claim::Idle) => return Flushed::Done,
        Some(Claim::Busy) => return Flushed::Busy,
        Some(Claim::Go {
            epoch,
            token,
            kinds,
        }) => (epoch, token, kinds),
    };

    let slot = InFlight::new(reg, guild, token);
    let fetched = fetch(audit, guild, &kinds).await;
    // M25: a voice event can trail the fetch's answer; give it GRACE so a
    // change whose unit the rows hold is seen before anything is decided,
    // and DECIDE_MARGIN so one stamped as the window closes has taken the
    // lock (M28b).
    if !fetched.forbidden {
        if let Some(at) = fetched.decide_at() {
            tokio::time::sleep_until(at).await;
        }
    }

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
        log_arrivals(guild, &fetched, &b.pending);
        // A kind read this round with a change around its fetch: whether
        // that change's unit is in the rows is unknown (M25).
        let overlapped: Vec<AuditKind> = BOTH
            .into_iter()
            .filter(|k| {
                fetched.ok.contains(k)
                    && b.pending.iter().any(|(c, at)| {
                        kind_of(c) == *k && fetched.arrival(*k, *at) == Arrival::Overlap
                    })
            })
            .collect();
        for k in &overlapped {
            tracing::debug!(
                "modlog: {guild}: {k:?}: voice activity overlapped the audit read; nothing \
                 of the kind is attributed this round"
            );
        }
        // Taken: the changes from before their kind's window (M8b, M19,
        // M25). The rest wait for the next round: overlapping ones, later
        // ones, and those of a kind not fetched.
        let (taken, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut b.pending)
            .into_iter()
            .partition(|(c, at)| fetched.arrival(kind_of(c), *at) == Arrival::Early);
        b.pending = rest;
        // A kind attributes only when it was read, needs no re-read (M15)
        // and nothing overlapped its read (M25); the taken changes of any
        // other kind are dropped. The re-read marks are read now, not when
        // the fetch started: one set during the round (M28c) holds it back.
        let ready: Vec<AuditKind> = BOTH
            .into_iter()
            .filter(|k| fetched.ok.contains(k) && !b.awaits_reread(*k) && !overlapped.contains(k))
            .collect();
        let attributable = |k: AuditKind| ready.contains(&k);
        let taken_count = taken.len();
        let changes: Vec<Change> = taken
            .into_iter()
            .map(|(c, _)| c)
            .filter(|c| attributable(kind_of(c)))
            .collect();
        if changes.len() < taken_count {
            tracing::debug!(
                "modlog: {guild}: {} changes dropped: their audit log was not read cleanly, or \
                 voice activity overlapped the read",
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
        // Every kind read is absorbed, overlapped or not: the units the rows
        // hold are spent either way.
        absorb(b, &fetched, now);
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

/// Whether a seed may fetch for the server now: no fetch running (a
/// running flush keeps the snapshots itself) and fetching not paused.
fn seedable(b: &GuildBatch, now: Instant) -> bool {
    b.in_flight.is_none() && !b.suspended(now)
}

/// Read both kinds into the snapshots, so the first forced action after the
/// log becomes active is attributable. For a server with the log on that is
/// not suspended and has no fetch running. It reads [`SKEW`] after it is
/// called (M28a; see the module docs). Pending changes that arrived before
/// their kind's window ([`SKEW`] before its fetch) are dropped: the seed
/// took in their units.
pub async fn seed(data: &Data, audit: &dyn AuditSource, guild: GuildId) {
    let reg = &data.modlog;
    let not_seeded = || {
        tracing::debug!(
            "modlog: {guild}: not seeded: the log is off, a fetch is running or fetching is paused"
        );
    };
    // Checked before the wait, so a server that cannot be seeded is not
    // waited on; and again as the slot is taken after it, since the log may
    // have been turned off, a flush started or a 403 paused fetching
    // meanwhile.
    // The batch life it was started for: a seed reads only for that life. If
    // the epoch moves during the wait, a resuming save bumped it and owes its
    // own seed (which waits its own SKEW), or a new life began: the first
    // flush of a new life seeds the kinds it reads, and the new life may
    // also get its own spawned seed. Reading now would be less than SKEW
    // after that restart, so a unit written in between could leak.
    let Some(Some(life)) =
        reg.batch_if_enabled(guild, |b| seedable(b, Instant::now()).then_some(b.epoch))
    else {
        not_seeded();
        return;
    };
    tokio::time::sleep(SKEW).await;
    let now = Instant::now();
    let claim = reg.batch_if_enabled(guild, |b| {
        if !seedable(b, now) || b.epoch != life {
            return None;
        }
        let token = next_fetch();
        b.in_flight = Some(token);
        Some((b.epoch, token))
    });
    let Some(Some((epoch, token))) = claim else {
        not_seeded();
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
        // The changes from before their kind's window went into the seed;
        // from the window on (M25) they wait for the next round, which
        // reads their units if the seed did not. A failed kind is marked for
        // a re-read, which drops its changes anyway.
        b.pending.retain(|(c, at)| {
            !time_of(&fetched.started, kind_of(c)).is_some_and(|s| *at < opens(s))
        });
        absorb(b, &fetched, now);
    });
    slot.release();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modlog::attribution::Snapshot;
    use crate::modlog::event::Actor;
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

    /// A change's whole round with no fetch delay: the debounce, then the
    /// grace and the margin before the decision.
    const ROUND: Duration = DEBOUNCE.saturating_add(GRACE).saturating_add(DECIDE_MARGIN);

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
            actor_is_bot: false,
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

    /// Move the paused clock on in one step, then let the timers run (a
    /// timer due inside the step fires at its end).
    async fn after(d: Duration) {
        tokio::time::advance(d).await;
        settle().await;
    }

    /// Let `d` pass with every timer firing at its own instant: the paused
    /// clock jumps from one due timer to the next while the test sleeps.
    async fn run(d: Duration) {
        tokio::time::sleep(d).await;
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
            self.change_at(c, Instant::now());
        }

        /// A change stamped as arriving at `at`.
        fn change_at(&self, c: Change, at: Instant) {
            record_change(
                &self.data,
                self.audit.clone(),
                self.sink.clone(),
                G,
                c,
                at,
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
                .batch_existing(G, |b| b.reseed.iter().map(|(k, _)| *k).collect())
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
        after(GRACE + DECIDE_MARGIN - ms(1)).await;
        assert!(
            w.sink.posted().is_empty(),
            "nothing is decided until GRACE and DECIDE_MARGIN after the fetch returned"
        );
        after(ms(1)).await;
        assert_eq!(
            w.sink.posted(),
            vec![
                (G, disc(T1, By(Actor::person(M1)))),
                (G, disc(T2, By(Actor::person(M1))))
            ]
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
        run(ROUND).await;
        assert_eq!(w.sink.events(), vec![disc(T1, By(Actor::person(M1)))]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unseeded_server_attributes_nothing_until_its_first_fetch() {
        let w = W::new();
        w.dc(vec![dc(5, M1, 3)]);
        w.change(left(T1));
        run(ROUND).await;
        assert!(w.sink.events().is_empty(), "3 old units are not T1's");
        let snaps = w.data.modlog.snapshots(G).expect("held");
        assert!(snaps.disconnect.seeded, "the flush seeded what it read");
        w.dc(vec![dc(5, M1, 4)]);
        w.change(left(T2));
        run(ROUND).await;
        assert_eq!(w.sink.events(), vec![disc(T2, By(Actor::person(M1)))]);
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

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_settings_save_lifts_the_suspension_once_a_minute(pool: sqlx::PgPool) {
        let w = W::with(Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        })));
        assert!(load_settings(&w.data, G).await);
        // On straight into the registry, so the saves below are the first.
        w.data.modlog.set_settings(on());
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

        // Paused again: a second save inside the minute is stored, but
        // resumes nothing (M26).
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        w.seed().await;
        assert!(w.data.modlog.is_suspended(G, Instant::now()));
        let before = epoch(&w);
        let elsewhere = ModLogSettings {
            channel: Some(ChannelId::new(556)),
            ..on()
        };
        assert_eq!(save_settings(&w.data, elsewhere.clone()).await, Ok(()));
        assert!(
            w.data.modlog.is_suspended(G, Instant::now()),
            "still paused"
        );
        assert_eq!(epoch(&w), before, "nothing outdated");
        assert_eq!(w.data.modlog.settings(G), elsewhere, "but saved");
        assert_eq!(
            crate::db::modlog::load_settings(&pool, G).await.unwrap(),
            Some(elsewhere)
        );
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
        run(ROUND).await;
        assert_eq!(w.kinds_since(s0), [Disconnect, Move], "a re-seed of both");
        assert!(w.sink.events().is_empty(), "T2 is not blamed for T1's unit");
        assert!(w.reseed().is_empty());
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T3));
        run(ROUND).await;
        assert_eq!(w.sink.events(), vec![disc(T3, By(Actor::person(M1)))]);
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
        run(ROUND).await;
        assert_eq!(
            w.sink.events(),
            vec![mov(T2, C2, By(Actor::person(M2)))],
            "the move is attributed; the leave is dropped"
        );
        assert_eq!(w.reseed(), [Disconnect]);
        // T3 leaves on their own; the log still holds T1's unseen unit.
        let s0 = w.fetches();
        w.change(left(T3));
        run(ROUND).await;
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert_eq!(w.sink.events().len(), 1, "T3 is not blamed for T1's unit");
        assert!(w.reseed().is_empty());
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T4));
        run(ROUND).await;
        assert_eq!(w.sink.events()[1..], [disc(T4, By(Actor::person(M1)))]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_kind_awaiting_a_reseed_is_read_without_its_changes() {
        let w = W::new();
        w.seed().await;
        w.audit
            .fail_next(G, Disconnect, AuditError::Other("boom".into()));
        w.change(left(T1));
        run(ROUND).await;
        let s0 = w.fetches();
        w.mv(vec![mv(2, M2, 1, C2)]);
        w.change(moved(T2, C2));
        run(ROUND).await;
        assert_eq!(w.kinds_since(s0), [Disconnect, Move]);
        assert_eq!(w.sink.events(), vec![mov(T2, C2, By(Actor::person(M2)))]);
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

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
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
    async fn a_leave_after_its_kinds_window_waits_for_the_next_round() {
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
        after(secs(1)).await; // t = 4: the disconnect window has closed
        after(ms(500)).await;
        // t = 4.5: M1 disconnects T3, after the disconnect rows were read.
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T3));
        // t = 5: T3's debounce finds the fetch still running.
        after(ms(500)).await;
        // t = 5.05: GRACE and DECIDE_MARGIN after the moves returned.
        after(DECIDE_MARGIN).await;
        assert_eq!(w.kinds_since(s0), [Disconnect, Move]);
        assert_eq!(
            w.sink.events(),
            vec![
                disc(T1, By(Actor::person(M1))),
                mov(T2, C2, By(Actor::person(M2)))
            ],
            "T3 is not judged by rows read before it left"
        );
        assert_eq!(w.pending_changes(), vec![left(T3)], "T3 waits");
        run(secs(10)).await;
        assert_eq!(w.sink.events()[2..], [disc(T3, By(Actor::person(M1)))]);
        // T4 leaves on their own: no unit is left over to pin on M1.
        w.change(left(T4));
        run(secs(10)).await;
        assert_eq!(w.sink.events().len(), 3);
    }

    /// A leave inside the disconnect window holds back the disconnects of
    /// that round, and only them: the moves are matched as usual.
    #[tokio::test(start_paused = true)]
    async fn an_overlapping_change_holds_back_only_its_kind() {
        let w = W::new();
        w.seed().await;
        w.dc(vec![dc(1, M1, 1)]);
        w.mv(vec![mv(2, M2, 1, C2)]);
        w.audit.set_delay(secs(1));
        w.change(left(T1));
        w.change(moved(T2, C2));
        after(DEBOUNCE).await; // t = 2: disconnects read until 3
        after(secs(1)).await; // t = 3: then moves, until 4
        after(ms(500)).await;
        // t = 3.5: M1 disconnects T3 after the disconnect rows were read,
        // but inside their window.
        w.dc(vec![dc(1, M1, 2)]);
        w.change(left(T3));
        after(ms(500)).await; // t = 4: moves returned
        after(GRACE).await; // t = 5: T3's debounce finds the fetch running
        after(DECIDE_MARGIN).await; // t = 5.05: decided
        assert_eq!(
            w.sink.events(),
            vec![mov(T2, C2, By(Actor::person(M2)))],
            "T1 is not matched in a round T3 overlapped"
        );
        assert_eq!(w.pending_changes(), vec![left(T3)], "T1 dropped, T3 waits");
        assert_eq!(
            w.data.modlog.snapshots(G).expect("held").disconnect.counts[&1],
            1,
            "the unit read went into the snapshot"
        );
        run(secs(10)).await;
        assert_eq!(
            w.sink.events()[1..],
            [disc(T3, By(Actor::person(M1)))],
            "the next round reads T3's unit"
        );
        w.change(left(T4));
        run(secs(10)).await;
        assert_eq!(w.sink.events().len(), 2, "T4 is not named");
    }

    /// The window's edges (M25): it opens SKEW before a fetch starts and
    /// closes GRACE after it returns, both edges inside it.
    #[tokio::test(start_paused = true)]
    async fn the_window_opens_skew_before_the_fetch_and_closes_grace_after_it() {
        // Four leaves keep the debounce going until the 5 s cap: the fetch
        // starts at 5, so the window opens at 4.75.
        for (last, matched) in [(ms(4750), false), (ms(4749), true)] {
            let w = W::new();
            w.seed().await;
            w.dc(vec![dc(1, M1, 4)]);
            w.change(left(T1));
            after(ms(1900)).await;
            w.change(left(T2));
            after(ms(1900)).await;
            w.change(left(T3));
            after(last - ms(3800)).await;
            w.change(left(T4));
            run(secs(10)).await;
            let want = if matched {
                [T1, T2, T3, T4]
                    .map(|t| disc(t, By(Actor::person(M1))))
                    .to_vec()
            } else {
                Vec::new()
            };
            assert_eq!(w.sink.events(), want, "the last leave at {last:?}");
            assert_eq!(w.pending(), 0, "a held-back leave gets a round of its own");
        }
        // No delay: the fetch returns at 2, so the window closes at 3.
        for (stamp, held_back) in [(secs(3), true), (ms(3001), false)] {
            let w = W::new();
            w.seed().await;
            let t0 = Instant::now();
            w.dc(vec![dc(1, M1, 1)]);
            w.change(left(T1));
            after(DEBOUNCE).await; // t = 2: fetched
            after(ms(500)).await;
            w.change_at(left(T2), t0 + stamp);
            after(ms(500)).await; // t = 3: the window closes
            assert!(w.sink.events().is_empty(), "not decided yet");
            after(DECIDE_MARGIN).await; // t = 3.05: decided
            let want = if held_back {
                Vec::new()
            } else {
                vec![disc(T1, By(Actor::person(M1)))]
            };
            assert_eq!(w.sink.events(), want, "T2 stamped at {stamp:?}");
            assert_eq!(w.pending_changes(), vec![left(T2)], "T2 waits either way");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_seed_drops_the_changes_whose_units_it_read() {
        let w = W::new();
        // T1 and T3 were forced well before the seed: their units are in
        // the log.
        w.dc(vec![dc(1, M1, 1)]);
        w.mv(vec![mv(2, M1, 1, C2)]);
        w.change(left(T1));
        w.change(moved(T3, C2));
        after(ms(250)).await; // t = 0.25: the seed is called; it reads SKEW later
        w.audit.set_delay(secs(1));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let seeding = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(ms(150)).await; // t = 0.4
        w.change(left(T4)); // within SKEW of the seed's fetch: kept
        after(ms(100)).await; // t = 0.5: disconnects are read until 1.5
        assert!(in_flight(&w));
        after(ms(500)).await; // t = 1
        w.change(left(T2)); // during that read: kept
        after(ms(500)).await; // t = 1.5: then moves, until 2.5
        after(secs(1)).await; // t = 2.5
        assert!(seeding.is_finished());
        assert_eq!(
            w.pending_changes(),
            vec![left(T4), left(T2)],
            "T1 and T3 went into the seed"
        );
        // M2 had disconnected T4 and T2, and the seed read neither unit.
        w.audit.set_delay(Duration::ZERO);
        w.dc(vec![dc(1, M1, 1), dc(3, M2, 2)]);
        let s0 = w.fetches();
        after(ms(500)).await; // t = 3: T2's debounce
        assert_eq!(w.kinds_since(s0), [Disconnect], "nothing left of the move");
        after(GRACE + DECIDE_MARGIN).await;
        assert_eq!(
            w.sink.events(),
            vec![
                disc(T2, By(Actor::person(M2))),
                disc(T4, By(Actor::person(M2)))
            ]
        );
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
        w.data.modlog.settings_saved(G, Instant::now());
        after(ms(500)).await; // t = 3: the 403, from before the save
        assert!(
            !w.data.modlog.is_suspended(G, Instant::now()),
            "a 403 from before the save does not suspend after it"
        );
        assert!(!in_flight(&w), "the slot is free");
        assert_eq!(w.pending(), 1, "T1 waits for a fetch after the save");
        after(DEBOUNCE).await; // t = 5: retried, read until 6
        after(secs(1)).await;
        after(GRACE + DECIDE_MARGIN).await; // t = 7.05: decided
        assert_eq!(w.sink.events(), vec![disc(T1, By(Actor::person(M1)))]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_save_outdates_a_running_seed_and_its_403() {
        let w = W::new();
        w.audit.set_delay(secs(1));
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let seeding = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(SKEW).await; // t = 0.25: the seed reads disconnects until 1.25
        assert!(in_flight(&w));
        after(ms(500)).await;
        w.data.modlog.settings_saved(G, Instant::now());
        after(ms(500)).await; // t = 1.25: the 403, from before the save
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
        let t = Instant::now();
        w.seed().await;
        assert_eq!(w.fetches(), s0);
        assert_eq!(Instant::now(), t, "not waited on either");
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
        run(ROUND).await;
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert!(w.sink.events().is_empty(), "never logged");
        // Its unit is spent: T1 leaving on their own is not blamed on M1.
        w.change(left(T1));
        run(ROUND).await;
        assert_eq!(w.kinds_since(s0), [Disconnect, Disconnect]);
        assert!(w.sink.events().is_empty());
    }

    /// M25 replaces M8b's "changes during the fetch join its batch": the
    /// read may or may not hold such a change's unit, so it holds its kind
    /// back for a round instead.
    #[tokio::test(start_paused = true)]
    async fn a_change_during_the_fetch_holds_its_kind_back() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.audit.set_delay(ms(500));
        w.dc(vec![dc(1, M1, 1)]);
        w.change(left(T1));
        after(DEBOUNCE).await;
        assert_eq!(w.kinds_since(s0), [Disconnect], "the fetch is running");
        // T2 is disconnected while it runs; the log it reads has both.
        w.change(left(T2));
        w.dc(vec![dc(1, M1, 2)]);
        after(ms(500)).await; // the response
        after(GRACE + DECIDE_MARGIN).await; // decided
        assert!(w.sink.events().is_empty(), "{:?}", w.sink.events());
        assert_eq!(w.pending_changes(), vec![left(T2)], "T1 dropped, T2 waits");
        assert_eq!(
            w.data.modlog.snapshots(G).expect("held").disconnect.counts[&1],
            2,
            "both units went into the snapshot"
        );
        run(secs(10)).await;
        assert_eq!(w.fetches(), s0 + 2, "T2 had a round of its own");
        assert!(w.sink.events().is_empty(), "its unit was already spent");
        // T3 leaves on their own: no unit is left over to pin on M1.
        w.change(left(T3));
        run(secs(10)).await;
        assert!(w.sink.events().is_empty(), "{:?}", w.sink.events());
    }

    /// A held-back round still spends the units it read: T1's forced unit
    /// must not wait for T2, who left on their own during the fetch.
    #[tokio::test(start_paused = true)]
    async fn a_held_back_round_spends_the_units_it_read() {
        let w = W::new();
        w.seed().await;
        w.audit.set_delay(ms(500));
        w.dc(vec![dc(1, M1, 1)]); // M1 disconnected T1
        w.change(left(T1));
        after(DEBOUNCE).await; // t = 2: the fetch runs until 2.5
        after(ms(100)).await;
        w.change(left(T2)); // T2 leaves on their own during it
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "T2 is not named for T1's unit: {:?}",
            w.sink.events()
        );
        assert_eq!(w.pending(), 0);
    }

    /// P1 (final review): Discord reads the log when the request reaches
    /// it, and a voice event can land after the response. T2's unit is in
    /// the rows while T2's leave is not yet seen; a diff would pin it on T1,
    /// who left on their own.
    #[tokio::test(start_paused = true)]
    async fn a_unit_read_before_its_change_arrives_names_nobody() {
        let w = W::new();
        w.seed().await;
        w.audit.read_at_start();
        w.audit.set_delay(ms(50));
        w.change(left(T1)); // T1 leaves on their own
        after(ms(1990)).await;
        w.dc(vec![dc(7, M1, 1)]); // t = 1.99: M1 disconnects T2
        after(ms(10)).await; // t = 2: the fetch reads T2's unit
        after(ms(50)).await; // t = 2.05: the response is back
        w.change(left(T2)); // ...and T2's voice event lands after it
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "nobody is named: {:?}",
            w.sink.events()
        );
        assert_eq!(w.pending(), 0, "T2 was judged in the next round");
        // T2's unit went into the snapshot: T3 leaving later is not blamed.
        w.change(left(T3));
        run(secs(10)).await;
        assert!(w.sink.events().is_empty(), "{:?}", w.sink.events());
    }

    /// P2 (final review): T2's leave lands during the fetch, but Discord
    /// wrote T2's unit after it read the log. Taking T2 then would leave the
    /// unit for whoever leaves next, however much later.
    #[tokio::test(start_paused = true)]
    async fn a_unit_written_after_the_read_is_not_carried_forward() {
        let w = W::new();
        w.seed().await;
        w.audit.read_at_start();
        w.audit.set_delay(ms(300));
        w.change(left(T1)); // T1 leaves on their own
        after(secs(2)).await; // t = 2: the fetch reads an unchanged log
        after(ms(100)).await; // t = 2.1
        w.dc(vec![dc(7, M1, 1)]); // M1 disconnects T2 after the read
        w.change(left(T2)); // ...and T2's voice event beats the response
        after(ms(200)).await; // t = 2.3: the response
        run(secs(5)).await;
        assert_eq!(
            w.sink.events(),
            vec![disc(T2, By(Actor::person(M1)))],
            "T2 is matched in the next round, which reads its unit"
        );
        run(secs(3600)).await;
        w.change(left(T3)); // an hour later, T3 leaves on their own
        run(secs(10)).await;
        assert_eq!(
            w.sink.events(),
            vec![disc(T2, By(Actor::person(M1)))],
            "T3 is not named"
        );
    }

    /// E1 (re-review): M1 disconnects T1 while the log is off, and the log
    /// is turned on 50 ms later (`save_settings`, then `after_save`'s
    /// seed). Discord writes the entry 100 ms after the voice event, after a
    /// seed reading at once would have read. That unit must not wait for
    /// whoever leaves next.
    #[tokio::test(start_paused = true)]
    async fn a_leave_dropped_just_before_the_log_is_turned_on_leaves_no_unit_behind() {
        let w = W::off();
        w.audit.read_at_start();
        w.audit.set_delay(ms(100));
        w.change(left(T1)); // t = 0: dropped, the log is off
        assert_eq!(w.pending(), 0);
        after(ms(50)).await; // t = 0.05: the log is turned on
        w.data.modlog.set_settings(on());
        assert!(w.data.modlog.settings_saved(G, Instant::now()));
        assert!(w.data.modlog.take_seed_owed(G));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let seeding = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(ms(50)).await; // t = 0.1: T1's entry is written
        w.dc(vec![dc(7, M1, 1)]);
        run(secs(1)).await;
        assert!(seeding.is_finished());
        run(secs(3600)).await;
        w.audit.set_delay(Duration::ZERO);
        w.change(left(T2)); // an hour later T2 leaves on their own
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "T2 must not be named: {:?}",
            w.sink.events()
        );
    }

    /// E2 (re-review): as E1, but the leave is dropped because a 403 paused
    /// fetching, and a settings save lifts the pause 50 ms later.
    #[tokio::test(start_paused = true)]
    async fn a_leave_dropped_just_before_a_save_lifts_a_pause_leaves_no_unit_behind() {
        let w = W::new();
        w.seed().await;
        w.audit.fail_next(G, Disconnect, AuditError::Forbidden);
        w.change(left(T3));
        run(ROUND).await;
        assert!(w.data.modlog.is_suspended(G, Instant::now()));
        run(secs(30)).await;
        w.audit.read_at_start();
        w.audit.set_delay(ms(100));
        w.change(left(T1)); // dropped: paused
        assert_eq!(w.pending(), 0);
        after(ms(50)).await; // a save lifts the pause
        assert!(w.data.modlog.settings_saved(G, Instant::now()));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let seeding = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(ms(50)).await; // T1's entry is written
        w.dc(vec![dc(7, M1, 1)]);
        run(secs(1)).await;
        assert!(seeding.is_finished());
        run(secs(3600)).await;
        w.audit.set_delay(Duration::ZERO);
        w.change(left(T2));
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "T2 must not be named: {:?}",
            w.sink.events()
        );
    }

    /// X1 (re-review): T2's voice event is stamped inside the window, at
    /// exactly return + GRACE, but takes the batch lock just after that
    /// instant (its handler ran on another worker between `Instant::now()`
    /// and the lock). The decision must not have run yet.
    #[tokio::test(start_paused = true)]
    async fn a_change_stamped_as_the_window_closes_is_seen_by_the_decision() {
        let w = W::new();
        w.seed().await;
        let t0 = Instant::now();
        w.dc(vec![dc(7, M1, 1)]); // M1 disconnected T2; the entry is written
        w.change(left(T1)); // T1 leaves on their own
        after(DEBOUNCE).await; // t = 2: fetched (no delay); the window closes at 3
        after(secs(1)).await; // t = 3
        w.change_at(left(T2), t0 + secs(3));
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "T1 must not be named: {:?}",
            w.sink.events()
        );
    }

    /// M28c: a re-read marked while a round runs (a room kick's disconnect
    /// found its member gone after the round's fetch started) holds that
    /// round's disconnects back: its rows may hold the unit of the leave the
    /// kick took for its own.
    #[tokio::test(start_paused = true)]
    async fn a_reread_marked_during_a_round_holds_its_kind_back() {
        let w = W::new();
        w.seed().await;
        w.audit.read_at_start();
        w.audit.set_delay(secs(1));
        w.change(left(T1)); // t = 0: T1 leaves on their own
        after(ms(1900)).await; // t = 1.9: a room kick of T2 is on its way
        w.data.modlog.expect_disconnect(G, T2, Instant::now());
        w.dc(vec![dc(7, M1, 1)]); // ...when M1 disconnects T2
        w.change(left(T2)); // T2's leave is taken for the kick's
        assert_eq!(w.pending(), 1);
        after(ms(100)).await; // t = 2: the round reads M1's unit, until 3
        after(ms(500)).await; // t = 2.5: the kick's disconnect finds T2 gone
        assert!(!w.data.modlog.clear_expected(G, T2));
        w.data.modlog.mark_reread(G, Disconnect, Instant::now());
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "T1 must not be named: {:?}",
            w.sink.events()
        );
        assert_eq!(
            w.reseed(),
            [Disconnect],
            "the mark outlives the round: its read started before the mark"
        );
    }

    /// M28c: a re-read mark is cleared only by a read that started SKEW
    /// after it. The unit of the leave the kick took for its own is written
    /// by then, and not necessarily before.
    #[tokio::test(start_paused = true)]
    async fn a_reread_mark_is_cleared_only_by_a_read_started_skew_after_it() {
        let w = W::new();
        w.seed().await;
        w.audit.read_at_start();
        w.change(left(T1)); // t = 0: T1 leaves on their own; read at 2
        after(ms(1900)).await; // t = 1.9: a room kick of T2 is on its way
        w.data.modlog.expect_disconnect(G, T2, Instant::now());
        w.change(left(T2)); // M1 disconnects T2: taken for the kick's
        after(ms(50)).await; // t = 1.95: the kick's disconnect finds T2 gone
        assert!(!w.data.modlog.clear_expected(G, T2));
        w.data.modlog.mark_reread(G, Disconnect, Instant::now());
        after(ms(50)).await; // t = 2: the round reads, before M1's entry
        after(ms(100)).await; // t = 2.1: M1's entry is written
        w.dc(vec![dc(7, M1, 1)]);
        run(secs(5)).await;
        assert!(w.sink.events().is_empty(), "{:?}", w.sink.events());
        assert_eq!(
            w.reseed(),
            [Disconnect],
            "a read started 50 ms after the mark leaves it"
        );
        run(secs(3600)).await;
        w.change(left(T3)); // an hour later T3 leaves on their own
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "T3 must not be named: {:?}",
            w.sink.events()
        );
        assert!(w.reseed().is_empty(), "that round's read cleared it");
        // And the next forced disconnect is matched again.
        w.dc(vec![dc(7, M1, 2)]);
        w.change(left(T4));
        run(secs(10)).await;
        assert_eq!(w.sink.events(), vec![disc(T4, By(Actor::person(M1)))]);
    }

    /// M28a: a seed checks again after its wait. A flush that took the slot
    /// meanwhile keeps the snapshots itself, so the seed reads nothing.
    #[tokio::test(start_paused = true)]
    async fn a_seed_reads_nothing_if_a_flush_starts_during_its_wait() {
        let w = W::new();
        w.seed().await;
        let s0 = w.fetches();
        w.audit.set_delay(secs(1));
        w.change(left(T1)); // its debounce: t = 2
        after(ms(1900)).await; // t = 1.9: a seed is called; it would read at 2.15
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let seeding = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        run(secs(5)).await;
        assert!(seeding.is_finished());
        assert_eq!(w.kinds_since(s0), [Disconnect], "only the flush's fetch");
        assert_eq!(w.audit.most_running(), 1);
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
        after(ms(500)).await;
        w.change(moved(T2, C2)); // t = 2.5: its debounce, 4.5
        after(DEBOUNCE).await; // t = 4.5: T2's debounce finds it running
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert_eq!(w.audit.most_running(), 1);
        after(ms(500)).await; // t = 5: returned
        after(GRACE + DECIDE_MARGIN).await; // t = 6.05: decided; T2's kind was not fetched
        assert_eq!(w.kinds_since(s0), [Disconnect]);
        assert_eq!(w.pending(), 1, "T2 waits");
        after(ms(450)).await; // t = 6.5: the next debounce
        assert_eq!(w.kinds_since(s0), [Disconnect, Move]);
        run(secs(5)).await;
        assert_eq!(w.audit.most_running(), 1, "never two at once");
        assert_eq!(w.sink.events(), vec![mov(T2, C2, By(Actor::person(M2)))]);
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
        // Cut mid-fetch: a seed reads SKEW after it is called.
        let before = w.fetches();
        let seed_dropped = tokio::time::timeout(SKEW + ms(10), w.seed()).await;
        assert!(seed_dropped.is_err());
        assert!(w.fetches() > before, "the seed's fetch had started");
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

    // ---- regression tests: a seed reads only for the life it was started for (M28) ----

    /// P1: GuildCreate's seed S1 is waiting its SKEW when the log is turned
    /// off, a moderator disconnects T1 (dropped), and the log is turned on
    /// again (a save that resumes and owes seed S2). Before the fix S1 woke
    /// and seeded the new life with a read SKEW-earlier than S2's, so T1's
    /// unit, written after that read, looked new to the next flush and
    /// T2's own leave was named as M1's. Now S1 sees the life changed and
    /// reads nothing; S2 seeds the new life, and T2 is not named.
    #[tokio::test(start_paused = true)]
    async fn seed_wait_p1_off_and_on_during_a_seeds_wait() {
        let w = W::new(); // GuildCreate read "on"
        w.audit.read_at_start();
        w.audit.set_delay(ms(300));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let s1 = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await }); // t = 0
        settle().await;
        after(ms(50)).await; // t = 0.05: off
        w.data.modlog.set_settings(ModLogSettings::off(G));
        after(ms(50)).await; // t = 0.1: M1 disconnects T1, dropped
        w.change(left(T1));
        assert_eq!(w.pending(), 0);
        after(ms(50)).await; // t = 0.15: on again
        w.data.modlog.set_settings(on());
        assert!(w.data.modlog.settings_saved(G, Instant::now()));
        assert!(w.data.modlog.take_seed_owed(G));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let s2 = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(ms(100)).await; // t = 0.25: S1 reads
        after(ms(50)).await; // t = 0.3: T1's entry is written (200 ms after, < SKEW)
        w.dc(vec![dc(7, M1, 1)]);
        run(secs(2)).await;
        assert!(s1.is_finished() && s2.is_finished());
        run(secs(3600)).await;
        w.audit.set_delay(Duration::ZERO);
        w.change(left(T2));
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "P1: T2 must not be named: {:?}",
            w.sink.events()
        );
    }

    /// P2: the log is turned on (resume + seed S1), off 50 ms later, a
    /// moderator disconnects T1 (dropped), and on again inside the resume
    /// minute (no seed owed). S1 wakes and seeds the new life.
    #[tokio::test(start_paused = true)]
    async fn seed_wait_p2_on_off_on_inside_the_resume_minute() {
        let w = W::off();
        w.audit.read_at_start();
        w.audit.set_delay(ms(100));
        w.data.modlog.set_settings(on()); // t = 0
        assert!(w.data.modlog.settings_saved(G, Instant::now()));
        assert!(w.data.modlog.take_seed_owed(G));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let s1 = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(ms(50)).await; // t = 0.05: off
        w.data.modlog.set_settings(ModLogSettings::off(G));
        after(ms(50)).await; // t = 0.1: dropped
        w.change(left(T1));
        assert_eq!(w.pending(), 0);
        after(ms(50)).await; // t = 0.15: on again, inside the minute
        w.data.modlog.set_settings(on());
        assert!(!w.data.modlog.settings_saved(G, Instant::now()));
        assert!(!w.data.modlog.take_seed_owed(G));
        after(ms(100)).await; // t = 0.25: S1 reads
        after(ms(50)).await; // t = 0.3: T1's entry is written
        w.dc(vec![dc(7, M1, 1)]);
        run(secs(2)).await;
        assert!(s1.is_finished());
        run(secs(3600)).await;
        w.audit.set_delay(Duration::ZERO);
        w.change(left(T2));
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "P2: T2 must not be named: {:?}",
            w.sink.events()
        );
    }

    /// P3: a GuildCreate seed waiting its SKEW while a save (log already on,
    /// nothing dropped) resumes: nothing is lost, and the next forced
    /// disconnect is still attributed (liveness control for any epoch guard).
    #[tokio::test(start_paused = true)]
    async fn seed_wait_p3_a_save_during_a_seeds_wait_still_seeds() {
        let w = W::new();
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let s1 = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(ms(100)).await;
        assert!(w.data.modlog.settings_saved(G, Instant::now()));
        assert!(w.data.modlog.take_seed_owed(G));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let s2 = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        run(secs(2)).await;
        assert!(s1.is_finished() && s2.is_finished());
        assert!(
            w.data.modlog.snapshots(G).unwrap().disconnect.seeded,
            "seeded"
        );
        w.dc(vec![dc(7, M1, 1)]);
        w.change(left(T2));
        run(secs(10)).await;
        assert_eq!(w.sink.events(), vec![disc(T2, By(Actor::person(M1)))]);
    }

    /// P4: a kick mark set while a seed's fetch is in flight survives it
    /// (the seed started before the mark's instant) and holds the next
    /// round's disconnects back.
    #[tokio::test(start_paused = true)]
    async fn seed_wait_p4_a_mark_set_during_a_seed_outlives_it() {
        let w = W::new();
        w.audit.read_at_start();
        w.audit.set_delay(ms(300));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let s1 = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(SKEW).await; // the seed reads disconnects, until 0.55
        after(ms(100)).await; // t = 0.35: a kick's leave was a moderator's
        w.data.modlog.mark_reread(G, Disconnect, Instant::now());
        after(ms(50)).await; // its entry is written after the seed read
        w.dc(vec![dc(7, M1, 1)]);
        run(secs(2)).await;
        assert!(s1.is_finished());
        assert_eq!(w.reseed(), [Disconnect], "the seed did not clear it");
        run(secs(3600)).await;
        w.audit.set_delay(Duration::ZERO);
        w.change(left(T2));
        run(secs(10)).await;
        assert!(w.sink.events().is_empty(), "P4: {:?}", w.sink.events());
        assert!(w.reseed().is_empty());
    }

    // ---- M31: a reconnect absorbs what happened while the bot was away ----

    /// A gateway re-identify (GuildCreate) in a server with a seeded life:
    /// a moderator disconnected T1 during the outage, so its unit is in the
    /// audit log but the bot saw no voice event. A flush that runs before
    /// the reconnect's own seed claims the slot must not pair that unit
    /// with T2's voluntary leave.
    #[tokio::test(start_paused = true)]
    async fn a_reconnect_absorbs_the_outages_units_before_its_seed_claims() {
        let w = W::new();
        w.seed().await; // before the outage: nothing in the audit log
        run(secs(5)).await;
        w.dc(vec![dc(7, M1, 1)]); // the outage: M1 disconnects T1, unseen
        w.data.modlog.mark_reconnected(G, Instant::now()); // GuildCreate
        assert_eq!(w.reseed(), [Disconnect, Move]);
        w.change(left(T2)); // T2 leaves on their own, before the seed claims
        run(secs(10)).await;
        assert!(
            w.sink.events().is_empty(),
            "T2 must not be named: {:?}",
            w.sink.events()
        );
        assert!(w.reseed().is_empty(), "that round's read cleared the marks");
        // Back to normal: a forced disconnect is matched again.
        w.dc(vec![dc(8, M1, 1), dc(7, M1, 1)]);
        w.change(left(T3));
        run(secs(10)).await;
        assert_eq!(w.sink.events(), vec![disc(T3, By(Actor::person(M1)))]);
    }

    /// A GuildCreate for a server with no batch life changes nothing: no
    /// life is created, no mark is held.
    #[tokio::test(start_paused = true)]
    async fn a_reconnect_of_a_server_with_no_life_marks_nothing() {
        let w = W::off();
        assert!(!w.data.modlog.mark_reconnected(G, Instant::now()));
        assert!(!w.data.modlog.has_state(G));
        w.data.modlog.set_settings(on()); // enabled, but no life yet
        assert!(!w.data.modlog.mark_reconnected(G, Instant::now()));
        assert!(!w.data.modlog.has_state(G));
        // A first flush still seeds what it reads and attributes normally.
        w.dc(vec![dc(1, M1, 1)]);
        w.seed().await;
        w.dc(vec![dc(2, M1, 1), dc(1, M1, 1)]);
        w.change(left(T1));
        run(secs(10)).await;
        assert_eq!(w.sink.events(), vec![disc(T1, By(Actor::person(M1)))]);
    }

    /// The reconnect's marks are cleared only by a fetch that started at or
    /// after their `from` (now + SKEW): a seed already reading when the
    /// reconnect arrives leaves them.
    #[tokio::test(start_paused = true)]
    async fn the_reconnect_marks_survive_a_fetch_that_started_before_them() {
        let w = W::new();
        w.seed().await;
        w.audit.read_at_start();
        w.audit.set_delay(ms(300));
        let (data, audit) = (w.data.clone(), w.audit.clone());
        let s1 = tokio::spawn(async move { seed(&data, audit.as_ref(), G).await });
        settle().await;
        after(SKEW).await; // the seed reads, until 0.55
        after(ms(100)).await; // t = 0.35: the reconnect
        w.data.modlog.mark_reconnected(G, Instant::now());
        run(secs(2)).await;
        assert!(s1.is_finished());
        assert_eq!(w.reseed(), [Disconnect, Move], "that seed started before");
        // A seed called now reads SKEW later, at or after `from`: it clears.
        w.seed().await;
        assert!(w.reseed().is_empty());
    }
}
