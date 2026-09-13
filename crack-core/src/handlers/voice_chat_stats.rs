use crate::{
    commands::admin::{deafen::deafen_internal, mute::mute_internal},
    messaging::messages::UNKNOWN,
    BotConfig, CamKickConfig,
};
use crack_types::to_fixed;
use poise::serenity_prelude as serenity;

use ::serenity::all::CacheHttp;
use colored::Colorize;
use serenity::{
    builder::CreateMessage, model::id::GuildId, Channel, ChannelId, Context as SerenityContext,
    GenericChannelId, Mentionable, UserId, VoiceState,
};
use std::{
    cmp::{Eq, PartialEq},
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Enum for the Camera status.
enum CamStatus {
    On,
    Off,
}

/// Implement Display for the Camera status enum.
impl std::fmt::Display for CamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CamStatus::On => write!(f, "On"),
            CamStatus::Off => write!(f, "Off"),
        }
    }
}

/// Implement From bool for the Camera status enum.
impl From<bool> for CamStatus {
    fn from(status: bool) -> Self {
        if status {
            CamStatus::On
        } else {
            CamStatus::Off
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// Struct for the our derived Camera change event.
struct CamPollEvent {
    user_id: UserId,
    guild_id: GuildId,
    chan_id: ChannelId,
    status: CamStatus,
    last_change: Instant,
}

impl CamPollEvent {
    /// Returns the key for the Camera change event.
    fn key(&self) -> (UserId, ChannelId) {
        (self.user_id, self.chan_id)
    }
}

/// Whether acting on a user -- deafening and muting them, and posting
/// `dc_msg` -- is switched on at all.
///
/// 🔑 Off, deliberately, and not reachable from any command or config file.
/// Server owners are expected to want camera enforcement, so the feature is
/// kept working rather than deleted: the loop tracks camera state and reports
/// who it WOULD act on (#515). Switching it on is a decision to make with the
/// guilds it applies to, not a setting to leave lying around.
const ENFORCE_CAMS: bool = false;

/// Fold one poll into the tracked camera state, and return every user whose
/// camera has now been off for longer than their channel's rule allows.
///
/// Pure -- no cache, no HTTP -- because both bugs that kept this feature dead
/// lived in exactly this bookkeeping (#515):
///
/// - 🪤 the poll was thrown away: a second `let mut new_cams` shadowed the
///   populated vec, so every iteration walked an empty one;
/// - 🪤 even un-shadowed, every poll re-stamped `last_change` with the poll's
///   own `Instant::now()` and wrote it back, so "off for N seconds" restarted
///   every interval and could never exceed any timeout.
///
/// An unchanged status keeps the time it was first seen; a change restarts the
/// clock; a user no longer in voice is forgotten, so rejoining starts fresh.
fn apply_poll(
    tracked: &mut HashMap<(UserId, ChannelId), CamPollEvent>,
    polled: Vec<CamPollEvent>,
    timeouts: &HashMap<ChannelId, Duration>,
    now: Instant,
) -> Vec<CamPollEvent> {
    let seen: HashSet<(UserId, ChannelId)> = polled.iter().map(CamPollEvent::key).collect();
    for poll in polled {
        match tracked.get(&poll.key()) {
            Some(prev) if prev.status == poll.status => {},
            _ => {
                tracked.insert(
                    poll.key(),
                    CamPollEvent {
                        last_change: now,
                        ..poll
                    },
                );
            },
        }
    }
    tracked.retain(|key, _| seen.contains(key));

    tracked
        .values()
        .filter(|cam| cam.status == CamStatus::Off)
        .filter(|cam| {
            timeouts
                .get(&cam.chan_id)
                .is_some_and(|timeout| now.saturating_duration_since(cam.last_change) > *timeout)
        })
        .copied()
        .collect()
}

/// Run the camera enforcement rules.
async fn run_cam_enforcement(
    //ctx: Arc<SerenityContext>,
    cache_http: &impl CacheHttp,
    new_cam: &CamPollEvent,
    guild_id: GuildId,
    user: ::serenity::model::prelude::User,
    kick_conf: &&CamKickConfig,
    cam_states: &mut HashMap<(UserId, ChannelId), CamPollEvent>,
) {
    // WARN: Disconnect the user
    // FIXME: Should this not be it's own function?
    // let dc_res = disconnect_member(ctx.clone(), *cam, guild).await;
    let dc_res1 = (
        deafen_internal(cache_http, guild_id, user.clone(), true).await,
        "deafen",
    );
    let dc_res2 = (
        mute_internal(cache_http, user.clone(), guild_id, true).await,
        "deafen",
    );
    // let dc_res1 = (
    //     server_defeafen_member(ctx.clone(), *new_cam, guild_id).await,
    //     "deafen",
    // );
    // let dc_res2 = (
    //     server_mute_member(ctx.clone(), *new_cam, guild_id).await,
    //     "mute",
    // );

    for (dc_res, state) in [dc_res1, dc_res2] {
        match dc_res {
            Ok(_) => {
                tracing::error!("User {} has been violated: {}", user.name, state);
                if state == "deafen" && kick_conf.msg_on_deafen
                    || state == "mute" && kick_conf.msg_on_mute
                    || state == "disconnect" && kick_conf.msg_on_dc
                {
                    let channel = GenericChannelId::new(kick_conf.chan_id);
                    let _ = channel
                        .send_message(
                            cache_http.http(),
                            CreateMessage::default().content({
                                format!("{} {}: {}", user.mention(), kick_conf.dc_msg, state)
                            }),
                        )
                        .await;
                }
                cam_states.remove(&new_cam.key());
            },
            Err(err) => {
                tracing::error!("Error violating user: {}", err);
            },
        }
    }
}

use extract_map::ExtractKey;
use extract_map::ExtractMap;
/// Check the camera statuses of all the users in voice channels per
/// guild and if there's rules aroun camera usage, enforce them.
async fn check_camera_status(
    ctx: Arc<SerenityContext>,
    guild_id: GuildId,
) -> (Vec<CamPollEvent>, String) {
    let (voice_states, guild_name): (ExtractMap<UserId, VoiceState>, String) =
        match guild_id.to_guild_cached(&ctx.cache.clone()) {
            Some(guild) => (guild.voice_states.clone(), guild.name.to_string()),
            None => {
                // Not an error: `guilds` is a snapshot, and a guild can leave
                // the cache between snapshot and poll -- or simply not have
                // arrived yet during the warm-up after a restart.
                tracing::debug!("Guild not found {guild_id}.");
                return (vec![], "".to_string());
            },
        };

    let mut cams = Vec::new();
    let mut output: String = format!("{}\n", guild_name.bright_green());

    for voice_state in voice_states.iter() {
        let user_id = voice_state.extract_key();
        let user = user_id.to_user(ctx.clone()).await;
        if let Some(chan_id) = voice_state.channel_id {
            let user_name = match user {
                Ok(user) => user.name,
                Err(err) => {
                    tracing::error!("Error getting user: {err}");
                    to_fixed(UNKNOWN)
                },
            };
            let channel_name = match chan_id
                .widen()
                .to_channel(ctx.clone(), Some(guild_id))
                .await
            {
                Ok(chan) => match chan {
                    Channel::Guild(chan) => chan.base.name.to_string(),
                    Channel::Private(chan) => chan.recipient.name.to_string(),
                    _ => UNKNOWN.to_string(),
                },
                Err(err) => {
                    tracing::error!(
                        r#"Error getting channel name for channel
                        {chan_id} in guild {guild_name}: {err}"#,
                    );
                    "Missing Access".to_string()
                },
            };
            let status = CamStatus::from(voice_state.self_video());
            let last_change = Instant::now();

            let info = CamPollEvent {
                user_id: *user_id,
                guild_id,
                chan_id,
                status,
                last_change,
            };

            cams.push(info);
            output.push_str(&format!(
                "{}|{}|{}|{}|{}|{}\n",
                guild_name, user_name, user_id, channel_name, chan_id, status,
            ));
        }
    }
    (cams, output)
}

/// The main loop that checks the camera status of all the users in voice channels
pub async fn cam_status_loop(
    ctx: Arc<SerenityContext>,
    config: Arc<BotConfig>,
    guilds: Vec<GuildId>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let configs = config.cam_kick.clone().unwrap_or_default();

        // 🪤 Poll only the guilds a rule exists for. This used to walk every
        // guild the bot is in -- ~140 -- resolving a user and a channel for
        // every voice state, every interval, to feed rules that exist in none
        // of them.
        let conf_guilds: HashSet<u64> = configs.iter().map(|x| x.guild_id).collect();
        let guilds: Vec<GuildId> = guilds
            .into_iter()
            .filter(|g| conf_guilds.contains(&g.get()))
            .collect();
        if guilds.is_empty() {
            tracing::debug!("No cam_kick rule matches any guild; camera status loop not started");
            return;
        }
        tracing::info!(
            "Starting camera status check loop for {} guild(s)",
            guilds.len()
        );

        let timeouts: HashMap<ChannelId, Duration> = configs
            .iter()
            .map(|c| (ChannelId::new(c.chan_id), Duration::from_secs(c.timeout)))
            .collect();
        let rules: HashMap<u64, &CamKickConfig> = configs.iter().map(|c| (c.chan_id, c)).collect();

        // Camera state per (user, channel), carried across polls. See
        // `apply_poll` for why the bookkeeping lives there.
        let mut tracked: HashMap<(UserId, ChannelId), CamPollEvent> = HashMap::new();

        loop {
            // 🪤 Was ERROR. This is a heartbeat -- it fires every
            // `video_status_poll_interval` seconds whether or not anything
            // happened, and says only that the loop is alive (#491).
            tracing::trace!("Checking camera status for {} guilds", guilds.len());
            let mut output = String::from("\n");
            let mut polled = Vec::new();
            for guild_id in &guilds {
                let (cams, add_output) = check_camera_status(Arc::clone(&ctx), *guild_id).await;
                polled.extend(cams);
                output.push_str(&add_output);
            }

            let due = apply_poll(&mut tracked, polled, &timeouts, Instant::now());
            for cam in due {
                if !ENFORCE_CAMS {
                    tracing::debug!(
                        "camera off past its limit for {} in {}; enforcement is off",
                        cam.user_id,
                        cam.chan_id
                    );
                    continue;
                }
                let Some(rule) = rules.get(&cam.chan_id.get()) else {
                    continue;
                };
                match cam.user_id.to_user(ctx.as_ref()).await {
                    Ok(user) => {
                        run_cam_enforcement(
                            ctx.as_ref(),
                            &cam,
                            cam.guild_id,
                            user,
                            rule,
                            &mut tracked,
                        )
                        .await
                    },
                    Err(err) => {
                        tracing::warn!("camera enforcement: cannot resolve {}: {err}", cam.user_id)
                    },
                }
            }

            // 🪤 Was WARN: a multi-line record naming every polled guild,
            // re-emitted every interval (#491).
            tracing::trace!("{}", output);
            tracing::trace!(
                "Sleeping for {} seconds",
                config.get_video_status_poll_interval()
            );
            tokio::time::sleep(Duration::from_secs(config.get_video_status_poll_interval())).await;
        }
    })
}

#[cfg(test)]
mod test {
    // Test CamStatus enum
    use super::*;

