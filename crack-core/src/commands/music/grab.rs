use crate::commands::help;
use crate::errors::CrackedError;
use crate::messaging::courier::{self, Destination};
use crate::messaging::interface;
use crate::messaging::render::RenderCx;
use crate::messaging::status::DiscordTransport;
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
    let chan_id = ctx.author().create_dm_channel(&ctx).await?.id.widen();
    let call = ctx.get_call().await?;

    let current = call.lock().await.queue().current();
    let track = current.ok_or(CrackedError::NothingPlaying)?;
    let msg = CrackedMessage::NowPlayingCard(Box::new(interface::now_playing_card(&track).await));
    let transport = DiscordTransport {
        http: ctx.serenity_context().http.clone(),
        cache: ctx.serenity_context().cache.clone(),
    };
    courier::post(
        &ctx.data(),
        &transport,
        Destination::Channel(chan_id),
        &msg,
        &RenderCx::now(),
    )
    .await;

    ctx.send_reply_embed(CrackedMessage::GrabbedNotice).await?;

    Ok(())
}
