use self::serenity::builder::CreateEmbed;
use crate::errors::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::utils::create_embed_response_poise;
use crate::{Context, Error};
use colored::Colorize;
use poise::serenity_prelude as serenity;
use songbird::tracks::TrackHandle;

/// Get or set the volume of the bot.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "The volume to set the player to"] level: Option<u32>,
) -> Result<(), Error> {
    tracing::info!("volume");
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            tracing::error!("guild_id is None");
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = match manager.get(guild_id) {
        Some(call) => call,
        None => {
            tracing::error!("Can't get call from manager.");
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let to_set = match level {
        Some(arg) => Some(arg as isize),
        None => {
            let handler = call.lock().await;
            let track_handle: Option<TrackHandle> = handler.queue().current();
            // if track_handle.is_none() {
            //     let mut embed = CreateEmbed::default();
            //     embed.description(format!("{}", CrackedError::NothingPlaying));

            //     create_embed_response_poise(ctx, embed).await?;
            //     return Ok(());
            // }
            let volume_track = match track_handle {
                Some(handle) => handle.get_info().await.unwrap().volume,
                None => 0.0,
            };
            let volume = ctx
                .data()
                .guild_settings_map
                .lock()
                .unwrap()
                .clone()
                .get(&guild_id)
                .unwrap()
                .volume;
            let mut embed = CreateEmbed::default();
            embed.description(format!(
                "Current volume is {:.0}% in settings, {:.0}% in track.",
                volume * 100.0,
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

    tracing::warn!(
        "guild_settings: {:?}",
        format!("{:?}", guild_settings).white(),
    );

    let new_guild_settings = guild_settings_map
        .entry(guild_id)
        .or_insert_with(|| GuildSettings::new(guild_id));

    tracing::warn!(
        "new_guild_settings: {:?}",
        format!("{:?}", new_guild_settings).white(),
    );

    let embed = create_volume_embed(old_volume, new_volume);
    let track_handle: TrackHandle = match handler.queue().current() {
        Some(handle) => handle,
        None => {
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };
    track_handle.set_volume(new_volume).unwrap();
    // lock the mutex inside of it's own block, so it isn't locked across an await.
    // let _x = {
    //     let data: &Arc<Data> = ctx.data();
    //     let mut settings = data.guild_settings_map.lock().unwrap().clone();
    //     let guild_settings = settings
    //         .entry(*guild_id.as_u64())
    //         .or_insert_with(|| GuildSettings::new(guild_id));
    //     guild_settings.set_default_volume(new_volume);
    //     new_volume
    // };

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
