//! `/room ...`: every room control as a slash command. Spec 1.7.
//!
//! 🔑 Gathering the facts and acting can take longer than Discord's 3 s, so
//! each subcommand defers privately first and answers with a private `Done`
//! or `Refused`. The room is the invoker's voice channel if it is a room, else
//! the channel the command was run in (a room's chat is the room's own
//! channel), else none.
//!
//! No `default_member_permissions`: anyone may try, and `act` decides by the
//! invoker's standing in the room.
use crate::messaging::courier;
use crate::messaging::message::CrackedMessage;
use crate::rooms::authority::{Reason, RoomAction};
use crate::rooms::card::{RoomCard, RoomDone, RoomRefusal};
use crate::rooms::facts::{gather, GuildFacts};
use crate::rooms::interact::Live;
use crate::rooms::ops::{act, user_target};
use crate::rooms::registry::RoomRegistry;
use crate::{Context, Error};
use serenity::all::{ChannelId, GenericChannelId, GuildId, RoleId, UserId};
use std::time::Instant;

/// The room a command applies to: the invoker's voice channel if it is a
/// room, else the invocation channel if it is a room, else none.
pub(crate) fn resolve_room(
    facts: &GuildFacts,
    registry: &RoomRegistry,
    invoker: UserId,
    channel: GenericChannelId,
) -> Option<ChannelId> {
    let in_voice = facts
        .voice
        .get(&invoker)
        .map(|(c, _)| *c)
        .filter(|c| registry.get(*c).is_some());
    in_voice.or_else(|| {
        let here = channel.expect_channel();
        registry.get(here).map(|_| here)
    })
}

/// The room's owner, read under its lock and released at once.
async fn owner_of(ctx: Context<'_>, room: ChannelId, guild: GuildId) -> Option<UserId> {
    let handle = ctx.data().rooms.get(room)?;
    let live = handle.lock().await;
    (live.guild == guild).then_some(live.state.owner)
}

/// Resolve the room, read the facts (R18: the invoker, the action's target and
/// the room's owner), and act.
async fn perform(
    ctx: Context<'_>,
    guild: GuildId,
    action: RoomAction,
) -> Result<RoomDone, RoomRefusal> {
    let invoker = ctx.author().id;
    let data = ctx.data();
    let sctx = ctx.serenity_context();
    let mut extra: Vec<UserId> = [Some(invoker), user_target(&action)]
        .into_iter()
        .flatten()
        .collect();
    let mut facts = gather(&sctx.cache, &sctx.http, guild, &extra)
        .await
        .ok_or(RoomRefusal::DiscordFailed)?;
    let room = resolve_room(&facts, &data.rooms, invoker, ctx.channel_id())
        .ok_or(RoomRefusal::NotInRoom)?;
    let owner = owner_of(ctx, room, guild)
        .await
        .ok_or(RoomRefusal::NotInRoom)?;
    if !extra.contains(&owner) {
        extra.push(owner);
        facts = gather(&sctx.cache, &sctx.http, guild, &extra)
            .await
            .ok_or(RoomRefusal::DiscordFailed)?;
    }
    let live = Live::of(sctx, guild);
    act(
        &live.cx(&data),
        &facts,
        room,
        invoker,
        action,
        Instant::now(),
    )
    .await
}

/// Defer privately, do `action` (or say why it could not even be built), and
/// answer privately.
async fn run(ctx: Context<'_>, action: Result<RoomAction, RoomRefusal>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let outcome = match (ctx.guild_id(), action) {
        (_, Err(refused)) => Err(refused),
        (None, Ok(_)) => Err(RoomRefusal::NotInRoom),
        (Some(guild), Ok(action)) => perform(ctx, guild, action).await,
    };
    let card = match outcome {
        Ok(done) => RoomCard::Done(done),
        Err(refused) => RoomCard::Refused(refused),
    };
    courier::reply_as(ctx, CrackedMessage::Room(Box::new(card)), true).await?;
    Ok(())
}

/// Control your voice room.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Rooms",
    slash_command,
    guild_only,
    subcommands(
        "kick",
        "letin",
        "mute",
        "unmute",
        "lock",
        "unlock",
        "chatlock",
        "chatunlock",
        "limit",
        "rename",
        "transfer",
        "helper",
        "allow",
        "block",
        "unblock",
        "claim",
        "panel"
    ),
    subcommand_required
)]
pub async fn room(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Disconnect someone and keep them out.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn kick(
    ctx: Context<'_>,
    #[description = "Who to kick"] member: UserId,
    #[description = "Why you're kicking them (required)"] reason: String,
) -> Result<(), Error> {
    run(
        ctx,
        Reason::parse(&reason).map(|reason| RoomAction::Kick {
            target: member,
            reason,
        }),
    )
    .await
}

/// Lift a kick so the person can rejoin.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn letin(
    ctx: Context<'_>,
    #[description = "Who to let back in"] member: UserId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::LetIn { target: member })).await
}

