use crate::{
    messaging::messages::{VOTE_TOPGG_LINK_TEXT, VOTE_TOPGG_TEXT, VOTE_TOPGG_URL},
    Context, Error,
};
use poise::serenity_prelude::GuildId;

/// Vote link for cracktunes on top.gg
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn vote(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: Option<GuildId> = ctx.guild_id();

    let reply_handle = ctx
        .reply(format!(
            "{} [{}]({})",
            VOTE_TOPGG_TEXT, VOTE_TOPGG_LINK_TEXT, VOTE_TOPGG_URL
        ))
        .await?;

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

async fn has_voted_recently(user_id: i64, pool: &PgPool) -> bool {
    let twelve_hours_ago = Utc::now().naive_utc() - Duration::hours(12);
    let site_name = "Top.gg"; // Define the site you are checking for votes from

    let result = sqlx::query_as!(
        UserVote,
        "SELECT * FROM user_votes WHERE user_id = $1 AND timestamp > $2 AND site = $3",
        user_id,
        twelve_hours_ago,
        site_name
    )
    .fetch_optional(pool)
    .await
    .expect("Failed to execute query");

    result.is_some()
}
