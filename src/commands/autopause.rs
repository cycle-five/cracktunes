use crate::{
    guild::settings::{GuildSettings, GuildSettingsMap},
    messaging::message::CrackedMessage,
    metrics::COMMAND_EXECUTIONS,
    utils::{create_response, get_interaction},
    Context, Error,
};

/// Toggle autopause at the end of everytrack.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    //COMMAND_EXECUTIONS.with_label_values(&["autopause"]).inc();
    match COMMAND_EXECUTIONS.get_metric_with_label_values(&["autopause"]) {
        Ok(metric) => {
            metric.inc();
        }
        Err(e) => {
            tracing::error!("Failed to get metric: {}", e);
        }
    }
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
            CrackedMessage::AutopauseOn,
        )
        .await
    } else {
        create_response(
            &ctx.serenity_context().http,
            &mut interaction,
            CrackedMessage::AutopauseOff,
        )
        .await
    }
}
