use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::utils::get_guild_name;
use crate::Context;
use crate::Error;

/// Get the current bot settings for this guild.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn get_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    {
        let settings_ro = {
            let mut guild_settings_map = ctx.data().guild_settings_map.lock().unwrap();
            let settings = guild_settings_map
                .entry(guild_id)
                .or_insert(GuildSettings::new(
                    guild_id,
                    Some(ctx.prefix()),
                    get_guild_name(ctx.serenity_context(), guild_id),
                ));
            settings.clone()
        };

        create_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings: {:?}", settings_ro)),
        )
        .await?;
    }

    Ok(())
}
