use crate::{
    messaging::message::ParrotMessage, utils::create_response_poise_text, utils::get_guild_id,
    Context, Error,
};

#[poise::command(prefix_command, slash_command)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = get_guild_id(&ctx).unwrap();
    let manager = songbird::get(&ctx.serenity_context()).await.unwrap();
    manager.remove(guild_id).await.unwrap();

    create_response_poise_text(&ctx, ParrotMessage::Leaving).await
}
