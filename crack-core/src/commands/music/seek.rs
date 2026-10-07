use crate::{
    commands::cmd_check_music,
    errors::{verify, CrackedError},
    messaging::courier,
    messaging::message::CrackedMessage,
    messaging::messages::{FAIL_MINUTES_PARSING, FAIL_NO_TRACK_PLAYING, FAIL_SECONDS_PARSING},
    music::ops::{self, OpCx, OpRefused},
    Context, Error,
};
use std::{borrow::Cow, time::Duration};

/// Seek to timestamp, in format `mm:ss`.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn seek(
    ctx: Context<'_>,
    #[description = "Seek to timestamp, in format `mm:ss`."] seek_time: String,
) -> Result<(), Error> {
    seek_internal(ctx, seek_time).await
}

/// Internal seek function.
pub async fn seek_internal(ctx: Context<'_>, seek_time: String) -> Result<(), Error> {
    let timestamp_str = seek_time.as_str();
    let mut units_iter = timestamp_str.split(':');

    let minutes = units_iter.next().and_then(|c| c.parse::<u64>().ok());
    let minutes = verify(minutes, CrackedError::Other(FAIL_MINUTES_PARSING))?;

    let seconds = units_iter.next().and_then(|c| c.parse::<u64>().ok());
    let seconds = verify(seconds, CrackedError::Other(FAIL_SECONDS_PARSING))?;

    let timestamp = minutes * 60 + seconds;

    let cx = OpCx::from_ctx(&ctx)?;
    let msg = match ops::seek(&cx, Duration::from_secs(timestamp)).await {
        Ok(done) => {
            let msg = CrackedMessage::Seek {
                timestamp: timestamp_str.to_owned(),
            };
            done.reply_then_settle(ctx, &cx, msg).await?;
            return Ok(());
        },
        Err(OpRefused::SeekFailed(e)) => CrackedMessage::SeekFail {
            timestamp: Cow::Owned(timestamp_str.to_owned()),
            error: e,
        },
        // /seek has always worded this its own way, not as NothingPlaying.
        Err(OpRefused::NothingPlaying) => {
            return Err(CrackedError::Other(FAIL_NO_TRACK_PLAYING).into());
        },
        Err(refused) => return Err(CrackedError::from(refused).into()),
    };

    let _ = courier::reply(ctx, msg).await?;
    Ok(())
}
