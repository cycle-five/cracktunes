use crate::commands::help;
use crate::poise_ext::MessageInterfaceCtxExt;
use crate::utils::send_now_playing;
use crate::{errors::CrackedError, Context, Error};

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

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NotConnected)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;
    let channel = ctx
        .author()
        .create_dm_channel(&ctx.serenity_context().http)
        .await?;

    let _ = send_now_playing(
        channel.id,
        ctx.serenity_context().http.clone(),
        call.clone(),
        None,
        None,
    )
    .await?;

    ctx.send_grabbed_notice().await?;

    Ok(())
}
