use crate::http_utils::CacheHttpExt;
use crate::{errors::CrackedError, guild::settings::GuildSettings, Context, Data, Error};
use serenity::all::GuildId;
use serenity::small_fixed_array::FixedString;
use sqlx::PgPool;
use std::sync::Arc;

/// Toggle autopause for the bot
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    rename = "autopause",
    required_permissions = "ADMINISTRATOR"
)]
#[cfg(not(tarpaulin_include))]
pub async fn toggle_autopause(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild_name = ctx.guild_name_from_guild_id(guild_id).await?;
    let res = toggle_autopause_internal(
        ctx.data(),
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

/// Toggle the autopause for the bot.
#[cfg(not(tarpaulin_include))]
pub async fn toggle_autopause_internal(
    data: Arc<Data>,
    pool: PgPool,
    guild_id: GuildId,
    guild_name: Option<FixedString>,
    prefix: String,
) -> Result<GuildSettings, CrackedError> {
    let res = data
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.toggle_autopause();
        })
        .or_insert_with(|| {
            GuildSettings::new(guild_id, Some(&prefix), guild_name)
                .toggle_autopause()
                .clone()
        })
        .clone();
    res.save(&pool).await?;
    Ok(res)
}
