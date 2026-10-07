use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    messaging::courier,
    messaging::message::CrackedMessage,
    music::ops::{self, OpCx, OpRefused},
    Context, Error,
};

/// Tell the bot to leave the voice channel it is in.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    aliases("dc", "fuckoff", "fuck off"),
    //subcommands("help"),
    check = "cmd_check_music"
)]
pub async fn leave(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    leave_internal(ctx).await
}

/// Leave a voice channel. Actually impl.
pub async fn leave_internal(ctx: Context<'_>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    match ops::leave(&cx).await {
        Ok(done) => {
            let _ = courier::reply(ctx, CrackedMessage::Leaving).await?;
            done.settle_now(&cx).await;
        },
        // Not being in a call is a reply, not an error.
        Err(OpRefused::NotConnected) => {
            let _ = courier::reply(
                ctx,
                CrackedMessage::CrackedError(CrackedError::NotConnected),
            )
            .await?;
        },
        Err(refused) => return Err(CrackedError::from(refused).into()),
    }
    Ok(())
}
