use crate::{
    errors::CrackedError, guild::settings::GuildSettings, http_utils::CacheHttpExt, Context, Data,
    Error,
};
use serenity::all::GuildId;
use sqlx::PgPool;

/// Toggle the self deafen for the bot.
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    guild_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn selfdeafen(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild_name = ctx.guild_name_from_guild_id(guild_id).await?;
    let res = toggle_self_deafen(
        ctx.data().clone(),
        ctx.data()
            .database_pool
            .clone()
            .ok_or(CrackedError::NoDatabasePool)?,
        guild_id,
        Some(guild_name),
        ctx.data().bot_settings.get_prefix(),
    )
    .await?;

    ctx.say(format!("Self-deafen is now {}", res.self_deafen))
        .await?;
    Ok(())
}

/// Toggle the self deafen for the bot.
#[cfg(not(tarpaulin_include))]
pub async fn toggle_self_deafen(
    data: Data,
    pool: PgPool,
    guild_id: GuildId,
    guild_name: Option<String>,
    prefix: String,
) -> Result<GuildSettings, CrackedError> {
    let res = data
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.toggle_self_deafen();
        })
        .or_insert_with(|| {
            GuildSettings::new(guild_id, Some(&prefix), guild_name)
                .toggle_self_deafen()
                .clone()
        })
        .clone();
    res.save(&pool).await?;
    Ok(res)
}
