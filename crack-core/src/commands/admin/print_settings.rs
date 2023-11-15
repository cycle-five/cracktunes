use crate::guild::settings::GuildSettingsMap;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;
// pub fn get_reply_handle(ctx: Context) -> ReplyHandle {
//     ctx.reply_handle()
// }
/// Get the current bot settings for this guild.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn print_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_settings_map = ctx.data().guild_settings_map.read().unwrap().clone(); //.unwrap().clone();

    for (guild_id, settings) in guild_settings_map.iter() {
        create_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
        )
        .await?;
    }

    let guild_settings_map = ctx.serenity_context().data.read().await;

    for (guild_id, settings) in guild_settings_map.get::<GuildSettingsMap>().unwrap().iter() {
        create_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
        )
        .await?;
    }
    Ok(())
}
