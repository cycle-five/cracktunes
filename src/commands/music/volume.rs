use self::serenity::builder::CreateEmbed;
use crate::errors::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::utils::create_embed_response_poise;
use crate::{Context, Error};
use colored::Colorize;
use poise::serenity_prelude as serenity;
use songbird::tracks::TrackHandle;

/// Get or set the volume of the bot.
#[poise::command(slash_command, prefix_command, guild_only, aliases("vol"))]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "The volume to set the player to"] level: Option<u32>,
) -> Result<(), Error> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            tracing::error!("guild_id is None");
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, &embed).await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = match manager.get(guild_id.get()) {
        Some(call) => call,
        None => {
            tracing::error!("Can't get call from manager.");
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, &embed).await?;
            return Ok(());
        }
    };

    let to_set = match level {
        Some(arg) => Some(arg as isize),
        None => {
            let handler = call.lock().await;
            let track_handle: Option<TrackHandle> = handler.queue().current();

            let volume_track = match track_handle {
                Some(handle) => handle.get_info().await.unwrap().volume,
                None => 0.0,
            };
            let mut guild_settings_map = ctx.data().guild_settings_map.lock().unwrap().clone();
            let guild_settings = guild_settings_map
                .entry(guild_id)
                .or_insert_with(|| GuildSettings::new(guild_id))
                .clone();
            let asdf = guild_settings_map.get(&guild_id).unwrap().volume;

            tracing::warn!(
                "asdf: {} guild_settings: {:?}",
                format!("{:?}", guild_settings).white(),
                asdf,
            );
            let mut embed = CreateEmbed::default();
            embed.description(format!(
                "Current volume is {:.0}% in settings, {:.0}% in track.",
                guild_settings.volume * 100.0,
                volume_track * 100.0
            ));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let handler = call.lock().await;
    let mut guild_settings_map = ctx.data().guild_settings_map.lock().unwrap().clone();
    let guild_settings = guild_settings_map
        .entry(guild_id)
        .or_insert_with(|| GuildSettings::new(guild_id));
    tracing::warn!(
        "guild_settings: {:?}",
        format!("{:?}", guild_settings).white(),
    );
    let new_volume = to_set.unwrap() as f32 / 100.0;
    let old_volume = guild_settings.volume;

    guild_settings.volume = new_volume;
    ctx.data()
        .guild_settings_map
        .lock()
        .unwrap()
        .insert(guild_id, guild_settings.clone());

    let embed = create_volume_embed(old_volume, new_volume);
    let track_handle: TrackHandle = match handler.queue().current() {
        Some(handle) => handle,
        None => {
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };
    track_handle.set_volume(new_volume).unwrap();
    create_embed_response_poise(ctx, embed).await?;
    Ok(())
}

pub fn create_volume_embed(old: f32, new: f32) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    embed.description(create_volume_desc(old, new));
    embed
}

pub fn create_volume_desc(old: f32, new: f32) -> String {
    format!(
        "Volume changed from {:.0}% to {:.0}%",
        old * 100.0,
        new * 100.0,
    )
}
