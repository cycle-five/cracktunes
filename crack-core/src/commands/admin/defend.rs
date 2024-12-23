use crate::errors::CrackedError;
use crate::guild::settings::AtomicU16Key;
use crate::Context;
use crate::Error;
use serenity::all::ChannelType;
use serenity::all::Context as SerenityContext;
use serenity::all::GuildChannel;
use serenity::all::Http;
use serenity::all::Member;
use serenity::async_trait;
use serenity::builder::CreateChannel;
use serenity::builder::EditMember;
use serenity::futures::StreamExt;
use songbird::Event;
use songbird::EventContext;
use songbird::EventHandler;
use songbird::Songbird;
use std::sync::atomic;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

#[allow(static_mut_refs)]
static mut TEMP_CHANNEL_NAMES: Vec<GuildChannel> = Vec::new();
static N: u64 = 15;

/// Defend the server.
#[poise::command(
    category = "Admin",
    prefix_command,
    subcommands("cancel"),
    owners_only,
    ephemeral
)]
pub async fn defend(
    ctx: Context<'_>,
    #[description = "Role to defend against"] role: serenity::all::Role,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let songbird = ctx.data().songbird.clone();
    let call = songbird
        .get(guild_id)
        .ok_or(CrackedError::WrongVoiceChannel)?;

    let next_action = Arc::new(atomic::AtomicU16::new(0));
    let handler = DefendHandler {
        ctx: Arc::new(ctx.serenity_context().clone()),
        http: Arc::new(ctx.http().clone()),
        manager: songbird.clone(),
        role: role.clone(),
        guild_id: Some(guild_id),
        next_action: Arc::clone(&next_action),
    };

    call.lock()
        .await
        .add_global_event(Event::Periodic(Duration::from_secs(N), None), handler);

    let data = ctx.data();
    type_map.insert::<AtomicU16Key>(next_action);

    poise::say_reply(ctx, format!("Tag attackers with role {}", role.name)).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn cancel(ctx: Context<'_>) -> Result<(), Error> {
    // let guild_id = ctx.guild_id().unwrap();

    ctx.data()
        .get_mut::<AtomicU16Key>()
        .unwrap()
        .store(u16::MAX, atomic::Ordering::Relaxed);

    ctx.say("Cancelled").await?;
    Ok(())
}

pub struct DefendHandler {
    pub ctx: Arc<SerenityContext>,
    pub http: Arc<Http>,
    pub manager: Arc<Songbird>,
    pub role: serenity::all::Role,
    pub guild_id: Option<serenity::all::GuildId>,
    pub next_action: Arc<atomic::AtomicU16>,
}

#[async_trait]
impl EventHandler for DefendHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        loop {
            unsafe {
                #[allow(static_mut_refs)]
                if TEMP_CHANNEL_NAMES.len() > 1 {
                    #[allow(static_mut_refs)]
                    let next_chan: GuildChannel = TEMP_CHANNEL_NAMES.pop().unwrap();
                    let _ = next_chan.delete(self.http.as_ref(), None).await;
                } else {
                    break;
                }
            }
        }

        if self.next_action.load(atomic::Ordering::Relaxed) == u16::MAX {
            return None;
        }

        let rand = rand::random::<u8>();
        if rand % 2 == 0 {
            unsafe {
                #[allow(static_mut_refs)]
                if !TEMP_CHANNEL_NAMES.is_empty() {
                    #[allow(static_mut_refs)]
                    let next_chan: GuildChannel = TEMP_CHANNEL_NAMES.pop().unwrap();
                    let _ = next_chan.delete(self.http.as_ref(), None).await;
                }
            }
        }

        // Get all users in the role
        let guild_id = self.guild_id.unwrap();
        let role = self.role.clone();

        tracing::error!("Getting all members");

        let attackers = get_members_by_role(self.http.clone(), guild_id, role.clone())
            .await
            .unwrap();

        tracing::error!("Getting guild: {:?}", guild_id);
        let guild = self
            .ctx
            .cache
            .guild(self.guild_id.unwrap())
            .unwrap()
            .clone();

        // Get all users in voice channels
        let voice_users = guild.voice_states.clone();

        // Filter possible attackers to only those in voice channels
        let active_attackers = attackers
            .into_iter()
            .filter(|attacker| voice_users.contains_key(&attacker.user.id));

        tracing::error!("Active attackers: {:?}", active_attackers);

        let channel = if self.next_action.fetch_add(0, Ordering::Relaxed) % 2 == 0 {
            // Create a random voice channel
            let now_str = chrono::Utc::now().to_rfc3339();
            let channel_name = format!("Losers-{}", now_str);
            tracing::warn!("Creating channel {}", channel_name);
            // Now create the channel
            let channel = guild
                .id
                .create_channel(
                    self.http.as_ref(),
                    CreateChannel::new(channel_name.clone()).kind(ChannelType::Voice),
                )
                .await
                .unwrap()
                .clone();
            // Now move all the attackers into the channel
            for mut attacker in active_attackers.clone() {
                let _ = attacker
                    .edit(
                        self.http.as_ref(),
                        EditMember::default().voice_channel(channel.id),
                    )
                    .await;
            }
            Some(channel)
        } else {
            None
        };

        let res = self.next_action.fetch_add(1, Ordering::Relaxed);
        if res == u16::MAX {
            return None;
        }

        if let Some(c) = channel {
            #[allow(static_mut_refs)]
            unsafe {
                TEMP_CHANNEL_NAMES.push(c)
            }
        };

        None
    }
}

async fn get_members_by_role(
    http: Arc<Http>,
    guild_id: serenity::all::GuildId,
    role: serenity::all::Role,
) -> Result<Vec<Member>, Error> {
    let role_id = role.id;

    let mut attackers: Vec<Member> = Vec::new();
    let mut members = guild_id.members_iter(&http).boxed();
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
                }
            },
            Err(_err) => continue,
        }
    }

    Ok(attackers)
}
