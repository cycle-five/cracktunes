use serenity::all::Channel;

use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_response_poise, Context,
    Error,
};

#[poise::command(prefix_command, required_permissions = "ADMINISTRATOR")]
pub async fn music_channel(
    ctx: Context<'_>,
    #[description = "Channel to respond to music commands in."] channel: Option<Channel>,
    #[description = "ChannelId of Channel to respond to music commands in."] channel_id: Option<
        serenity::model::id::ChannelId,
    >,
) -> Result<(), Error> {
    if channel.is_none() && channel_id.is_none() {
        return Err(CrackedError::Other("Must provide either a channel or a channel id").into());
    }
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let channel_id = if let Some(channel) = channel {
        channel.id()
    } else {
        channel_id.unwrap()
    };

    let data = ctx.data();
    let _ = data
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.set_music_channel(channel_id.get());
        });

    let opt_settings = data.guild_settings_map.read().unwrap().clone();
    let settings = opt_settings.get(&guild_id);

    let pg_pool = ctx.data().database_pool.clone().unwrap();
    settings.map(|s| s.save(&pg_pool)).unwrap().await?;

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Music channel set to {}", channel_id)),
        true,
    )
    .await?;

    Ok(())
}
