use crate::commands::help;
use crate::errors::CrackedError;
use crate::messaging::courier;
use crate::messaging::interface;
use crate::messaging::messages::GRAB_DM_FAILED;
use crate::messaging::render::{render, RenderCx};
use crate::messaging::status::DiscordTransport;
use crate::poise_ext::ContextExt;
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
    let chan_id = ctx.author().create_dm_channel(&ctx).await?.id.widen();
    let call = ctx.get_call().await?;

    let current = call.lock().await.queue().current();
    let track = current.ok_or(CrackedError::NothingPlaying)?;
    let msg = CrackedMessage::NowPlayingCard(Box::new(interface::now_playing_card(&track).await));
    let transport = DiscordTransport {
        http: ctx.serenity_context().http.clone(),
        cache: ctx.serenity_context().cache.clone(),
    };
    courier::post_message(&transport, chan_id, &render(&msg, &RenderCx::now()))
        .await
        .map_err(|err| {
            tracing::warn!("/grab: the DM was not delivered: {err:?}");
            CrackedError::Other(GRAB_DM_FAILED)
        })?;

    courier::reply(ctx, CrackedMessage::GrabbedNotice).await?;

    Ok(())
}
