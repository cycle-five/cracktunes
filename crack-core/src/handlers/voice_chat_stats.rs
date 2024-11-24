use crate::{
    commands::admin::{deafen::deafen_internal, mute::mute_internal},
    db::to_fixed,
    errors::CrackedError,
    messaging::messages::UNKNOWN,
    BotConfig, CamKickConfig,
};
use poise::serenity_prelude as serenity;

use ::serenity::all::CacheHttp;
use colored::Colorize;
use serenity::{
    builder::CreateMessage, model::id::GuildId, Channel, ChannelId, Context as SerenityContext,
    Mentionable, UserId, VoiceState,
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

/// Check the camera status of a user and enforce the rules if necessary.
async fn check_and_enforce_cams(
    cur_cam: CamPollEvent,
    new_cam: &CamPollEvent,
    cam_states: &mut HashMap<(UserId, ChannelId), CamPollEvent>,
    config_map: &HashMap<u64, &CamKickConfig>,
    //status_changes: &mut Vec<CamStatusChangeEvent>,
    //ctx: Arc<SerenityContext>,
    cache_http: &impl CacheHttp,
) -> Result<(), CrackedError> {
    let kick_conf = config_map
        .get(&cur_cam.chan_id.get())
        .ok_or(CrackedError::Other("Channel not found"))?;
    tracing::trace!("kick_conf: {}", format!("{:?}", kick_conf).blue());
    if cur_cam.status != new_cam.status {
        let cam_event = CamPollEvent {
            last_change: Instant::now(),
            ..*new_cam
        };

        cam_states.insert(cam_event.key(), cam_event);
    } else {
        tracing::trace!("cur: {}, prev: {}", cur_cam.status, new_cam.status);
        tracing::trace!(
            "elapsed: {:?}, timeout: {}",
            cur_cam.last_change.elapsed(),
            kick_conf.timeout
        );
        if cur_cam.status == CamStatus::Off
            && cur_cam.last_change.elapsed() > Duration::from_secs(kick_conf.timeout)
        {
            let user = match new_cam.user_id.to_user(cache_http).await {
                Ok(user) => user,
                Err(err) => {
                    tracing::error!("Error getting user: {err}");
                    return Err(CrackedError::Other("Error getting user"));
                },
            };
            tracing::info!(
                "User {} has been cammed down for {} seconds",
                user.name,
                cur_cam.last_change.elapsed().as_secs()
            );

            // let guild = cam.guild_id.to_guild_cached(&ctx.cache).unwrap();
            let guild_id = new_cam.guild_id;
            tracing::info!("about to deafen {:?}", new_cam.user_id);

            if false {
                run_cam_enforcement(cache_http, new_cam, guild_id, user, kick_conf, cam_states)
                    .await;
            }
        }
    };
    Ok(())
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

    for (dc_res, state) in vec![dc_res1, dc_res2] {
        match dc_res {
            Ok(_) => {
                tracing::error!("User {} has been violated: {}", user.name, state);
                if state == "deafen" && kick_conf.msg_on_deafen
                    || state == "mute" && kick_conf.msg_on_mute
                    || state == "disconnect" && kick_conf.msg_on_dc
                {
                    let channel = ChannelId::new(kick_conf.chan_id);
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
                tracing::error!("Guild not found {guild_id}.");
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
            let channel_name = match chan_id.to_channel(ctx.clone(), Some(guild_id)).await {
                Ok(chan) => match chan {
                    Channel::Guild(chan) => chan.name.to_string(),
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
                guild_name, &user_name, &user_id, &channel_name, &chan_id, status,
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
        tracing::info!("Starting camera status check loop");
        let configs = config.cam_kick.clone().unwrap_or_default();
        let conf_guilds = configs.iter().map(|x| x.guild_id).collect::<HashSet<_>>();

        // This HashMap is used to keep track of the camera status of all the users in voice.
        // channels. It gets initialized empty here and then is updated every iteration of the loop.
        let mut cur_cams: HashMap<(UserId, ChannelId), CamPollEvent> =
            HashMap::<(UserId, ChannelId), CamPollEvent>::new();
        // This is
        let channels: HashMap<u64, &CamKickConfig> = configs
            .iter()
            .map(|x| (x.chan_id, x))
            .collect::<HashMap<_, _>>();

        tracing::trace!("conf_guilds: {}", format!("{:?}", conf_guilds).green());
        loop {
            // We clone Context again here, because Arc is owned, so it moves to the
            // new function.
            tracing::error!("Checking camera status for {} guilds", guilds.len());
            // Go through all the guilds we have cached and check the camera status
            // for all the users we can see in voice channels.
            let mut output = String::from("\n");
            let mut new_cams = vec![];
            for guild_id in &guilds {
                let (add_new_cams, add_output) =
                    check_camera_status(Arc::clone(&ctx), *guild_id).await;
                new_cams.extend(add_new_cams);
                output.push_str(&add_output);
            }

            //let total_active_cams = cams.len();
            let mut new_cams = Vec::<&CamPollEvent>::new();
            //let mut status_changes = Vec::<CamStatusChangeEvent>::new();

            for new_cam in new_cams.iter_mut() {
                if let Some(status) = cur_cams.get(&new_cam.key()) {
                    let _ =
                        check_and_enforce_cams(*status, new_cam, &mut cur_cams, &channels, &ctx)
                            .await;
                } else {
                    cur_cams.insert(new_cam.key(), **new_cam);
                }
            }
            let res: i32 = new_cams
                .iter()
                .map(|x| Into::<i32>::into(cur_cams.insert(x.key(), **x).is_none()))
                .sum();

            tracing::warn!("{}", output);
            tracing::warn!("num new cams: {}", res);
            tracing::warn!(
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
    use ::serenity::all::Token;
    use crack_types::get_valid_token;

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

    #[tokio::test]
    async fn test_check_and_enforce_cams() {
        let user_id = UserId::new(123);
        let chan_id = ChannelId::new(456);
        let cam = CamPollEvent {
            user_id,
            guild_id: GuildId::new(789),
            chan_id,
            status: CamStatus::On,
            last_change: Instant::now(),
        };
        let mut cam_states = HashMap::<(UserId, ChannelId), CamPollEvent>::new();
        let config_map = HashMap::<u64, &CamKickConfig>::new();
        let http = poise::serenity_prelude::http::Http::new(get_valid_token());
        let cache = Arc::new(poise::serenity_prelude::Cache::new());
        let cache_http = (Some(&cache), &http);
        // let ctx = Arc::new(SerenityContext::new());
        let res =
            check_and_enforce_cams(cam, &cam, &mut cam_states, &config_map, &cache_http).await;
        let want = CrackedError::Other("Channel not found");
        assert_eq!(res, Err(want));
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
