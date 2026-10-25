//! Every message a room sends: the panel, the guide, one-line replies and the
//! status report. [`render_card`] is the only place they get a look.
//!
//! The panel is the room's documentation, so it names every control in plain
//! words next to the slash command that does the same thing.
use super::ids::RoomButton;
use super::preflight::{Finding, Where};
use super::settings::RoomSettings;
use crate::messaging::format::{clip, escape, DESCRIPTION_MAX, EMBED_TITLE_MAX, FIELD_MAX};
use crate::messaging::messages::*;
use crate::messaging::render::{RenderCx, Rendered};
use crack_model::{Button, ButtonStyle, Card, Field, Mention, RoleRef, RoomRef, Row, UserRef};
use crack_types::discord::ToRef;
use serenity::all::{ChannelId, Permissions};

/// Why a room action was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomRefusal {
    /// The member is not in a voice room.
    NotInRoom,
    /// The member's standing in the room does not allow this action.
    NotAllowed,
    /// A helper tried something only the room's owner (or a moderator) may do.
    OwnerOnly,
    /// The target is protected: oneself, CrackTunes, a moderator, the owner
    /// (unless a moderator acts) or, for a helper, another helper.
    ProtectedTarget,
    /// The target is not in this room.
    TargetNotInRoom,
    /// The target is not blocked from this room.
    NotBlocked,
    /// Unmute was pressed, and nobody in the room is muted.
    NobodyMuted,
    /// Another action on the room held it longer than a modal can wait.
    Busy,
    /// The server's own permissions keep the member or role out of this room.
    CannotJoinHere,
    /// Renaming is rate limited; try again after the wait.
    RenameLimited { wait_secs: u32 },
    /// The member limit is out of range.
    BadLimit,
    /// The room name is empty or too long.
    BadName,
    /// A reason is required and was empty.
    ReasonRequired,
    /// The reason is longer than the maximum.
    ReasonTooLong,
    /// A modal was submitted without the member, role or mode its action needs.
    MissingChoice,
    /// The room already has its owner present.
    OwnerPresent,
    /// The member already owns this room.
    AlreadyOwner,
    /// The would-be owner already owns another room in this server.
    OwnsAnotherRoom,
    /// Blocking this role would lock out the room's owner (`@everyone`, or
    /// a role the owner holds) or CrackTunes (a role it holds).
    CannotBlockRole,
    /// Allowing `@everyone` in would leave a locked room locking nobody out.
    CannotAllowRole,
    /// There are too many options to fit in a select menu.
    TooManyForMenu,
    /// The bot lacks a Discord permission it needs.
    MissingPermissions,
    /// Discord rejected the call.
    DiscordFailed,
    /// Rooms need a database and none is configured.
    NoDatabase,
    /// Rooms are not enabled on this server.
    RoomsOff,
}

/// What the panel is drawn from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub room: RoomRef,
    /// The room's current channel name, for the title.
    pub name: String,
    pub owner: UserRef,
    pub helpers: Vec<UserRef>,
    pub locked: bool,
    pub chat_locked: bool,
    /// 0 means no limit.
    pub limit: u8,
}

/// A one-line confirmation of something done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomDone {
    Kicked(UserRef),
    LetIn(UserRef),
    Muted(UserRef),
    Unmuted(UserRef),
    Locked,
    Unlocked,
    ChatLocked,
    ChatUnlocked,
    LimitSet(u8),
    Renamed(String),
    Transferred(UserRef),
    HelperAdded(UserRef),
    HelperRemoved(UserRef),
    AllowedUser(UserRef),
    AllowedRole(RoleRef),
    RoleBlocked(RoleRef),
    RoleUnblocked(RoleRef),
    Claimed,
    PanelPosted,
}

/// Every message a room sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomCard {
    /// The control panel, with buttons.
    Panel(PanelView),
    /// The long private help.
    Guide,
    Done(RoomDone),
    Refused(RoomRefusal),
    /// `/rooms status`. `findings` is `None` when the cache could not answer.
    Status {
        settings: RoomStatusView,
        findings: Option<Vec<FindingView>>,
        dashboard: Option<String>,
    },
    /// Rooms turned on or off.
    Switched {
        on: bool,
    },
}

/// The settings `/rooms status` shows, with the hub as a ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomStatusView {
    pub enabled: bool,
    pub hub: Option<RoomRef>,
    pub name_template: String,
    /// 0 means no limit.
    pub default_limit: u8,
    pub empty_grace_secs: u16,
}

impl From<&RoomSettings> for RoomStatusView {
    fn from(s: &RoomSettings) -> Self {
        Self {
            enabled: s.enabled,
            hub: s.hub.map(|h| h.to_ref()),
            name_template: s.name_template.clone(),
            default_limit: s.default_limit,
            empty_grace_secs: s.empty_grace_secs,
        }
    }
}

/// Where a finding applies, with channels as refs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhereView {
    Category(RoomRef),
    Hub(RoomRef),
    Server,
}

impl From<Where> for WhereView {
    fn from(at: Where) -> Self {
        match at {
            Where::Category(c) => Self::Category(c.to_ref()),
            Where::Hub(h) => Self::Hub(h.to_ref()),
            Where::Server => Self::Server,
        }
    }
}

/// A preflight finding with its permissions as names, so the card holds no
/// serenity type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingView {
    Missing { at: WhereView, perms: Vec<String> },
    Optional { at: WhereView, perms: Vec<String> },
    LockMayFail { perms: Vec<String> },
}

impl From<&Finding> for FindingView {
    fn from(f: &Finding) -> Self {
        match f {
            Finding::Missing { at, perms } => Self::Missing {
                at: (*at).into(),
                perms: permission_names(*perms),
            },
            Finding::Optional { at, perms } => Self::Optional {
                at: (*at).into(),
                perms: permission_names(*perms),
            },
            Finding::LockMayFail { perms } => Self::LockMayFail {
                perms: permission_names(*perms),
            },
        }
    }
}

impl From<RoomCard> for crate::messaging::message::CrackedMessage {
    fn from(card: RoomCard) -> Self {
        Self::Room(Box::new(card))
    }
}

/// For logs: the kind of card.
impl std::fmt::Display for RoomCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Panel(_) => "panel",
            Self::Guide => "guide",
            Self::Done(_) => "done",
            Self::Refused(_) => "refused",
            Self::Status { .. } => "status",
            Self::Switched { .. } => "switched",
        };
        write!(f, "room: {kind}")
    }
}

/// What a room card looks like in Discord. Reached through `messaging::render`.
/// Nothing here pings: every card keeps `Mentions::None`.
#[must_use]
pub fn render_card(card: &RoomCard, _cx: &RenderCx) -> Rendered {
    match card {
        RoomCard::Panel(view) => panel(view),
        RoomCard::Guide => Rendered::card(Card {
            title: Some(ROOM_GUIDE_TITLE.to_string()),
            description: Some(clip(&guide_text(), DESCRIPTION_MAX)),
            ..Card::default()
        }),
        RoomCard::Done(done) => Rendered::text(done_text(done)),
        RoomCard::Refused(refusal) => Rendered::text(refusal_text(*refusal)),
        RoomCard::Status {
            settings,
            findings,
            dashboard,
        } => Rendered::card(Card {
            title: Some(ROOM_STATUS_TITLE.to_string()),
            description: Some(clip(
                &status_text(settings, findings.as_deref(), dashboard.as_deref()),
                DESCRIPTION_MAX,
            )),
            ..Card::default()
        }),
        RoomCard::Switched { on } => Rendered::text(if *on {
            ROOM_SWITCHED_ON
        } else {
            ROOM_SWITCHED_OFF
        }),
    }
}

