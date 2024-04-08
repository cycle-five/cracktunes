use crate::{messaging::messages::VOTE_TOPGG, Context, Error};
use poise::serenity_prelude::GuildId;

/// Vote link for cracktunes on top.gg
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn vote(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: Option<GuildId> = ctx.guild_id();

    let reply_handle = ctx.reply(VOTE_TOPGG).await?;

    let msg = reply_handle.into_message().await?;

    guild_id.map(|id| ctx.data().add_msg_to_cache(id, msg));

    Ok(())
}

// /// Check when the calling user last voted for cracktunes.
// #[cfg(not(tarpaulin_include))]
// #[poise::command(slash_command, prefix_command)]
// pub async fn check_last_vote(ctx: Context<'_>) -> Result<(), Error> {
//     let guild_id: Option<GuildId> = ctx.guild_id();

//     let reply_handle = ctx.reply(VOTE_TOPGG).await?;

//     let msg = reply_handle.into_message().await?;

//     guild_id.map(|id| ctx.data().add_msg_to_cache(id, msg));

//     Ok(())
// }
