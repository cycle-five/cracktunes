use crate::{errors::CrackedError, Context, Error};

/// Admin commands.
#[poise::command(
    prefix_command,
    slash_command,
    subcommands("authorize", "deauthorize"),
    ephemeral,
    owners_only
)]
pub async fn admin(_ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Admin command called");

    Ok(())
}

/// Authorize a user to use the bot.
#[poise::command(prefix_command, slash_command, owners_only, ephemeral)]
pub async fn authorize(
    ctx: Context<'_>,
    #[description = "The user id to add to authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    data.guild_settings_map
        .lock()
        .unwrap()
        .get_mut(guild_id.as_u64())
        .expect("Failed to get guild settings map")
        .authorized_users
        .insert(id);

    Ok(())
}

/// Deauthorize a user from using the bot.
#[poise::command(prefix_command, slash_command, owners_only, ephemeral)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[description = "The user id to remove from the authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let res = data
        .guild_settings_map
        .lock()
        .unwrap()
        .get_mut(guild_id.as_u64())
        .expect("Failed to get guild settings map")
        .authorized_users
        .remove(&id);

    if res {
        Ok(())
    } else {
        Err(CrackedError::Other("User did not exist in authorized list").into())
    }
}
