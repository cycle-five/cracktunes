use crate::{
    guild::settings::{GuildSettings, GuildSettingsMap},
    messaging::message::CrackedMessage,
    utils::create_response_poise,
    Context, Error,
};

/// Toggle autopause at the end of everytrack.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let mut data = ctx.serenity_context().data.write().await;
    let settings = data.get_mut::<GuildSettingsMap>().unwrap();

    let guild_settings = settings
        .entry(guild_id)
        .or_insert_with(|| GuildSettings::new(guild_id));
    guild_settings.toggle_autopause();
    guild_settings.save()?;

    if guild_settings.autopause {
        create_response_poise(ctx, CrackedMessage::AutopauseOn).await
    } else {
        create_response_poise(ctx, CrackedMessage::AutopauseOff).await
    }
}
