use crate::errors::CrackedError;
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::Channel;

/// Set the join-leave log channel.
#[poise::command(prefix_command, ephemeral, required_permissions = "ADMINISTRATOR")]
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
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.set_join_leave_log_channel(channel_id.get());
        });

    let settings_temp = data.get_guild_settings(guild_id).await;
    let settings = settings_temp.as_ref();

    let pg_pool = ctx.data().database_pool.clone().unwrap();
    settings.map(|s| s.save(&pg_pool)).unwrap().await?;

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Join-leave log channel set to {}", channel_id)),
        true,
    )
    .await?;

    Ok(())
}
