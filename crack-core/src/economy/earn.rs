//! Earning: one credit tick a minute for members in voice, and a count of
//! messages (cooldown-limited) that the same tick pays out.
//!
//! The rules (`voice_earners`, `cooldown_allows`, `earn_rows`) are pure. The
//! tick (`tick_with`) takes its snapshots and its writer as arguments, so it
//! runs without a Discord cache or a database.

use super::registry::EconomyRegistry;
use crate::db::economy::{credit_earnings, EarnRow};
use crate::Data;
use serenity::all::{Cache, ChannelId, GuildId, Message, UserId, VoiceState};
use serenity::async_trait;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, MissedTickBehavior};

/// How often credits are paid out.
pub const TICK: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceMember {
    pub user: UserId,
    pub channel: ChannelId,
    pub bot: bool,
    pub self_deaf: bool,
    /// The camera is on. A screen share alone is not a camera.
    pub camera: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceSnapshot {
    pub afk: Option<ChannelId>,
    pub members: Vec<VoiceMember>,
}

/// Who earns voice credits this minute, sorted. A member earns when not a
/// bot, not self-deafened, not opted out, not in the AFK channel, on camera
/// when `camera_required`, and at least one OTHER non-bot member shares the
/// channel. That company counts whether or not they are deafened, opted out
/// or on camera.
pub fn voice_earners(
    s: &VoiceSnapshot,
    opted_out: &HashSet<UserId>,
    camera_required: bool,
) -> Vec<UserId> {
    let mut humans_in: HashMap<ChannelId, usize> = HashMap::new();
    for m in s.members.iter().filter(|m| !m.bot) {
        *humans_in.entry(m.channel).or_insert(0) += 1;
    }
    let mut out: Vec<UserId> = s
        .members
        .iter()
        .filter(|m| {
            !m.bot
                && !m.self_deaf
                && (m.camera || !camera_required)
                && !opted_out.contains(&m.user)
                && s.afk != Some(m.channel)
                && humans_in.get(&m.channel).copied().unwrap_or(0) >= 2
        })
        .map(|m| m.user)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Whether a message at `now` counts, given when the last one did.
pub fn cooldown_allows(last: Option<Instant>, now: Instant, cooldown: Duration) -> bool {
    last.is_none_or(|l| now.saturating_duration_since(l) >= cooldown)
}

/// One row per member to credit: voice and message credits merged, all-zero
/// rows dropped, sorted by member.
pub fn earn_rows(
    voice: &[UserId],
    per_minute: u32,
    messages: &HashMap<UserId, u32>,
    per_message: u32,
) -> Vec<EarnRow> {
    let mut by_user: HashMap<UserId, (i64, i64)> = HashMap::new();
    for u in voice {
        by_user.entry(*u).or_default().0 += i64::from(per_minute);
    }
    for (u, n) in messages {
        by_user.entry(*u).or_default().1 += i64::from(*n) * i64::from(per_message);
    }
    let mut rows: Vec<EarnRow> = by_user
        .into_iter()
        .filter(|(_, (v, m))| *v != 0 || *m != 0)
        .map(|(user, (voice, messages))| EarnRow {
            user,
            voice,
            messages,
        })
        .collect();
    rows.sort_by_key(|r| r.user);
    rows
}

/// Counts one message towards its author's earnings. Synchronous: the event
/// handler never waits on it.
pub fn on_message(data: &Data, msg: &Message) {
    let Some(guild) = msg.guild_id else { return };
    if msg.author.bot() {
        return;
    }
    let reg: &EconomyRegistry = &data.economy;
    if !reg.loaded(guild) || !reg.is_enabled(guild) || reg.is_opted_out(guild, msg.author.id) {
        return;
    }
    let cfg = reg.settings(guild).config;
    if cfg.message_credits == 0 {
        return;
    }
    reg.count_message(
        guild,
        msg.author.id,
        Instant::now(),
        Duration::from_secs(u64::from(cfg.message_cooldown_seconds)),
    );
}

/// Where earned credits are written; a fake in tests.
#[async_trait]
pub trait EarnWriter: Send + Sync {
    async fn write(&self, guild: GuildId, rows: &[EarnRow]) -> sqlx::Result<()>;
}

#[async_trait]
impl EarnWriter for PgPool {
    async fn write(&self, guild: GuildId, rows: &[EarnRow]) -> sqlx::Result<()> {
        credit_earnings(self, guild, rows).await
    }
}

/// One payout. A server that isn't loaded or isn't enabled has its pending
/// messages dropped and earns nothing. A failed write is logged and that
/// minute's messages are lost.
pub async fn tick_with(
    data: &Data,
    snapshots: Vec<(GuildId, VoiceSnapshot)>,
    writer: &dyn EarnWriter,
) {
    let reg = &data.economy;
    let mut snaps: HashMap<GuildId, VoiceSnapshot> = snapshots.into_iter().collect();
    let mut guilds: HashSet<GuildId> = snaps.keys().copied().collect();
    guilds.extend(reg.pending_guilds());
    for guild in guilds {
        let pending = reg.take_pending_messages(guild);
        if !reg.loaded(guild) || !reg.is_enabled(guild) {
            continue;
        }
        let cfg = reg.settings(guild).config;
        let voice = snaps
            .remove(&guild)
            .map(|s| voice_earners(&s, &reg.opted_out(guild), cfg.voice_requires_camera))
            .unwrap_or_default();
        let rows = earn_rows(
            &voice,
            cfg.voice_credits_per_minute,
            &pending,
            cfg.message_credits,
        );
        if rows.is_empty() {
            continue;
        }
        if let Err(e) = writer.write(guild, &rows).await {
            tracing::warn!("economy: crediting earnings in {guild} failed: {e}");
        }
    }
}

/// One member's voice state as the earning rules see it; `None` when they
/// are in no channel.
pub fn voice_member(vs: &VoiceState, bot: bool) -> Option<VoiceMember> {
    Some(VoiceMember {
        user: vs.user_id,
        channel: vs.channel_id?,
        bot,
        self_deaf: vs.self_deaf(),
        camera: vs.self_video(),
    })
}

/// The voice picture of one server from the cache.
pub fn snapshot(cache: &Cache, guild: GuildId) -> Option<VoiceSnapshot> {
    let g = cache.guild(guild)?;
    let members = g
        .voice_states
        .iter()
        .filter_map(|vs| {
            let bot = vs
                .member
                .as_ref()
                .map(|m| m.user.bot())
                .or_else(|| g.members.get(&vs.user_id).map(|m| m.user.bot()))
                .unwrap_or(false);
            voice_member(vs, bot)
        })
        .collect();
    Some(VoiceSnapshot {
        afk: g.afk_metadata.as_ref().map(|a| a.afk_channel_id),
        members,
    })
}

/// Spawns the once-a-minute payout, the first time only and only with a
/// database.
pub fn start(data: Data, cache: Arc<Cache>) {
    if !data.economy.start_once() {
        return;
    }
    let Some(pool) = data.database_pool.clone() else {
        return;
    };
    super::effects::spawn_supervised("earn", async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        tick.tick().await; // the first tick is immediate; skip it
        loop {
            tick.tick().await;
            let snaps: Vec<_> = data
                .economy
                .enabled_guilds()
                .into_iter()
                .filter_map(|g| snapshot(&cache, g).map(|s| (g, s)))
                .collect();
            tick_with(&data, snaps, &pool).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::settings::EconomySettings;
    use std::sync::Mutex;

    fn u(n: u64) -> UserId {
        UserId::new(n)
    }
    fn c(n: u64) -> ChannelId {
        ChannelId::new(n)
    }
    fn m(user: u64, ch: u64) -> VoiceMember {
        VoiceMember {
            user: u(user),
            channel: c(ch),
            bot: false,
            self_deaf: false,
            camera: false,
        }
    }

    fn cam(user: u64, ch: u64) -> VoiceMember {
        VoiceMember {
            camera: true,
            ..m(user, ch)
        }
    }

    #[test]
    fn voice_earners_rules() {
        let s = VoiceSnapshot {
            afk: Some(c(99)),
            members: vec![
                m(1, 10), // alone
                m(2, 20),
                m(3, 20), // a pair
                m(4, 30),
                VoiceMember {
                    self_deaf: true,
                    ..m(5, 30)
                }, // one deafened
                m(6, 40),
                VoiceMember {
                    bot: true,
                    ..m(7, 40)
                }, // only a bot for company
                m(8, 50),
                m(9, 50), // 8 opted out
                m(10, 99),
                m(11, 99), // AFK channel
                m(12, 60),
                m(13, 61), // split
                m(14, 70),
                m(15, 70),
                VoiceMember {
                    bot: true,
                    ..m(16, 70)
                }, // a bot among two humans
            ],
        };
        let out = HashSet::from([u(8)]);
        assert_eq!(
            voice_earners(&s, &out, false),
            vec![u(2), u(3), u(4), u(9), u(14), u(15)]
        );
    }

    #[test]
    fn with_the_camera_required_only_cameras_earn() {
        let s = VoiceSnapshot {
            afk: None,
            members: vec![
                cam(1, 10),
                m(2, 10), // company without a camera still counts
                m(3, 20),
                m(4, 20),   // a pair, both cameras off
                cam(5, 30), // alone on camera
                VoiceMember {
                    self_deaf: true,
                    ..cam(6, 40)
                },
                m(7, 40), // on camera but deafened, with company
            ],
        };
        let none = HashSet::new();
        assert_eq!(voice_earners(&s, &none, true), vec![u(1)]);
        assert_eq!(
            voice_earners(&s, &none, false),
            vec![u(1), u(2), u(3), u(4), u(7)]
        );
    }

    /// A voice state as Discord sends it.
    fn state(self_video: bool, self_stream: bool) -> serenity::all::VoiceState {
        serde_json::from_str(&format!(
            r#"{{"channel_id":"10","user_id":"1","session_id":"s","deaf":false,"mute":false,
                "self_deaf":false,"self_mute":false,"self_stream":{self_stream},
                "self_video":{self_video},"suppress":false,"request_to_speak_timestamp":null}}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_camera_is_video_and_a_screen_share_is_not() {
        let camera = |vs| voice_member(&vs, false).unwrap().camera;
        assert!(camera(state(true, false)));
        assert!(
            !camera(state(false, true)),
            "a screen share is not a camera"
        );
        assert!(!camera(state(false, false)));
        assert_eq!(
            voice_member(&state(true, true), true),
            Some(VoiceMember {
                user: u(1),
                channel: c(10),
                bot: true,
                self_deaf: false,
                camera: true,
            })
        );
    }

    #[test]
    fn cooldown_allows_edges() {
        let t0 = Instant::now();
        let cd = Duration::from_secs(60);
        assert!(cooldown_allows(None, t0, cd));
        assert!(cooldown_allows(Some(t0), t0 + cd, cd));
        assert!(!cooldown_allows(
            Some(t0),
            t0 + cd - Duration::from_millis(1),
            cd
        ));
    }

    #[test]
    fn earn_rows_merges_and_drops_zero() {
        let msgs = HashMap::from([(u(3), 2), (u(2), 1)]);
        let rows = earn_rows(&[u(2), u(1)], 0, &msgs, 1);
        assert_eq!(
            rows,
            vec![
                EarnRow {
                    user: u(2),
                    voice: 0,
                    messages: 1
                },
                EarnRow {
                    user: u(3),
                    voice: 0,
                    messages: 2
                },
            ]
        );
        assert!(earn_rows(&[u(1)], 0, &msgs, 0).is_empty());
        let rows = earn_rows(&[u(2)], 5, &msgs, 3);
        assert_eq!(
            rows[0],
            EarnRow {
                user: u(2),
                voice: 5,
                messages: 3
            }
        );
        assert_eq!(
            rows[1],
            EarnRow {
                user: u(3),
                voice: 0,
                messages: 6
            }
        );
    }

    #[derive(Default)]
    struct Fake(Mutex<Vec<(GuildId, Vec<EarnRow>)>>);

    #[async_trait]
    impl EarnWriter for Fake {
        async fn write(&self, guild: GuildId, rows: &[EarnRow]) -> sqlx::Result<()> {
            self.0.lock().unwrap().push((guild, rows.to_vec()));
            Ok(())
        }
    }

    struct Failing;

    #[async_trait]
    impl EarnWriter for Failing {
        async fn write(&self, _: GuildId, _: &[EarnRow]) -> sqlx::Result<()> {
            Err(sqlx::Error::PoolClosed)
        }
    }

    const G: GuildId = GuildId::new(1);

    fn pair() -> VoiceSnapshot {
        VoiceSnapshot {
            afk: None,
            members: vec![m(1, 10), m(2, 10)],
        }
    }

    fn data_with(enabled: bool, loaded: bool) -> Data {
        let data = Data::default();
        let mut s = EconomySettings::off(G);
        s.enabled = enabled;
        data.economy.set_settings(s);
        if loaded {
            data.economy.mark_loaded(G);
        }
        data
    }

    async fn count(data: &Data, user: u64) {
        assert!(data
            .economy
            .count_message(G, u(user), Instant::now(), Duration::from_secs(60)));
    }

    #[tokio::test(start_paused = true)]
    async fn tick_skips_unloaded_guilds() {
        let data = data_with(true, false);
        count(&data, 3).await;
        let w = Fake::default();
        tick_with(&data, vec![(G, pair())], &w).await;
        assert!(w.0.lock().unwrap().is_empty());
        assert!(data.economy.take_pending_messages(G).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn tick_writes_rows_for_enabled_loaded_guilds() {
        let data = data_with(true, true);
        count(&data, 3).await;
        let w = Fake::default();
        tick_with(&data, vec![(G, pair())], &w).await;
        let calls = w.0.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, G);
        assert_eq!(
            calls[0].1,
            vec![
                EarnRow {
                    user: u(1),
                    voice: 1,
                    messages: 0
                },
                EarnRow {
                    user: u(2),
                    voice: 1,
                    messages: 0
                },
                EarnRow {
                    user: u(3),
                    voice: 0,
                    messages: 1
                },
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn tick_excludes_the_opted_out_from_voice() {
        let data = data_with(true, true);
        data.economy.set_one_opted_out(G, u(1), true);
        let w = Fake::default();
        tick_with(&data, vec![(G, pair())], &w).await;
        let calls = w.0.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![EarnRow {
                user: u(2),
                voice: 1,
                messages: 0
            }]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn tick_pays_only_cameras_when_the_server_requires_one() {
        let data = Data::default();
        let mut s = EconomySettings::off(G);
        s.enabled = true;
        s.config.voice_requires_camera = true;
        data.economy.set_settings(s);
        data.economy.mark_loaded(G);
        let snap = VoiceSnapshot {
            afk: None,
            members: vec![cam(1, 10), m(2, 10)],
        };
        let w = Fake::default();
        tick_with(&data, vec![(G, snap)], &w).await;
        let calls = w.0.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![EarnRow {
                user: u(1),
                voice: 1,
                messages: 0
            }]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn tick_drops_messages_for_disabled_guilds() {
        let data = data_with(false, true);
        count(&data, 3).await;
        let w = Fake::default();
        tick_with(&data, vec![(G, pair())], &w).await;
        assert!(w.0.lock().unwrap().is_empty());
        assert!(data.economy.take_pending_messages(G).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_write_drops_that_minutes_messages() {
        let data = data_with(true, true);
        count(&data, 3).await;
        tick_with(&data, vec![], &Failing).await;
        assert!(data.economy.take_pending_messages(G).is_empty());
    }
}
