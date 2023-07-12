use crate::{
    messaging::message::CrackedMessage,
    utils::create_response_poise_text,
    utils::{count_command, get_guild_id},
    Context, Error,
};

/// Leave the current voice channel.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    count_command("leave");
    let guild_id = get_guild_id(&ctx).unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    manager.remove(guild_id).await.unwrap();

    create_response_poise_text(&ctx, CrackedMessage::Leaving).await
}
