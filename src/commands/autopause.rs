use crate::{
    guild::settings::{GuildSettings, GuildSettingsMap},
    messaging::message::ParrotMessage,
    utils::{create_response, get_interaction},
    Context, Error,
};
use poise::serenity_prelude as serenity;
#[poise::command(slash_command, prefix_command)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    let mut interaction = get_interaction(ctx).unwrap();
    let guild_id = interaction.guild_id.unwrap();
    let mut data = ctx.serenity_context().data.write().await;
    let settings = data.get_mut::<GuildSettingsMap>().unwrap();

    let guild_settings = settings
        .entry(guild_id)
        .or_insert_with(|| GuildSettings::new(guild_id));
    guild_settings.toggle_autopause();
    guild_settings.save()?;

    if guild_settings.autopause {
        create_response(
            &ctx.serenity_context().http,
            &mut interaction,
            ParrotMessage::AutopauseOn,
        )
        .await
    } else {
        create_response(
            &ctx.serenity_context().http,
            &mut interaction,
            ParrotMessage::AutopauseOff,
        )
        .await
    }
}
