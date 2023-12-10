use crate::Data;
use crate::{
    guild::settings::GuildSettings,
    messaging::message::CrackedMessage,
    utils::{get_guild_name, send_response_poise},
    Context, Error,
};

/// Toggle autopause at the end of everytrack.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    let prefix = ctx.data().bot_settings.get_prefix();
    let guild_id = ctx.guild_id().unwrap();

    let autopause = {
        let mut guild_settings = {
            let mut settings = ctx.data().guild_settings_map.write().await;
            settings
                .entry(guild_id)
                .or_insert_with(|| {
                    GuildSettings::new(
                        guild_id,
                        Some(&prefix),
                        get_guild_name(ctx.serenity_context(), guild_id),
                    )
                })
                .clone()
        };
        guild_settings.toggle_autopause();
        let data: &mut Data = &mut ctx.data().clone();
        guild_settings.save(data.get_db_pool()).await?;
        guild_settings.autopause
    };
    if autopause {
        send_response_poise(ctx, CrackedMessage::AutopauseOn)
    } else {
        send_response_poise(ctx, CrackedMessage::AutopauseOff)
    }
    .await
    .map(|_| ())
    //  Ok(())
}
