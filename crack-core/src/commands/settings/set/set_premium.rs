use serenity::all::GuildId;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use crate::db::GuildEntity;
use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_response_poise;
use crate::{Context, Error};

/// Convenience type for readability.
type TSGuildSettingsMap = Arc<RwLock<HashMap<GuildId, GuildSettings>>>;

/// Do the actual settings of the premium status internally.
pub async fn do_set_premium(
    guild_id: serenity::model::id::GuildId,
    guild_name: String,
    prefix: String,
    guild_settings_map: TSGuildSettingsMap,
    premium: bool,
) -> Result<GuildSettings, Error> {
    let mut write_guard = guild_settings_map.write().await;
    let settings = write_guard
        .entry(guild_id)
        .and_modify(|e| {
            e.premium = premium;
        })
        .or_insert(
            GuildSettings::new(guild_id, Some(&prefix.clone()), Some(guild_name.clone()))
                .with_premium(premium),
        );
    Ok(settings.clone())
}

/// Internal set premium function without #command macro.
#[cfg(not(tarpaulin_include))]
pub async fn set_premium_(ctx: Context<'_>, premium: bool) -> Result<GuildSettings, Error> {
    let guild_id = ctx.guild_id().unwrap();
    let guild_name = get_guild_name(ctx.serenity_context(), guild_id).unwrap_or_default();
    let prefix = ctx.data().bot_settings.get_prefix();

    // here premium is parsed
    let guild_settings_map = ctx.data().guild_settings_map.clone(); // Clone the guild_settings_map
    let settings = do_set_premium(
        guild_id,
        guild_name.clone(),
        prefix,
        guild_settings_map, // Use the cloned guild_settings_map
        premium,
    )
    .await?;
    let pool = ctx.data().database_pool.clone().unwrap();
    GuildEntity::update_premium(&pool, settings.guild_id.get() as i64, premium)
        .await
        .unwrap();
    send_response_poise(ctx, CrackedMessage::Premium(premium), true).await?;
    Ok(settings)
}

/// Set the premium status of the guild.
#[poise::command(prefix_command, owners_only)]
#[cfg(not(tarpaulin_include))]
pub async fn premium(
    ctx: Context<'_>,
    #[description = "True or false setting for premium."] premium: bool,
) -> Result<(), Error> {
    set_premium_(ctx, premium).await.map(|_| ())
}

#[cfg(test)]
mod test {
    use super::*;

    /// Test setting premium of the Guild settings structure
    #[tokio::test]
    async fn test_set_premium() {
        let guild_id = GuildId::new(1);
        let guild_name = "test".to_string();
        let prefix = "!".to_string();
        let guild_settings_map = HashMap::<GuildId, GuildSettings>::default(); // Change the type to GuildSettingsMap
        let params = Arc::new(RwLock::new(guild_settings_map));
        let settings = do_set_premium(guild_id, guild_name, prefix, params, true)
            .await
            .unwrap();
        assert_eq!(settings.premium, true);
    }
}
