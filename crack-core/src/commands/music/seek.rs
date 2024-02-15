use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    messaging::messages::{FAIL_MINUTES_PARSING, FAIL_SECONDS_PARSING},
    utils::send_response_poise,
    Context, Error,
};
use std::time::Duration;

/// Seek to timestamp, in format `mm:ss`.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn seek(
    ctx: Context<'_>,
    #[description = "Seek to timestamp, in format `mm:ss`."] seek_time: String,
) -> Result<(), Error> {
    // let mut interaction = get_interaction(ctx).unwrap();
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

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

    let msg = send_response_poise(
        ctx,
        CrackedMessage::Seek {
            timestamp: timestamp_str.to_owned(),
        },
        true,
    )
    .await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}
