use crate::commands::CrackedError;
use crate::db::GuildEntity;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{Context, Error};

// /// Convenience type for readability.
// type TSGuildSettingsMap = Arc<RwLock<HashMap<GuildId, GuildSettings>>>;

// /// Do the actual settings of the premium status internally.
// pub async fn do_set_premium(
//     guild_id: serenity::model::id::GuildId,
//     guild_name: String,
//     prefix: String,
//     guild_settings_map: TSGuildSettingsMap,
//     premium: bool,
// ) -> Result<GuildSettings, Error> {
//     let mut write_guard = guild_settings_map.write().await;
//     let settings = write_guard
//         .entry(guild_id)
//         .and_modify(|e| {
//             e.premium = premium;
//         })
//         .or_insert(
//             GuildSettings::new(guild_id, Some(&prefix.clone()), Some(guild_name.clone()))
//                 .with_premium(premium),
//         );
//     Ok(settings.clone())
// }

use crate::guild::operations::GuildSettingsOperations;
/// Internal set premium function without #command macro.
#[cfg(not(tarpaulin_include))]
pub async fn set_premium_internal(ctx: Context<'_>, premium: bool) -> Result<(), CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    ctx.data().set_premium(guild_id, premium).await;
    let pool = ctx.data().database_pool.clone().unwrap();
    GuildEntity::update_premium(&pool, guild_id.get() as i64, premium)
        .await
        .unwrap();
    let _ = send_reply(ctx, CrackedMessage::Premium(premium), true).await?;
    Ok(())
}

/// Set the premium status of the guild.
#[poise::command(prefix_command, owners_only)]
#[cfg(not(tarpaulin_include))]
pub async fn premium(
    ctx: Context<'_>,
    #[description = "True or false setting for premium."] premium: bool,
) -> Result<(), Error> {
    set_premium_internal(ctx, premium).await.map_err(Into::into)
}
