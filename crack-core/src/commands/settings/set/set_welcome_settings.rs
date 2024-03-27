use crate::{
    errors::CrackedError,
    guild::settings::{GuildSettings, WelcomeSettings},
    utils::get_guild_name,
    Context, Data, Error,
};
use serenity::all::{Channel, GuildId};

/// Set the welcome settings for the server.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn welcome_settings(
    ctx: Context<'_>,
    #[description = "The channel to send welcome messages"] channel: Channel,
    #[rest]
    #[description = "Welcome message template use {user} for username"]
    message: String,
) -> Result<(), Error> {
    let prefix = ctx.data().bot_settings.get_prefix();
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let welcome_settings = WelcomeSettings {
        channel_id: Some(channel.id().get()),
        message: Some(message.clone()),
        auto_role: None,
    };
    // let msg = {
    //     let res = {
    //         let mut write_guard = ctx.data().guild_settings_map.write().unwrap();
    //         write_guard
    //             .entry(guild_id)
    //             .and_modify(|e| {
    //                 e.set_welcome_settings3(channel.id().get(), message.clone());
    //             })
    //             .or_insert_with(|| {
    //                 // GuildEntity::new_guild()
    //                 GuildSettings::new(
    //                     guild_id,
    //                     Some(&prefix),
    //                     get_guild_name(ctx.serenity_context(), guild_id),
    //                 )
    //                 .with_welcome_settings(Some(welcome_settings))
    //             })
    //             .welcome_settings
    //             .clone()
    //     };
    //     match res {
    //         Some(welcome_settings) => {
    //             welcome_settings
    //                 .save(&ctx.data().database_pool.clone().unwrap(), guild_id.get())
    //                 .await?;
    //             welcome_settings.to_string()
    //         }
    //         None => "Welcome settings failed to update?!?".to_string(),
    //     }
    // };
    let msg = set_welcome_settings(
        ctx.data().clone(),
        guild_id,
        get_guild_name(ctx.serenity_context(), guild_id),
        prefix.to_string(),
        welcome_settings,
    )
    .await?;
    ctx.say(msg).await?;
    Ok(())
}

pub async fn set_welcome_settings(
    data: Data,
    guild_id: GuildId,
    guild_name: Option<String>,
    prefix: String,
    init_welcome_settings: WelcomeSettings,
) -> Result<String, CrackedError> {
    let guild_settings_map = &data.guild_settings_map;
    let channel_id = init_welcome_settings.channel_id.unwrap();
    let message = init_welcome_settings.message.as_ref().unwrap();
    let res = {
        let mut write_guard = guild_settings_map.write().unwrap();
        write_guard
            .entry(guild_id)
            .and_modify(|e| {
                e.set_welcome_settings3(channel_id, message.clone());
            })
            .or_insert_with(|| {
                // GuildEntity::new_guild()
                GuildSettings::new(guild_id, Some(&prefix), guild_name)
                    .with_welcome_settings(Some(init_welcome_settings))
            })
            .welcome_settings
            .clone()
    };
    match res {
        Some(welcome_settings) => {
            match &data.database_pool.clone() {
                Some(pool) => welcome_settings.save(pool, guild_id.get()).await?,
                None => tracing::warn!("No database pool to save welcome settings"),
            }
            Ok(welcome_settings.to_string())
        }
        None => Err(CrackedError::Other("Welcome settings failed to update?!?")),
    }
}

#[cfg(test)]
mod test {
    use super::set_welcome_settings;
    use crate::guild::settings::{GuildSettingsMapParam, WelcomeSettings};
    use serenity::model::id::GuildId;
    use tokio;

    #[tokio::test]
    async fn test_set_welcome_settings() {
        let guild_id = GuildId::new(1);
        let guild_settings_map = GuildSettingsMapParam::default();
        let data = crate::Data::default().with_guild_settings_map(guild_settings_map);
        let guild_name = Some("Test".to_string());
        let prefix = "!".to_string();
        let init_welcome_settings = WelcomeSettings {
            channel_id: Some(1),
            message: Some("Welcome {user}!".to_string()),
            auto_role: None,
        };

        let res =
            set_welcome_settings(data, guild_id, guild_name, prefix, init_welcome_settings).await;
        assert!(res.is_ok());
    }
}