/// Every control, one line each, then the two facts worth knowing.
pub(crate) fn reference_lines() -> [&'static str; 18] {
    [
        ROOM_REF_KICK,
        ROOM_REF_MUTE,
        ROOM_REF_UNMUTE,
        ROOM_REF_LET_IN,
        ROOM_REF_LOCK,
        ROOM_REF_CHAT_LOCK,
        ROOM_REF_LIMIT,
        ROOM_REF_RENAME,
        ROOM_REF_TRANSFER,
        ROOM_REF_HELPERS,
        ROOM_REF_WHO_CAN_JOIN,
        ROOM_REF_CLAIM,
        ROOM_REF_PANEL,
        ROOM_REF_HELP,
        "",
        ROOM_GUIDE_WHO,
        ROOM_REF_DELETED,
        ROOM_REF_NO_DEAFEN,
    ]
}

/// The reference block under its heading.
fn reference_text() -> String {
    format!(
        "**{ROOM_FIELD_REFERENCE}**\n{}",
        reference_lines().join("\n")
    )
}

fn guide_text() -> String {
    let mut out = format!("{ROOM_GUIDE_INTRO}\n{ROOM_GUIDE_RUN}\n\n");
    out.push_str(&reference_text());
    out
}

fn mention(user: &UserRef) -> String {
    Mention::User(user.clone()).to_string()
}

fn room_mention(room: &RoomRef) -> String {
    Mention::Room(room.clone()).to_string()
}

fn role_mention(role: &RoleRef) -> String {
    Mention::Role(role.clone()).to_string()
}

fn panel(view: &PanelView) -> Rendered {
    let helpers = if view.helpers.is_empty() {
        ROOM_NO_HELPERS.to_string()
    } else {
        clip(
            &view
                .helpers
                .iter()
                .map(mention)
                .collect::<Vec<_>>()
                .join(" "),
            FIELD_MAX,
        )
    };
    let access = format!(
        "{} · {}",
        if view.locked {
            ROOM_STATE_LOCKED
        } else {
            ROOM_STATE_OPEN
        },
        if view.chat_locked {
            ROOM_STATE_CHAT_LOCKED
        } else {
            ROOM_STATE_CHAT_OPEN
        },
    );
    let limit = if view.limit == 0 {
        ROOM_NO_LIMIT.to_string()
    } else {
        format!("{} {ROOM_LIMIT_PEOPLE}", view.limit)
    };
    let field = |name: &str, value: String| Field {
        name: name.to_string(),
        value,
        inline: true,
    };
    let embed = Card {
        title: Some(clip(
            &format!("{ROOM_PANEL_TITLE_PREFIX} {}", view.name),
            EMBED_TITLE_MAX,
        )),
        description: Some(clip(&reference_text(), DESCRIPTION_MAX)),
        fields: vec![
            field(ROOM_FIELD_OWNER, mention(&view.owner)),
            field(ROOM_FIELD_HELPERS, helpers),
            field(ROOM_FIELD_ACCESS, access),
            field(ROOM_FIELD_LIMIT, limit),
        ],
        ..Card::default()
    };
    Rendered::card(embed).with_rows(panel_rows(view))
}

fn panel_rows(view: &PanelView) -> Vec<Row> {
    let b = |kind: RoomButton, label: &'static str, style| Button {
        id: kind.custom_id_for(&view.room),
        label: Some(label.to_string()),
        emoji: None,
        style,
        disabled: false,
    };
    let (danger, primary, secondary) = (
        ButtonStyle::Danger,
        ButtonStyle::Primary,
        ButtonStyle::Secondary,
    );
    let lock = if view.locked {
        b(RoomButton::Unlock, ROOM_BTN_UNLOCK, secondary)
    } else {
        b(RoomButton::Lock, ROOM_BTN_LOCK, secondary)
    };
    let chat = if view.chat_locked {
        b(RoomButton::ChatUnlock, ROOM_BTN_CHAT_UNLOCK, secondary)
    } else {
        b(RoomButton::ChatLock, ROOM_BTN_CHAT_LOCK, secondary)
    };
    vec![
        Row::Buttons(vec![
            b(RoomButton::Kick, ROOM_BTN_KICK, danger),
            b(RoomButton::Mute, ROOM_BTN_MUTE, secondary),
            b(RoomButton::Unmute, ROOM_BTN_UNMUTE, secondary),
            b(RoomButton::LetIn, ROOM_BTN_LET_IN, secondary),
        ]),
        Row::Buttons(vec![
            lock,
            chat,
            b(RoomButton::Limit, ROOM_BTN_LIMIT, secondary),
            b(RoomButton::Rename, ROOM_BTN_RENAME, secondary),
        ]),
        Row::Buttons(vec![
            b(RoomButton::Transfer, ROOM_BTN_TRANSFER, secondary),
            b(RoomButton::Helpers, ROOM_BTN_HELPERS, secondary),
            b(RoomButton::WhoCanJoin, ROOM_BTN_WHO_CAN_JOIN, secondary),
            b(RoomButton::Claim, ROOM_BTN_CLAIM, secondary),
            b(RoomButton::Help, ROOM_BTN_HELP, primary),
        ]),
    ]
}

fn done_text(done: &RoomDone) -> String {
    match done {
        RoomDone::Kicked(u) => format!("🚫 {ROOM_DONE_KICKED} {}.", mention(u)),
        RoomDone::LetIn(u) => format!("↩️ {ROOM_DONE_LET_IN} {}.", mention(u)),
        RoomDone::Muted(u) => format!("🔇 {ROOM_DONE_MUTED} {}.", mention(u)),
        RoomDone::Unmuted(u) => format!("🔊 {ROOM_DONE_UNMUTED} {}.", mention(u)),
        RoomDone::Locked => ROOM_DONE_LOCKED.to_string(),
        RoomDone::Unlocked => ROOM_DONE_UNLOCKED.to_string(),
        RoomDone::ChatLocked => ROOM_DONE_CHAT_LOCKED.to_string(),
        RoomDone::ChatUnlocked => ROOM_DONE_CHAT_UNLOCKED.to_string(),
        RoomDone::LimitSet(0) => ROOM_DONE_LIMIT_CLEARED.to_string(),
        RoomDone::LimitSet(n) => format!("{ROOM_DONE_LIMIT_SET} {n}."),
        RoomDone::Renamed(name) => format!("{ROOM_DONE_RENAMED} **{}**.", escape(name)),
        RoomDone::Transferred(u) => format!("{ROOM_DONE_TRANSFERRED} {}.", mention(u)),
        RoomDone::HelperAdded(u) => format!("{ROOM_DONE_HELPER_ADDED} {}", mention(u)),
        RoomDone::HelperRemoved(u) => format!("{ROOM_DONE_HELPER_REMOVED} {}", mention(u)),
        RoomDone::AllowedUser(u) => format!("{ROOM_DONE_ALLOWED_USER} {}", mention(u)),
        RoomDone::AllowedRole(r) => format!("{ROOM_DONE_ALLOWED_ROLE} {}", role_mention(r)),
        RoomDone::RoleBlocked(r) => format!("{ROOM_DONE_ROLE_BLOCKED} {}", role_mention(r)),
        RoomDone::RoleUnblocked(r) => format!("{ROOM_DONE_ROLE_UNBLOCKED} {}", role_mention(r)),
        RoomDone::Claimed => ROOM_DONE_CLAIMED.to_string(),
        RoomDone::PanelPosted => ROOM_DONE_PANEL_POSTED.to_string(),
    }
}

