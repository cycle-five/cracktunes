use crate::commands::{cancel_autoplay, enable_autoplay};
use crate::{messaging::message::CrackedMessage, utils::send_response_poise, Context, Error};

/// Toggle autoplay at the end of the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn toggle_autoplay(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let autoplay = {
        ctx.data()
            .guild_cache_map
            .lock()
            .unwrap()
            .entry(guild_id)
            .or_default()
            .autoplay
    };
    let msg = if autoplay {
        cancel_autoplay(ctx.data(), guild_id).await?;
        send_response_poise(ctx, CrackedMessage::AutoplayOff)
    } else {
        enable_autoplay(ctx.data(), guild_id).await?;
        send_response_poise(ctx, CrackedMessage::AutoplayOn)
    }
    .await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}
