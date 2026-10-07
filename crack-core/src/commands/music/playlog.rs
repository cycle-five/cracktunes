use crate::commands::cmd_check_music;
use crate::messaging::courier;
use crate::messaging::format::escape;
use crate::messaging::message::CrackedMessage;
use crate::utils::{create_paged_embed, PagedStyle};
use crate::{poise_ext::ContextExt, Context, Error};

/// Get recently played tracks form the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn playlog(ctx: Context<'_>) -> Result<(), Error> {
    playlog_internal(ctx).await
}

/// The play log's entries are track titles and artists from third-party
/// metadata: escaped, one line each, before they reach a message.
fn escaped_log(entries: &[String]) -> Vec<String> {
    entries.iter().map(|e| escape(e)).collect()
}

/// Get recently played tracks for the guild.
#[cfg(not(tarpaulin_include))]
pub async fn playlog_internal(ctx: Context<'_>) -> Result<(), Error> {
    let last_played = ctx.get_last_played().await?;
    let last_played_str = escaped_log(&last_played).join("\n");

    create_paged_embed(
        ctx,
        ctx.author().name.clone(),
        "Playlog".to_string(),
        last_played_str,
        756,
        PagedStyle::default(),
    )
    .await?;
    // let _ = send_reply(&ctx, CrackedMessage::PlayLog(last_played), true).await?;

    Ok(())
}

/// Get recently played tracks for the calling user in the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn myplaylog(ctx: Context<'_>) -> Result<(), Error> {
    myplaylog_(ctx).await
}

// use crate::commands::CrackedError;
#[cfg(not(tarpaulin_include))]
pub async fn myplaylog_(ctx: Context<'_>) -> Result<(), Error> {
    // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let user_id = ctx.author().id;

    let last_played = ctx.get_last_played_by_user(user_id).await?;

    let _ = courier::reply(ctx, CrackedMessage::PlayLog(escaped_log(&last_played))).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::escaped_log;

    /// A logged title is third-party text: it must not become a masked link,
    /// a mention, or formatting.
    #[test]
    fn logged_titles_are_escaped() {
        let log = escaped_log(&[
            "[free](https://evil.example) - @everyone".to_owned(),
            "**bold** - <@1>".to_owned(),
        ]);
        assert_eq!(
            log,
            vec![
                r"\[free\](https://evil.example) - \@everyone",
                r"\*\*bold\*\* - \<\@1\>",
            ]
        );
    }
}
