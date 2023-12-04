use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Set the prefix for the bot.
#[poise::command(prefix_command, owners_only)]
pub async fn add_prefix(
    ctx: Context<'_>,
    #[description = "The prefix to add to the bot"] prefix: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    ctx.data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.additional_prefixes = e
                .additional_prefixes
                .clone()
                .into_iter()
                .chain(vec![prefix.clone()])
                .collect();
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
    let additional_prefixes = ctx
        .data()
        .guild_settings_map
        .read()
        .unwrap()
        .get(&guild_id)
        .unwrap()
        .additional_prefixes
        .clone()
        .join(", ");

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!(
            "Current additional prefixes {}",
            additional_prefixes
        )),
    )
    .await?;

    Ok(())
}
