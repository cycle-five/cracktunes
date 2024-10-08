use crate::commands::help;
use crate::messaging::interface;
use crate::poise_ext::{ContextExt, PoiseContextExt};
use crate::{Context, CrackedMessage, Error};

/// Send the current tack to your DMs.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    aliases("save"),
    guild_only
)]
pub async fn grab(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show the help menu."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    grab_internal(ctx).await
}

#[cfg(not(tarpaulin_include))]
/// Internal function for grab.
async fn grab_internal(ctx: Context<'_>) -> Result<(), Error> {
    let chan_id = ctx.author().create_dm_channel(&ctx).await?.id;
    let call = ctx.get_call().await?;

    interface::send_now_playing(chan_id, ctx.serenity_context().http.clone(), call).await?;

    ctx.send_reply_embed(CrackedMessage::GrabbedNotice).await?;

    Ok(())
}
