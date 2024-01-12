use crate::{
    errors::CrackedError, guild::settings::GuildSettings, utils::get_guild_name, Context, Error,
};

/// Toggle the self deafen for the bot.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn toggle_self_deafen(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let res = ctx
        .data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .and_modify(|e| {
            e.toggle_self_deafen();
        })
        .or_insert_with(|| {
            GuildSettings::new(
                ctx.guild_id().unwrap(),
                Some(&ctx.data().bot_settings.get_prefix()),
                get_guild_name(ctx.serenity_context(), guild_id),
            )
            .toggle_self_deafen()
            .clone()
        })
        .clone();
    res.save(&ctx.data().database_pool.clone().unwrap()).await?;

    ctx.say(format!("Self-deafen is now {}", res.self_deafen))
        .await?;
    Ok(())
}
