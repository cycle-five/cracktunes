use crate::db::UserVote;
use crate::{
    messaging::messages::{VOTE_TOPGG_LINK_TEXT, VOTE_TOPGG_TEXT, VOTE_TOPGG_URL},
    Context, Error,
};
use poise::serenity_prelude::GuildId;
use topgg::Client;

/// Vote link for cracktunes on top.gg
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn vote(ctx: Context<'_>) -> Result<(), Error> {
    use crate::errors::CrackedError;

    let guild_id: Option<GuildId> = ctx.guild_id();

    let user_id = ctx.author().id;

    tracing::warn!("user_id: {:?}, guild_id: {:?}", user_id, guild_id);

    // Check if they have voted with the topgg library.
    let client: Client = ctx.data().topgg_client.clone();
    let has_voted = client
        .has_voted(user_id.get().to_string())
        .await
        .map_err(|e| {
            tracing::error!("Error checking if user has voted: {:?}", e);
            CrackedError::InvalidTopGGToken
        })?;

    let has_voted_db = UserVote::has_voted_recently_topgg(
        user_id.get() as i64,
        ctx.data().database_pool.as_ref().unwrap(),
    )
    .await
    .unwrap_or(false);

    let record_vote = has_voted && !has_voted_db;

    if record_vote {
        UserVote::insert_user_vote(
            ctx.data().database_pool.as_ref().unwrap(),
            user_id.get() as i64,
            "top.gg".to_string(),
        )
        .await?;
    }

    let msg_str = if has_voted {
        "Thank you for voting! Remember to vote again in 12 hours!"
    } else {
        "You haven't voted recently! Here is the link to vote :)"
    };

    let reply_handle = ctx
        .reply(format!(
            "{}\n{} [{}]({})",
            msg_str, VOTE_TOPGG_TEXT, VOTE_TOPGG_LINK_TEXT, VOTE_TOPGG_URL
        ))
        .await?;

    let msg = reply_handle.into_message().await?;

    guild_id.map(|id| ctx.data().add_msg_to_cache(id, msg));

    Ok(())
}
