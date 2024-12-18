use crate::{
    commands::cmd_check_music, db::playlist::Playlist, messaging::message::CrackedMessage,
    utils::send_reply, Context, Error,
};

/// Deletes a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only,
    rename = "delete"
)]
pub async fn delete_playlist(ctx: Context<'_>, playlist_id: i32) -> Result<(), Error> {
    // Assuming you have a way to fetch the user_id of the command issuer
    let user_id = ctx.author().id.get() as i64;
    let data = ctx.data();
    let pool = data.database_pool.as_ref().unwrap();

    Playlist::delete_playlist_by_id(pool, playlist_id, user_id).await?;

    send_reply(
        &ctx,
        CrackedMessage::Other(format!(
            "Successfully deleted playlist with ID: {}",
            playlist_id
        )),
        true,
    )
    .await?;

    Ok(())
}
