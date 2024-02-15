use crate::{messaging::message::CrackedMessage, utils::send_response_poise, Context, Error};

/// Get the current version of the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let reply_with_embed = ctx
        .data()
        .get_guild_settings(guild_id)
        .unwrap()
        .reply_with_embed;
    let current = option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| "Unknown");
    let hash = option_env!("GIT_HASH").unwrap_or_else(|| "Unknown");
    let msg = send_response_poise(
        ctx,
        CrackedMessage::Version {
            current: current.to_owned(),
            hash: hash.to_owned(),
        },
        reply_with_embed,
    )
    .await?;
    ctx.data().add_msg_to_cache(ctx.guild_id().unwrap(), msg);
    Ok(())
}
