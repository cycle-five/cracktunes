use crate::{
    commands::{cmd_check_music, music::send_skip_reply},
    errors::CrackedError,
    messaging::message::CrackedMessage,
    music::ops::{self, OpCx, Vote},
    poise_ext::{ContextExt, PoiseContextExt},
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
    match &done.outcome {
        Vote::Skipped(s) => {
            // Never anchored, so it does not now.
            send_skip_reply(ctx, s.message(), false).await?;
        },
        Vote::Voted { missing } => {
            ctx.send_reply_embed(CrackedMessage::VoteSkip {
                mention: ctx.get_user_id().mention(),
                missing: *missing,
            })
            .await?
            .into_message()
            .await
            .map_err(CrackedError::from)?;
        },
    }
    done.settle_now(&cx).await;
    Ok(())
}
