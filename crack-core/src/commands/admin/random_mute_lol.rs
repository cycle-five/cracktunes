use crate::errors::CrackedError;
use crate::Context;
use crate::Error;
use rand::Rng;
use serenity::all::{Context as SerenityContext, GuildId, User};
use songbird::{Call, Event, EventContext, EventHandler};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

/// Randomly mute a user in the server.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, ephemeral, owners_only)]
pub async fn random_mute(
    ctx: Context<'_>,
    #[description = "User to randomly mute and unmute"] user: User,
    #[description = "Number of seconds to wait"] n: Option<u64>,
) -> Result<(), Error> {
    // Create a handler that exists while the user is in vc
    // If the user leaves vc, the handler waits for 30 minutes
    // and then it is deleted.

    let guild_id = ctx.guild_id().unwrap();

    let songbird = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = songbird
        .get(guild_id)
        .ok_or(CrackedError::WrongVoiceChannel)?;

    let handler = RandomMuteHandler {
        ctx: Arc::new(ctx.serenity_context().clone()),
        call: call.clone(),
        user,
        guild_id,
    };

    call.lock().await.add_global_event(
        Event::Periodic(Duration::from_secs(n.unwrap_or(2)), None),
        handler,
    );
    Ok(())
}

pub struct RandomMuteHandler {
    pub ctx: Arc<SerenityContext>,
    pub call: Arc<Mutex<Call>>,
    pub user: User,
    pub guild_id: GuildId,
}

use crate::commands::mute_impl;
use async_trait::async_trait;
#[async_trait]
impl EventHandler for RandomMuteHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        // let guild_id = self.guild_id.unwrap();
        // let guild = guild_id.to_guild_cached(&self.ctx).await.unwrap();
        // let member = guild.member(&self.ctx, self.user.id).await.unwrap();
        let r = rand::thread_rng().gen_range(0..100);
        if r < 50 {
            let _msg = mute_impl(self.ctx.clone(), self.user.clone(), self.guild_id, true)
                .await
                .unwrap();
        // } else if r < 75 {
        } else {
            let _msg = mute_impl(self.ctx.clone(), self.user.clone(), self.guild_id, false)
                .await
                .unwrap();
        }
        None
    }
}
