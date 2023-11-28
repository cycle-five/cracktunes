use crate::{
    db::playlist::Playlist, messaging::message::CrackedMessage, utils::send_response_poise,
    Context, Error,
};

#[poise::command(prefix_command, slash_command)]
pub async fn delete_playlist(ctx: Context<'_>, playlist_id: i64) -> Result<(), Error> {
    // Assuming you have a way to fetch the user_id of the command issuer
    let user_id = ctx.author().id.get() as i64;
    let pool = ctx.data().database_pool.as_ref().unwrap();

    Playlist::delete_playlist_by_id(pool, playlist_id, user_id).await?;

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!(
            "Successfully deleted playlist with ID: {}",
            playlist_id
        )),
    )
    .await?;

    Ok(())
}
