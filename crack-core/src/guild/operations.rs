use std::sync::Arc;

use ::serenity::all::Context as SerenityContext;
use serenity::all::GuildId;

/// Get all guilds the bot is in (that are cached).
#[cfg(not(tarpaulin_include))]
pub async fn get_guilds(ctx: Arc<SerenityContext>) -> Vec<GuildId> {
    ctx.http
        .get_guilds(None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|x| x.id)
        .collect::<Vec<_>>()
}
