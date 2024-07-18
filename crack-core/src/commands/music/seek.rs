use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    messaging::messages::{FAIL_MINUTES_PARSING, FAIL_SECONDS_PARSING},
    poise_ext::ContextExt,
    utils::send_reply,
    Context, Error,
};
use std::time::Duration;

/// Seek to timestamp, in format `mm:ss`.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Music", prefix_command, slash_command, guild_only)]
pub async fn seek(
    ctx: Context<'_>,
    #[description = "Seek to timestamp, in format `mm:ss`."] seek_time: String,
) -> Result<(), Error> {
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

    let _callback = track.seek(Duration::from_secs(timestamp));

    let _ = send_reply(
        &ctx,
        CrackedMessage::Seek {
            timestamp: timestamp_str.to_owned(),
        },
        true,
    )
    .await?;
    Ok(())
}
