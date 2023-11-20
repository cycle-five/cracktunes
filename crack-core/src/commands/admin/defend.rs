use crate::Context;
use crate::Error;
use serenity::all::Channel;
use serenity::all::ChannelType;
use serenity::all::Context as SerenityContext;
use serenity::all::Http;
use serenity::all::Member;
use serenity::async_trait;
use serenity::builder::CreateChannel;
use serenity::builder::EditMember;
use songbird::Event;
use songbird::EventContext;
use songbird::EventHandler;
use songbird::Songbird;
use std::sync::Arc;
use std::time::Duration;

static mut TEMP_CHANNEL_NAMES: Vec<Channel> = Vec::new();

/// Defend the server.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn defend(
    ctx: Context<'_>,
    #[description = "Role to defend against"] role: serenity::all::Role,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let songbird = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = songbird.get(guild_id).unwrap();
    let mut handler = call.lock().await;

    tracing::error!("Defending against {}", role.name);

    handler.add_global_event(
        Event::Periodic(Duration::from_secs(5), None),
        DefendHandler {
            ctx: Arc::new(ctx.serenity_context().clone()),
            http: ctx.serenity_context().http.clone(),
            manager: songbird.clone(),
            role: role.clone(),
            guild_id: Some(guild_id),
            prev_channel: Vec::new(),
        },
    );

    drop(handler);
    Ok(())
}

pub struct DefendHandler {
    pub ctx: Arc<SerenityContext>,
    pub http: Arc<Http>,
    pub manager: Arc<Songbird>,
    pub role: serenity::all::Role,
    pub guild_id: Option<serenity::all::GuildId>,
    pub prev_channel: Vec<serenity::all::Channel>,
}

#[async_trait]
impl EventHandler for DefendHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        tracing::error!("DefendHandler act called");

        // let prev_channel = &mut self.prev_channel.clone();
        unsafe {
            if !TEMP_CHANNEL_NAMES.is_empty() {
                let channel = TEMP_CHANNEL_NAMES.pop().unwrap();
                let _ = channel.delete(&self.http).await;
            }
        }
        //let channel = self.prev_channel.lock().map(|c| c.delete(&self.http));
        // Get all users in the role
        let guild_id = self.guild_id.unwrap();
        let role = self.role.clone();
        let role_id = role.id;
        // let role_name = role.name;
        use serenity::futures::StreamExt;
        // use serenity::model::guild::MembersIter;

        tracing::error!("Getting all members");

        let mut attackers: Vec<Member> = Vec::new();
        let mut members = guild_id.members_iter(&self.http).boxed();
        while let Some(member_result) = members.next().await {
            match member_result {
                Ok(member) => {
                    if member
                        .roles
                        .iter()
                        .any(|mem_role_id| mem_role_id.get() == role_id.get())
                    {
                        tracing::error!("{} is in the role {}", member.user.name, role.name);
                        attackers.push(member);
                        // println!("{} is in the role {}", member.user.name, role_name);
                    }
                }
                Err(_err) => continue, //eprintln!("Uh oh!  Error: {}", error),
            }
        }

        tracing::error!("Getting guild: {:?}", guild_id);
        let guild = self
            .ctx
            .cache
            .guild(self.guild_id.unwrap())
            .unwrap()
            .clone();
        // Get all users in voice channels
        let voice_users = guild.voice_states.clone();

        let active_attackers = attackers
            .into_iter()
            .filter(|attacker| voice_users.contains_key(&attacker.user.id));

        tracing::error!("Active attackers: {:?}", active_attackers);

        // Create a random voice channel
        let now_str = chrono::Utc::now().to_rfc3339();
        let channel_name = format!("Losers-{}", now_str);
        tracing::error!("Creating channel {}", channel_name);
        // Now create the channel
        let channel = guild
            .create_channel(
                &self.http,
                CreateChannel::new(channel_name.clone()).kind(ChannelType::Voice),
            )
            .await
            .unwrap()
            .clone();

        // Now move all the attackers into the channel
        for mut attacker in active_attackers.clone() {
            let _ = attacker
                .edit(
                    self.http.clone(),
                    EditMember::default().voice_channel(channel.id),
                )
                .await;
        }

        // Sleep for 10 seconds
        let _ = tokio::time::sleep(Duration::from_secs(10)).await;

        for mut attacker in active_attackers {
            let prev_channel = voice_users
                .get(&attacker.user.id)
                .unwrap()
                .clone()
                .channel_id;
            if let Some(prev_channel) = prev_channel {
                let _ = attacker
                    .edit(
                        self.http.clone(),
                        EditMember::default().voice_channel(prev_channel),
                    )
                    .await;
            }
        }
        // let prev_write = &mut self.prev_channel.clone();
        //TEMP_CHANNEL_NAMES.push(poise::serenity_prelude::Channel::Guild(channel));
        unsafe {
            TEMP_CHANNEL_NAMES.push(poise::serenity_prelude::Channel::Guild(channel));
        }

        // Sleep for 10 seconds
        let _ = tokio::time::sleep(Duration::from_secs(10)).await;
        None
    }
}
