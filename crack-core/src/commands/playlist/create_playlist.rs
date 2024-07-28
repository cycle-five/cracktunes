use crate::{
    commands::cmd_check_music, db::playlist::Playlist, messaging::message::CrackedMessage,
    utils::send_reply, Context, Error,
};

/// Creates a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only,
    rename = "create"
)]
pub async fn create_playlist(ctx: Context<'_>, name: String) -> Result<(), Error> {
    // Assuming you have a way to fetch the user_id of the command issuer
    let user_id = ctx.author().id.get() as i64;

    let res = Playlist::create(ctx.data().database_pool.as_ref().unwrap(), &name, user_id).await?;

    send_reply(
        &ctx,
        CrackedMessage::PlaylistCreated(res.name.clone(), 0),
        true,
    )
    .await?;

    Ok(())
}
