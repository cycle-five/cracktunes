use crate::{
    commands::{cmd_check_music, music::send_skip_reply},
    errors::CrackedError,
    messaging::{courier, message::CrackedMessage},
    music::ops::{self, OpCx, Vote},
    poise_ext::ContextExt,
    Context, Error,
};
use poise::serenity_prelude as serenity;
use serenity::Mentionable;

/// Vote to skip the current track
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn voteskip(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show the help menu."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    let cx = OpCx::from_ctx(&ctx)?;
    let done = ops::voteskip(&cx, ctx.get_user_id())
        .await
        .map_err(CrackedError::from)?;
    let anchor = match done.outcome() {
        Vote::Skipped(s) => {
            // Always visible, so it anchors the status, as /skip's visible reply
            // does: the status must land below it even before its gateway echo
            // reaches the cache.
            let msg = send_skip_reply(ctx, s.message(), false).await?;
            Some((msg.channel_id, msg.id))
        },
        Vote::Voted { missing } => {
            courier::reply(
                ctx,
                CrackedMessage::VoteSkip {
                    mention: ctx.get_user_id().mention(),
                    missing: *missing,
                },
            )
            .await?
            .into_message()
            .await
            .map_err(CrackedError::from)?;
            // A counted vote settles `Nothing`; there is nothing to anchor.
            None
        },
    };
    done.settle_after(&cx, anchor).await;
    Ok(())
}
