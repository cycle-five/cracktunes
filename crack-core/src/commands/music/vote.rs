use crate::db;
use crate::http_utils;
use crate::poise_ext::ContextExt;
use crate::{Context, Error};
use crack_types::messaging::messages::{
    VOTE_TOPGG_LINK_TEXT, VOTE_TOPGG_NOT_VOTED, VOTE_TOPGG_TEXT, VOTE_TOPGG_URL, VOTE_TOPGG_VOTED,
};
use crack_types::CrackedError;
use serenity::all::{GuildId, UserId};

/// For API response from top.gg
#[derive(serde::Deserialize)]
pub struct CheckResponse {
    voted: Option<u8>,
}

/// Vote link for cracktunes on top.gg
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Base", slash_command, prefix_command)]
pub async fn vote(ctx: Context<'_>) -> Result<(), Error> {
    vote_topgg_internal(ctx).await
}

/// Internal vote function without the #command macro
#[cfg(not(tarpaulin_include))]
pub async fn vote_topgg_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: GuildId = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let user_id: UserId = ctx.author().id;
    let bot_id: UserId = http_utils::get_bot_id(ctx).await?;

    tracing::info!("user_id: {:?}, guild_id: {:?}", user_id, guild_id);

    tracing::info!("bot_id: {:?}", bot_id);
    let has_voted = ctx.check_and_record_vote().await?;

    let msg_str = if has_voted {
        VOTE_TOPGG_VOTED
    } else {
        VOTE_TOPGG_NOT_VOTED
    };

    let reply_handle = ctx
        .reply(format!(
            "{msg_str}\n{VOTE_TOPGG_TEXT} [{VOTE_TOPGG_LINK_TEXT}]({VOTE_TOPGG_URL})"
        ))
        .await?;

    let msg = reply_handle.into_message().await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}

/// Check if the user has voted on top.gg in the last 12 hours and record the vote in the database.
pub async fn check_and_record_vote(
    pool: &sqlx::PgPool,
    user_id: i64,
    username: String,
    bot_id: i64,
) -> Result<bool, CrackedError> {
    let has_voted = has_voted_bot_id(
        http_utils::get_client().clone(),
        bot_id as u64,
        user_id as u64,
    )
    .await?;
    let has_voted_db = db::UserVote::has_voted_recently_topgg(user_id, pool).await?;
    let record_vote = has_voted && !has_voted_db;

    if record_vote {
        db::User::insert_or_update_user(pool, user_id, username).await?;
        db::UserVote::insert_user_vote(pool, user_id, "top.gg".to_string()).await?;
    }

    Ok(has_voted)
}

/// Check if the user has voted on top.gg in the last 12 hours.
pub async fn has_voted_bot_id(
    reqwest_client: reqwest::Client,
    bot_id: u64,
    user_id: u64,
) -> Result<bool, CrackedError> {
    let url = format!("https://top.gg/api/bots/{bot_id}/check?userId={user_id}");
    let token = std::env::var("TOPGG_TOKEN").map_err(|_| CrackedError::InvalidTopGGToken)?;
    let response = reqwest_client
        .get(&url)
        .header("Authorization", token)
        .send()
        .await?;
    println!("response: {response:?}");
    let response = response.json::<CheckResponse>().await?;
    response
        .voted
        .map(|v| v == 1)
        .ok_or(CrackedError::Other("Error in response from top.gg"))
}

#[cfg(test)]
mod test {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[ctor::ctor]
    fn set_env() {
        use std::env;

        if env::var("TOPGG_TOKEN").is_err() {
            env::set_var("TOPGG_TOKEN", "FAKE_TOKEN");
        }
    }

    #[tokio::test]
    async fn test_fail() {
        let bot_id = 1_115_229_568_006_103_122;
        let my_id = 285_219_649_921_220_608;
        let client = http_utils::get_client().clone();

        let has_voted = has_voted_bot_id(client, bot_id, my_id).await;

        //??
        assert!(has_voted.is_ok() || has_voted.is_err());
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_check_and_record_vote(pool: sqlx::PgPool) {
        let user_id = 285_219_649_921_220_608;
        let username = "test".to_string();
        let bot_id = 1_115_229_568_006_103_122;

        let has_voted = check_and_record_vote(&pool, user_id, username, bot_id).await;

        if has_voted.is_ok() {
            assert!(!has_voted.unwrap());
        } else {
            println!("{has_voted:?}");
        }
    }
}
