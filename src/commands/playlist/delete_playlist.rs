use crate::{playlist::Playlist, Context, Error};

#[poise::command(prefix_command, slash_command)]
pub async fn delete_playlist(ctx: Context<'_>, playlist_id: i64) -> Result<(), Error> {
    // Assuming you have a way to fetch the user_id of the command issuer
    let user_id = ctx.author().id.0 as i64;
    let pool = ctx.data().database_pool.as_ref().unwrap();

    match Playlist::delete_playlist_by_id(pool, playlist_id, user_id).await {
        Ok(_) => {
            poise::say_reply(
                ctx,
                format!("Successfully deleted playlist with ID: {}", playlist_id),
            )
            .await?;
        }
        Err(e) => {
            poise::say_reply(ctx, format!("Error deleting playlist: {}", e)).await?;
        }
    }

    Ok(())
}
