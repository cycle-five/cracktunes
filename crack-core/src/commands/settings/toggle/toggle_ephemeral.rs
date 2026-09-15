use crate::http_utils::CacheHttpExt;
use crate::messaging::messages::{EPHEMERAL_REPLIES_OFF, EPHEMERAL_REPLIES_ON};
use crate::{errors::CrackedError, guild::settings::GuildSettings, Context, Data, Error};
use serenity::all::GuildId;
use serenity::small_fixed_array::FixedString;
use sqlx::PgPool;
use std::sync::Arc;

/// Toggle whether /play, /skip and /nowplaying reply privately.
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    rename = "ephemeral",
    required_permissions = "ADMINISTRATOR"
)]
#[cfg(not(tarpaulin_include))]
pub async fn toggle_ephemeral(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild_name = ctx.guild_name_from_guild_id(guild_id).await?;
    let res = toggle_ephemeral_internal(
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

    let reply = if res.ephemeral_replies {
        EPHEMERAL_REPLIES_ON
    } else {
        EPHEMERAL_REPLIES_OFF
    };
    ctx.say(reply).await?;
    Ok(())
}

/// Toggle ephemeral replies for a guild and save it.
#[cfg(not(tarpaulin_include))]
pub async fn toggle_ephemeral_internal(
    data: Arc<Data>,
    pool: PgPool,
    guild_id: GuildId,
    guild_name: Option<FixedString>,
    prefix: String,
) -> Result<GuildSettings, CrackedError> {
    // 🔑 Before mutating: make sure what is in memory came from Postgres. A
    // guild whose boot load failed holds defaults, and `save()` below is a
    // full-row upsert that would write them over its stored row.
    data.ensure_settings_loaded(guild_id).await?;

    let res = data
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.toggle_ephemeral_replies();
        })
        .or_insert_with(|| {
            GuildSettings::new(guild_id, Some(&prefix), guild_name)
                .toggle_ephemeral_replies()
                .clone()
        })
        .clone();
    res.save(&pool).await?;
    Ok(res)
}