fn refusal_text(refusal: RoomRefusal) -> String {
    match refusal {
        RoomRefusal::NotInRoom => ROOM_NOT_IN_ROOM.to_string(),
        RoomRefusal::NotAllowed => ROOM_NOT_ALLOWED.to_string(),
        RoomRefusal::OwnerOnly => ROOM_OWNER_ONLY.to_string(),
        RoomRefusal::ProtectedTarget => ROOM_PROTECTED_TARGET.to_string(),
        RoomRefusal::TargetNotInRoom => ROOM_TARGET_NOT_IN_ROOM.to_string(),
        RoomRefusal::NotBlocked => ROOM_NOT_BLOCKED.to_string(),
        RoomRefusal::NobodyMuted => ROOM_NOBODY_MUTED.to_string(),
        RoomRefusal::Busy => ROOM_BUSY.to_string(),
        RoomRefusal::CannotJoinHere => ROOM_CANNOT_JOIN_HERE.to_string(),
        RoomRefusal::RenameLimited { wait_secs } => {
            format!("{ROOM_RENAME_LIMITED} {wait_secs}s.")
        },
        RoomRefusal::BadLimit => ROOM_BAD_LIMIT.to_string(),
        RoomRefusal::BadName => ROOM_BAD_NAME.to_string(),
        RoomRefusal::ReasonRequired => ROOM_REASON_REQUIRED.to_string(),
        RoomRefusal::ReasonTooLong => ROOM_REASON_TOO_LONG.to_string(),
        RoomRefusal::MissingChoice => ROOM_MISSING_CHOICE.to_string(),
        RoomRefusal::OwnerPresent => ROOM_OWNER_PRESENT.to_string(),
        RoomRefusal::AlreadyOwner => ROOM_ALREADY_OWNER.to_string(),
        RoomRefusal::OwnsAnotherRoom => ROOM_OWNS_ANOTHER_ROOM.to_string(),
        RoomRefusal::CannotBlockRole => ROOM_CANNOT_BLOCK_ROLE.to_string(),
        RoomRefusal::CannotAllowRole => ROOM_CANNOT_ALLOW_ROLE.to_string(),
        RoomRefusal::TooManyForMenu => ROOM_TOO_MANY_FOR_MENU.to_string(),
        RoomRefusal::MissingPermissions => ROOM_MISSING_PERMISSIONS.to_string(),
        RoomRefusal::DiscordFailed => ROOM_DISCORD_FAILED.to_string(),
        RoomRefusal::NoDatabase => ROOM_NO_DATABASE.to_string(),
        RoomRefusal::RoomsOff => ROOM_ROOMS_OFF.to_string(),
    }
}

fn permission_names(perms: Permissions) -> Vec<String> {
    perms
        .get_permission_names()
        .iter()
        .map(|n| (*n).to_string())
        .collect()
}

