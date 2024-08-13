use crate::{messaging::message::CrackedMessage, utils::send_reply, Context, Error};


/// Clean up old messages from the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", prefix_command, slash_command)]
pub async fn get_letter_league_players(ctx: Context<'_>) -> Result<(), Error> {
    get_letter_league_players_internal(ctx).await
}

/// Get all the users playing letter league by their presence.
pub async fn get_letter_league_players_internal(ctx: Context<'_>) -> Result<(), Error> {
    // Get all users playing letter league by their presence.
    

    send_reply(&ctx, CrackedMessage::Other("5".to_string()), true).await?;
    Ok(())
}
