//! What the room panel's buttons and modals do. Spec 1.7.
//!
//! 🔑 A button that needs input answers with a modal as the interaction's
//! FIRST response: Discord refuses a modal after an acknowledgement. So such
//! a press is checked (the room, then whether the presser may) before
//! anything is sent, and a refusal is the press's one private response. The
//! modal's submit then checks everything again through [`act`]: the room may
//! have changed while the modal was open.
//!
//! Like `messaging::buttons`, the Discord glue ([`handle_press`],
//! [`handle_submit`]) only gathers what a press needs; [`respond_press`] and
//! [`respond_submit`] decide, and tests drive them with fakes.
use super::authority::{is_moderator, may, standing, Reason, RoomAction};
use super::card::{RoomCard, RoomRefusal};
use super::discord::LiveRoomsDiscord;
use super::facts::{gather, GuildFacts};
use super::ids::{field, ButtonId, ModalId, RoomButton, RoomModal};
use super::ops::{act, validate, RoomCx};
use super::settings::ROOM_NAME_MAX;
use super::state::RoomState;
use crate::messaging::courier;
use crate::messaging::format::clip;
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::{
    ROOM_LABEL_LIMIT, ROOM_LABEL_MEMBER, ROOM_LABEL_MODE, ROOM_LABEL_NAME, ROOM_LABEL_REASON,
    ROOM_LABEL_REASON_OPTIONAL, ROOM_LABEL_ROLE_OPTIONAL, ROOM_LABEL_USER_OPTIONAL,
    ROOM_MODAL_HELPERS, ROOM_MODAL_KICK, ROOM_MODAL_LET_IN, ROOM_MODAL_LIMIT, ROOM_MODAL_MUTE,
    ROOM_MODAL_RENAME, ROOM_MODAL_TRANSFER, ROOM_MODAL_UNMUTE, ROOM_MODAL_WHO_CAN_JOIN,
    ROOM_MODE_ALLOW_ROLE, ROOM_MODE_ALLOW_USER, ROOM_MODE_BLOCK_ROLE, ROOM_MODE_HELPER_ADD,
    ROOM_MODE_HELPER_REMOVE, ROOM_MODE_UNBLOCK_ROLE, ROOM_PICK_MEMBER, ROOM_PICK_MODE,
    ROOM_PICK_ROLE, ROOM_PLACEHOLDER_LIMIT, ROOM_PLACEHOLDER_NAME, ROOM_PLACEHOLDER_REASON,
};
use crate::messaging::render::RenderCx;
use crate::messaging::transport::{DiscordModalPress, DiscordPress, DiscordTransport, Press};
use crate::Data;
use serenity::all::{
    Cache, ChannelId, ComponentInteraction, CreateInputText, CreateLabel, CreateModal,
    CreateModalComponent, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, GuildId,
    InputTextStyle, LabelComponent, ModalComponent, ModalInteraction, RoleId, UserId,
};
use std::borrow::Cow;
use std::time::Instant;

/// The values of a modal's MODE choice ([`field::MODE`]).
pub mod mode {
    pub const HELPER_ADD: &str = "add";
    pub const HELPER_REMOVE: &str = "remove";
    pub const ALLOW_USER: &str = "allow-user";
    pub const ALLOW_ROLE: &str = "allow-role";
    pub const BLOCK_ROLE: &str = "block-role";
    pub const UNBLOCK_ROLE: &str = "unblock-role";
}

/// Discord's cap on a select menu's options.
const MENU_MAX: usize = 25;
/// Discord's cap on a select option's label.
const OPTION_LABEL_MAX: usize = 100;

/// What a room modal's submit carried, by field name ([`field`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Submitted {
    pub member: Option<UserId>,
    pub role: Option<RoleId>,
    pub reason: Option<String>,
    pub value: Option<String>,
    pub mode: Option<String>,
}

/// A press on a `room:` button. Nothing is acknowledged here: a button that
/// opens a modal must open it as its first response.
pub async fn handle_press(data: &Data, ctx: &serenity::all::Context, i: &ComponentInteraction) {
    let press = DiscordPress {
        http: &ctx.http,
        interaction: i,
    };
    let presser = i.user.id;
    let (Some(id), Some(guild)) = (ButtonId::parse(&i.data.custom_id), i.guild_id) else {
        tracing::warn!(
            "rooms: press with an unusable id {:?} from {presser}",
            i.data.custom_id
        );
        answer_first(&press, RoomCard::Refused(RoomRefusal::NotInRoom)).await;
        return;
    };
    // R18: whoever the decision is about, beyond the people in voice.
    let state = room_state(data, id.room, guild).await;
    let owner = state.as_ref().map(|s| s.owner);
    let extra: Vec<UserId> = std::iter::once(presser).chain(owner).collect();
    let Some(mut facts) = gather(&ctx.cache, &ctx.http, guild, &extra).await else {
        tracing::debug!("rooms: {guild} is not cached; press refused");
        answer_first(&press, RoomCard::Refused(RoomRefusal::DiscordFailed)).await;
        return;
    };
    // Unmute and Let back in list people who may have left voice: name them
    // from the cache, never over HTTP (the modal must open inside 3 s).
    if let Some(state) = &state {
        let listed: Vec<UserId> = match id.kind {
            RoomButton::Unmute => state.muted.iter().copied().collect(),
            RoomButton::LetIn => state.blocked.iter().copied().collect(),
            _ => Vec::new(),
        };
        name_from_cache(&ctx.cache, &mut facts, &listed);
    }
    let discord = LiveRoomsDiscord {
        http: ctx.http.clone(),
    };
    let transport = DiscordTransport::of(ctx);
    let cx = RoomCx {
        data,
        discord: &discord,
        transport: &transport,
    };
    respond_press(&cx, &press, &facts, id, presser, Instant::now()).await;
}

/// A submit of a `roomm:` modal.
pub async fn handle_submit(data: &Data, ctx: &serenity::all::Context, i: &ModalInteraction) {
    let press = DiscordModalPress {
        http: &ctx.http,
        interaction: i,
    };
    let submitter = i.user.id;
    let (Some(id), Some(guild)) = (ModalId::parse(&i.data.custom_id), i.guild_id) else {
        tracing::warn!(
            "rooms: submit with an unusable id {:?} from {submitter}",
            i.data.custom_id
        );
        answer_first(&press, RoomCard::Refused(RoomRefusal::NotInRoom)).await;
        return;
    };
    let submitted = read_submit(i);
    // Facts as they are now, not as they were when the modal opened (R18:
    // the submitter, the target and the room's owner).
    let owner = room_state(data, id.room, guild).await.map(|s| s.owner);
    let extra: Vec<UserId> = [Some(submitter), submitted.member, owner]
        .into_iter()
        .flatten()
        .collect();
    let Some(facts) = gather(&ctx.cache, &ctx.http, guild, &extra).await else {
        tracing::debug!("rooms: {guild} is not cached; submit refused");
        answer_first(&press, RoomCard::Refused(RoomRefusal::DiscordFailed)).await;
        return;
    };
    let discord = LiveRoomsDiscord {
        http: ctx.http.clone(),
    };
    let transport = DiscordTransport::of(ctx);
    let cx = RoomCx {
        data,
        discord: &discord,
        transport: &transport,
    };
    respond_submit(
        &cx,
        &press,
        &facts,
        id,
        submitted,
        submitter,
        Instant::now(),
    )
    .await;
}

