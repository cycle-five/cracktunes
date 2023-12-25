use crate::{
    db::playlist::Playlist, messaging::message::CrackedMessage, utils::send_response_poise,
    Context, Error,
};

/// Creates a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "create")]
pub async fn create_playlist(ctx: Context<'_>, name: String) -> Result<(), Error> {
    // Assuming you have a way to fetch the user_id of the command issuer
    let user_id = ctx.author().id.get() as i64;

    let res = Playlist::create(ctx.data().database_pool.as_ref().unwrap(), &name, user_id).await?;

    send_response_poise(ctx, CrackedMessage::PlaylistCreated(res.name.clone())).await?;

    Ok(())
}