/// Stop someone from speaking here.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn mute(
    ctx: Context<'_>,
    #[description = "Who to mute"] member: UserId,
    #[description = "Why (optional)"] reason: Option<String>,
) -> Result<(), Error> {
    let reason = match reason.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(raw) => Reason::parse(raw).map(Some),
    };
    run(
        ctx,
        reason.map(|reason| RoomAction::Mute {
            target: member,
            reason,
        }),
    )
    .await
}

/// Let a muted person speak again.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn unmute(
    ctx: Context<'_>,
    #[description = "Who to unmute"] member: UserId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Unmute { target: member })).await
}

/// Close the room to newcomers.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn lock(ctx: Context<'_>) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Lock)).await
}

/// Open the room to newcomers again.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn unlock(ctx: Context<'_>) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Unlock)).await
}

/// Stop people from typing in this room's chat.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn chatlock(ctx: Context<'_>) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::ChatLock)).await
}

/// Let people type in this room's chat again.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn chatunlock(ctx: Context<'_>) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::ChatUnlock)).await
}

/// Cap how many people fit (0 means no limit).
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn limit(
    ctx: Context<'_>,
    #[description = "How many people may join (0 = no limit)"]
    #[min = 0]
    #[max = 99]
    people: u8,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Limit(people))).await
}

/// Change the room's name.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn rename(
    ctx: Context<'_>,
    #[description = "The new name"] name: String,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Rename(name))).await
}

/// Hand the room to someone else in it.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn transfer(
    ctx: Context<'_>,
    #[description = "Who gets the room"] member: UserId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Transfer { target: member })).await
}

/// Let people run these controls with you.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    guild_only,
    subcommands("helper_add", "helper_remove"),
    subcommand_required
)]
pub async fn helper(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Make someone a helper.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, rename = "add")]
pub async fn helper_add(
    ctx: Context<'_>,
    #[description = "Who to add"] member: UserId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::HelperAdd { target: member })).await
}

/// Take a helper off.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, rename = "remove")]
pub async fn helper_remove(
    ctx: Context<'_>,
    #[description = "Who to remove"] member: UserId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::HelperRemove { target: member })).await
}

/// Let a person or a role into a locked room.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    guild_only,
    subcommands("allow_member", "allow_role"),
    subcommand_required
)]
pub async fn allow(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Let a person in.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, rename = "member")]
pub async fn allow_member(
    ctx: Context<'_>,
    #[description = "Who to let in"] member: UserId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::AllowUser { target: member })).await
}

/// Let a role in.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, rename = "role")]
pub async fn allow_role(
    ctx: Context<'_>,
    #[description = "Which role to let in"] role: RoleId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::AllowRole { role })).await
}

/// Keep a role out of the room.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn block(
    ctx: Context<'_>,
    #[description = "Which role to keep out"] role: RoleId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::BlockRole { role })).await
}

/// Stop keeping a role out.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn unblock(
    ctx: Context<'_>,
    #[description = "Which role to let back"] role: RoleId,
) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::UnblockRole { role })).await
}

/// Take over a room whose owner has left.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn claim(ctx: Context<'_>) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Claim)).await
}

