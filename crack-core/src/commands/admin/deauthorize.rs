use crate::utils::check_reply;
use crate::Context;
use crate::Error;
use poise::CreateReply;

/// Deauthorize a user from using the bot.
#[poise::command(prefix_command, owners_only)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[rest]
    #[description = "The user id to remove from the authorized list"]
    user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    // FIXME: ASDFASDF
    let _res = data
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|settings| {
            settings.authorized_users.remove(&id);
        })
        .key();

    //if res {
    check_reply(
        ctx.send(
            CreateReply::default()
                .content("User deauthorized.")
                .reply(true),
        )
        .await,
    );
    Ok(())
    // } else {
    //     Err(CrackedError::UnauthorizedUser.into())
    // }
}