/// What a press does.
enum Plan {
    /// The guide, privately. Nobody's standing matters.
    Guide,
    /// Acknowledge, act, and answer only a refusal: the redrawn panel is the
    /// answer.
    Act(RoomAction),
    /// Open this modal, if the presser may.
    Ask(RoomModal),
}

fn plan(kind: RoomButton) -> Plan {
    match kind {
        RoomButton::Help => Plan::Guide,
        RoomButton::Lock => Plan::Act(RoomAction::Lock),
        RoomButton::Unlock => Plan::Act(RoomAction::Unlock),
        RoomButton::ChatLock => Plan::Act(RoomAction::ChatLock),
        RoomButton::ChatUnlock => Plan::Act(RoomAction::ChatUnlock),
        RoomButton::Claim => Plan::Act(RoomAction::Claim),
        RoomButton::Kick => Plan::Ask(RoomModal::Kick),
        RoomButton::Mute => Plan::Ask(RoomModal::Mute),
        RoomButton::Unmute => Plan::Ask(RoomModal::Unmute),
        RoomButton::LetIn => Plan::Ask(RoomModal::LetIn),
        RoomButton::Limit => Plan::Ask(RoomModal::Limit),
        RoomButton::Rename => Plan::Ask(RoomModal::Rename),
        RoomButton::Transfer => Plan::Ask(RoomModal::Transfer),
        RoomButton::Helpers => Plan::Ask(RoomModal::Helpers),
        RoomButton::WhoCanJoin => Plan::Ask(RoomModal::WhoCanJoin),
    }
}

/// [`handle_press`] without Discord.
pub(crate) async fn respond_press(
    cx: &RoomCx<'_>,
    press: &dyn Press,
    facts: &GuildFacts,
    id: ButtonId,
    presser: UserId,
    now: Instant,
) {
    // The room this panel was drawn for, in the server it was pressed in.
    let Some(state) = room_state(cx.data, id.room, facts.guild).await else {
        answer_first(press, RoomCard::Refused(RoomRefusal::NotInRoom)).await;
        return;
    };
    match plan(id.kind) {
        Plan::Guide => answer_first(press, RoomCard::Guide).await,
        Plan::Act(action) => {
            // `act` makes Discord calls, which can outlast the 3 s window.
            courier::acknowledge(press).await;
            if let Err(refused) = act(cx, facts, id.room, presser, action, now).await {
                answer_after(press, RoomCard::Refused(refused)).await;
            }
        },
        Plan::Ask(kind) => {
            let modal = may_ask(facts, &state, id.room, presser, kind)
                .and_then(|()| modal_for(kind, id.room, facts, &state, presser));
            match modal {
                Ok(modal) => {
                    if let Err(err) = press.open_modal(modal).await {
                        tracing::warn!("rooms: opening the {kind:?} modal failed: {err:?}");
                    }
                },
                Err(refused) => answer_first(press, RoomCard::Refused(refused)).await,
            }
        },
    }
}

/// Whether the presser may take this kind of action at all, before a modal
/// asks them whom or what for. Targets are the submit's to check.
fn may_ask(
    facts: &GuildFacts,
    state: &RoomState,
    room: ChannelId,
    presser: UserId,
    kind: RoomModal,
) -> Result<(), RoomRefusal> {
    let who = standing(
        presser,
        state,
        is_moderator(facts.guild_perms(presser)),
        facts.in_room(presser, room),
    );
    may(
        who,
        &sample(kind, presser)?,
        facts.in_room(state.owner, room),
    )
}

/// An action of the kind the modal leads to. Its target and reason are
/// stand-ins: `may` reads neither.
fn sample(kind: RoomModal, presser: UserId) -> Result<RoomAction, RoomRefusal> {
    let target = presser;
    Ok(match kind {
        RoomModal::Kick => RoomAction::Kick {
            target,
            reason: Reason::parse("x")?,
        },
        RoomModal::Mute => RoomAction::Mute {
            target,
            reason: None,
        },
        RoomModal::Unmute => RoomAction::Unmute { target },
        RoomModal::LetIn => RoomAction::LetIn { target },
        RoomModal::Limit => RoomAction::Limit(0),
        RoomModal::Rename => RoomAction::Rename(String::new()),
        RoomModal::Transfer => RoomAction::Transfer { target },
        RoomModal::Helpers => RoomAction::HelperAdd { target },
        RoomModal::WhoCanJoin => RoomAction::AllowUser { target },
    })
}

/// The modal a button opens, or why it can't: an empty pick list (Discord
/// rejects a select with no options) or one longer than a select holds.
pub(crate) fn modal_for(
    kind: RoomModal,
    room: ChannelId,
    facts: &GuildFacts,
    state: &RoomState,
    presser: UserId,
) -> Result<CreateModal<'static>, RoomRefusal> {
    let occupants = || -> Vec<UserId> {
        facts
            .humans_in(room)
            .into_iter()
            .filter(|u| *u != presser)
            .collect()
    };
    let inputs = match kind {
        RoomModal::Kick => vec![
            pick_member(facts, occupants(), RoomRefusal::TargetNotInRoom)?,
            reason(true),
        ],
        RoomModal::Mute => vec![
            pick_member(facts, occupants(), RoomRefusal::TargetNotInRoom)?,
            reason(false),
        ],
        RoomModal::Unmute => vec![pick_member(
            facts,
            state.muted.iter().copied().collect(),
            RoomRefusal::NobodyMuted,
        )?],
        RoomModal::LetIn => vec![pick_member(
            facts,
            state.blocked.iter().copied().collect(),
            RoomRefusal::NotBlocked,
        )?],
        RoomModal::Transfer => vec![pick_member(
            facts,
            occupants(),
            RoomRefusal::TargetNotInRoom,
        )?],
        RoomModal::Limit => vec![text(ROOM_LABEL_LIMIT, ROOM_PLACEHOLDER_LIMIT, 2)],
        RoomModal::Rename => vec![text(
            ROOM_LABEL_NAME,
            ROOM_PLACEHOLDER_NAME,
            u16::try_from(ROOM_NAME_MAX).unwrap_or(u16::MAX),
        )],
        RoomModal::Helpers => vec![
            choose(&[
                (ROOM_MODE_HELPER_ADD, mode::HELPER_ADD),
                (ROOM_MODE_HELPER_REMOVE, mode::HELPER_REMOVE),
            ]),
            pick_user(ROOM_LABEL_MEMBER, true),
        ],
        RoomModal::WhoCanJoin => vec![
            choose(&[
                (ROOM_MODE_ALLOW_USER, mode::ALLOW_USER),
                (ROOM_MODE_ALLOW_ROLE, mode::ALLOW_ROLE),
                (ROOM_MODE_BLOCK_ROLE, mode::BLOCK_ROLE),
                (ROOM_MODE_UNBLOCK_ROLE, mode::UNBLOCK_ROLE),
            ]),
            pick_user(ROOM_LABEL_USER_OPTIONAL, false),
            pick_role(ROOM_LABEL_ROLE_OPTIONAL),
        ],
    };
    Ok(CreateModal::new(ModalId { kind, room }.custom_id(), title(kind)).components(inputs))
}