/// Post the room's panel again.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn panel(ctx: Context<'_>) -> Result<(), Error> {
    run(ctx, Ok(RoomAction::Panel)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rooms::card::reference_lines;
    use crate::rooms::facts::tests::empty;
    use crate::rooms::registry::{LiveRoom, RenameWindow};
    use crate::rooms::state::RoomState;
    use serenity::all::Permissions;

    const G: GuildId = GuildId::new(100);
    const VOICE_ROOM: ChannelId = ChannelId::new(500);
    const CHAT_ROOM: ChannelId = ChannelId::new(501);
    const PLAIN: ChannelId = ChannelId::new(502);
    const ME: UserId = UserId::new(1);

    fn names(c: &crate::Command) -> Vec<String> {
        let mut v: Vec<String> = c.subcommands.iter().map(|s| s.name.to_string()).collect();
        v.sort();
        v
    }

    fn sorted(list: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn room_is_a_registered_guild_command_open_to_everyone() {
        let command = room();
        assert_eq!(command.name, "room");
        assert!(command.slash_action.is_some());
        assert!(command.guild_only, "guild only");
        assert!(
            command.default_member_permissions.is_empty(),
            "open to everyone"
        );
        assert!(command.required_permissions.is_empty());
        assert!(
            crate::commands::commands_to_register()
                .into_iter()
                .any(|c| c.name == "room"),
            "registered"
        );
        assert_eq!(
            names(&command),
            sorted(&[
                "kick",
                "letin",
                "mute",
                "unmute",
                "lock",
                "unlock",
                "chatlock",
                "chatunlock",
                "limit",
                "rename",
                "transfer",
                "helper",
                "allow",
                "block",
                "unblock",
                "claim",
                "panel",
            ])
        );
        let sub = |n: &str| {
            command
                .subcommands
                .iter()
                .find(|s| s.name == n)
                .unwrap_or_else(|| panic!("no /room {n}"))
        };
        assert_eq!(names(sub("helper")), sorted(&["add", "remove"]));
        assert_eq!(names(sub("allow")), sorted(&["member", "role"]));
        assert!(command
            .subcommands
            .iter()
            .all(|s| s.guild_only && s.default_member_permissions.is_empty()));
        assert_eq!(
            Permissions::empty(),
            command.default_member_permissions,
            "no default_member_permissions"
        );
    }

    #[test]
    fn kick_subcommand_reason_is_required() {
        let command = room();
        let kick = command
            .subcommands
            .iter()
            .find(|s| s.name == "kick")
            .unwrap();
        let param = |n: &str| kick.parameters.iter().find(|p| p.name == n).unwrap();
        assert!(param("reason").required, "kick needs a reason");
        // The reason is not sent to the member (the mod log will record it).
        assert_eq!(
            param("reason").description.as_deref(),
            Some("Why you're kicking them (required)")
        );
        assert!(param("member").required);
        let mute = command
            .subcommands
            .iter()
            .find(|s| s.name == "mute")
            .unwrap();
        let reason = mute.parameters.iter().find(|p| p.name == "reason").unwrap();
        assert!(!reason.required, "a mute's reason is optional");
        let limit = command
            .subcommands
            .iter()
            .find(|s| s.name == "limit")
            .unwrap();
        assert_eq!(limit.parameters.len(), 1);
        assert_eq!(limit.parameters[0].name, "people");
    }

    #[test]
    fn every_room_command_the_panel_mentions_is_registered() {
        let command = room();
        let mut seen = 0;
        for line in reference_lines() {
            for span in line.split('`').skip(1).step_by(2) {
                let Some(rest) = span.strip_prefix("/room") else {
                    continue;
                };
                seen += 1;
                let mut at = &command;
                for word in rest.split_whitespace() {
                    at = at
                        .subcommands
                        .iter()
                        .find(|s| s.name == word)
                        .unwrap_or_else(|| panic!("the panel names `{span}` but no `{word}`"));
                }
            }
        }
        assert!(seen >= 17, "the panel names the commands: saw {seen}");
    }

    fn registry() -> RoomRegistry {
        let reg = RoomRegistry::default();
        for ch in [VOICE_ROOM, CHAT_ROOM] {
            reg.insert(LiveRoom {
                guild: G,
                channel: ch,
                name: "room".into(),
                state: RoomState::new(UserId::new(ch.get())),
                baseline: vec![],
                limit: 0,
                renames: RenameWindow::default(),
            });
        }
        reg
    }

    #[test]
    fn resolve_room_prefers_voice_then_chat_then_none() {
        let reg = registry();
        let mut facts = empty(G);
        // In a room's voice channel: that room, wherever the command is run.
        facts.voice.insert(ME, (VOICE_ROOM, false));
        assert_eq!(
            resolve_room(&facts, &reg, ME, CHAT_ROOM.widen()),
            Some(VOICE_ROOM)
        );
        assert_eq!(
            resolve_room(&facts, &reg, ME, PLAIN.widen()),
            Some(VOICE_ROOM)
        );
        // In voice elsewhere (not a room): the chat it was run in, if a room.
        facts.voice.insert(ME, (PLAIN, false));
        assert_eq!(
            resolve_room(&facts, &reg, ME, CHAT_ROOM.widen()),
            Some(CHAT_ROOM)
        );
        // Not in voice at all: the same.
        facts.voice.remove(&ME);
        assert_eq!(
            resolve_room(&facts, &reg, ME, CHAT_ROOM.widen()),
            Some(CHAT_ROOM)
        );
        // Neither: none, which `run` answers with NotInRoom.
        assert_eq!(resolve_room(&facts, &reg, ME, PLAIN.widen()), None);
    }
}
