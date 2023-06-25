use crate::{errors::CrackedError, Context, Error};

#[poise::command(prefix_command, slash_command)]
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

#[poise::command(prefix_command, slash_command)]
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
