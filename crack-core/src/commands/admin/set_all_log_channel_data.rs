use serenity::all::ChannelId;
use serenity::all::GuildId;

use crate::guild::settings::GuildSettings;
use crate::guild::settings::DEFAULT_PREFIX;
use crate::Data;
use crate::Error;

pub async fn set_all_log_channel_data(
    data: Data,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<GuildSettings, Error> {
    //let mut data = map.write().await;
    //let entry = data
    Ok(data
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.set_all_log_channel(channel_id.into());
        })
        .or_insert({
            GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), None)
                .set_all_log_channel(channel_id.into())
                .to_owned()
        })
        .to_owned())

    //Ok(entry.clone())
    // let settings = data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .get_mut(&guild_id);

    // send_response_poise(
    //     ctx,
    //     CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
    // )
    // .await?;

    //    Ok(())
}
