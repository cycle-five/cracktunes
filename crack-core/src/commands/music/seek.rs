use crate::{
    commands::cmd_check_music,
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    messaging::messages::{FAIL_MINUTES_PARSING, FAIL_SECONDS_PARSING},
    poise_ext::ContextExt,
    utils::send_reply,
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
    let call = ctx.get_call().await?;

    let timestamp_str = seek_time.as_str();
    let mut units_iter = timestamp_str.split(':');

    let minutes = units_iter.next().and_then(|c| c.parse::<u64>().ok());
    let minutes = verify(minutes, CrackedError::Other(FAIL_MINUTES_PARSING))?;

    let seconds = units_iter.next().and_then(|c| c.parse::<u64>().ok());
    let seconds = verify(seconds, CrackedError::Other(FAIL_SECONDS_PARSING))?;

    let timestamp = minutes * 60 + seconds;

    let handler = call.lock().await;
    let track = handler
        .queue()
        .current()
        .ok_or(CrackedError::Other("No track playing"))?;
    drop(handler);

    let callback = track.seek(Duration::from_secs(timestamp));
    let msg = match callback.result_async().await {
        Ok(_) => CrackedMessage::Seek {
            timestamp: timestamp_str.to_owned(),
        },
        Err(e) => CrackedMessage::SeekFail {
            timestamp: Cow::Owned(timestamp_str.to_owned()),
            error: e,
        },
    };

    let _ = send_reply(&ctx, msg, true).await?;
    Ok(())
}
