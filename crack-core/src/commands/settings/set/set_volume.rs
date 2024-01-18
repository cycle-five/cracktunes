use serenity::all::GuildId;

use crate::{Context, Data, Error};

/// Get the current `volume` and `old_volume` setting for the guild.
pub fn set_volume(data: &Data, guild_id: GuildId, vol: f32) -> (f32, f32) {
    let guild_settings = {
        let mut asdf = data.guild_settings_map.write().unwrap();
        asdf.entry(guild_id)
            .and_modify(|e| {
                e.volume = vol;
                e.old_volume = e.volume;
            })
            .or_default()
            .clone()
    };
    (guild_settings.volume, guild_settings.old_volume)
}

/// Set the volume for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "Volume to set the bot settings to"] volume: f32,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let (vol, old_vol) = set_volume(data, guild_id, volume);

    ctx.say(format!("vol: {}, old_vol: {}", vol, old_vol))
        .await
        .map_err(|e| e.into())
        .map(|_| ())
}