fn title(kind: RoomModal) -> &'static str {
    match kind {
        RoomModal::Kick => ROOM_MODAL_KICK,
        RoomModal::Mute => ROOM_MODAL_MUTE,
        RoomModal::Unmute => ROOM_MODAL_UNMUTE,
        RoomModal::LetIn => ROOM_MODAL_LET_IN,
        RoomModal::Limit => ROOM_MODAL_LIMIT,
        RoomModal::Rename => ROOM_MODAL_RENAME,
        RoomModal::Transfer => ROOM_MODAL_TRANSFER,
        RoomModal::Helpers => ROOM_MODAL_HELPERS,
        RoomModal::WhoCanJoin => ROOM_MODAL_WHO_CAN_JOIN,
    }
}

/// A required pick of one of `people`, each by name (else by id), valued by
/// user id.
fn pick_member(
    facts: &GuildFacts,
    people: Vec<UserId>,
    nobody: RoomRefusal,
) -> Result<CreateModalComponent<'static>, RoomRefusal> {
    if people.is_empty() {
        return Err(nobody);
    }
    if people.len() > MENU_MAX {
        return Err(RoomRefusal::TooManyForMenu);
    }
    let options: Vec<CreateSelectMenuOption<'static>> = people
        .into_iter()
        .map(|u| {
            let name = facts
                .display
                .get(&u)
                .map_or_else(|| u.to_string(), |n| clip(n, OPTION_LABEL_MAX));
            CreateSelectMenuOption::new(name, u.to_string())
        })
        .collect();
    let menu = CreateSelectMenu::new(
        field::MEMBER,
        CreateSelectMenuKind::String {
            options: Cow::Owned(options),
        },
    )
    .placeholder(ROOM_PICK_MEMBER)
    .min_values(1)
    .max_values(1)
    .required(true);
    Ok(CreateModalComponent::Label(CreateLabel::select_menu(
        ROOM_LABEL_MEMBER,
        menu,
    )))
}

/// A pick of any one member of the server.
fn pick_user(label: &'static str, required: bool) -> CreateModalComponent<'static> {
    let menu = CreateSelectMenu::new(
        field::MEMBER,
        CreateSelectMenuKind::User {
            default_users: None,
        },
    )
    .placeholder(ROOM_PICK_MEMBER);
    CreateModalComponent::Label(CreateLabel::select_menu(label, one(menu, required)))
}

/// An optional pick of one role.
fn pick_role(label: &'static str) -> CreateModalComponent<'static> {
    let menu = CreateSelectMenu::new(
        field::ROLE,
        CreateSelectMenuKind::Role {
            default_roles: None,
        },
    )
    .placeholder(ROOM_PICK_ROLE);
    CreateModalComponent::Label(CreateLabel::select_menu(label, one(menu, false)))
}

/// A select of at most one; an optional one may be left empty (Discord
/// allows `min_values` 0 only when `required` is false).
fn one(menu: CreateSelectMenu<'static>, required: bool) -> CreateSelectMenu<'static> {
    menu.min_values(u8::from(required))
        .max_values(1)
        .required(required)
}

/// A required choice of one `(label, value)`.
fn choose(choices: &[(&'static str, &'static str)]) -> CreateModalComponent<'static> {
    let options: Vec<CreateSelectMenuOption<'static>> = choices
        .iter()
        .map(|&(label, value)| CreateSelectMenuOption::new(label, value))
        .collect();
    let menu = CreateSelectMenu::new(
        field::MODE,
        CreateSelectMenuKind::String {
            options: Cow::Owned(options),
        },
    )
    .placeholder(ROOM_PICK_MODE);
    CreateModalComponent::Label(CreateLabel::select_menu(ROOM_LABEL_MODE, one(menu, true)))
}

/// A kick's reason (required) or a mute's (optional).
fn reason(required: bool) -> CreateModalComponent<'static> {
    let max = u16::try_from(Reason::MAX).unwrap_or(u16::MAX);
    let mut input = CreateInputText::new(InputTextStyle::Paragraph, field::REASON)
        .placeholder(ROOM_PLACEHOLDER_REASON)
        .max_length(max)
        .required(required);
    let label = if required {
        input = input.min_length(1);
        ROOM_LABEL_REASON
    } else {
        ROOM_LABEL_REASON_OPTIONAL
    };
    CreateModalComponent::Label(CreateLabel::input_text(label, input))
}

/// A required one-line value, 1 to `max` characters.
fn text(label: &'static str, placeholder: &'static str, max: u16) -> CreateModalComponent<'static> {
    let input = CreateInputText::new(InputTextStyle::Short, field::VALUE)
        .placeholder(placeholder)
        .min_length(1)
        .max_length(max)
        .required(true);
    CreateModalComponent::Label(CreateLabel::input_text(label, input))
}

/// The fields of a room modal's submit.
pub fn read_submit(i: &ModalInteraction) -> Submitted {
    read_fields(&i.data.components)
}

/// [`read_submit`] over the submitted components. Each input sits in a label;
/// a select carries its picks as `values`, a text input its `value`. An id
/// that is not one (0, `u64::MAX`, not a number) reads as nothing picked.
pub fn read_fields(components: &[ModalComponent]) -> Submitted {
    let mut s = Submitted::default();
    for c in components {
        let ModalComponent::Label(label) = c else {
            continue;
        };
        match &label.component {
            LabelComponent::SelectMenu(menu) => {
                let first = menu.values.first().map(String::as_str);
                match &*menu.custom_id {
                    field::MEMBER => s.member = first.and_then(snowflake).map(UserId::new),
                    field::ROLE => s.role = first.and_then(snowflake).map(RoleId::new),
                    field::MODE => s.mode = first.map(str::to_owned),
                    _ => {},
                }
            },
            LabelComponent::InputText(input) => match &*input.custom_id {
                field::REASON => s.reason = Some(input.value.to_string()),
                field::VALUE => s.value = Some(input.value.to_string()),
                _ => {},
            },
            _ => {},
        }
    }
    s
}

/// An id serenity can hold: `new` panics on `u64::MAX`, and 0 is never issued.
fn snowflake(raw: &str) -> Option<u64> {
    raw.parse::<u64>().ok().filter(|&n| n != 0 && n != u64::MAX)
}

