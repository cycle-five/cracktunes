use crate::{
    errors::CrackedError, guild::settings::GuildSettings, utils::get_guild_name, Context, Data,
    Error,
};
use serenity::all::GuildId;
use sqlx::PgPool;

/// Toggle the self deafen for the bot.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn self_deafen(ctx: Context<'_>) -> Result<(), Error> {
    let res = toggle_self_deafen(
        ctx.data().clone(),
        ctx.data()
            .database_pool
            .clone()
            .ok_or(CrackedError::NoDatabasePool)?,
        ctx.guild_id().ok_or(CrackedError::NoGuildId)?,
        get_guild_name(ctx.serenity_context(), ctx.guild_id().unwrap()),
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
        .unwrap()
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
    let _ = res.save(&pool).await?;
    Ok(res)
}
