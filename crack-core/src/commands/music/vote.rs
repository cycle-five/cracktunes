use crate::db;
use crate::errors::CrackedError;
use crate::http_utils;
use crate::{
    messaging::messages::{
        VOTE_TOPGG_LINK_TEXT, VOTE_TOPGG_NOT_VOTED, VOTE_TOPGG_TEXT, VOTE_TOPGG_URL,
        VOTE_TOPGG_VOTED,
    },
    Context, Error,
};
use serenity::all::{GuildId, UserId};

/// Vote link for cracktunes on top.gg
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn vote(ctx: Context<'_>) -> Result<(), Error> {
    vote_(ctx).await
}

/// Internal vote function without the #command macro
pub async fn vote_(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: Option<GuildId> = ctx.guild_id();

    let user_id: UserId = ctx.author().id;

    tracing::info!("user_id: {:?}, guild_id: {:?}", user_id, guild_id);

    let bot_id: UserId = http_utils::get_bot_id(ctx.http()).await?;
    tracing::info!("bot_id: {:?}", bot_id);
    let has_voted: bool =
        has_voted_bot_id(ctx.data().http_client.clone(), bot_id.get(), user_id.get()).await?;
    tracing::info!("has_voted: {:?}", has_voted);

    let has_voted_db = db::UserVote::has_voted_recently_topgg(
        user_id.get() as i64,
        ctx.data().database_pool.as_ref().unwrap(),
    )
    .await?;
    tracing::info!("has_voted_db: {:?}", has_voted_db);

    let record_vote = has_voted && !has_voted_db;

    if record_vote {
        let username = ctx.author().name.clone();
        tracing::info!("username: {:?}", username);
        db::User::insert_or_update_user(
            ctx.data().database_pool.as_ref().unwrap(),
            user_id.get() as i64,
            username,
        )
        .await?;
        db::UserVote::insert_user_vote(
            ctx.data().database_pool.as_ref().unwrap(),
            user_id.get() as i64,
            "top.gg".to_string(),
        )
        .await?;
    }

    let msg_str = if has_voted {
        VOTE_TOPGG_VOTED
    } else {
        VOTE_TOPGG_NOT_VOTED
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
    voted: u8,
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
    Ok(response.voted == 1_u8)
}

#[cfg(test)]
mod test {
    use super::*;

    #[ctor::ctor]
    fn set_env() {
        use std::env;

        if env::var("TOPGG_TOKEN").is_err() {
            env::set_var("TOPGG_TOKEN", "FAKE_TOKEN");
        }
    }

    #[tokio::test]
    async fn test_fail() {
        let bot_id = 1115229568006103122;
        let my_id = 285219649921220608;
        let client = http_utils::get_client().clone();

        let has_voted = has_voted_bot_id(client, bot_id, my_id).await;

        assert!(has_voted.is_err());
    }
}
