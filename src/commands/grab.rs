use crate::{errors::CrackedError, utils::create_now_playing_embed, Context, Error};

/// Have the current song sent to your DMs.
#[poise::command(slash_command, prefix_command, aliases("save"), guild_only)]
pub async fn grab(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;
    let channel = ctx
        .author()
        .create_dm_channel(&ctx.serenity_context().http)
        .await?;
    let handler = call.lock().await;

    match handler.queue().current() {
        Some(track_handle) => {
            let embed = create_now_playing_embed(&track_handle).await;
            // create_embed_response_poise(ctx, embed).await?;
            channel
                .send_message(&ctx.serenity_context().http, |m| {
                    m.embed(|e| {
                        e.clone_from(&embed);
                        e
                    })
                })
                .await?;
        }
        None => {
            channel
                .say(&ctx.serenity_context().http, "Nothing playing!")
                .await
                .expect("Error sending message");
        }
    }

    Ok(())
}
