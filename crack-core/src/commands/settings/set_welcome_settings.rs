use crate::{
    errors::CrackedError,
    guild::settings::{GuildSettings, WelcomeSettings},
    utils::get_guild_name,
    Context, Error,
};
use serenity::all::Channel;

/// Set the welcome settings for the server.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn set_welcome_settings(
    ctx: Context<'_>,
    #[description = "The channel to send welcome messages"] channel: Channel,
    #[rest]
    #[description = "Welcome message template use {user} for username"]
    message: String,
) -> Result<(), Error> {
    let prefix = ctx.data().bot_settings.get_prefix();
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let welcome_settings = WelcomeSettings {
        channel_id: Some(channel.id().get()),
        message: Some(message.clone()),
        auto_role: None,
    };
    let msg = {
        let res = {
            let mut write_guard = ctx.data().guild_settings_map.write().unwrap();
            write_guard
                .entry(guild_id)
                .and_modify(|e| {
                    e.set_welcome_settings3(channel.id().get(), message.clone());
                })
                .or_insert_with(|| {
                    // GuildEntity::new_guild()
                    GuildSettings::new(
                        guild_id,
                        Some(&prefix),
                        get_guild_name(ctx.serenity_context(), guild_id),
                    )
                    .with_welcome_settings(Some(welcome_settings))
                })
                .welcome_settings
                .clone()
        };
        match res {
            Some(welcome_settings) => {
                welcome_settings
                    .save(&ctx.data().database_pool.clone().unwrap(), guild_id.get())
                    .await?;
                welcome_settings.to_string()
            }
            None => "Welcome settings failed to update?!?".to_string(),
        }
    };
    ctx.say(msg).await?;
    Ok(())
}
