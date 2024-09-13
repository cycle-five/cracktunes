use crate::poise_ext::PoiseContextExt;
use crate::CrackedMessage;
use crate::{
    guild::settings::GuildSettingsMapParam,
    guild::settings::{GuildSettings, DEFAULT_PREFIX},
    Context, Error,
};
use serenity::all::GuildId;

/// Get the current `volume` and `old_volume` setting for the guild.
pub async fn set_volume(
    guild_settings_map: &GuildSettingsMapParam,
    guild_id: GuildId,
    vol: f32,
) -> (f32, f32) {
    let mut guild_settings_mut = guild_settings_map.write().await;
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

/// Sets default volume for the bot. If the bot is in user, this will *not*
/// take effect immediately.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "Default volume for the bot."] volume: f32,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    use crate::commands::CrackedError;

    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let (vol, old_vol) = {
        let guild_settings_map = &ctx.data().guild_settings_map;
        set_volume(guild_settings_map, guild_id, volume).await
    };

    let _msg = ctx
        .send_reply(CrackedMessage::Volume { vol, old_vol }, true)
        .await;
    // let msg = ctx
    //     .say(format!("vol: {}, old_vol: {}", vol, old_vol))
    //     .await?
    //     .into_message()
    //     .await?;
    //ctx.data().add_msg_to_cache(guild_id, msg).await;
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::commands::settings::set::set_volume::set_volume;
    use crate::guild::settings::GuildSettingsMapParam;
    use serenity::model::id::GuildId;

    #[tokio::test]
    async fn test_set_volume() {
        let guild_id = GuildId::new(1);
        let guild_settings_map = GuildSettingsMapParam::default();

        // let init_volume = guild_settings_map
        //     .read()
        //     .await
        //     .get(&guild_id)
        //     .map(|x| x.volume)
        //     .unwrap_or(DEFAULT_VOLUME_LEVEL);
        let (vol, old_vol) = set_volume(&guild_settings_map, guild_id, 0.5).await;
        assert_eq!(vol, 0.5);
        assert_eq!(old_vol, 0.5);
        assert_eq!(
            guild_settings_map
                .read()
                .await
                .get(&guild_id)
                .unwrap()
                .volume,
            vol
        );

        let (vol, old_vol) = set_volume(&guild_settings_map, guild_id, 0.6).await;
        assert_eq!(vol, 0.6);
        assert_eq!(old_vol, 0.5);
        assert_eq!(
            guild_settings_map
                .read()
                .await
                .get(&guild_id)
                .unwrap()
                .volume,
            vol
        );
    }
}