/// The action a submit asks for, with its input checked.
pub fn action_for(kind: RoomModal, s: &Submitted) -> Result<RoomAction, RoomRefusal> {
    let member = || s.member.ok_or(RoomRefusal::MissingChoice);
    let role = || s.role.ok_or(RoomRefusal::MissingChoice);
    Ok(match kind {
        RoomModal::Kick => RoomAction::Kick {
            target: member()?,
            reason: Reason::parse(s.reason.as_deref().unwrap_or_default())?,
        },
        RoomModal::Mute => RoomAction::Mute {
            target: member()?,
            reason: match s.reason.as_deref().map(str::trim) {
                None | Some("") => None,
                Some(raw) => Some(Reason::parse(raw)?),
            },
        },
        RoomModal::Unmute => RoomAction::Unmute { target: member()? },
        RoomModal::LetIn => RoomAction::LetIn { target: member()? },
        RoomModal::Transfer => RoomAction::Transfer { target: member()? },
        RoomModal::Limit => {
            let n = s
                .value
                .as_deref()
                .and_then(|v| v.trim().parse::<u8>().ok())
                .ok_or(RoomRefusal::BadLimit)?;
            validate(RoomAction::Limit(n))?
        },
        RoomModal::Rename => validate(RoomAction::Rename(s.value.clone().unwrap_or_default()))?,
        RoomModal::Helpers => match s.mode.as_deref() {
            Some(mode::HELPER_ADD) => RoomAction::HelperAdd { target: member()? },
            Some(mode::HELPER_REMOVE) => RoomAction::HelperRemove { target: member()? },
            _ => return Err(RoomRefusal::MissingChoice),
        },
        RoomModal::WhoCanJoin => match s.mode.as_deref() {
            Some(mode::ALLOW_USER) => RoomAction::AllowUser { target: member()? },
            Some(mode::ALLOW_ROLE) => RoomAction::AllowRole { role: role()? },
            Some(mode::BLOCK_ROLE) => RoomAction::BlockRole { role: role()? },
            Some(mode::UNBLOCK_ROLE) => RoomAction::UnblockRole { role: role()? },
            _ => return Err(RoomRefusal::MissingChoice),
        },
    })
}

/// [`handle_submit`] without Discord. Acknowledged first (a deferred update:
/// `act` makes Discord calls and can outlast the 3 s window), then `act`
/// checks the room, the submitter's standing and the target's presence as
/// they are now. The answer is a private follow-up; the redrawn panel is
/// `act`'s.
pub(crate) async fn respond_submit(
    cx: &RoomCx<'_>,
    press: &dyn Press,
    facts: &GuildFacts,
    id: ModalId,
    s: Submitted,
    submitter: UserId,
    now: Instant,
) {
    courier::acknowledge(press).await;
    let outcome = match action_for(id.kind, &s) {
        Ok(action) => act(cx, facts, id.room, submitter, action, now).await,
        Err(refused) => Err(refused),
    };
    let card = match outcome {
        Ok(done) => RoomCard::Done(done),
        Err(refused) => RoomCard::Refused(refused),
    };
    answer_after(press, card).await;
}

/// A copy of the room's state, if it is registered in `guild`. The room's
/// lock is held only to copy it.
async fn room_state(data: &Data, room: ChannelId, guild: GuildId) -> Option<RoomState> {
    let handle = data.rooms.get(room)?;
    let live = handle.lock().await;
    (live.guild == guild).then(|| live.state.clone())
}

/// Display names for `users` from the cache only, for those the facts lack.
fn name_from_cache(cache: &Cache, facts: &mut GuildFacts, users: &[UserId]) {
    if users.is_empty() {
        return;
    }
    let Some(g) = cache.guild(facts.guild) else {
        return;
    };
    for u in users {
        if let Some(m) = g.members.get(u) {
            facts
                .display
                .entry(*u)
                .or_insert_with(|| m.display_name().to_string());
        }
    }
}

/// The press's one response, privately.
async fn answer_first(press: &dyn Press, card: RoomCard) {
    let msg = CrackedMessage::from(card);
    if let Err(err) = courier::respond(press, &msg, &RenderCx::now(), true).await {
        tracing::warn!("rooms: answering a press failed: {err:?}");
    }
}

