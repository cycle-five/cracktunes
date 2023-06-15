use crate::{
    errors::{verify, CrackedError},
    messaging::message::ParrotMessage,
    messaging::messages::{FAIL_MINUTES_PARSING, FAIL_SECONDS_PARSING},
    utils::{create_response, get_interaction},
    Context, Error,
};
use std::time::Duration;

#[poise::command(prefix_command, slash_command)]
pub async fn seek(ctx: Context<'_>) -> Result<(), Error> {
    let mut interaction = get_interaction(ctx).unwrap();
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(&ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let args = interaction.data.options.clone();
    let seek_time = args.first().unwrap().value.as_ref().unwrap();

    let timestamp_str = seek_time.as_str().unwrap();
    let mut units_iter = timestamp_str.split(':');

    let minutes = units_iter.next().and_then(|c| c.parse::<u64>().ok());
    let minutes = verify(minutes, CrackedError::Other(FAIL_MINUTES_PARSING).into())?;

    let seconds = units_iter.next().and_then(|c| c.parse::<u64>().ok());
    let seconds = verify(seconds, CrackedError::Other(FAIL_SECONDS_PARSING).into())?;

    let timestamp = minutes * 60 + seconds;

    let handler = call.lock().await;
    let track = handler
        .queue()
        .current()
        .ok_or(CrackedError::Other("No track playing"))?;
    drop(handler);

    track.seek_time(Duration::from_secs(timestamp)).unwrap();

    create_response(
        &ctx.serenity_context().http,
        &mut interaction,
        ParrotMessage::Seek {
            timestamp: timestamp_str.to_owned(),
        },
    )
    .await
}
