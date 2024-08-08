use crate::commands::help;
use crate::poise_ext::MessageInterfaceCtxExt;
use crate::{Context, Error};

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

    ctx.send_now_playing(chan_id).await?;

    ctx.send_grabbed_notice().await?;

    Ok(())
}
