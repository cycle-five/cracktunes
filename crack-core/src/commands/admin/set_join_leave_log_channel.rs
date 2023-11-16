use serenity::builder::CreateChannel;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Set the join-leave log channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn set_join_leave_log_channel(
    ctx: Context<'_>,
    #[description = "Channel to send join/leave logs"] channel: Channel,
) -> Result<(), Error> {
    let channel_id = channel.id();
    let guild_id = ctx.guild_id().unwrap();
    let mut data = ctx.serenity_context().data.write().await;
    let _entry = &data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.set_join_leave_log_channel(channel_id.get()));

    let settings = data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .get_mut(&guild_id);

    let _res = settings.map(|s| s.save()).unwrap();

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Join-leave log channel set to {}", channel_id)),
    )
    .await?;

    Ok(())
}
