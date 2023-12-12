use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
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
    let guild_name = get_guild_name(ctx.serenity_context(), guild_id).unwrap_or_default();
    let additional_prefixes = {
        let mut settings = ctx.data().guild_settings_map.write().unwrap();
        // let additional_prefixes = settings
        //     .get(&guild_id)
        //     .map(|e| e.additional_prefixes.clone())
        //     .expect("Guild seetings settings found");
        let new_settings = settings
            .entry(guild_id)
            .and_modify(|e| {
                e.additional_prefixes = e
                    .additional_prefixes
                    .clone()
                    .into_iter()
                    .chain(vec![prefix.clone()])
                    .collect();
            })
            .or_insert(GuildSettings::new(
                guild_id,
                Some(&prefix.clone()),
                Some(guild_name),
            ));
        new_settings.additional_prefixes.clone()
    };
    send_response_poise(
        ctx,
        CrackedMessage::Other(format!(
            "Current additional prefixes {}",
            additional_prefixes.join(", ")
        )),
    )
    .await?;
    Ok(())
}
