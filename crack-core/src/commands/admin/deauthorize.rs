use crate::errors::CrackedError;
use crate::utils::check_reply;
use crate::Context;
use crate::Error;
use poise::CreateReply;

/// Deauthorize a user from using the bot.
#[poise::command(prefix_command, owners_only)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[description = "The user id to remove from the authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let mut guild_settings = data
        .guild_settings_map
        .lock()
        .unwrap()
        .get_mut(&guild_id)
        .expect("Failed to get guild settings map")
        .clone();
    let res = guild_settings.authorized_users.remove(&id);
    guild_settings.save()?;

    if res {
        check_reply(
            ctx.send(CreateReply::new().content("User deauthorized.").reply(true))
                .await,
        );
        Ok(())
    } else {
        Err(CrackedError::UnauthorizedUser.into())
    }
}