/// A private follow-up to an acknowledged press.
async fn answer_after(press: &dyn Press, card: RoomCard) {
    courier::answer_privately(press, &CrackedMessage::from(card), &RenderCx::now()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::messages::*;
    use crate::messaging::test_support::{FakePress, FakeTransport, Op, PressOp};
    use crate::rooms::authority::Reason;
    use crate::rooms::facts::tests::empty;
    use crate::rooms::ids::{field, RoomButton};
    use crate::rooms::ops::act;
    use crate::rooms::registry::{LiveRoom, RenameWindow};
    use crate::rooms::state::{OverwriteTarget, StoredOverwrite};
    use crate::rooms::test_support::{Call, FakeRoomsDiscord};
    use crate::DataInner;
    use serde_json::Value;
    use serenity::all::{GuildId, MessageId, ModalInteractionData, Permissions};
    use std::sync::Arc;

    const G: GuildId = GuildId::new(100);
    const ROOM: ChannelId = ChannelId::new(500);
    const PANEL: MessageId = MessageId::new(900);
    const OWNER: UserId = UserId::new(1);
    const HELPER: UserId = UserId::new(2);
    const TARGET: UserId = UserId::new(3);
    const OUTSIDE: UserId = UserId::new(4);
    const MODERATOR: UserId = UserId::new(5);
    const BOT: UserId = UserId::new(99);
    const OTHER_BOT: UserId = UserId::new(98);
    const MOD_ROLE: RoleId = RoleId::new(7);
    const C: Permissions = Permissions::CONNECT;
    const V: Permissions = Permissions::VIEW_CHANNEL;
    const NONE: Permissions = Permissions::empty();

    /// Owner, helper, target, CrackTunes and another bot in the room; one
    /// member outside it; a moderator (Move Members) who is not in voice.
    /// The target has no display name.
    fn facts() -> GuildFacts {
        let mut f = empty(G);
        f.bot = BOT;
        f.role_perms = [(MOD_ROLE, Permissions::MOVE_MEMBERS)].into();
        f.roles = [
            (OWNER, vec![]),
            (HELPER, vec![]),
            (TARGET, vec![]),
            (OUTSIDE, vec![]),
            (MODERATOR, vec![MOD_ROLE]),
        ]
        .into();
        f.voice = [
            (OWNER, (ROOM, false)),
            (HELPER, (ROOM, false)),
            (TARGET, (ROOM, false)),
            (BOT, (ROOM, true)),
            (OTHER_BOT, (ROOM, true)),
            (OUTSIDE, (ChannelId::new(501), false)),
        ]
        .into();
        f.display = [
            (OWNER, "Olive".to_string()),
            (HELPER, "Hal".to_string()),
            (OUTSIDE, "Oscar".to_string()),
        ]
        .into();
        f
    }

    fn live() -> LiveRoom {
        let mut state = RoomState::new(OWNER);
        state.helpers.insert(HELPER);
        state.panel = Some(PANEL);
        LiveRoom {
            guild: G,
            channel: ROOM,
            name: "Olive's room".into(),
            state,
            baseline: vec![],
            limit: 0,
            renames: RenameWindow::default(),
        }
    }

    fn ow(target: OverwriteTarget, allow: Permissions, deny: Permissions) -> StoredOverwrite {
        StoredOverwrite {
            target,
            allow,
            deny,
        }
    }

    struct World {
        data: Data,
        discord: FakeRoomsDiscord,
        transport: FakeTransport,
        press: FakePress,
    }

    impl World {
        fn new(room: LiveRoom) -> Self {
            let data = Data(Arc::new(DataInner {
                ..Default::default()
            }));
            data.rooms.insert(room);
            Self {
                data,
                discord: FakeRoomsDiscord::default(),
                transport: FakeTransport::default(),
                press: FakePress::default(),
            }
        }

        fn cx(&self) -> RoomCx<'_> {
            RoomCx {
                data: &self.data,
                discord: &self.discord,
                transport: &self.transport,
            }
        }

        async fn press(&self, kind: RoomButton, presser: UserId) {
            self.press_with(&facts(), kind, presser).await;
        }

        async fn press_with(&self, facts: &GuildFacts, kind: RoomButton, presser: UserId) {
            let id = ButtonId { kind, room: ROOM };
            respond_press(&self.cx(), &self.press, facts, id, presser, Instant::now()).await;
        }

        async fn submit(&self, kind: RoomModal, s: Submitted, submitter: UserId) {
            let id = ModalId { kind, room: ROOM };
            respond_submit(
                &self.cx(),
                &self.press,
                &facts(),
                id,
                s,
                submitter,
                Instant::now(),
            )
            .await;
        }

        async fn room(&self) -> LiveRoom {
            self.data
                .rooms
                .get(ROOM)
                .expect("the room is registered")
                .lock()
                .await
                .clone()
        }

        /// Nothing reached Discord but the press's own answers.
        fn sent_nothing(&self) {
            assert_eq!(self.discord.calls(), vec![], "no channel calls");
            assert_eq!(self.transport.ops(), vec![], "no messages");
        }
    }

    fn private(text: &str) -> PressOp {
        PressOp::Respond {
            ephemeral: true,
            text: text.to_owned(),
        }
    }

    fn followup(text: &str) -> PressOp {
        PressOp::Followup {
            ephemeral: true,
            text: text.to_owned(),
        }
    }

    fn json(modal: &CreateModal<'static>) -> Value {
        serde_json::to_value(modal).expect("a modal serializes")
    }

    /// The input inside the label whose input has `custom_id`.
    fn input(modal: &Value, custom_id: &str) -> Value {
        modal["components"]
            .as_array()
            .expect("components")
            .iter()
            .map(|label| {
                assert_eq!(label["type"], 18, "every input sits in a label");
                assert!(label["label"]
                    .as_str()
                    .is_some_and(|l| l.chars().count() <= 45));
                label["component"].clone()
            })
            .find(|c| c["custom_id"] == custom_id)
            .unwrap_or_else(|| panic!("no {custom_id} in {modal}"))
    }

    /// `(label, value)` of each option of a string select.
    fn options(modal: &Value, custom_id: &str) -> Vec<(String, String)> {
        input(modal, custom_id)["options"]
            .as_array()
            .expect("a string select")
            .iter()
            .map(|o| {
                (
                    o["label"].as_str().unwrap().to_owned(),
                    o["value"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    fn picked(member: UserId) -> Submitted {
        Submitted {
            member: Some(member),
            ..Default::default()
        }
    }

    fn kick_of(member: UserId, reason: &str) -> Submitted {
        Submitted {
            member: Some(member),
            reason: Some(reason.into()),
            ..Default::default()
        }
    }

    // Presses.

    #[tokio::test]
    async fn kick_button_opens_a_modal_as_first_response() {
        let w = World::new(live());
        w.press(RoomButton::Kick, OWNER).await;
        assert_eq!(
            w.press.ops(),
            vec![PressOp::Modal("roomm:kick:500".into())],
            "the modal is the one response: no acknowledgement before it"
        );
        w.sent_nothing();
    }

    #[tokio::test]
    async fn kick_button_from_an_outsider_refuses_privately_without_modal() {
        let w = World::new(live());
        w.press(RoomButton::Kick, OUTSIDE).await;
        assert_eq!(w.press.ops(), vec![private(ROOM_NOT_ALLOWED)]);
        assert!(w.press.modals.lock().unwrap().is_empty());
        w.sent_nothing();
    }

    /// The pre-check is `may` for each input button: a helper may kick, mute,
    /// unmute, let in and limit, and nothing else; a moderator outside the
    /// room may do all of it.
    #[tokio::test]
    async fn input_buttons_check_who_may_before_opening_their_modal() {
        let mut room = live();
        room.state.muted.insert(TARGET);
        room.state.blocked.insert(OUTSIDE);
        let inputs = [
            (RoomButton::Kick, "kick", true),
            (RoomButton::Mute, "mute", true),
            (RoomButton::Unmute, "unmute", true),
            (RoomButton::LetIn, "let-in", true),
            (RoomButton::Limit, "limit", true),
            (RoomButton::Rename, "rename", false),
            (RoomButton::Transfer, "transfer", false),
            (RoomButton::Helpers, "helpers", false),
            (RoomButton::WhoCanJoin, "who", false),
        ];
        for (kind, slug, helper_may) in inputs {
            let modal = PressOp::Modal(format!("roomm:{slug}:500"));
            let w = World::new(room.clone());
            w.press(kind, HELPER).await;
            let want = if helper_may {
                modal.clone()
            } else {
                private(ROOM_NOT_ALLOWED)
            };
            assert_eq!(w.press.ops(), vec![want], "helper, {kind:?}");

            let w = World::new(room.clone());
            w.press(kind, MODERATOR).await;
            assert_eq!(w.press.ops(), vec![modal.clone()], "moderator, {kind:?}");

            let w = World::new(room.clone());
            w.press(kind, TARGET).await;
            assert_eq!(w.press.ops(), vec![private(ROOM_NOT_ALLOWED)], "occupant");
            w.sent_nothing();
        }
    }

    #[tokio::test]
    async fn lock_button_acknowledges_acts_and_redraws() {
        let w = World::new(live());
        w.press(RoomButton::Lock, HELPER).await;
        assert_eq!(
            w.press.ops(),
            vec![PressOp::Acknowledge],
            "the redrawn panel is the answer"
        );
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Put(
                    ROOM.get(),
                    ow(OverwriteTarget::Member { id: OWNER }, C, NONE)
                ),
                Call::Put(
                    ROOM.get(),
                    ow(OverwriteTarget::Member { id: HELPER }, C, NONE)
                ),
                Call::Put(
                    ROOM.get(),
                    ow(
                        OverwriteTarget::Role {
                            id: RoleId::new(G.get())
                        },
                        NONE,
                        C
                    )
                ),
            ]
        );
        assert!(w.room().await.state.locked);
        assert_eq!(w.transport.ops(), vec![Op::Edit(ROOM.get(), PANEL.get())]);
    }

    #[tokio::test]
    async fn the_other_direct_buttons_act_too() {
        let mut room = live();
        room.state.locked = true;
        room.state.chat_locked = true;
        for (kind, check) in [
            (
                RoomButton::Unlock,
                (|r: &LiveRoom| !r.state.locked) as fn(&LiveRoom) -> bool,
            ),
            (RoomButton::ChatUnlock, |r| !r.state.chat_locked),
        ] {
            let w = World::new(room.clone());
            w.press(kind, OWNER).await;
            assert_eq!(w.press.ops(), vec![PressOp::Acknowledge], "{kind:?}");
            assert!(check(&w.room().await), "{kind:?}");
        }
        let w = World::new(live());
        w.press(RoomButton::ChatLock, OWNER).await;
        assert_eq!(w.press.ops(), vec![PressOp::Acknowledge]);
        assert!(w.room().await.state.chat_locked);

        // Claim, by someone in the room while the owner is away.
        let mut away = facts();
        away.voice.remove(&OWNER);
        let w = World::new(live());
        w.press_with(&away, RoomButton::Claim, TARGET).await;
        assert_eq!(w.press.ops(), vec![PressOp::Acknowledge]);
        assert_eq!(w.room().await.state.owner, TARGET);
    }

    #[tokio::test]
    async fn a_refused_direct_button_is_answered_privately_after_the_ack() {
        let w = World::new(live());
        w.press(RoomButton::Claim, TARGET).await;
        assert_eq!(
            w.press.ops(),
            vec![PressOp::Acknowledge, followup(ROOM_OWNER_PRESENT)]
        );
        w.sent_nothing();

        let w = World::new(live());
        w.press(RoomButton::Lock, OUTSIDE).await;
        assert_eq!(
            w.press.ops(),
            vec![PressOp::Acknowledge, followup(ROOM_NOT_ALLOWED)]
        );
        w.sent_nothing();
        assert!(!w.room().await.state.locked);
    }

    #[tokio::test]
    async fn help_answers_privately() {
        let w = World::new(live());
        // Nobody's standing matters: an outsider gets the guide too.
        w.press(RoomButton::Help, OUTSIDE).await;
        let ops = w.press.ops();
        assert_eq!(ops.len(), 1, "{ops:?}");
        match &ops[0] {
            PressOp::Respond {
                ephemeral: true,
                text,
            } => assert!(text.contains(ROOM_GUIDE_INTRO), "{text}"),
            other => panic!("not a private answer: {other:?}"),
        }
        w.sent_nothing();
    }

    #[tokio::test]
    async fn a_press_for_a_gone_room_or_another_server_is_refused_privately() {
        let w = World::new(live());
        w.data.rooms.remove(ROOM);
        w.press(RoomButton::Kick, OWNER).await;
        assert_eq!(w.press.ops(), vec![private(ROOM_NOT_IN_ROOM)]);

        let mut elsewhere = facts();
        elsewhere.guild = GuildId::new(101);
        for kind in [RoomButton::Kick, RoomButton::Lock] {
            let w = World::new(live());
            w.press_with(&elsewhere, kind, OWNER).await;
            assert_eq!(w.press.ops(), vec![private(ROOM_NOT_IN_ROOM)], "{kind:?}");
            w.sent_nothing();
            assert!(w.press.modals.lock().unwrap().is_empty());
        }
    }

    // Modals.

    #[test]
    fn kick_modal_lists_only_humans_in_the_room_minus_presser() {
        let modal =
            json(&modal_for(RoomModal::Kick, ROOM, &facts(), &live().state, OWNER).unwrap());
        assert_eq!(modal["custom_id"], "roomm:kick:500");
        assert_eq!(modal["title"], ROOM_MODAL_KICK);
        // Not the presser, not a bot, not CrackTunes, not someone elsewhere;
        // a member with no known name is listed by id.
        assert_eq!(
            options(&modal, field::MEMBER),
            vec![
                ("Hal".to_string(), "2".to_string()),
                ("3".to_string(), "3".to_string())
            ]
        );
        let menu = input(&modal, field::MEMBER);
        assert_eq!(menu["type"], 3, "a string select");
        assert_eq!(
            (menu["min_values"].clone(), menu["max_values"].clone()),
            (1.into(), 1.into())
        );
        assert_eq!(menu["required"], true);
    }

    #[test]
    fn kick_modal_requires_a_reason() {
        let modal =
            json(&modal_for(RoomModal::Kick, ROOM, &facts(), &live().state, OWNER).unwrap());
        let reason = input(&modal, field::REASON);
        assert_eq!(reason["type"], 4, "a text input");
        assert_eq!(reason["required"], true);
        assert_eq!(reason["min_length"], 1);
        assert_eq!(reason["max_length"], 200);
        assert_eq!(reason["style"], 2, "a paragraph");

        // Mute's reason is optional.
        let modal =
            json(&modal_for(RoomModal::Mute, ROOM, &facts(), &live().state, OWNER).unwrap());
        assert_eq!(modal["custom_id"], "roomm:mute:500");
        let reason = input(&modal, field::REASON);
        assert_eq!(reason["required"], false);
        assert_eq!(reason["max_length"], 200);
        assert_eq!(options(&modal, field::MEMBER).len(), 2);
    }

    #[test]
    fn select_modals_list_whom_they_act_on() {
        let mut state = live().state;
        state.muted.extend([TARGET, OUTSIDE]);
        state.blocked.insert(UserId::new(42));
        let f = facts();
        let modal = |kind| json(&modal_for(kind, ROOM, &f, &state, OWNER).unwrap());
        assert_eq!(
            options(&modal(RoomModal::Unmute), field::MEMBER),
            vec![("3".into(), "3".into()), ("Oscar".into(), "4".into())],
            "the muted, here or not"
        );
        assert_eq!(
            options(&modal(RoomModal::LetIn), field::MEMBER),
            vec![("42".into(), "42".into())],
            "the blocked"
        );
        assert_eq!(
            options(&modal(RoomModal::Transfer), field::MEMBER),
            vec![("Hal".into(), "2".into()), ("3".into(), "3".into())],
            "the room's people but the presser"
        );
    }

    #[test]
    fn an_empty_or_overfull_pick_list_refuses_instead_of_opening() {
        let state = live().state;
        let mut alone = facts();
        alone.voice.retain(|u, _| *u == OWNER || *u == BOT);
        for kind in [RoomModal::Kick, RoomModal::Mute, RoomModal::Transfer] {
            assert_eq!(
                modal_for(kind, ROOM, &alone, &state, OWNER).err(),
                Some(RoomRefusal::TargetNotInRoom),
                "{kind:?}"
            );
        }
        assert_eq!(
            modal_for(RoomModal::LetIn, ROOM, &facts(), &state, OWNER).err(),
            Some(RoomRefusal::NotBlocked)
        );
        assert_eq!(
            modal_for(RoomModal::Unmute, ROOM, &facts(), &state, OWNER).err(),
            Some(RoomRefusal::NobodyMuted)
        );

        // 25 others fit; 26 do not.
        let mut full = facts();
        full.voice = (1..=26).map(|n| (UserId::new(n), (ROOM, false))).collect();
        assert!(modal_for(RoomModal::Kick, ROOM, &full, &state, OWNER).is_ok());
        full.voice.insert(UserId::new(27), (ROOM, false));
        for kind in [RoomModal::Kick, RoomModal::Mute, RoomModal::Transfer] {
            assert_eq!(
                modal_for(kind, ROOM, &full, &state, OWNER).err(),
                Some(RoomRefusal::TooManyForMenu),
                "{kind:?}"
            );
        }
        let mut crowded = state.clone();
        crowded.blocked = (100..126).map(UserId::new).collect();
        crowded.muted = crowded.blocked.clone();
        for kind in [RoomModal::LetIn, RoomModal::Unmute] {
            assert_eq!(
                modal_for(kind, ROOM, &facts(), &crowded, OWNER).err(),
                Some(RoomRefusal::TooManyForMenu),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn text_and_choice_modals_carry_their_fields() {
        let state = live().state;
        let f = facts();
        let modal = |kind| json(&modal_for(kind, ROOM, &f, &state, OWNER).unwrap());

        let limit = input(&modal(RoomModal::Limit), field::VALUE);
        assert_eq!(
            (limit["min_length"].clone(), limit["max_length"].clone()),
            (1.into(), 2.into())
        );
        assert_eq!(
            (limit["style"].clone(), limit["required"].clone()),
            (1.into(), true.into())
        );
        let name = input(&modal(RoomModal::Rename), field::VALUE);
        assert_eq!(
            (name["min_length"].clone(), name["max_length"].clone()),
            (1.into(), 100.into())
        );

        let helpers = modal(RoomModal::Helpers);
        let values = |m: &Value| -> Vec<String> {
            options(m, field::MODE)
                .into_iter()
                .map(|(_, v)| v)
                .collect()
        };
        assert_eq!(values(&helpers), ["add", "remove"]);
        let member = input(&helpers, field::MEMBER);
        assert_eq!(
            (member["type"].clone(), member["required"].clone()),
            (5.into(), true.into())
        );

        let who = modal(RoomModal::WhoCanJoin);
        assert_eq!(
            values(&who),
            ["allow-user", "allow-role", "block-role", "unblock-role"]
        );
        let member = input(&who, field::MEMBER);
        let role = input(&who, field::ROLE);
        assert_eq!(
            (member["type"].clone(), role["type"].clone()),
            (5.into(), 6.into())
        );
        for optional in [member, role] {
            assert_eq!(optional["required"], false);
            assert_eq!(optional["min_values"], 0);
        }
        for kind in RoomModal::ALL {
            let mut state = state.clone();
            state.muted.insert(TARGET);
            state.blocked.insert(OUTSIDE);
            let m = json(&modal_for(kind, ROOM, &f, &state, OWNER).unwrap());
            assert!(
                m["title"].as_str().unwrap().chars().count() <= 45,
                "{kind:?}"
            );
            assert!(m["components"].as_array().unwrap().len() <= 5, "{kind:?}");
        }
    }

    // Reading a submit.

    #[test]
    fn read_fields_pulls_each_input_by_name() {
        // Discord's documented modal-submit shape: each input inside a label.
        let data: ModalInteractionData = serde_json::from_str(
            r#"{
                "custom_id": "roomm:who:500",
                "components": [
                    {"type": 18, "id": 1, "component":
                        {"type": 3, "id": 2, "custom_id": "mode", "values": ["allow-role"]}},
                    {"type": 18, "id": 3, "component":
                        {"type": 5, "id": 4, "custom_id": "member", "values": ["3"]}},
                    {"type": 18, "id": 5, "component":
                        {"type": 6, "id": 6, "custom_id": "role", "values": ["18"]}},
                    {"type": 18, "id": 7, "component":
                        {"type": 4, "id": 8, "custom_id": "reason", "value": "spamming"}},
                    {"type": 18, "id": 9, "component":
                        {"type": 4, "id": 10, "custom_id": "value", "value": " 12 "}}
                ],
                "resolved": {}
            }"#,
        )
        .expect("Discord's payload deserializes");
        assert_eq!(
            read_fields(&data.components),
            Submitted {
                member: Some(TARGET),
                role: Some(RoleId::new(18)),
                reason: Some("spamming".into()),
                value: Some(" 12 ".into()),
                mode: Some("allow-role".into()),
            }
        );

        // An optional select left empty, and ids that are not ids.
        let data: ModalInteractionData = serde_json::from_str(
            r#"{
                "custom_id": "roomm:who:500",
                "components": [
                    {"type": 18, "id": 1, "component":
                        {"type": 5, "id": 2, "custom_id": "member", "values": []}},
                    {"type": 18, "id": 3, "component":
                        {"type": 6, "id": 4, "custom_id": "role", "values": ["18446744073709551615"]}},
                    {"type": 18, "id": 5, "component":
                        {"type": 3, "id": 6, "custom_id": "other", "values": ["0"]}}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(read_fields(&data.components), Submitted::default());
        let data: ModalInteractionData = serde_json::from_str(
            r#"{"custom_id": "roomm:kick:500", "components": [
                {"type": 18, "id": 1, "component":
                    {"type": 3, "id": 2, "custom_id": "member", "values": ["0"]}}]}"#,
        )
        .unwrap();
        assert_eq!(read_fields(&data.components).member, None);
    }

    #[test]
    fn action_for_builds_each_action() {
        let t = Some(TARGET);
        let role = Some(RoleId::new(18));
        let with = |mode: &str| Submitted {
            member: t,
            role,
            mode: Some(mode.into()),
            ..Default::default()
        };
        let reason = Reason::parse("spamming").unwrap();
        let cases = [
            (
                RoomModal::Kick,
                kick_of(TARGET, " spamming "),
                RoomAction::Kick {
                    target: TARGET,
                    reason: reason.clone(),
                },
            ),
            (
                RoomModal::Mute,
                kick_of(TARGET, "spamming"),
                RoomAction::Mute {
                    target: TARGET,
                    reason: Some(reason),
                },
            ),
            (
                RoomModal::Mute,
                kick_of(TARGET, "  "),
                RoomAction::Mute {
                    target: TARGET,
                    reason: None,
                },
            ),
            (
                RoomModal::Mute,
                picked(TARGET),
                RoomAction::Mute {
                    target: TARGET,
                    reason: None,
                },
            ),
            (
                RoomModal::Unmute,
                picked(TARGET),
                RoomAction::Unmute { target: TARGET },
            ),
            (
                RoomModal::LetIn,
                picked(TARGET),
                RoomAction::LetIn { target: TARGET },
            ),
            (
                RoomModal::Transfer,
                picked(TARGET),
                RoomAction::Transfer { target: TARGET },
            ),
            (
                RoomModal::Limit,
                Submitted {
                    value: Some(" 7 ".into()),
                    ..Default::default()
                },
                RoomAction::Limit(7),
            ),
            (
                RoomModal::Rename,
                Submitted {
                    value: Some("  Den  ".into()),
                    ..Default::default()
                },
                RoomAction::Rename("Den".into()),
            ),
            (
                RoomModal::Helpers,
                with("add"),
                RoomAction::HelperAdd { target: TARGET },
            ),
            (
                RoomModal::Helpers,
                with("remove"),
                RoomAction::HelperRemove { target: TARGET },
            ),
            (
                RoomModal::WhoCanJoin,
                with("allow-user"),
                RoomAction::AllowUser { target: TARGET },
            ),
            (
                RoomModal::WhoCanJoin,
                with("allow-role"),
                RoomAction::AllowRole {
                    role: RoleId::new(18),
                },
            ),
            (
                RoomModal::WhoCanJoin,
                with("block-role"),
                RoomAction::BlockRole {
                    role: RoleId::new(18),
                },
            ),
            (
                RoomModal::WhoCanJoin,
                with("unblock-role"),
                RoomAction::UnblockRole {
                    role: RoleId::new(18),
                },
            ),
        ];
        for (kind, s, want) in cases {
            assert_eq!(action_for(kind, &s), Ok(want), "{kind:?} {s:?}");
        }
    }

    #[test]
    fn action_for_missing_member_is_missing_choice() {
        for kind in [
            RoomModal::Kick,
            RoomModal::Mute,
            RoomModal::Unmute,
            RoomModal::LetIn,
            RoomModal::Transfer,
        ] {
            let s = Submitted {
                reason: Some("spamming".into()),
                ..Default::default()
            };
            assert_eq!(
                action_for(kind, &s),
                Err(RoomRefusal::MissingChoice),
                "{kind:?}"
            );
        }
        let no_member = Submitted {
            mode: Some("add".into()),
            ..Default::default()
        };
        assert_eq!(
            action_for(RoomModal::Helpers, &no_member),
            Err(RoomRefusal::MissingChoice)
        );
        let no_mode = picked(TARGET);
        assert_eq!(
            action_for(RoomModal::Helpers, &no_mode),
            Err(RoomRefusal::MissingChoice)
        );
        let unknown_mode = Submitted {
            mode: Some("promote".into()),
            ..picked(TARGET)
        };
        assert_eq!(
            action_for(RoomModal::Helpers, &unknown_mode),
            Err(RoomRefusal::MissingChoice)
        );
    }

    #[test]
    fn action_for_bad_limit() {
        for raw in ["100", "abc", "-1", "", "255", "3.5"] {
            let s = Submitted {
                value: Some(raw.into()),
                ..Default::default()
            };
            assert_eq!(
                action_for(RoomModal::Limit, &s),
                Err(RoomRefusal::BadLimit),
                "{raw:?}"
            );
        }
        assert_eq!(
            action_for(RoomModal::Limit, &Submitted::default()),
            Err(RoomRefusal::BadLimit)
        );
        for (raw, n) in [("0", 0), ("99", 99)] {
            let s = Submitted {
                value: Some(raw.into()),
                ..Default::default()
            };
            assert_eq!(action_for(RoomModal::Limit, &s), Ok(RoomAction::Limit(n)));
        }
    }

    #[test]
    fn action_for_empty_reason_is_reason_required() {
        for reason in [Some(""), Some("   "), None] {
            let s = Submitted {
                reason: reason.map(Into::into),
                ..picked(TARGET)
            };
            assert_eq!(
                action_for(RoomModal::Kick, &s),
                Err(RoomRefusal::ReasonRequired),
                "{reason:?}"
            );
        }
        let long = kick_of(TARGET, &"x".repeat(201));
        assert_eq!(
            action_for(RoomModal::Kick, &long),
            Err(RoomRefusal::ReasonTooLong)
        );
        assert_eq!(
            action_for(RoomModal::Mute, &long),
            Err(RoomRefusal::ReasonTooLong)
        );
    }

    #[test]
    fn action_for_who_can_join_role_choices_need_a_role() {
        for mode in ["allow-role", "block-role", "unblock-role"] {
            let s = Submitted {
                mode: Some(mode.into()),
                ..picked(TARGET)
            };
            assert_eq!(
                action_for(RoomModal::WhoCanJoin, &s),
                Err(RoomRefusal::MissingChoice),
                "{mode}"
            );
        }
        let s = Submitted {
            mode: Some("allow-user".into()),
            role: Some(RoleId::new(18)),
            ..Default::default()
        };
        assert_eq!(
            action_for(RoomModal::WhoCanJoin, &s),
            Err(RoomRefusal::MissingChoice)
        );
        assert_eq!(
            action_for(RoomModal::WhoCanJoin, &Submitted::default()),
            Err(RoomRefusal::MissingChoice)
        );
    }

    #[test]
    fn action_for_rename_empty_is_bad_name() {
        for raw in ["", "   ", &"x".repeat(101)] {
            let s = Submitted {
                value: Some(raw.to_string()),
                ..Default::default()
            };
            assert_eq!(
                action_for(RoomModal::Rename, &s),
                Err(RoomRefusal::BadName),
                "{raw:?}"
            );
        }
        assert_eq!(
            action_for(RoomModal::Rename, &Submitted::default()),
            Err(RoomRefusal::BadName)
        );
    }

    // Submits.

    #[tokio::test]
    async fn kick_submit_acknowledges_kicks_and_confirms_privately() {
        let w = World::new(live());
        w.submit(RoomModal::Kick, kick_of(TARGET, "spamming"), HELPER)
            .await;
        assert_eq!(
            w.press.ops(),
            vec![PressOp::Acknowledge, followup("🚫 Kicked <@3>.")]
        );
        assert_eq!(
            w.discord.calls(),
            vec![
                Call::Move(TARGET.get(), None),
                Call::Put(
                    ROOM.get(),
                    ow(OverwriteTarget::Member { id: TARGET }, NONE, V | C)
                ),
            ]
        );
        assert_eq!(w.transport.ops(), vec![Op::Edit(ROOM.get(), PANEL.get())]);
        assert!(w.room().await.state.blocked.contains(&TARGET));
    }

    #[tokio::test]
    async fn a_bad_submit_is_refused_after_the_ack_and_sends_nothing() {
        let w = World::new(live());
        let s = Submitted {
            value: Some("abc".into()),
            ..Default::default()
        };
        w.submit(RoomModal::Limit, s, OWNER).await;
        assert_eq!(
            w.press.ops(),
            vec![PressOp::Acknowledge, followup(ROOM_BAD_LIMIT)]
        );
        w.sent_nothing();
    }

    /// Review Focus 4: the room went while the modal was open.
    #[tokio::test]
    async fn submit_after_room_deleted_refuses_and_sends_nothing() {
        let w = World::new(live());
        w.press(RoomButton::Kick, OWNER).await;
        assert_eq!(w.press.ops(), vec![PressOp::Modal("roomm:kick:500".into())]);
        w.data.rooms.remove(ROOM);
        w.submit(RoomModal::Kick, kick_of(TARGET, "spamming"), OWNER)
            .await;
        assert_eq!(
            w.press.ops()[1..],
            [PressOp::Acknowledge, followup(ROOM_NOT_IN_ROOM)]
        );
        w.sent_nothing();
    }

    /// Review Focus 4: the presser owned the room when the modal opened, and
    /// handed it over before submitting.
    #[tokio::test]
    async fn submit_after_transfer_rechecks_authority() {
        let w = World::new(live());
        w.press(RoomButton::Kick, OWNER).await;
        assert_eq!(w.press.ops(), vec![PressOp::Modal("roomm:kick:500".into())]);
        act(
            &w.cx(),
            &facts(),
            ROOM,
            OWNER,
            RoomAction::Transfer { target: TARGET },
            Instant::now(),
        )
        .await
        .expect("the transfer happens");
        let (calls, msgs) = (w.discord.calls().len(), w.transport.ops().len());

        w.submit(RoomModal::Kick, kick_of(HELPER, "spamming"), OWNER)
            .await;
        assert_eq!(
            w.press.ops()[1..],
            [PressOp::Acknowledge, followup(ROOM_NOT_ALLOWED)]
        );
        assert_eq!(w.discord.calls().len(), calls, "no channel calls");
        assert_eq!(w.transport.ops().len(), msgs, "no messages");
        assert!(w.room().await.state.blocked.is_empty());
    }
}