    #[test]
    fn test_cam_status() {
        let on = CamStatus::On;
        let off = CamStatus::Off;
        assert_eq!(on, CamStatus::On);
        assert_eq!(off, CamStatus::Off);
    }

    #[test]
    fn test_cam_status_display() {
        let on = CamStatus::On;
        let off = CamStatus::Off;
        assert_eq!(format!("{}", on), "On");
        assert_eq!(format!("{}", off), "Off");
    }

    #[test]
    fn test_cam_status_from_bool() {
        let on = CamStatus::from(true);
        let off = CamStatus::from(false);
        assert_eq!(on, CamStatus::On);
        assert_eq!(off, CamStatus::Off);
    }

    // CamPollEvent tests
    #[test]
    fn test_cam_poll_event_key() {
        let user_id = UserId::new(123);
        let chan_id = ChannelId::new(456);
        let cam = CamPollEvent {
            user_id,
            guild_id: GuildId::new(789),
            chan_id,
            status: CamStatus::On,
            last_change: Instant::now(),
        };
        assert_eq!(cam.key(), (user_id, chan_id));
    }

    fn cam(user: u64, chan: u64, status: CamStatus, at: Instant) -> CamPollEvent {
        CamPollEvent {
            user_id: UserId::new(user),
            guild_id: GuildId::new(789),
            chan_id: ChannelId::new(chan),
            status,
            last_change: at,
        }
    }

