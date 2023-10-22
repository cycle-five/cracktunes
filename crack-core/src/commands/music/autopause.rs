use crate::{
    guild::settings::GuildSettings,
    messaging::message::CrackedMessage,
    utils::{create_response_poise, get_guild_name},
    Context, Error,
};

/// Toggle autopause at the end of everytrack.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    let prefix = ctx.data().bot_settings.get_prefix();
    let guild_id = ctx.guild_id().unwrap();
    // ctx.data().guild_settings_map.lock().unwrap().entry(guild_id).or_insert_with(|| {
    //     GuildSettings::new(
    //         guild_id,
    //         Some(&prefix),
    //         get_guild_name(ctx.serenity_context(), guild_id),
    //     )
    // });
    // let mut data = ctx.serenity_context().data.write().await;
    {
        let mut settings = ctx.data().guild_settings_map.lock().unwrap();

        let guild_settings = settings.entry(guild_id).or_insert_with(|| {
            GuildSettings::new(
                guild_id,
                Some(&prefix),
                get_guild_name(ctx.serenity_context(), guild_id),
            )
        });
        guild_settings.toggle_autopause();
        guild_settings.save()?;

        if guild_settings.autopause {
            create_response_poise(ctx, CrackedMessage::AutopauseOn)
        } else {
            create_response_poise(ctx, CrackedMessage::AutopauseOff)
        }
    }
    .await?;
    Ok(())
}
