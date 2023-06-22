use crate::{
    Context, Error,
};

#[poise::command(prefix_command, slash_command)]
pub async fn authorize(
    ctx: Context<'_>,
    #[description = "The user id to add to authorized list"] user_id: u64,
) -> Result<(), Error> {
    // let guild_id = interaction.guild_id.unwrap();
    let _user = ctx.serenity_context().http.get_user(user_id).await?;

    //ctx.data()

    Ok(())
}
