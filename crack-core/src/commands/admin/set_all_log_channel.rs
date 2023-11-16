use serenity::all::Channel;

use crate::{
    commands::set_all_log_channel_old_data, messaging::message::CrackedMessage,
    utils::send_response_poise, Context, Error,
};
/// Set the join-leave log channel.
#[poise::command(prefix_command, owners_only)]
pub async fn set_all_log_channel(
    ctx: Context<'_>,
    #[description = "Channel to send all logs"] channel: Channel,
) -> Result<(), Error> {
    let channel_id = channel.id();
    let guild_id = ctx.guild_id().unwrap();
    // let mut data = ctx.serenity_context().data.write().await;
    // let data = &ctx.serenity_context().data;

    set_all_log_channel_old_data(ctx.serenity_context().data.clone(), guild_id, channel_id).await?;
    // set_all_log_channel_old_data(ctx.data().clone(), guild_id, channel_id).await?;
    // let _entry = &data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .entry(guild_id)
    //     .and_modify(|e| e.set_all_log_channel(channel_id.get()));

    // let settings = data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .get_mut(&guild_id);

    // let _res = settings.map(|s| s.save()).unwrap();

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
    )
    .await?;

    Ok(())
}
