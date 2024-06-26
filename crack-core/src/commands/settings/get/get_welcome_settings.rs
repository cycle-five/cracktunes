use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::PoiseContextExt;
use crate::utils::get_guild_name;
use crate::{Context, Error};

#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Settings", slash_command, prefix_command, owners_only)]
pub async fn welcome_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let welcome_settings = {
        let mut guild_settings_map = ctx.data().guild_settings_map.write().await;
        let settings = guild_settings_map
            .entry(guild_id)
            .or_insert(GuildSettings::new(
                guild_id,
                Some(ctx.prefix()),
                get_guild_name(ctx.serenity_context(), guild_id).await,
            ));
        settings.welcome_settings.clone()
    };

    ctx.send_reply(
        CrackedMessage::WelcomeSettings(welcome_settings.unwrap_or_default().to_string()),
        true,
    )
    .await?;

    Ok(())
}
