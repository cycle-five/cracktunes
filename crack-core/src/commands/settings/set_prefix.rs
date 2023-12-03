use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Set the prefix for the bot.
#[poise::command(prefix_command, owners_only)]
pub async fn set_prefix(
    ctx: Context<'_>,
    #[description = "The prefix to set for the bot"] prefix: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    ctx.data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.prefix = prefix.clone();
        });
    // ctx.serenity_context()
    //     .data
    //     .write()
    //     .await
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .entry(guild_id)
    //     .and_modify(|e| {
    //         e.prefix = prefix.clone();
    //         e.prefix_up = prefix.to_uppercase();
    //     });
    // let _entry = &data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .entry(guild_id)
    //     .and_modify(|e| e.prefix = prefix.clone())
    //     .and_modify(|e| e.prefix_up = prefix.to_uppercase());

    // let settings = data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .get_mut(&guild_id);

    // let _res = settings.map(|s| s.save()).unwrap();

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Prefix set to {}", prefix)),
    )
    .await?;

    Ok(())
}
