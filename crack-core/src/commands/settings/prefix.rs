use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use crate::{commands::CrackedError, http_utils::CacheHttpExt};

/// Add an additional prefix to the bot for the current guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn add_prefix(
    ctx: Context<'_>,
    #[description = "The prefix to add to the bot"] prefix: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild_name = ctx.guild_name_from_guild_id(guild_id).await?;
    let additional_prefixes = {
        let mut settings = ctx.data().guild_settings_map.write().await;
        let new_settings = settings
            .entry(guild_id)
            .and_modify(|e| {
                e.additional_prefixes = e
                    .additional_prefixes
                    .clone()
                    .into_iter()
                    .chain(vec![prefix.clone()])
                    .collect();
            })
            .or_insert(GuildSettings::new(
                guild_id,
                Some(&prefix.clone()),
                Some(guild_name),
            ));
        new_settings.additional_prefixes.clone()
    };

    let _ = send_reply(&ctx, CrackedMessage::Prefixes(additional_prefixes), true).await?;

    Ok(())
}

/// Clear all additional prefixes from the bot for the current guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    prefix_command,
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn clear_prefixes(ctx: Context<'_>) -> Result<(), Error> {
    use crate::commands::CrackedError;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let mut settings = ctx.data().guild_settings_map.write().await;
    let _ = settings
        .entry(guild_id)
        .and_modify(|e| {
            e.additional_prefixes = vec![];
        })
        .key();
    let _ = send_reply(&ctx, CrackedMessage::Prefixes(vec![]), true).await?;
    Ok(())
}

/// Get the current prefix settings.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    prefix_command,
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn get_prefixes(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let additional_prefixes = {
        let settings = ctx.data().guild_settings_map.read().await;
        settings
            .get(&guild_id)
            .map(|e| e.additional_prefixes.clone())
            .unwrap_or_default()
    };
    let _ = send_reply(&ctx, CrackedMessage::Prefixes(additional_prefixes), true).await?;
    Ok(())
}
