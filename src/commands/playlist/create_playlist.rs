use crate::{
    messaging::message::CrackedMessage, playlist::Playlist, utils::create_response_poise, Context,
    Error,
};

#[poise::command(prefix_command, slash_command)]
pub async fn create_playlist(ctx: Context<'_>, name: String) -> Result<(), Error> {
    // Assuming you have a way to fetch the user_id of the command issuer
    let user_id = ctx.author().id.0 as i64;

    match Playlist::create(ctx.data().database_pool.as_ref().unwrap(), &name, user_id).await {
        Ok(playlist) => {
            create_response_poise(ctx, CrackedMessage::PlaylistCreated(playlist.name.clone()))
                .await?;
        }
        Err(e) => {
            poise::say_reply(ctx, format!("Error creating playlist: {}", e)).await?;
        }
    }

    Ok(())
}
