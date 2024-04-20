use crate::db::UserVote;
use crate::errors::CrackedError;
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
    let has_voted = client.has_voted(user_id.get()).await.map_err(|e| {
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

#[derive(serde::Deserialize)]
pub struct CheckResponse {
    voted: bool,
    error: Option<String>,
}
/// Check if the user has voted on top.gg in the last 12 hours.
pub async fn has_voted_bot_id(
    reqwest_client: reqwest::Client,
    bot_id: u64,
    user_id: u64,
) -> Result<bool, CrackedError> {
    let url = format!(
        "https://top.gg/api/bots/{}/check?userId={}",
        bot_id, user_id
    );
    let token = std::env::var("TOPGG_TOKEN").map_err(|_| CrackedError::InvalidTopGGToken)?;
    let response = reqwest_client
        .get(&url)
        .header("Authorization", token)
        .send()
        .await?
        .json::<CheckResponse>()
        .await?;
    Ok(response.voted)
}

#[cfg(test)]
mod test {
    use super::*;

    #[ctor::ctor]
    fn set_env() {
        use std::env;

        env::set_var("TOPGG_TOKEN", "XXXX");
        env::set_var("TOPGG_TOKEN", "XXXX");
    }

    #[tokio::test]
    async fn test_topgg_api() {
        let token = std::env::var("TOPGG_TOKEN").unwrap();
        let bot_id = 1115229568006103122;
        let my_id = 285219649921220608;
        let client = reqwest::Client::new();

        let has_voted = has_voted_bot_id(client, bot_id, my_id).await.unwrap();

        assert!(has_voted);
    }
}
