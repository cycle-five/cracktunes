use crate::{
    guild::settings::{GuildSettings, WelcomeSettings},
    utils::get_guild_name,
    Context, Data, Error,
};
use crack_types::CrackedError;
use serenity::all::{Channel, GuildId, Role};
use std::sync::Arc;

/// Set password verification for the server.
#[poise::command(
    category = "Settings",
    prefix_command,
    slash_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "SEND_MESSAGES|MANAGE_ROLES"
)]
#[cfg(not(tarpaulin_include))]
pub async fn password_verify(
    ctx: Context<'_>,
    #[description = "The channel to use for verification message"] channel: Channel,
    #[description = "Password to verify"] password: String,
    #[description = "Role to add after successful verification"] auto_role: Role,
    #[rest]
    #[description = "Welcome message template use {user} for username"]
    message: String,
) -> Result<(), Error> {
    let prefix = ctx.data().bot_settings.get_prefix();
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild_name = get_guild_name(&ctx, guild_id).await;
    let welcome_settings = WelcomeSettings {
        channel_id: Some(channel.id().get()),
        message: Some(message.clone()),
        auto_role: Some(auto_role.id.get()),
        password: Some(password),
    };
    let msg = set_welcome_settings(
        ctx.data().clone(),
        guild_id,
        guild_name,
        prefix.to_string(),
        welcome_settings,
    )
    .await?;
    ctx.say(msg).await?;
    Ok(())
}

/// Set the welcome settings for the server.
#[poise::command(
    category = "Settings",
    prefix_command,
    slash_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "SEND_MESSAGES|MANAGE_ROLES"
)]
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
    let guild_name = get_guild_name(&ctx, guild_id).await;
    let welcome_settings = WelcomeSettings {
        channel_id: Some(channel.id().get()),
        message: Some(message.clone()),
        auto_role: None,
        password: None,
    };
    let msg = set_welcome_settings(
        ctx.data(),
        guild_id,
        guild_name,
        prefix.to_string(),
        welcome_settings,
    )
    .await?;
    ctx.say(msg).await?;
    Ok(())
}

use serenity::small_fixed_array::FixedString;
/// Set the welcome settings for a given guild.
#[cfg(not(tarpaulin_include))]
pub async fn set_welcome_settings(
    data: Arc<Data>,
    guild_id: GuildId,
    guild_name: Option<FixedString>,
    prefix: String,
    init_welcome_settings: WelcomeSettings,
) -> Result<String, CrackedError> {
    let guild_settings_map = &data.guild_settings_map;
    let channel_id = init_welcome_settings.channel_id.unwrap();
    let message = init_welcome_settings.message.as_ref().unwrap();
    let res = {
        let mut write_guard = guild_settings_map.write().await;
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
            if let Some(pool) = &data.database_pool.clone() { welcome_settings.save(pool, guild_id.get()).await? } else { tracing::warn!("No database pool to save welcome settings") }
            Ok(welcome_settings.to_string())
        },
        None => Err(CrackedError::Other("Welcome settings failed to update?!?")),
    }
}

#[cfg(test)]
mod test {
    use super::set_welcome_settings;
    use crate::guild::settings::{GuildSettingsMapParam, WelcomeSettings};
    use crack_types::to_fixed;
    use serenity::model::id::GuildId;

    #[tokio::test]
    async fn test_set_welcome_settings() {
        let guild_id = GuildId::new(1);
        let guild_settings_map = GuildSettingsMapParam::default();
        let data = crate::Data::default().with_guild_settings_map(guild_settings_map);
        let guild_name = Some(to_fixed("Test"));
        let prefix = "!".to_string();
        let init_welcome_settings = WelcomeSettings {
            channel_id: Some(1),
            message: Some("Welcome {user}!".to_string()),
            ..Default::default()
        };

        let res = set_welcome_settings(
            data.into(),
            guild_id,
            guild_name,
            prefix,
            init_welcome_settings,
        )
        .await;
        assert!(res.is_ok());
    }
}