fn names_text(names: &[String], bold: bool) -> String {
    names
        .iter()
        .map(|n| if bold { format!("**{n}**") } else { n.clone() })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn perm_names(perms: Permissions, bold: bool) -> String {
    names_text(&permission_names(perms), bold)
}

fn where_text(at: &WhereView, name_of: &impl Fn(&RoomRef) -> String) -> String {
    match at {
        WhereView::Category(c) => format!("{ROOM_AT_CATEGORY} {}", name_of(c)),
        WhereView::Hub(h) => format!("{ROOM_AT_HUB} {}", name_of(h)),
        WhereView::Server => ROOM_AT_SERVER.to_string(),
    }
}

/// One finding as a line of text. The one wording for the Discord Status card
/// (`name_of` makes a channel mention, `bold` on) and the dashboard (channel
/// names, plain text), so it lives here once.
pub fn finding_text(
    finding: &Finding,
    name_of: impl Fn(ChannelId) -> String,
    bold: bool,
) -> String {
    finding_line(
        &FindingView::from(finding),
        // A ref made from a `ChannelId` converts back, even a stored 0, which
        // the checked conversion refuses but serenity holds.
        &|r| channel_of(r).map_or_else(|| r.raw().as_str().to_owned(), &name_of),
        bold,
    )
}

/// The channel a room ref names, 0 included. serenity holds 0 but panics on
/// `u64::MAX`, which is refused here.
fn channel_of(r: &RoomRef) -> Option<ChannelId> {
    r.raw()
        .as_str()
        .parse::<u64>()
        .ok()
        .filter(|n| *n != u64::MAX)
        .map(ChannelId::new)
}

fn finding_line(
    finding: &FindingView,
    name_of: &impl Fn(&RoomRef) -> String,
    bold: bool,
) -> String {
    match finding {
        FindingView::Missing { at, perms } => {
            format!(
                "{ROOM_FINDING_MISSING} {} {}",
                names_text(perms, bold),
                where_text(at, name_of)
            )
        },
        FindingView::Optional { at, perms } => {
            format!(
                "{ROOM_FINDING_OPTIONAL} {} {}",
                names_text(perms, bold),
                where_text(at, name_of)
            )
        },
        FindingView::LockMayFail { perms } => {
            format!("{ROOM_FINDING_LOCK} {}.", names_text(perms, bold))
        },
    }
}

fn status_text(
    settings: &RoomStatusView,
    findings: Option<&[FindingView]>,
    dashboard: Option<&str>,
) -> String {
    let mut lines = vec![if settings.enabled {
        ROOM_STATUS_ENABLED.to_string()
    } else {
        ROOM_STATUS_DISABLED.to_string()
    }];
    lines.push(match &settings.hub {
        Some(hub) => format!("{ROOM_STATUS_HUB} {}", room_mention(hub)),
        None => ROOM_STATUS_NO_HUB.to_string(),
    });
    lines.push(format!(
        "{ROOM_STATUS_TEMPLATE} {}",
        escape(&settings.name_template)
    ));
    lines.push(if settings.default_limit == 0 {
        format!("{ROOM_STATUS_LIMIT} {ROOM_NO_LIMIT}")
    } else {
        format!("{ROOM_STATUS_LIMIT} {}", settings.default_limit)
    });
    lines.push(format!(
        "{ROOM_STATUS_GRACE} {}s",
        settings.empty_grace_secs
    ));
    lines.push(String::new());
    match findings {
        None if settings.hub.is_none() => lines.push(ROOM_STATUS_NO_HUB_YET.to_string()),
        None => lines.push(ROOM_STATUS_UNKNOWN.to_string()),
        Some([]) => lines.push(ROOM_STATUS_READY.to_string()),
        Some(list) => lines.extend(list.iter().map(|f| finding_line(f, &room_mention, true))),
    }
    if let Some(url) = dashboard {
        lines.push(String::new());
        lines.push(format!("{ROOM_STATUS_DASHBOARD} {url}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::render::Mentions;
    use crate::rooms::ids::ButtonId;
    use serenity::all::{GuildId, RoleId, UserId};

    const ROOM: ChannelId = ChannelId::new(555);

    fn view() -> PanelView {
        PanelView {
            room: ROOM.to_ref(),
            name: "Ann's room".into(),
            owner: UserId::new(7).to_ref(),
            helpers: vec![UserId::new(8).to_ref(), UserId::new(9).to_ref()],
            locked: false,
            chat_locked: false,
            limit: 0,
        }
    }

    fn render(card: &RoomCard) -> Rendered {
        render_card(card, &RenderCx::default())
    }

    fn json<T: serde::Serialize>(t: &T) -> serde_json::Value {
        serde_json::to_value(t).unwrap()
    }

    /// `(custom_id, label)` per button, per row.
    fn rows(r: &Rendered) -> Vec<Vec<(String, String)>> {
        crate::messaging::discord::action_rows(&r.rows)
            .iter()
            .map(|row| {
                json(row)["components"]
                    .as_array()
                    .expect("an action row")
                    .iter()
                    .map(|b| {
                        (
                            b["custom_id"].as_str().unwrap().to_owned(),
                            b["label"].as_str().expect("a label").to_owned(),
                        )
                    })
                    .collect()
            })
            .collect()
    }

    fn description(r: &Rendered) -> String {
        json(&crate::messaging::discord::first_embed(r).unwrap())["description"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Everything a render sends, as exact bytes.
    fn sent(r: &Rendered) -> String {
        format!(
            "{:?}|{}|{}|{}|{:?}",
            r.content,
            serde_json::to_string(&crate::messaging::discord::first_embed(r)).unwrap(),
            serde_json::to_string(&crate::messaging::discord::extra_embeds(r)).unwrap(),
            serde_json::to_string(&crate::messaging::discord::action_rows(&r.rows)).unwrap(),
            r.mentions
        )
    }

    fn pin_cases() -> Vec<(&'static str, RoomCard)> {
        let u = |n| UserId::new(n).to_ref();
        let findings = vec![
            Finding::Missing {
                at: Where::Category(ChannelId::new(20)),
                perms: Permissions::MOVE_MEMBERS | Permissions::CONNECT,
            },
            Finding::Missing {
                at: Where::Hub(ChannelId::new(10)),
                perms: Permissions::CONNECT,
            },
            Finding::Optional {
                at: Where::Server,
                perms: Permissions::MANAGE_CHANNELS,
            },
            Finding::LockMayFail {
                perms: Permissions::MANAGE_MESSAGES,
            },
        ];
        let status =
            |settings: RoomSettings, findings: Option<Vec<Finding>>, dashboard| RoomCard::Status {
                settings: (&settings).into(),
                findings: findings.map(|f| f.iter().map(Into::into).collect()),
                dashboard,
            };
        let on = RoomSettings {
            enabled: true,
            hub: Some(ChannelId::new(10)),
            name_template: "{name}'s *room*".into(),
            default_limit: 4,
            empty_grace_secs: 60,
            ..RoomSettings::off(GuildId::new(1))
        };
        vec![
            ("panel_open", RoomCard::Panel(view())),
            (
                "panel_shut",
                RoomCard::Panel(PanelView {
                    locked: true,
                    chat_locked: true,
                    limit: 5,
                    ..view()
                }),
            ),
            (
                "panel_lock_only",
                RoomCard::Panel(PanelView {
                    locked: true,
                    ..view()
                }),
            ),
            (
                "panel_chat_lock_only",
                RoomCard::Panel(PanelView {
                    chat_locked: true,
                    ..view()
                }),
            ),
            (
                "panel_no_helpers",
                RoomCard::Panel(PanelView {
                    helpers: vec![],
                    name: "@everyone".into(),
                    ..view()
                }),
            ),
            (
                "panel_many_helpers",
                RoomCard::Panel(PanelView {
                    helpers: (1..=80).map(|i| u(1_000_000_000_000_000 + i)).collect(),
                    name: "n".repeat(300),
                    ..view()
                }),
            ),
            ("guide", RoomCard::Guide),
            (
                "status_findings",
                status(
                    on.clone(),
                    Some(findings),
                    Some("https://dash.example/g/1".into()),
                ),
            ),
            ("status_ready", status(on.clone(), Some(vec![]), None)),
            ("status_unknown", status(on, None, None)),
            (
                "status_off",
                status(RoomSettings::off(GuildId::new(1)), None, None),
            ),
            ("done_kicked", RoomCard::Done(RoomDone::Kicked(u(7)))),
            ("done_let_in", RoomCard::Done(RoomDone::LetIn(u(7)))),
            ("done_muted", RoomCard::Done(RoomDone::Muted(u(7)))),
            ("done_unmuted", RoomCard::Done(RoomDone::Unmuted(u(7)))),
            ("done_locked", RoomCard::Done(RoomDone::Locked)),
            ("done_unlocked", RoomCard::Done(RoomDone::Unlocked)),
            ("done_chat_locked", RoomCard::Done(RoomDone::ChatLocked)),
            ("done_chat_unlocked", RoomCard::Done(RoomDone::ChatUnlocked)),
            ("done_limit_set", RoomCard::Done(RoomDone::LimitSet(6))),
            ("done_limit_cleared", RoomCard::Done(RoomDone::LimitSet(0))),
            (
                "done_renamed",
                RoomCard::Done(RoomDone::Renamed("@everyone *x*".into())),
            ),
            (
                "done_transferred",
                RoomCard::Done(RoomDone::Transferred(u(7))),
            ),
            (
                "done_helper_added",
                RoomCard::Done(RoomDone::HelperAdded(u(7))),
            ),
            (
                "done_helper_removed",
                RoomCard::Done(RoomDone::HelperRemoved(u(7))),
            ),
            (
                "done_allowed_user",
                RoomCard::Done(RoomDone::AllowedUser(u(7))),
            ),
            (
                "done_allowed_role",
                RoomCard::Done(RoomDone::AllowedRole(RoleId::new(9).to_ref())),
            ),
            (
                "done_role_blocked",
                RoomCard::Done(RoomDone::RoleBlocked(RoleId::new(9).to_ref())),
            ),
            (
                "done_role_unblocked",
                RoomCard::Done(RoomDone::RoleUnblocked(RoleId::new(9).to_ref())),
            ),
            ("done_claimed", RoomCard::Done(RoomDone::Claimed)),
            ("done_panel_posted", RoomCard::Done(RoomDone::PanelPosted)),
            (
                "refused",
                RoomCard::Refused(RoomRefusal::RenameLimited { wait_secs: 42 }),
            ),
            ("switched_on", RoomCard::Switched { on: true }),
            ("switched_off", RoomCard::Switched { on: false }),
        ]
    }

    #[test]
    fn room_cards_render_as_before() {
        let cases = pin_cases();
        assert_eq!(cases.len(), EXPECTED.len());
        for ((name, card), (expected_name, expected)) in cases.iter().zip(EXPECTED) {
            assert_eq!(name, expected_name);
            let actual = sent(&render(card));
            assert_eq!(&actual, expected, "{name}: {actual}");
        }
    }

    #[test]
    fn panel_has_three_rows_and_every_button_targets_this_room() {
        let r = render(&RoomCard::Panel(view()));
        let rows = rows(&r);
        assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [4, 4, 5]);
        for (id, label) in rows.iter().flatten() {
            let parsed = ButtonId::parse(id).unwrap_or_else(|| panic!("unparseable {id}"));
            assert_eq!(parsed.room, ROOM, "{id}");
            assert!(
                label.chars().any(|c| c.is_alphabetic()) && !label.starts_with(char::is_alphabetic),
                "emoji then a word: {label}"
            );
        }
        let kinds: Vec<_> = rows
            .iter()
            .flatten()
            .map(|(id, _)| ButtonId::parse(id).unwrap().kind)
            .collect();
        use RoomButton::*;
        assert_eq!(
            kinds,
            [
                Kick, Mute, Unmute, LetIn, Lock, ChatLock, Limit, Rename, Transfer, Helpers,
                WhoCanJoin, Claim, Help
            ]
        );
    }

    #[test]
    fn panel_buttons_fit_discords_limits_in_every_lock_state() {
        for (locked, chat_locked) in [(false, false), (true, false), (false, true), (true, true)] {
            let r = render(&RoomCard::Panel(PanelView {
                room: ChannelId::new(u64::MAX - 1).to_ref(),
                locked,
                chat_locked,
                ..view()
            }));
            for row in rows(&r) {
                assert!(row.len() <= 5, "row of {}", row.len());
                for (id, label) in row {
                    assert!(id.len() <= 100, "{id} is {} bytes", id.len());
                    assert!(label.chars().count() <= 80, "{label}");
                }
            }
        }
    }

    #[test]
    fn panel_draws_unlock_when_locked_and_lock_when_not() {
        let kinds = |v: PanelView| -> Vec<RoomButton> {
            rows(&render(&RoomCard::Panel(v)))
                .iter()
                .flatten()
                .map(|(id, _)| ButtonId::parse(id).unwrap().kind)
                .collect()
        };
        let open = kinds(view());
        assert!(open.contains(&RoomButton::Lock) && open.contains(&RoomButton::ChatLock));
        assert!(!open.contains(&RoomButton::Unlock) && !open.contains(&RoomButton::ChatUnlock));
        let shut = kinds(PanelView {
            locked: true,
            chat_locked: true,
            ..view()
        });
        assert!(shut.contains(&RoomButton::Unlock) && shut.contains(&RoomButton::ChatUnlock));
        assert!(!shut.contains(&RoomButton::Lock) && !shut.contains(&RoomButton::ChatLock));
        // Each half is drawn independently.
        let only_chat = kinds(PanelView {
            chat_locked: true,
            ..view()
        });
        assert!(
            only_chat.contains(&RoomButton::Lock) && only_chat.contains(&RoomButton::ChatUnlock)
        );
    }

    #[test]
    fn panel_mentions_nobody() {
        let r = render(&RoomCard::Panel(PanelView {
            name: "@everyone".into(),
            ..view()
        }));
        assert_eq!(r.mentions, Mentions::None);
        assert_eq!(r.content, None);
        let embed = json(&crate::messaging::discord::first_embed(&r).unwrap());
        assert_eq!(embed["title"], "🔊 @everyone");
        let owner = embed["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "Owner")
            .unwrap();
        assert_eq!(owner["value"], "<@7>");
        // `@everyone` is only ever inside the embed.
        assert!(!json(&crate::messaging::discord::action_rows(&r.rows))
            .to_string()
            .contains("@everyone"));
    }

    #[test]
    fn panel_lists_every_control_with_its_slash_command() {
        let d = description(&render(&RoomCard::Panel(view())));
        for cmd in [
            "/room kick",
            "/room mute",
            "/room unmute",
            "/room letin",
            "/room lock",
            "/room unlock",
            "/room chatlock",
            "/room chatunlock",
            "/room limit",
            "/room rename",
            "/room transfer",
            "/room helper add",
            "/room helper remove",
            "/room allow",
            "/room block",
            "/room unblock",
            "/room claim",
            "/room panel",
        ] {
            assert!(d.contains(cmd), "missing {cmd}");
        }
        assert!(d.contains("What the buttons do"));
        assert!(d.contains("moderators") && d.contains("claim it"));
        assert!(d.contains("This room is deleted when everyone leaves."));
        assert!(d.contains("Discord can't deafen someone in just one channel"));
        assert!(d.contains("disconnect someone and keep them out; asks for a reason."));
    }

    #[test]
    fn panel_shows_limit_helpers_and_state() {
        let r = render(&RoomCard::Panel(PanelView {
            limit: 5,
            locked: true,
            ..view()
        }));
        let fields =
            json(&crate::messaging::discord::first_embed(&r).unwrap())["fields"].to_string();
        assert!(fields.contains("5 people") && fields.contains("<@8> <@9>"));
        assert!(fields.contains("🔒 Locked · 💬 Chat open"));
    }

    fn status(findings: Option<Vec<Finding>>) -> String {
        description(&render(&RoomCard::Status {
            settings: (&RoomSettings {
                hub: Some(ChannelId::new(10)),
                ..RoomSettings::off(GuildId::new(1))
            })
                .into(),
            findings: findings.map(|f| f.iter().map(Into::into).collect()),
            dashboard: Some("https://dash.example/g/1".into()),
        }))
    }

    #[test]
    fn status_names_missing_permission_and_place() {
        let d = status(Some(vec![
            Finding::Missing {
                at: Where::Category(ChannelId::new(20)),
                perms: Permissions::MOVE_MEMBERS,
            },
            Finding::Missing {
                at: Where::Hub(ChannelId::new(10)),
                perms: Permissions::CONNECT,
            },
        ]));
        assert!(
            d.contains("Give CrackTunes **Move Members** on <#20>"),
            "{d}"
        );
        assert!(d.contains("**Connect** on the hub <#10>"), "{d}");
        assert!(d.contains("https://dash.example/g/1"));
        assert!(status(Some(vec![])).contains("CrackTunes has everything it needs."));
        assert!(status(None).contains(
            "Couldn't check permissions: CrackTunes may not be able to see the hub or its category — give it View Channel there — or it's still starting up."
        ));
    }

    #[test]
    fn finding_text_is_plain_with_channel_names_for_the_dashboard() {
        let name_of = |c: ChannelId| format!("chan-{c}");
        let missing = Finding::Missing {
            at: Where::Category(ChannelId::new(20)),
            perms: Permissions::MOVE_MEMBERS,
        };
        assert_eq!(
            finding_text(&missing, name_of, false),
            "Give CrackTunes Move Members on chan-20"
        );
        assert_eq!(
            finding_text(&missing, |c| format!("<#{c}>"), true),
            "Give CrackTunes **Move Members** on <#20>"
        );
        let lock = Finding::LockMayFail {
            perms: Permissions::MANAGE_MESSAGES,
        };
        assert_eq!(
            finding_text(&lock, name_of, false),
            format!("{ROOM_FINDING_LOCK} Manage Messages.")
        );
    }

    #[test]
    fn finding_text_does_not_panic_on_a_stored_zero_channel() {
        let zero = Finding::Missing {
            at: Where::Category(ChannelId::new(0)),
            perms: Permissions::MOVE_MEMBERS,
        };
        assert_eq!(
            finding_text(&zero, |c| format!("chan-{c}"), false),
            "Give CrackTunes Move Members on chan-0"
        );
    }

    #[test]
    fn channel_of_refuses_what_serenity_cannot_hold() {
        use crack_model::Platform;
        let room = |raw: &str| RoomRef::new(Platform::Discord, raw).unwrap();
        assert_eq!(channel_of(&room("0")), Some(ChannelId::new(0)));
        assert_eq!(channel_of(&room("7")), Some(ChannelId::new(7)));
        assert_eq!(channel_of(&room(&u64::MAX.to_string())), None);
        assert_eq!(channel_of(&room("abc")), None);
    }

    #[test]
    fn status_without_a_hub_says_so_instead_of_blaming_permissions() {
        let d = description(&render(&RoomCard::Status {
            settings: (&RoomSettings::off(GuildId::new(1))).into(),
            findings: None,
            dashboard: None,
        }));
        assert!(d.contains("No hub set yet — run /rooms setup."), "{d}");
        assert!(!d.contains("Couldn't check permissions"), "{d}");
    }

    #[test]
    fn every_refusal_renders_nonempty_text() {
        use RoomRefusal::*;
        let all = [
            NotInRoom,
            NotAllowed,
            OwnerOnly,
            ProtectedTarget,
            TargetNotInRoom,
            NotBlocked,
            NobodyMuted,
            Busy,
            CannotJoinHere,
            RenameLimited { wait_secs: 42 },
            BadLimit,
            BadName,
            ReasonRequired,
            ReasonTooLong,
            MissingChoice,
            OwnerPresent,
            AlreadyOwner,
            OwnsAnotherRoom,
            CannotBlockRole,
            CannotAllowRole,
            TooManyForMenu,
            MissingPermissions,
            DiscordFailed,
            NoDatabase,
            RoomsOff,
        ];
        let mut seen = std::collections::HashSet::new();
        for r in all {
            let out = render(&RoomCard::Refused(r));
            let text = out.content.expect("a text reply");
            assert!(!text.trim().is_empty() && out.cards.is_empty(), "{r:?}");
            assert_eq!(out.mentions, Mentions::None);
            assert!(seen.insert(text), "{r:?} repeats another refusal");
        }
        assert_eq!(
            render(&RoomCard::Refused(NotAllowed)).content.unwrap(),
            "Only this room's owner or helpers can do that."
        );
        assert!(render(&RoomCard::Refused(RenameLimited { wait_secs: 42 }))
            .content
            .unwrap()
            .contains("42s"));
        // Every protected target is named, helpers included (spec 1.4).
        let protected = render(&RoomCard::Refused(ProtectedTarget)).content.unwrap();
        for who in [
            "yourself",
            "owner",
            "another helper",
            "moderator",
            "CrackTunes",
        ] {
            assert!(protected.contains(who), "{who}: {protected}");
        }
        let block = render(&RoomCard::Refused(CannotBlockRole)).content.unwrap();
        assert!(
            block.contains("owner") && block.contains("CrackTunes"),
            "{block}"
        );
        let missing = render(&RoomCard::Refused(MissingPermissions))
            .content
            .unwrap();
        assert!(missing.contains("category") && missing.contains("/rooms status"));
    }

    #[test]
    fn done_cards_and_the_guide_render() {
        let done = render(&RoomCard::Done(RoomDone::Kicked(UserId::new(7).to_ref())));
        assert_eq!(done.content.as_deref(), Some("🚫 Kicked <@7>."));
        let named = render(&RoomCard::Done(RoomDone::Renamed("@everyone".into())));
        assert!(named.content.unwrap().contains("\\@everyone"));
        assert!(render(&RoomCard::Done(RoomDone::LimitSet(0)))
            .content
            .unwrap()
            .contains("removed"));
        let guide = description(&render(&RoomCard::Guide));
        assert!(guide.contains("/room kick"));
        assert!(
            guide.contains("claim") && guide.contains("moderators"),
            "{guide}"
        );
        assert!(render(&RoomCard::Switched { on: true }).content.is_some());
    }

    /// Captured at e9a778fb, before the cards went neutral.
    const EXPECTED: &[(&str, &str)] = &[
        (
            "panel_open",
            r##"None|{"title":"🔊 Ann's room","type":"rich","description":"**What the buttons do**\n**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.\n**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.\n**Unmute** · `/room unmute` — let a muted person speak again.\n**Let back in** · `/room letin` — lift a kick so the person can rejoin.\n**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.\n**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.\n**Limit** · `/room limit` — cap how many people fit (0 means no limit).\n**Rename** · `/room rename` — change the room's name.\n**Transfer** · `/room transfer` — hand the room to someone else in it.\n**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.\n**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.\n**Claim** · `/room claim` — take over a room whose owner has left.\n**Panel** · `/room panel` — post this panel again.\n**Help** — a private, longer guide.\n\nThe owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.\nThis room is deleted when everyone leaves.\nDeafen isn't available: Discord can't deafen someone in just one channel.","fields":[{"name":"Owner","value":"<@7>","inline":true},{"name":"Helpers","value":"<@8> <@9>","inline":true},{"name":"Locked / chat","value":"🔓 Open · 💬 Chat open","inline":true},{"name":"Limit","value":"No limit","inline":true}]}|[]|[{"type":1,"components":[{"style":4,"type":2,"custom_id":"room:kick:555","label":"🚫 Kick","disabled":false},{"style":2,"type":2,"custom_id":"room:mute:555","label":"🔇 Mute","disabled":false},{"style":2,"type":2,"custom_id":"room:unmute:555","label":"🔊 Unmute","disabled":false},{"style":2,"type":2,"custom_id":"room:let-in:555","label":"↩️ Let back in","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:lock:555","label":"🔒 Lock","disabled":false},{"style":2,"type":2,"custom_id":"room:chat-lock:555","label":"🤐 Lock chat","disabled":false},{"style":2,"type":2,"custom_id":"room:limit:555","label":"👥 Limit","disabled":false},{"style":2,"type":2,"custom_id":"room:rename:555","label":"✏️ Rename","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:transfer:555","label":"🔑 Transfer","disabled":false},{"style":2,"type":2,"custom_id":"room:helpers:555","label":"🛡️ Helpers","disabled":false},{"style":2,"type":2,"custom_id":"room:who:555","label":"🚪 Who can join","disabled":false},{"style":2,"type":2,"custom_id":"room:claim:555","label":"👑 Claim","disabled":false},{"style":1,"type":2,"custom_id":"room:help:555","label":"❓ Help","disabled":false}]}]|None"##,
        ),
        (
            "panel_shut",
            r##"None|{"title":"🔊 Ann's room","type":"rich","description":"**What the buttons do**\n**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.\n**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.\n**Unmute** · `/room unmute` — let a muted person speak again.\n**Let back in** · `/room letin` — lift a kick so the person can rejoin.\n**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.\n**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.\n**Limit** · `/room limit` — cap how many people fit (0 means no limit).\n**Rename** · `/room rename` — change the room's name.\n**Transfer** · `/room transfer` — hand the room to someone else in it.\n**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.\n**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.\n**Claim** · `/room claim` — take over a room whose owner has left.\n**Panel** · `/room panel` — post this panel again.\n**Help** — a private, longer guide.\n\nThe owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.\nThis room is deleted when everyone leaves.\nDeafen isn't available: Discord can't deafen someone in just one channel.","fields":[{"name":"Owner","value":"<@7>","inline":true},{"name":"Helpers","value":"<@8> <@9>","inline":true},{"name":"Locked / chat","value":"🔒 Locked · 🤐 Chat locked","inline":true},{"name":"Limit","value":"5 people","inline":true}]}|[]|[{"type":1,"components":[{"style":4,"type":2,"custom_id":"room:kick:555","label":"🚫 Kick","disabled":false},{"style":2,"type":2,"custom_id":"room:mute:555","label":"🔇 Mute","disabled":false},{"style":2,"type":2,"custom_id":"room:unmute:555","label":"🔊 Unmute","disabled":false},{"style":2,"type":2,"custom_id":"room:let-in:555","label":"↩️ Let back in","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:unlock:555","label":"🔓 Unlock","disabled":false},{"style":2,"type":2,"custom_id":"room:chat-unlock:555","label":"💬 Unlock chat","disabled":false},{"style":2,"type":2,"custom_id":"room:limit:555","label":"👥 Limit","disabled":false},{"style":2,"type":2,"custom_id":"room:rename:555","label":"✏️ Rename","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:transfer:555","label":"🔑 Transfer","disabled":false},{"style":2,"type":2,"custom_id":"room:helpers:555","label":"🛡️ Helpers","disabled":false},{"style":2,"type":2,"custom_id":"room:who:555","label":"🚪 Who can join","disabled":false},{"style":2,"type":2,"custom_id":"room:claim:555","label":"👑 Claim","disabled":false},{"style":1,"type":2,"custom_id":"room:help:555","label":"❓ Help","disabled":false}]}]|None"##,
        ),
        (
            "panel_lock_only",
            r##"None|{"title":"🔊 Ann's room","type":"rich","description":"**What the buttons do**\n**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.\n**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.\n**Unmute** · `/room unmute` — let a muted person speak again.\n**Let back in** · `/room letin` — lift a kick so the person can rejoin.\n**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.\n**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.\n**Limit** · `/room limit` — cap how many people fit (0 means no limit).\n**Rename** · `/room rename` — change the room's name.\n**Transfer** · `/room transfer` — hand the room to someone else in it.\n**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.\n**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.\n**Claim** · `/room claim` — take over a room whose owner has left.\n**Panel** · `/room panel` — post this panel again.\n**Help** — a private, longer guide.\n\nThe owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.\nThis room is deleted when everyone leaves.\nDeafen isn't available: Discord can't deafen someone in just one channel.","fields":[{"name":"Owner","value":"<@7>","inline":true},{"name":"Helpers","value":"<@8> <@9>","inline":true},{"name":"Locked / chat","value":"🔒 Locked · 💬 Chat open","inline":true},{"name":"Limit","value":"No limit","inline":true}]}|[]|[{"type":1,"components":[{"style":4,"type":2,"custom_id":"room:kick:555","label":"🚫 Kick","disabled":false},{"style":2,"type":2,"custom_id":"room:mute:555","label":"🔇 Mute","disabled":false},{"style":2,"type":2,"custom_id":"room:unmute:555","label":"🔊 Unmute","disabled":false},{"style":2,"type":2,"custom_id":"room:let-in:555","label":"↩️ Let back in","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:unlock:555","label":"🔓 Unlock","disabled":false},{"style":2,"type":2,"custom_id":"room:chat-lock:555","label":"🤐 Lock chat","disabled":false},{"style":2,"type":2,"custom_id":"room:limit:555","label":"👥 Limit","disabled":false},{"style":2,"type":2,"custom_id":"room:rename:555","label":"✏️ Rename","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:transfer:555","label":"🔑 Transfer","disabled":false},{"style":2,"type":2,"custom_id":"room:helpers:555","label":"🛡️ Helpers","disabled":false},{"style":2,"type":2,"custom_id":"room:who:555","label":"🚪 Who can join","disabled":false},{"style":2,"type":2,"custom_id":"room:claim:555","label":"👑 Claim","disabled":false},{"style":1,"type":2,"custom_id":"room:help:555","label":"❓ Help","disabled":false}]}]|None"##,
        ),
        (
            "panel_chat_lock_only",
            r##"None|{"title":"🔊 Ann's room","type":"rich","description":"**What the buttons do**\n**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.\n**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.\n**Unmute** · `/room unmute` — let a muted person speak again.\n**Let back in** · `/room letin` — lift a kick so the person can rejoin.\n**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.\n**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.\n**Limit** · `/room limit` — cap how many people fit (0 means no limit).\n**Rename** · `/room rename` — change the room's name.\n**Transfer** · `/room transfer` — hand the room to someone else in it.\n**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.\n**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.\n**Claim** · `/room claim` — take over a room whose owner has left.\n**Panel** · `/room panel` — post this panel again.\n**Help** — a private, longer guide.\n\nThe owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.\nThis room is deleted when everyone leaves.\nDeafen isn't available: Discord can't deafen someone in just one channel.","fields":[{"name":"Owner","value":"<@7>","inline":true},{"name":"Helpers","value":"<@8> <@9>","inline":true},{"name":"Locked / chat","value":"🔓 Open · 🤐 Chat locked","inline":true},{"name":"Limit","value":"No limit","inline":true}]}|[]|[{"type":1,"components":[{"style":4,"type":2,"custom_id":"room:kick:555","label":"🚫 Kick","disabled":false},{"style":2,"type":2,"custom_id":"room:mute:555","label":"🔇 Mute","disabled":false},{"style":2,"type":2,"custom_id":"room:unmute:555","label":"🔊 Unmute","disabled":false},{"style":2,"type":2,"custom_id":"room:let-in:555","label":"↩️ Let back in","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:lock:555","label":"🔒 Lock","disabled":false},{"style":2,"type":2,"custom_id":"room:chat-unlock:555","label":"💬 Unlock chat","disabled":false},{"style":2,"type":2,"custom_id":"room:limit:555","label":"👥 Limit","disabled":false},{"style":2,"type":2,"custom_id":"room:rename:555","label":"✏️ Rename","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:transfer:555","label":"🔑 Transfer","disabled":false},{"style":2,"type":2,"custom_id":"room:helpers:555","label":"🛡️ Helpers","disabled":false},{"style":2,"type":2,"custom_id":"room:who:555","label":"🚪 Who can join","disabled":false},{"style":2,"type":2,"custom_id":"room:claim:555","label":"👑 Claim","disabled":false},{"style":1,"type":2,"custom_id":"room:help:555","label":"❓ Help","disabled":false}]}]|None"##,
        ),
        (
            "panel_no_helpers",
            r##"None|{"title":"🔊 @everyone","type":"rich","description":"**What the buttons do**\n**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.\n**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.\n**Unmute** · `/room unmute` — let a muted person speak again.\n**Let back in** · `/room letin` — lift a kick so the person can rejoin.\n**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.\n**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.\n**Limit** · `/room limit` — cap how many people fit (0 means no limit).\n**Rename** · `/room rename` — change the room's name.\n**Transfer** · `/room transfer` — hand the room to someone else in it.\n**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.\n**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.\n**Claim** · `/room claim` — take over a room whose owner has left.\n**Panel** · `/room panel` — post this panel again.\n**Help** — a private, longer guide.\n\nThe owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.\nThis room is deleted when everyone leaves.\nDeafen isn't available: Discord can't deafen someone in just one channel.","fields":[{"name":"Owner","value":"<@7>","inline":true},{"name":"Helpers","value":"None","inline":true},{"name":"Locked / chat","value":"🔓 Open · 💬 Chat open","inline":true},{"name":"Limit","value":"No limit","inline":true}]}|[]|[{"type":1,"components":[{"style":4,"type":2,"custom_id":"room:kick:555","label":"🚫 Kick","disabled":false},{"style":2,"type":2,"custom_id":"room:mute:555","label":"🔇 Mute","disabled":false},{"style":2,"type":2,"custom_id":"room:unmute:555","label":"🔊 Unmute","disabled":false},{"style":2,"type":2,"custom_id":"room:let-in:555","label":"↩️ Let back in","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:lock:555","label":"🔒 Lock","disabled":false},{"style":2,"type":2,"custom_id":"room:chat-lock:555","label":"🤐 Lock chat","disabled":false},{"style":2,"type":2,"custom_id":"room:limit:555","label":"👥 Limit","disabled":false},{"style":2,"type":2,"custom_id":"room:rename:555","label":"✏️ Rename","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:transfer:555","label":"🔑 Transfer","disabled":false},{"style":2,"type":2,"custom_id":"room:helpers:555","label":"🛡️ Helpers","disabled":false},{"style":2,"type":2,"custom_id":"room:who:555","label":"🚪 Who can join","disabled":false},{"style":2,"type":2,"custom_id":"room:claim:555","label":"👑 Claim","disabled":false},{"style":1,"type":2,"custom_id":"room:help:555","label":"❓ Help","disabled":false}]}]|None"##,
        ),
        (
            "panel_many_helpers",
            r##"None|{"title":"🔊 nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn…","type":"rich","description":"**What the buttons do**\n**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.\n**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.\n**Unmute** · `/room unmute` — let a muted person speak again.\n**Let back in** · `/room letin` — lift a kick so the person can rejoin.\n**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.\n**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.\n**Limit** · `/room limit` — cap how many people fit (0 means no limit).\n**Rename** · `/room rename` — change the room's name.\n**Transfer** · `/room transfer` — hand the room to someone else in it.\n**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.\n**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.\n**Claim** · `/room claim` — take over a room whose owner has left.\n**Panel** · `/room panel` — post this panel again.\n**Help** — a private, longer guide.\n\nThe owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.\nThis room is deleted when everyone leaves.\nDeafen isn't available: Discord can't deafen someone in just one channel.","fields":[{"name":"Owner","value":"<@7>","inline":true},{"name":"Helpers","value":"<@1000000000000001> <@1000000000000002> <@1000000000000003> <@1000000000000004> <@1000000000000005> <@1000000000000006> <@1000000000000007> <@1000000000000008> <@1000000000000009> <@1000000000000010> <@1000000000000011> <@1000000000000012> <@1000000000000013> <@1000000000000014> <@1000000000000015> <@1000000000000016> <@1000000000000017> <@1000000000000018> <@1000000000000019> <@1000000000000020> <@1000000000000021> <@1000000000000022> <@1000000000000023> <@1000000000000024> <@1000000000000025> <@1000000000000026> <@1000000000000027> <@1000000000000028> <@1000000000000029> <@1000000000000030> <@1000000000000031> <@1000000000000032> <@1000000000000033> <@1000000000000034> <@1000000000000035> <@1000000000000036> <@1000000000000037> <@1000000000000038> <@1000000000000039> <@1000000000000040> <@1000000000000041> <@1000000000000042> <@1000000000000043> <@1000000000000044> <@1000000000000045> <@1000000000000046> <@1000000000000047> <@1000000000000048> <@1000000000000049> <@1000000000000050> <@1000000000000051> <@1…","inline":true},{"name":"Locked / chat","value":"🔓 Open · 💬 Chat open","inline":true},{"name":"Limit","value":"No limit","inline":true}]}|[]|[{"type":1,"components":[{"style":4,"type":2,"custom_id":"room:kick:555","label":"🚫 Kick","disabled":false},{"style":2,"type":2,"custom_id":"room:mute:555","label":"🔇 Mute","disabled":false},{"style":2,"type":2,"custom_id":"room:unmute:555","label":"🔊 Unmute","disabled":false},{"style":2,"type":2,"custom_id":"room:let-in:555","label":"↩️ Let back in","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:lock:555","label":"🔒 Lock","disabled":false},{"style":2,"type":2,"custom_id":"room:chat-lock:555","label":"🤐 Lock chat","disabled":false},{"style":2,"type":2,"custom_id":"room:limit:555","label":"👥 Limit","disabled":false},{"style":2,"type":2,"custom_id":"room:rename:555","label":"✏️ Rename","disabled":false}]},{"type":1,"components":[{"style":2,"type":2,"custom_id":"room:transfer:555","label":"🔑 Transfer","disabled":false},{"style":2,"type":2,"custom_id":"room:helpers:555","label":"🛡️ Helpers","disabled":false},{"style":2,"type":2,"custom_id":"room:who:555","label":"🚪 Who can join","disabled":false},{"style":2,"type":2,"custom_id":"room:claim:555","label":"👑 Claim","disabled":false},{"style":1,"type":2,"custom_id":"room:help:555","label":"❓ Help","disabled":false}]}]|None"##,
        ),
        (
            "guide",
            r##"None|{"title":"How rooms work","type":"rich","description":"Join the hub channel and CrackTunes makes you a voice room of your own. You own it, and you can share the controls with helpers. Every control is a button on the room's panel and a `/room` command; either works.\nRun `/room` commands from the room's voice channel or from its chat.\n\n**What the buttons do**\n**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.\n**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.\n**Unmute** · `/room unmute` — let a muted person speak again.\n**Let back in** · `/room letin` — lift a kick so the person can rejoin.\n**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.\n**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.\n**Limit** · `/room limit` — cap how many people fit (0 means no limit).\n**Rename** · `/room rename` — change the room's name.\n**Transfer** · `/room transfer` — hand the room to someone else in it.\n**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.\n**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.\n**Claim** · `/room claim` — take over a room whose owner has left.\n**Panel** · `/room panel` — post this panel again.\n**Help** — a private, longer guide.\n\nThe owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.\nThis room is deleted when everyone leaves.\nDeafen isn't available: Discord can't deafen someone in just one channel."}|[]|[]|None"##,
        ),
        (
            "status_findings",
            r##"None|{"title":"Voice rooms","type":"rich","description":"Rooms are **on**.\nHub: <#10>\nName: {name}'s \\*room\\*\nDefault limit: 4\nEmpty-room grace: 60s\n\nGive CrackTunes **Connect**, **Move Members** on <#20>\nGive CrackTunes **Connect** on the hub <#10>\nOptional: give CrackTunes **Manage Channels** server-wide\nLocking a room may fail: CrackTunes lacks **Manage Messages**.\n\nDashboard settings: https://dash.example/g/1"}|[]|[]|None"##,
        ),
        (
            "status_ready",
            r##"None|{"title":"Voice rooms","type":"rich","description":"Rooms are **on**.\nHub: <#10>\nName: {name}'s \\*room\\*\nDefault limit: 4\nEmpty-room grace: 60s\n\nCrackTunes has everything it needs."}|[]|[]|None"##,
        ),
        (
            "status_unknown",
            r##"None|{"title":"Voice rooms","type":"rich","description":"Rooms are **on**.\nHub: <#10>\nName: {name}'s \\*room\\*\nDefault limit: 4\nEmpty-room grace: 60s\n\nCouldn't check permissions: CrackTunes may not be able to see the hub or its category — give it View Channel there — or it's still starting up."}|[]|[]|None"##,
        ),
        (
            "status_off",
            r##"None|{"title":"Voice rooms","type":"rich","description":"Rooms are **off**.\nHub: not set\nName: {name}'s room\nDefault limit: No limit\nEmpty-room grace: 30s\n\nNo hub set yet — run /rooms setup."}|[]|[]|None"##,
        ),
        (
            "done_kicked",
            r##"Some("🚫 Kicked <@7>.")|null|[]|[]|None"##,
        ),
        (
            "done_let_in",
            r##"Some("↩\u{fe0f} Let back in <@7>.")|null|[]|[]|None"##,
        ),
        ("done_muted", r##"Some("🔇 Muted <@7>.")|null|[]|[]|None"##),
        (
            "done_unmuted",
            r##"Some("🔊 Unmuted <@7>.")|null|[]|[]|None"##,
        ),
        (
            "done_locked",
            r##"Some("🔒 Room locked. Nobody new can join.")|null|[]|[]|None"##,
        ),
        (
            "done_unlocked",
            r##"Some("🔓 Room unlocked.")|null|[]|[]|None"##,
        ),
        (
            "done_chat_locked",
            r##"Some("🤐 Chat locked.")|null|[]|[]|None"##,
        ),
        (
            "done_chat_unlocked",
            r##"Some("💬 Chat unlocked.")|null|[]|[]|None"##,
        ),
        (
            "done_limit_set",
            r##"Some("👥 Limit set to 6.")|null|[]|[]|None"##,
        ),
        (
            "done_limit_cleared",
            r##"Some("👥 Limit removed.")|null|[]|[]|None"##,
        ),
        (
            "done_renamed",
            r##"Some("✏\u{fe0f} Renamed to **\\@everyone \\*x\\***.")|null|[]|[]|None"##,
        ),
        (
            "done_transferred",
            r##"Some("🔑 The room now belongs to <@7>.")|null|[]|[]|None"##,
        ),
        (
            "done_helper_added",
            r##"Some("🛡\u{fe0f} Helper added: <@7>")|null|[]|[]|None"##,
        ),
        (
            "done_helper_removed",
            r##"Some("🛡\u{fe0f} Helper removed: <@7>")|null|[]|[]|None"##,
        ),
        (
            "done_allowed_user",
            r##"Some("🚪 Allowed in: <@7>")|null|[]|[]|None"##,
        ),
        (
            "done_allowed_role",
            r##"Some("🚪 Allowed in: <@&9>")|null|[]|[]|None"##,
        ),
        (
            "done_role_blocked",
            r##"Some("🚫 Kept out: <@&9>")|null|[]|[]|None"##,
        ),
        (
            "done_role_unblocked",
            r##"Some("🚪 No longer kept out: <@&9>")|null|[]|[]|None"##,
        ),
        (
            "done_claimed",
            r##"Some("👑 You now own this room.")|null|[]|[]|None"##,
        ),
        (
            "done_panel_posted",
            r##"Some("Panel posted.")|null|[]|[]|None"##,
        ),
        (
            "refused",
            r##"Some("Renaming is limited to twice every 10 minutes. Try again in 42s.")|null|[]|[]|None"##,
        ),
        (
            "switched_on",
            r##"Some("Voice rooms are on.")|null|[]|[]|None"##,
        ),
        (
            "switched_off",
            r##"Some("Voice rooms are off. Existing rooms stay until they empty.")|null|[]|[]|None"##,
        ),
    ];
}

impl crate::messaging::render::Payload for RoomCard {
    fn render(&self, cx: &RenderCx) -> Rendered {
        render_card(self, cx)
    }
}
