use serenity::all::Channel;
use serenity::all::Role;

use crate::guild::settings::WelcomeSettings;
use crate::Context;
use crate::Error;

#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn set_welcome_settings(
    ctx: Context<'_>,
    #[description = "The channel to send welcome messages"] channel: Channel,
    // #[description = "The role to assign to new users"] auto_role: Role,
    // #[description = "The role to assign to new users"] auto_role: u64,
    #[rest]
    #[description = "Welcome message template use {user} for username"]
    message: String,
) -> Result<(), Error> {
    let welcome_settings = WelcomeSettings {
        channel_id: Some(channel.id().get()),
        message: Some(message.clone()),
        auto_role: None, // auto_role: Some(auto_role.id.get()), // auto_role.map(|x| x.id.get()),
    };
    let _res = ctx
        .data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .and_modify(|e| {
            e.set_welcome_settings3(
                welcome_settings.channel_id.unwrap(),
                welcome_settings.message.unwrap(),
            );
        });
    Ok(())
}

#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn set_auto_role(
    ctx: Context<'_>,
    #[description = "The role to assign to new users"] auto_role: Role,
) -> Result<(), Error> {
    let _res = ctx
        .data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .and_modify(|e| {
            e.set_auto_role(Some(auto_role.id.get()));
        });
    Ok(())
}
