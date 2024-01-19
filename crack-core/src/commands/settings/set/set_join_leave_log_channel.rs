use serenity::all::Channel;
use serenity::builder::CreateChannel;

use crate::errors::CrackedError;
use crate::guild::settings::GuildSettingsMap;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Set the join-leave log channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn join_leave_log_channel(
    ctx: Context<'_>,
    #[description = "Channel to send join/leave logs"] channel: Option<Channel>,
    #[description = "ChannelId to send join/leave logs"] channel_id: Option<
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
    // let mut data = ctx.serenity_context().data.write().await;
    // let _entry = &data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .entry(guild_id)
    //     .and_modify(|e| {
    //         e.set_join_leave_log_channel(channel_id.get());
    //     });

    // let settings = data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .get_mut(&guild_id);

    let data = ctx.data();
    let _ = data
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.set_join_leave_log_channel(channel_id.get());
        });

    let opt_settings = data.guild_settings_map.read().unwrap().clone();
    let settings = opt_settings.get(&guild_id);

    let pg_pool = ctx.data().database_pool.clone().unwrap();
    settings.map(|s| s.save(&pg_pool)).unwrap().await?;

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Join-leave log channel set to {}", channel_id)),
    )
    .await?;

    Ok(())
}
