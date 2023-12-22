use self::serenity::builder::CreateEmbed;
use crate::errors::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::utils::{get_guild_name, send_embed_response_poise};
use crate::{Context, Error};
use colored::Colorize;
use poise::serenity_prelude as serenity;
use songbird::tracks::TrackHandle;

/// Get or set the volume of the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only, aliases("vol"))]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "Set the volume of the bot"] level: Option<u32>,
) -> Result<(), Error> {
    tracing::error!("volume");
    let prefix = ctx.data().bot_settings.get_prefix();
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            tracing::error!("guild_id is None");
            let embed =
                CreateEmbed::default().description(format!("{}", CrackedError::NotConnected));
            let _ = send_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };
    tracing::error!("guild_id: {:?}", guild_id);
    let embed = {
        tracing::error!("embed");
        let manager = match songbird::get(ctx.serenity_context()).await {
            Some(manager) => manager,
            None => {
                tracing::error!("Can't get manager.");
                let embed =
                    CreateEmbed::default().description(format!("{}", CrackedError::NotConnected));
                let msg = send_embed_response_poise(ctx, embed).await?;
                ctx.data().add_msg_to_cache(guild_id, msg);
                return Ok(());
            }
        };
        let call = match manager.get(guild_id) {
            Some(call) => call,
            None => {
                tracing::error!("Can't get call from manager.");
                let embed =
                    CreateEmbed::default().description(format!("{}", CrackedError::NotConnected));
                let msg = send_embed_response_poise(ctx, embed).await?;
                ctx.data().add_msg_to_cache(guild_id, msg);
                return Ok(());
            }
        };

        let handler = call.lock().await;

        let track_handle: Option<TrackHandle> = handler.queue().current();

        let to_set = match level {
            Some(arg) => Some(arg as isize),
            None => {
                let volume_track = match track_handle {
                    Some(handle) => handle.get_info().await.unwrap().volume,
                    None => 0.0,
                };
                ctx.data()
                    .guild_settings_map
                    .write()
                    .unwrap()
                    .entry(guild_id)
                    .or_insert_with(|| {
                        GuildSettings::new(
                            guild_id,
                            Some(&prefix),
                            get_guild_name(ctx.serenity_context(), guild_id),
                        )
                    });
                let guild_settings = ctx
                    .data()
                    .guild_settings_map
                    .read()
                    .unwrap()
                    .get(&guild_id)
                    .unwrap()
                    .clone();
                let asdf = guild_settings.volume;

                tracing::warn!(
                    "asdf: {} guild_settings: {:?}",
                    format!("{:?}", guild_settings).white(),
                    asdf,
                );
                let embed = CreateEmbed::default().description(format!(
                    "Current volume is {:.0}% in settings, {:.0}% in track.",
                    guild_settings.volume * 100.0,
                    volume_track * 100.0
                ));
                let msg = send_embed_response_poise(ctx, embed).await?;
                ctx.data().add_msg_to_cache(guild_id, msg);
                return Ok(());
            }
        };

        let new_vol = to_set.unwrap() as f32 / 100.0;
        let old_vol = {
            let mut guild_settings_guard = ctx.data().guild_settings_map.write().unwrap();
            let guild_settings = guild_settings_guard
                .entry(guild_id)
                .and_modify(|guild_settings| {
                    guild_settings.set_volume(new_vol);
                })
                .or_insert_with(|| {
                    let guild_settings = GuildSettings::new(
                        guild_id,
                        Some(&prefix),
                        get_guild_name(ctx.serenity_context(), guild_id),
                    )
                    .set_volume(new_vol)
                    .clone();

                    tracing::warn!(
                        "guild_settings: {:?}",
                        format!("{:?}", guild_settings).white(),
                    );

                    guild_settings
                });
            tracing::warn!(
                "guild_settings: {}",
                format!("{:?}", guild_settings).white(),
            );
            guild_settings.old_volume
        };

        let embed = create_volume_embed(old_vol, new_vol);
        let track_handle: TrackHandle = match track_handle {
            Some(handle) => handle,
            None => {
                return send_embed_response_poise(ctx, embed).await.map(|m| {
                    ctx.data().add_msg_to_cache(guild_id, m);
                })
            }
        };

        track_handle.set_volume(new_vol).unwrap();
        embed
    };
    send_embed_response_poise(ctx, embed).await.map(|m| {
        ctx.data().add_msg_to_cache(guild_id, m);
    })
}

pub fn create_volume_embed(old: f32, new: f32) -> CreateEmbed {
    CreateEmbed::default().description(create_volume_desc(old, new))
}

pub fn create_volume_desc(old: f32, new: f32) -> String {
    format!(
        "Volume changed from {:.0}% to {:.0}%",
        old * 100.0,
        new * 100.0,
    )
}
