use crate::{Context, Error};

#[poise::command(prefix_command, slash_command)]
pub async fn authorize(
    ctx: Context<'_>,
    #[description = "The user id to add to authorized list"] user_id: u64,
) -> Result<(), Error> {
    // let guild_id = interaction.guild_id.unwrap();
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    data.guild_settings_map
        .lock()
        .unwrap()
        .get_mut(&guild_id.as_u64())
        .unwrap()
        .authorized_users
        .insert(user_id);

    Ok(())
}

#[poise::command(prefix_command, slash_command)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[description = "The user id to remove from the authorized list"] user_id: u64,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    data.guild_settings_map
        .lock()
        .unwrap()
        .get_mut(&guild_id.as_u64())
        .unwrap()
        .authorized_users
        .remove(&user_id);

    Ok(())
}