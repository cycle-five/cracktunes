//! Every message a room sends: the panel, the guide, one-line replies and the
//! status report. [`render_card`] is the only place they get a look.
//!
//! The panel is the room's documentation, so it names every control in plain
//! words next to the slash command that does the same thing.
use super::ids::{ButtonId, RoomButton};
use super::preflight::{Finding, Where};
use super::settings::RoomSettings;
use crate::messaging::format::{clip, escape, DESCRIPTION_MAX, EMBED_TITLE_MAX, FIELD_MAX};
use crate::messaging::messages::*;
use crate::messaging::render::{RenderCx, Rendered};
use serenity::all::{
    ButtonStyle, ChannelId, CreateActionRow, CreateButton, CreateComponent, CreateEmbed,
    Permissions, RoleId, UserId,
};
use std::borrow::Cow;

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
    /// Blocking this role would lock the room's owner out (`@everyone`, or a
    /// role the owner holds).
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
    pub room: ChannelId,
    /// The room's current channel name, for the title.
    pub name: String,
    pub owner: UserId,
    pub helpers: Vec<UserId>,
    pub locked: bool,
    pub chat_locked: bool,
    /// 0 means no limit.
    pub limit: u8,
}

/// A one-line confirmation of something done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomDone {
    Kicked(UserId),
    LetIn(UserId),
    Muted(UserId),
    Unmuted(UserId),
    Locked,
    Unlocked,
    ChatLocked,
    ChatUnlocked,
    LimitSet(u8),
    Renamed(String),
    Transferred(UserId),
    HelperAdded(UserId),
    HelperRemoved(UserId),
    AllowedUser(UserId),
    AllowedRole(RoleId),
    RoleBlocked(RoleId),
    RoleUnblocked(RoleId),
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
        settings: RoomSettings,
        findings: Option<Vec<Finding>>,
        dashboard: Option<String>,
    },
    /// Rooms turned on or off.
    Switched {
        on: bool,
    },
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
        RoomCard::Guide => Rendered::embed(
            CreateEmbed::new()
                .title(ROOM_GUIDE_TITLE)
                .description(clip(&guide_text(), DESCRIPTION_MAX)),
        ),
        RoomCard::Done(done) => Rendered::text(done_text(done)),
        RoomCard::Refused(refusal) => Rendered::text(refusal_text(*refusal)),
        RoomCard::Status {
            settings,
            findings,
            dashboard,
        } => Rendered::embed(
            CreateEmbed::new()
                .title(ROOM_STATUS_TITLE)
                .description(clip(
                    &status_text(settings, findings.as_deref(), dashboard.as_deref()),
                    DESCRIPTION_MAX,
                )),
        ),
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

fn mention(user: UserId) -> String {
    format!("<@{user}>")
}

