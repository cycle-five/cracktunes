use serenity::all::Channel;

use crate::guild::settings::WelcomeSettings;
use crate::Context;
use crate::Error;

#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn set_welcome_settings(
    ctx: Context<'_>,
    #[description = "The channel to send welcome messages"] channel: Channel,
    #[description = "Welcome message template use {user} for username"] message: String,
) -> Result<(), Error> {
    let welcome_settings = WelcomeSettings {
        channel_id: Some(channel.id().get()),
        message: Some(message.clone()),
        auto_role: None,
    };
    let _res = ctx
        .data()
        .guild_settings_map
        .lock()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .and_modify(|e| {
            e.welcome_settings = Some(welcome_settings.clone());
        });
    Ok(())
}
