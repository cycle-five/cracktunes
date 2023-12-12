use crate::db::GuildEntity;
use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Set the prefix for the bot.
#[poise::command(prefix_command, owners_only)]
pub async fn set_premium(
    ctx: Context<'_>,
    #[description = "True or false setting for premium."] set_premium: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let guild_name = get_guild_name(ctx.serenity_context(), guild_id).unwrap_or_default();
    let prefix = ctx.data().bot_settings.get_prefix();

    let premium = set_premium.parse::<bool>()?;
    let settings = {
        let mut write_guard = ctx.data().guild_settings_map.write().unwrap();
        let settings = write_guard
            .entry(guild_id)
            .and_modify(|e| {
                e.premium = premium;
            })
            .or_insert(
                GuildSettings::new(guild_id, Some(&prefix.clone()), Some(guild_name.clone()))
                    .with_premium(set_premium.parse::<bool>().unwrap_or(false)),
            );
        settings.clone()
    };
    let pool = ctx.data().database_pool.clone().unwrap();
    GuildEntity::write_settings(&pool, &settings).await.unwrap();
    send_response_poise(ctx, CrackedMessage::Premium(premium)).await?;
    Ok(())
}