fn panel(view: &PanelView) -> Rendered {
    let helpers = if view.helpers.is_empty() {
        ROOM_NO_HELPERS.to_string()
    } else {
        clip(
            &view
                .helpers
                .iter()
                .map(|h| mention(*h))
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
    let embed = CreateEmbed::new()
        .title(clip(
            &format!("{ROOM_PANEL_TITLE_PREFIX} {}", view.name),
            EMBED_TITLE_MAX,
        ))
        .description(clip(&reference_text(), DESCRIPTION_MAX))
        .field(ROOM_FIELD_OWNER, mention(view.owner), true)
        .field(ROOM_FIELD_HELPERS, helpers, true)
        .field(ROOM_FIELD_ACCESS, access, true)
        .field(ROOM_FIELD_LIMIT, limit, true);
    Rendered::embed(embed).with_components(panel_rows(view))
}

fn panel_rows(view: &PanelView) -> Vec<CreateComponent<'static>> {
    let b = |kind, label: &'static str, style| {
        CreateButton::new(
            ButtonId {
                kind,
                room: view.room,
            }
            .custom_id(),
        )
        .label(label)
        .style(style)
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
    let row = |buttons: Vec<CreateButton<'static>>| {
        CreateComponent::ActionRow(CreateActionRow::Buttons(Cow::Owned(buttons)))
    };
    vec![
        row(vec![
            b(RoomButton::Kick, ROOM_BTN_KICK, danger),
            b(RoomButton::Mute, ROOM_BTN_MUTE, secondary),
            b(RoomButton::Unmute, ROOM_BTN_UNMUTE, secondary),
            b(RoomButton::LetIn, ROOM_BTN_LET_IN, secondary),
        ]),
        row(vec![
            lock,
            chat,
            b(RoomButton::Limit, ROOM_BTN_LIMIT, secondary),
            b(RoomButton::Rename, ROOM_BTN_RENAME, secondary),
        ]),
        row(vec![
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
        RoomDone::Kicked(u) => format!("🚫 {ROOM_DONE_KICKED} {}.", mention(*u)),
        RoomDone::LetIn(u) => format!("↩️ {ROOM_DONE_LET_IN} {}.", mention(*u)),
        RoomDone::Muted(u) => format!("🔇 {ROOM_DONE_MUTED} {}.", mention(*u)),
        RoomDone::Unmuted(u) => format!("🔊 {ROOM_DONE_UNMUTED} {}.", mention(*u)),
        RoomDone::Locked => ROOM_DONE_LOCKED.to_string(),
        RoomDone::Unlocked => ROOM_DONE_UNLOCKED.to_string(),
        RoomDone::ChatLocked => ROOM_DONE_CHAT_LOCKED.to_string(),
        RoomDone::ChatUnlocked => ROOM_DONE_CHAT_UNLOCKED.to_string(),
        RoomDone::LimitSet(0) => ROOM_DONE_LIMIT_CLEARED.to_string(),
        RoomDone::LimitSet(n) => format!("{ROOM_DONE_LIMIT_SET} {n}."),
        RoomDone::Renamed(name) => format!("{ROOM_DONE_RENAMED} **{}**.", escape(name)),
        RoomDone::Transferred(u) => format!("{ROOM_DONE_TRANSFERRED} {}.", mention(*u)),
        RoomDone::HelperAdded(u) => format!("{ROOM_DONE_HELPER_ADDED} {}", mention(*u)),
        RoomDone::HelperRemoved(u) => format!("{ROOM_DONE_HELPER_REMOVED} {}", mention(*u)),
        RoomDone::AllowedUser(u) => format!("{ROOM_DONE_ALLOWED_USER} {}", mention(*u)),
        RoomDone::AllowedRole(r) => format!("{ROOM_DONE_ALLOWED_ROLE} <@&{r}>"),
        RoomDone::RoleBlocked(r) => format!("{ROOM_DONE_ROLE_BLOCKED} <@&{r}>"),
        RoomDone::RoleUnblocked(r) => format!("{ROOM_DONE_ROLE_UNBLOCKED} <@&{r}>"),
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

fn perm_names(perms: Permissions, bold: bool) -> String {
    perms
        .get_permission_names()
        .iter()
        .map(|n| {
            if bold {
                format!("**{n}**")
            } else {
                (*n).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn where_text(at: Where, name_of: &impl Fn(ChannelId) -> String) -> String {
    match at {
        Where::Category(c) => format!("{ROOM_AT_CATEGORY} {}", name_of(c)),
        Where::Hub(h) => format!("{ROOM_AT_HUB} {}", name_of(h)),
        Where::Server => ROOM_AT_SERVER.to_string(),
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
    match finding {
        Finding::Missing { at, perms } => {
            format!(
                "{ROOM_FINDING_MISSING} {} {}",
                perm_names(*perms, bold),
                where_text(*at, &name_of)
            )
        },
        Finding::Optional { at, perms } => {
            format!(
                "{ROOM_FINDING_OPTIONAL} {} {}",
                perm_names(*perms, bold),
                where_text(*at, &name_of)
            )
        },
        Finding::LockMayFail { perms } => {
            format!("{ROOM_FINDING_LOCK} {}.", perm_names(*perms, bold))
        },
    }
}

fn status_text(
    settings: &RoomSettings,
    findings: Option<&[Finding]>,
    dashboard: Option<&str>,
) -> String {
    let mut lines = vec![if settings.enabled {
        ROOM_STATUS_ENABLED.to_string()
    } else {
        ROOM_STATUS_DISABLED.to_string()
    }];
    lines.push(match settings.hub {
        Some(hub) => format!("{ROOM_STATUS_HUB} <#{hub}>"),
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
        Some(list) => lines.extend(
            list.iter()
                .map(|f| finding_text(f, |c| format!("<#{c}>"), true)),
        ),
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
    use serenity::all::GuildId;

    const ROOM: ChannelId = ChannelId::new(555);

    fn view() -> PanelView {
        PanelView {
            room: ROOM,
            name: "Ann's room".into(),
            owner: UserId::new(7),
            helpers: vec![UserId::new(8), UserId::new(9)],
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
        r.components
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
        json(r.embed.as_ref().unwrap())["description"]
            .as_str()
            .unwrap()
            .to_owned()
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
                room: ChannelId::new(u64::MAX - 1),
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
        let embed = json(r.embed.as_ref().unwrap());
        assert_eq!(embed["title"], "🔊 @everyone");
        let owner = embed["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "Owner")
            .unwrap();
        assert_eq!(owner["value"], "<@7>");
        // `@everyone` is only ever inside the embed.
        assert!(!json(&r.components).to_string().contains("@everyone"));
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
        let fields = json(r.embed.as_ref().unwrap())["fields"].to_string();
        assert!(fields.contains("5 people") && fields.contains("<@8> <@9>"));
        assert!(fields.contains("🔒 Locked · 💬 Chat open"));
    }

    fn status(findings: Option<Vec<Finding>>) -> String {
        description(&render(&RoomCard::Status {
            settings: RoomSettings {
                hub: Some(ChannelId::new(10)),
                ..RoomSettings::off(GuildId::new(1))
            },
            findings,
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
    fn status_without_a_hub_says_so_instead_of_blaming_permissions() {
        let d = description(&render(&RoomCard::Status {
            settings: RoomSettings::off(GuildId::new(1)),
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
            assert!(!text.trim().is_empty() && out.embed.is_none(), "{r:?}");
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
        let missing = render(&RoomCard::Refused(MissingPermissions))
            .content
            .unwrap();
        assert!(missing.contains("category") && missing.contains("/rooms status"));
    }

    #[test]
    fn done_cards_and_the_guide_render() {
        let done = render(&RoomCard::Done(RoomDone::Kicked(UserId::new(7))));
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
}
