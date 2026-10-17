//! Stand-ins for the audit log and the log channel: tests script what the
//! audit log answers and assert on every fetch and every post.
use super::attribution::{AuditKind, AuditRow};
use super::audit::{AuditError, AuditSource};
use super::batch::ModLogSink;
use super::event::ModLogEvent;
use serenity::all::GuildId;
use serenity::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Default)]
struct AuditInner {
    /// (guild, kind, limit) per fetch, in the order they started.
    calls: Vec<(GuildId, AuditKind, u8)>,
    /// What the audit log holds now, per (guild, kind).
    rows: HashMap<(GuildId, AuditKind), Vec<AuditRow>>,
    /// Failures for the next fetches of a (guild, kind), in order.
    failures: HashMap<(GuildId, AuditKind), VecDeque<AuditError>>,
    /// How long each fetch takes (on tokio's clock).
    delay: Duration,
    running: usize,
    most_running: usize,
}

/// A scripted audit log. A fetch answers the rows set for its (guild,
/// kind) when it ends (after [`Self::set_delay`]), or the next queued
/// failure.
#[derive(Default)]
pub struct FakeAudit {
    inner: Mutex<AuditInner>,
}

impl FakeAudit {
    /// The audit log of `kind` in `g` now holds `rows`.
    pub fn set(&self, g: GuildId, kind: AuditKind, rows: Vec<AuditRow>) {
        self.inner.lock().unwrap().rows.insert((g, kind), rows);
    }

    /// The next fetch of `kind` in `g` fails with `e` (queued per kind).
    pub fn fail_next(&self, g: GuildId, kind: AuditKind, e: AuditError) {
        self.inner
            .lock()
            .unwrap()
            .failures
            .entry((g, kind))
            .or_default()
            .push_back(e);
    }

    /// Every fetch from now on takes `d`.
    pub fn set_delay(&self, d: Duration) {
        self.inner.lock().unwrap().delay = d;
    }

    /// Every fetch started so far: (guild, kind, limit).
    pub fn calls(&self) -> Vec<(GuildId, AuditKind, u8)> {
        self.inner.lock().unwrap().calls.clone()
    }

    /// The kinds fetched so far, in order.
    pub fn kinds(&self) -> Vec<AuditKind> {
        self.calls().into_iter().map(|(_, k, _)| k).collect()
    }

    /// The most fetches that ever ran at once.
    pub fn most_running(&self) -> usize {
        self.inner.lock().unwrap().most_running
    }
}

#[async_trait]
impl AuditSource for FakeAudit {
    async fn recent(
        &self,
        guild: GuildId,
        kind: AuditKind,
        limit: u8,
    ) -> Result<Vec<AuditRow>, AuditError> {
        let delay = {
            let mut i = self.inner.lock().unwrap();
            i.calls.push((guild, kind, limit));
            i.running += 1;
            i.most_running = i.most_running.max(i.running);
            i.delay
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let mut i = self.inner.lock().unwrap();
        i.running -= 1;
        if let Some(e) = i
            .failures
            .get_mut(&(guild, kind))
            .and_then(VecDeque::pop_front)
        {
            return Err(e);
        }
        Ok(i.rows.get(&(guild, kind)).cloned().unwrap_or_default())
    }
}

/// Records every event posted.
#[derive(Default)]
pub struct FakeSink {
    posted: Mutex<Vec<(GuildId, ModLogEvent)>>,
}

impl FakeSink {
    pub fn posted(&self) -> Vec<(GuildId, ModLogEvent)> {
        self.posted.lock().unwrap().clone()
    }

    pub fn events(&self) -> Vec<ModLogEvent> {
        self.posted().into_iter().map(|(_, e)| e).collect()
    }
}

#[async_trait]
impl ModLogSink for FakeSink {
    async fn post(&self, guild: GuildId, e: ModLogEvent) {
        self.posted.lock().unwrap().push((guild, e));
    }
}
