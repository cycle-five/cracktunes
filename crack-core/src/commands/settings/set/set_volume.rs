use crate::{
    guild::settings::GuildSettingsMapParam,
    guild::settings::{GuildSettings, DEFAULT_PREFIX},
    Context, Error,
};
use serenity::all::GuildId;

/// Get the current `volume` and `old_volume` setting for the guild.
pub fn set_volume(
    guild_settings_map: &GuildSettingsMapParam,
    guild_id: GuildId,
    vol: f32,
) -> (f32, f32) {
    let mut guild_settings_mut = guild_settings_map.write().unwrap();
    guild_settings_mut
        .entry(guild_id)
        .and_modify(|e| {
            e.old_volume = e.volume;
            e.volume = vol;
        })
        .or_insert(GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), None).with_volume(vol));
    let guild_settings = guild_settings_mut.get(&guild_id).unwrap();
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

    let (vol, old_vol) = {
        let guild_settings_map = &ctx.data().guild_settings_map;
        set_volume(guild_settings_map, guild_id, volume)
    };

    let msg = ctx
        .say(format!("vol: {}, old_vol: {}", vol, old_vol))
        .await?
        .into_message()
        .await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::commands::settings::set::set_volume::set_volume;
    use crate::guild::settings::{GuildSettingsMapParam, DEFAULT_VOLUME_LEVEL};
    use serenity::model::id::GuildId;

    #[test]
    fn test_set_volume() {
        let guild_id = GuildId::new(1);
        let guild_settings_map = GuildSettingsMapParam::default();

        let (vol, old_vol) = set_volume(&guild_settings_map, guild_id, 0.5);
        assert_eq!(vol, 0.5);
        assert_eq!(old_vol, DEFAULT_VOLUME_LEVEL);
        assert_eq!(
            guild_settings_map
                .read()
                .unwrap()
                .get(&guild_id)
                .unwrap()
                .volume,
            vol
        );

        let (vol, old_vol) = set_volume(&guild_settings_map, guild_id, 0.6);
        assert_eq!(vol, 0.6);
        assert_eq!(old_vol, 0.5);
        assert_eq!(
            guild_settings_map
                .read()
                .unwrap()
                .get(&guild_id)
                .unwrap()
                .volume,
            vol
        );
    }
}