    fn rule(chan: u64, secs: u64) -> HashMap<ChannelId, Duration> {
        HashMap::from([(ChannelId::new(chan), Duration::from_secs(secs))])
    }

    /// 🪤 The re-stamp bug (#515). Every poll carries its own `Instant::now()`;
    /// writing that back restarted "off for N seconds" every interval, so no
    /// timeout could ever be exceeded.
    #[test]
    fn an_unchanged_status_keeps_the_time_it_was_first_seen() {
        let t0 = Instant::now();
        let rules = rule(10, 600);
        let mut tracked = HashMap::new();
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, t0)],
            &rules,
            t0,
        );
        let later = t0 + Duration::from_secs(120);
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, later)],
            &rules,
            later,
        );
        let state = tracked[&(UserId::new(1), ChannelId::new(10))];
        assert_eq!(
            state.last_change, t0,
            "an unchanged Off must not restart its clock"
        );
    }

    /// 🪤 The shadow bug (#515): the poll was discarded, so nothing was ever
    /// tracked and nothing was ever due.
    #[test]
    fn off_past_the_limit_is_due_and_on_never_is() {
        let t0 = Instant::now();
        let rules = rule(10, 60);
        let mut tracked = HashMap::new();
        let first = vec![
            cam(1, 10, CamStatus::Off, t0),
            cam(2, 10, CamStatus::On, t0),
        ];
        assert!(
            apply_poll(&mut tracked, first, &rules, t0).is_empty(),
            "nobody is past the limit yet"
        );
        let t1 = t0 + Duration::from_secs(61);
        let second = vec![
            cam(1, 10, CamStatus::Off, t1),
            cam(2, 10, CamStatus::On, t1),
        ];
        let due: Vec<u64> = apply_poll(&mut tracked, second, &rules, t1)
            .iter()
            .map(|c| c.user_id.get())
            .collect();
        assert_eq!(due, vec![1], "only the camera that stayed off is due");
    }

    #[test]
    fn exactly_at_the_limit_is_not_yet_due() {
        let t0 = Instant::now();
        let rules = rule(10, 60);
        let mut tracked = HashMap::new();
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, t0)],
            &rules,
            t0,
        );
        let at = t0 + Duration::from_secs(60);
        assert!(apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, at)],
            &rules,
            at
        )
        .is_empty());
    }

    #[test]
    fn a_status_change_restarts_the_clock() {
        let t0 = Instant::now();
        let rules = rule(10, 60);
        let mut tracked = HashMap::new();
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, t0)],
            &rules,
            t0,
        );
        let t1 = t0 + Duration::from_secs(50);
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::On, t1)],
            &rules,
            t1,
        );
        let t2 = t0 + Duration::from_secs(55);
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, t2)],
            &rules,
            t2,
        );
        // 70s after the first Off, but only 15s after the latest one.
        let t3 = t0 + Duration::from_secs(70);
        assert!(
            apply_poll(
                &mut tracked,
                vec![cam(1, 10, CamStatus::Off, t3)],
                &rules,
                t3
            )
            .is_empty(),
            "the clock restarted when the camera came back on"
        );
    }

    #[test]
    fn a_channel_without_a_rule_is_never_due() {
        let t0 = Instant::now();
        let rules = rule(10, 0);
        let mut tracked = HashMap::new();
        apply_poll(
            &mut tracked,
            vec![cam(1, 99, CamStatus::Off, t0)],
            &rules,
            t0,
        );
        let later = t0 + Duration::from_secs(3600);
        assert!(apply_poll(
            &mut tracked,
            vec![cam(1, 99, CamStatus::Off, later)],
            &rules,
            later
        )
        .is_empty());
    }

    #[test]
    fn a_user_who_left_voice_is_forgotten() {
        let t0 = Instant::now();
        let rules = rule(10, 60);
        let mut tracked = HashMap::new();
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, t0)],
            &rules,
            t0,
        );
        apply_poll(&mut tracked, vec![], &rules, t0 + Duration::from_secs(30));
        assert!(tracked.is_empty(), "left voice, so no stale clock survives");
        // Rejoining starts fresh: 61s after the ORIGINAL Off is 30s after rejoining.
        let t2 = t0 + Duration::from_secs(31);
        apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, t2)],
            &rules,
            t2,
        );
        let t3 = t0 + Duration::from_secs(61);
        assert!(apply_poll(
            &mut tracked,
            vec![cam(1, 10, CamStatus::Off, t3)],
            &rules,
            t3
        )
        .is_empty());
    }

    /// 🔑 A product decision, pinned. Server owners are expected to want camera
    /// enforcement; switching it on is a deliberate change made with them
    /// (#515), not a side effect of fixing the tracking.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn camera_enforcement_ships_switched_off() {
        assert!(!ENFORCE_CAMS);
    }

    // fn new_serenity_context() -> Arc<SerenityContext> {
    //     let token = std::env::var("DISCORD_BOT_TOKEN")?;
    //     let shard_info = ShardInfo {
    //         id: ShardId(0),
    //         total: 1,
    //     };

    //     // retrieve the gateway response, which contains the URL to connect to
    //     let gateway = Arc::new(Mutex::new(http.get_gateway().await?.url));
    //     let shard = Shard::new(gateway, &token, shard_info, GatewayIntents::all(), None).await?;
    //     Arc::new(SerenityContext {
    //         data: Arc::new(tokio::sync::RwLock::new(
    //             poise::serenity_prelude::prelude::TypeMap::new(),
    //         )),
    //         http: Arc::new(poise::serenity_prelude::http::Http::new("")),
    //         shard: poise::serenity_prelude::Shard::new("".to_string(), "".to_string()),
    //         cache: Arc::new(poise::serenity_prelude::Cache::new()),
    //         shard_id: poise::serenity_prelude::ShardId(0),
    //     })
    // }
}
