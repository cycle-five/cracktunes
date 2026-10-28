//! The buttons on a prank card: "Rename them back" and "Reveal".
//!
//! 🔑 Like the room and now-playing buttons, an id carries everything a press
//! needs (`econ:back:<action>`, `econ:reveal:<action>`; the modal Rename them
//! back opens is `econm:back:<action>`), so a card on an old message still
//! works after a restart. Only a landed rename or clown card carries them.
//! Every press is answered: a card from before the economy was switched off
//! gets the refusal the commands give.
use super::discord::EconDiscordError;
use super::ledger::{self, EconomyLogEvent, Unmasker};
use super::settings::ActionKind;
use super::shop::{buy_error_text, reveal, BuyOutcome};
use crate::commands::economy::shop::{
    revealed_reply_text, run_prank, unmasked_text, Answer, PrankEnv,
};
use crate::db::economy::{self as db, ActionRow};
use crate::messaging::messages::{
    ECONOMY_BACK_DESC, ECONOMY_BACK_LABEL, ECONOMY_BACK_MODAL_TITLE, ECONOMY_BACK_PLACEHOLDER,
    ECONOMY_BACK_REVEAL_FIRST, ECONOMY_BUTTON_NOT_YOURS, ECONOMY_BUTTON_RENAME_BACK,
    ECONOMY_BUTTON_REVEAL, ECONOMY_BUTTON_STALE, ECONOMY_CANT_TELL, ECONOMY_NO_DATABASE,
    ECONOMY_OFF, ECONOMY_PURCHASE_FAILED, ECONOMY_TARGET_LEFT,
};
use crate::messaging::render::Rendered;
use crate::messaging::transport::{
    DiscordModalPress, DiscordPress, DiscordTransport, Press, Transport,
};
use crate::{Data, Error};
use crack_model::{Button, ButtonStyle, Row};
use serenity::all::{
    ChannelId, ComponentInteraction, CreateInputText, CreateLabel, CreateModal,
    CreateModalComponent, GuildId, InputTextStyle, LabelComponent, ModalComponent,
    ModalInteraction, UserId,
};

/// Every economy button's custom id starts with this.
pub const BUTTON_PREFIX: &str = "econ:";
/// Every economy modal's custom id starts with this.
pub const MODAL_PREFIX: &str = "econm:";

/// The nickname input inside the Rename them back modal.
const NICK_FIELD: &str = "nick";
/// Discord's longest nickname.
const NICK_MAX: u16 = 32;

/// One button on a card, with the action it was drawn for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconButton {
    Back { action: i64 },
    Reveal { action: i64 },
}

/// A positive id written plainly: digits only, no sign, no leading zero.
fn action_id(raw: &str) -> Option<i64> {
    let id: i64 = raw.parse().ok()?;
    (id > 0 && id.to_string() == raw).then_some(id)
}

impl EconButton {
    pub fn custom_id(self) -> String {
        match self {
            Self::Back { action } => format!("{BUTTON_PREFIX}back:{action}"),
            Self::Reveal { action } => format!("{BUTTON_PREFIX}reveal:{action}"),
        }
    }
}

/// Read a button's custom id; anything that isn't one of ours is `None`.
pub fn parse_id(raw: &str) -> Option<EconButton> {
    let (kind, id) = raw.strip_prefix(BUTTON_PREFIX)?.split_once(':')?;
    let action = action_id(id)?;
    match kind {
        "back" => Some(EconButton::Back { action }),
        "reveal" => Some(EconButton::Reveal { action }),
        _ => None,
    }
}

/// The id of the modal Rename them back opens.
pub fn modal_id(action: i64) -> String {
    format!("{MODAL_PREFIX}back:{action}")
}

/// The action a modal id names.
pub fn parse_modal_id(raw: &str) -> Option<i64> {
    action_id(raw.strip_prefix(MODAL_PREFIX)?.strip_prefix("back:")?)
}

/// Whether a purchase's card carries the buttons: a landed rename or clown
/// only. A bounce's "buyer" is its own target, and a block changed nobody.
pub fn carries_buttons(kind: ActionKind, outcome: BuyOutcome) -> bool {
    outcome == BuyOutcome::Landed && matches!(kind, ActionKind::Rename | ActionKind::Clown)
}

/// The row of buttons under a card.
pub fn card_buttons(action: i64) -> Vec<Row> {
    let button = |b: EconButton, label: &'static str| Button {
        id: b.custom_id(),
        label: Some(label.to_owned()),
        emoji: None,
        style: ButtonStyle::Secondary,
        disabled: false,
    };
    vec![Row::Buttons(vec![
        button(EconButton::Back { action }, ECONOMY_BUTTON_RENAME_BACK),
        button(EconButton::Reveal { action }, ECONOMY_BUTTON_REVEAL),
    ])]
}

/// Why a press is refused before anything else is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackRefusal {
    /// The presser is not who the action was done to.
    NotTheTarget,
    /// Nobody knows who bought it yet.
    BuyerUnknown,
}

impl BackRefusal {
    pub fn text(self) -> &'static str {
        match self {
            Self::NotTheTarget => ECONOMY_BUTTON_NOT_YOURS,
            Self::BuyerUnknown => ECONOMY_BACK_REVEAL_FIRST,
        }
    }
}

/// Only the target of the action may press its buttons.
pub fn only_the_target(presser: UserId, target: Option<UserId>) -> Result<(), BackRefusal> {
    if target == Some(presser) {
        Ok(())
    } else {
        Err(BackRefusal::NotTheTarget)
    }
}

/// Rename them back, for the target of an action whose buyer is known: signed
/// or revealed. While the buyer is anonymous a revenge rename would unmask
/// them for the price of a rename.
pub fn rename_back_allowed(
    signed: bool,
    revealed: bool,
    presser: UserId,
    target: UserId,
) -> Result<(), BackRefusal> {
    only_the_target(presser, Some(target))?;
    if signed || revealed {
        Ok(())
    } else {
        Err(BackRefusal::BuyerUnknown)
    }
}

/// What a press on an old card hears when the economy cannot be used.
pub fn unusable(enabled: bool, has_database: bool) -> Option<&'static str> {
    if !enabled {
        Some(ECONOMY_OFF)
    } else if !has_database {
        Some(ECONOMY_NO_DATABASE)
    } else {
        None
    }
}

/// What a Rename them back submit hears when the original buyer can't be
/// read: gone from the server, or (anything else) not readable right now.
/// Either way nothing is charged.
pub fn member_fetch_refusal(err: &EconDiscordError) -> &'static str {
    match err {
        EconDiscordError::UnknownMember => ECONOMY_TARGET_LEFT,
        other => {
            tracing::warn!("economy: reading a rename-back target failed: {other:?}");
            ECONOMY_CANT_TELL
        },
    }
}

/// The modal Rename them back opens.
pub fn rename_back_modal(action: i64) -> CreateModal<'static> {
    let input = CreateInputText::new(InputTextStyle::Short, NICK_FIELD)
        .placeholder(ECONOMY_BACK_PLACEHOLDER)
        .min_length(1)
        .max_length(NICK_MAX)
        .required(true);
    CreateModal::new(modal_id(action), ECONOMY_BACK_MODAL_TITLE).components(vec![
        CreateModalComponent::Label(
            CreateLabel::input_text(ECONOMY_BACK_LABEL, input).description(ECONOMY_BACK_DESC),
        ),
    ])
}

/// The nickname typed into the modal, as submitted.
pub fn read_nick(components: &[ModalComponent]) -> Option<String> {
    components.iter().find_map(|c| match c {
        ModalComponent::Label(label) => match &label.component {
            LabelComponent::InputText(input) if &*input.custom_id == NICK_FIELD => {
                Some(input.value.to_string())
            },
            _ => None,
        },
        _ => None,
    })
}

/// A press's answers as the prank pipeline's: follow-ups to a deferred press.
struct PressAnswer<'a>(&'a dyn Press);

#[async_trait::async_trait]
impl Answer for PressAnswer<'_> {
    async fn private(&self, text: String) -> Result<(), Error> {
        Ok(self.0.followup(Rendered::text(text), true).await?)
    }
    async fn public(&self, out: Rendered) -> Result<(), Error> {
        Ok(self.0.followup(out, false).await?)
    }
}

/// Tell the presser something only they see, as the press's one response.
async fn refuse(press: &dyn Press, text: impl Into<String>) {
    if let Err(err) = press.respond(Rendered::text(text), true).await {
        tracing::warn!("economy: answering a button press failed: {err:?}");
    }
}

/// Tell the presser something only they see, after the press was deferred.
async fn refuse_after(press: &dyn Press, text: impl Into<String>) {
    if let Err(err) = press.followup(Rendered::text(text), true).await {
        tracing::warn!("economy: answering a button press failed: {err:?}");
    }
}

/// What the card's action must be for a press to go on, or the words for why
/// not: the economy is on, the action exists here and was done to the presser.
async fn pressed_action(
    data: &Data,
    guild: GuildId,
    presser: UserId,
    action: i64,
) -> Result<ActionRow, String> {
    if let Some(text) = unusable(data.economy.is_enabled(guild), data.database_pool.is_some()) {
        return Err(text.to_string());
    }
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(ECONOMY_NO_DATABASE.to_string());
    };
    let row = match db::action_by_id(pool, guild, action).await {
        Ok(Some(row)) => row,
        Ok(None) => return Err(ECONOMY_BUTTON_STALE.to_string()),
        Err(err) => {
            tracing::warn!("economy: reading action {action} for a button failed: {err}");
            return Err(ECONOMY_PURCHASE_FAILED.to_string());
        },
    };
    only_the_target(presser, row.target).map_err(|r| r.text().to_string())?;
    Ok(row)
}

/// A press of a card button.
pub async fn handle_press(data: &Data, ctx: &serenity::all::Context, i: &ComponentInteraction) {
    let press = DiscordPress {
        http: &ctx.http,
        interaction: i,
    };
    let transport = DiscordTransport::of(ctx);
    respond_press(
        data,
        &press,
        &transport,
        i.guild_id,
        i.user.id,
        &i.data.custom_id,
    )
    .await;
}

/// [`handle_press`] without Discord. Rename them back checks, then opens its
/// modal as the response; Reveal defers first, then buys.
pub(crate) async fn respond_press(
    data: &Data,
    press: &dyn Press,
    transport: &dyn Transport,
    guild: Option<GuildId>,
    presser: UserId,
    custom_id: &str,
) {
    let (Some(button), Some(guild)) = (parse_id(custom_id), guild) else {
        tracing::warn!("economy: press with an unusable id {custom_id:?} from {presser}");
        return refuse(press, ECONOMY_BUTTON_STALE).await;
    };
    match button {
        EconButton::Back { action } => {
            let row = match pressed_action(data, guild, presser, action).await {
                Ok(row) => row,
                Err(text) => return refuse(press, text).await,
            };
            if let Err(text) = back_allowed(&row, presser) {
                return refuse(press, text).await;
            }
            if let Err(err) = press.open_modal(rename_back_modal(action)).await {
                tracing::warn!("economy: opening the rename-back modal failed: {err:?}");
            }
        },
        EconButton::Reveal { action } => {
            // Deferred first: the purchase reads and writes the database.
            if let Err(err) = press.acknowledge().await {
                tracing::warn!("economy: acknowledging a reveal press failed: {err:?}");
            }
            if let Err(text) = pressed_action(data, guild, presser, action).await {
                return refuse_after(press, text).await;
            }
            let cfg = data.economy.settings(guild).config;
            match reveal(&data.economy_data(), guild, presser, Some(action)).await {
                Ok(done) => {
                    refuse_after(
                        press,
                        revealed_reply_text(
                            done.kind,
                            done.buyer,
                            done.balance,
                            &cfg.currency_name,
                            &cfg.currency_emoji,
                        ),
                    )
                    .await;
                    // The private reply names the buyer: a lost public post
                    // loses nothing.
                    let unmasked = Rendered::text(unmasked_text(presser, done.kind, done.buyer));
                    if let Err(err) = press.followup(unmasked, false).await {
                        tracing::warn!("economy: the public unmasking failed: {err:?}");
                    }
                    ledger::post(
                        &data.economy_data(),
                        transport,
                        guild,
                        EconomyLogEvent::Unmasked {
                            action: done.kind,
                            target: presser,
                            buyer: done.buyer,
                            by: Unmasker::Target,
                        },
                    )
                    .await;
                },
                Err(e) => refuse_after(press, buy_error_text(&e, &cfg.currency_name)).await,
            }
        },
    }
}

/// Rename them back is for the target of a rename or clown whose buyer is known.
fn back_allowed(row: &ActionRow, presser: UserId) -> Result<(), String> {
    if !matches!(row.kind, ActionKind::Rename | ActionKind::Clown) {
        return Err(ECONOMY_BUTTON_STALE.to_string());
    }
    let target = row.target.unwrap_or(presser);
    rename_back_allowed(row.signed, row.revealed_at.is_some(), presser, target)
        .map_err(|r| r.text().to_string())
}

/// The submit of the Rename them back modal.
pub async fn handle_submit(data: &Data, ctx: &serenity::all::Context, i: &ModalInteraction) {
    let press = DiscordModalPress {
        http: &ctx.http,
        interaction: i,
    };
    // Deferred first: buying reads the database and then the cache.
    if let Err(err) = press.acknowledge().await {
        tracing::warn!("economy: acknowledging a rename-back submit failed: {err:?}");
    }
    let submitter = i.user.id;
    let (Some(action), Some(guild)) = (parse_modal_id(&i.data.custom_id), i.guild_id) else {
        tracing::warn!(
            "economy: submit with an unusable id {:?} from {submitter}",
            i.data.custom_id
        );
        return refuse_after(&press, ECONOMY_BUTTON_STALE).await;
    };
    // The card may have changed since the modal opened: check it all again.
    let row = match pressed_action(data, guild, submitter, action).await {
        Ok(row) => row,
        Err(text) => return refuse_after(&press, text).await,
    };
    if let Err(text) = back_allowed(&row, submitter) {
        return refuse_after(&press, text).await;
    }
    let Some(original_buyer) = Some(row.buyer).filter(|b| *b != submitter) else {
        return refuse_after(&press, ECONOMY_BUTTON_STALE).await;
    };
    // Nothing resolved the original buyer for us: the cache first, then
    // Discord. Already deferred, so the fetch has time.
    let target = match guild.member(ctx, original_buyer).await {
        Ok(m) => m,
        Err(err) => return refuse_after(&press, member_fetch_refusal(&err.into())).await,
    };
    // The submitter as the interaction resolved them.
    let buyer = match i.member.as_deref() {
        Some(m) => m.clone(),
        None => match guild.member(ctx, submitter).await {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!("economy: reading rename-back submitter {submitter}: {err}");
                return refuse_after(&press, ECONOMY_CANT_TELL).await;
            },
        },
    };
    let env = PrankEnv {
        data,
        ctx,
        guild,
        buyer: &buyer,
        invoked_in: ChannelId::new(i.channel_id.get()),
        answer: &PressAnswer(&press),
    };
    // Bought like /shop rename: unsigned, no note, every check.
    let nick = read_nick(&i.data.components).unwrap_or_default();
    if let Err(err) = run_prank(
        &env,
        ActionKind::Rename,
        Some(&target),
        Some(nick),
        None,
        false,
    )
    .await
    {
        tracing::warn!("economy: rename-back by {submitter} failed to answer: {err:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::settings::EconomySettings;
    use crate::messaging::discord;
    use crate::messaging::test_support::{FakePress, FakeTransport, PressOp};
    use crate::DataInner;
    use serenity::all::ModalInteractionData;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);

    #[test]
    fn econ_ids_round_trip_and_reject_malformed() {
        for b in [
            EconButton::Back { action: 7 },
            EconButton::Reveal { action: i64::MAX },
        ] {
            assert_eq!(parse_id(&b.custom_id()), Some(b));
        }
        assert_eq!(
            parse_id("econ:back:42"),
            Some(EconButton::Back { action: 42 })
        );
        assert_eq!(
            parse_id("econ:reveal:42"),
            Some(EconButton::Reveal { action: 42 })
        );
        for bad in [
            "",
            "econ:",
            "econ:back",
            "econ:back:",
            "econ:back:0",
            "econ:back:-3",
            "econ:back:+3",
            "econ:back:007",
            "econ:back:3:4",
            "econ:back:x",
            "econ:back:9223372036854775808",
            "econ:other:3",
            "econm:back:3",
            "room:back:3",
            "back:3",
        ] {
            assert_eq!(parse_id(bad), None, "{bad:?}");
        }
        assert_eq!(parse_modal_id(&modal_id(9)), Some(9));
        for bad in [
            "",
            "econm:",
            "econm:back:",
            "econm:back:0",
            "econm:reveal:3",
            "econ:back:3",
            "econm:back:3x",
            "roomm:back:3",
        ] {
            assert_eq!(parse_modal_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn econ_prefixes_do_not_collide() {
        let prefixes = [
            BUTTON_PREFIX,
            MODAL_PREFIX,
            crate::rooms::ids::ROOM_PREFIX,
            crate::rooms::ids::ROOM_MODAL_PREFIX,
            crate::commands::music::gp::GP_CUSTOM_ID_PREFIX,
            crate::messaging::buttons::NP_PREFIX,
        ];
        for (i, a) in prefixes.iter().enumerate() {
            for (j, b) in prefixes.iter().enumerate() {
                assert!(i == j || !a.starts_with(b), "{a:?} starts with {b:?}");
            }
        }
    }

    #[test]
    fn rename_back_needs_a_known_buyer() {
        let (me, other) = (UserId::new(1), UserId::new(2));
        assert_eq!(
            rename_back_allowed(false, false, me, me),
            Err(BackRefusal::BuyerUnknown)
        );
        assert_eq!(rename_back_allowed(true, false, me, me), Ok(()));
        assert_eq!(rename_back_allowed(false, true, me, me), Ok(()));
        // Standing comes first: a stranger learns nothing about the buyer.
        assert_eq!(
            rename_back_allowed(false, false, other, me),
            Err(BackRefusal::NotTheTarget)
        );
        assert_eq!(BackRefusal::BuyerUnknown.text(), "Reveal who did it first.");
    }

    #[test]
    fn only_the_target_may_press() {
        let (me, other) = (UserId::new(1), UserId::new(2));
        assert_eq!(only_the_target(me, Some(me)), Ok(()));
        assert_eq!(
            only_the_target(other, Some(me)),
            Err(BackRefusal::NotTheTarget)
        );
        assert_eq!(only_the_target(me, None), Err(BackRefusal::NotTheTarget));
        assert_eq!(
            rename_back_allowed(true, true, other, me),
            Err(BackRefusal::NotTheTarget)
        );
    }

    #[test]
    fn only_a_landed_rename_or_clown_card_has_buttons() {
        use ActionKind::*;
        use BuyOutcome::*;
        assert!(carries_buttons(Rename, Landed));
        assert!(carries_buttons(Clown, Landed));
        assert!(!carries_buttons(Rename, Bounced));
        assert!(!carries_buttons(Rename, Blocked));
        assert!(!carries_buttons(Clown, Bounced));
        for k in [Lock, Drag, Mute, Deafen, Jail] {
            assert!(!carries_buttons(k, Landed), "{k:?}");
        }
    }

    #[test]
    fn the_card_row_renders_as_before() {
        assert_eq!(
            serde_json::to_string(&discord::action_rows(&card_buttons(12))).unwrap(),
            r#"[{"type":1,"components":[{"style":2,"type":2,"custom_id":"econ:back:12","label":"Rename them back","disabled":false},{"style":2,"type":2,"custom_id":"econ:reveal:12","label":"Reveal","disabled":false}]}]"#
        );
    }

    #[test]
    fn the_card_row_names_its_action() {
        let row = serde_json::to_value(discord::action_rows(&card_buttons(12))).unwrap();
        let buttons = &row[0]["components"];
        assert_eq!(buttons.as_array().map(Vec::len), Some(2));
        assert_eq!(buttons[0]["custom_id"], "econ:back:12");
        assert_eq!(buttons[0]["label"], "Rename them back");
        assert_eq!(buttons[1]["custom_id"], "econ:reveal:12");
        assert_eq!(buttons[1]["label"], "Reveal");
    }

    #[test]
    fn the_modal_asks_for_one_short_name() {
        let v = serde_json::to_value(rename_back_modal(5)).unwrap();
        assert_eq!(v["custom_id"], "econm:back:5");
        let input = &v["components"][0]["component"];
        assert_eq!(input["custom_id"], "nick");
        assert_eq!(input["max_length"], 32);
        assert_eq!(input["required"], true);
    }

    #[test]
    fn the_typed_nick_is_read_back() {
        let data: ModalInteractionData = serde_json::from_str(
            r#"{"custom_id": "econm:back:5", "components": [
                {"type": 18, "id": 1, "component":
                    {"type": 4, "id": 2, "custom_id": "nick", "value": " Bob "}}]}"#,
        )
        .unwrap();
        assert_eq!(read_nick(&data.components), Some(" Bob ".to_string()));
        let data: ModalInteractionData = serde_json::from_str(
            r#"{"custom_id": "econm:back:5", "components": [
                {"type": 18, "id": 1, "component":
                    {"type": 4, "id": 2, "custom_id": "other", "value": "x"}}]}"#,
        )
        .unwrap();
        assert_eq!(read_nick(&data.components), None);
    }

    #[test]
    fn a_rename_back_target_who_left_is_said_so() {
        assert_eq!(
            member_fetch_refusal(&EconDiscordError::UnknownMember),
            "They're not in this server any more."
        );
        for err in [
            EconDiscordError::MissingPermissions,
            EconDiscordError::Other("timeout".into()),
        ] {
            assert_eq!(
                member_fetch_refusal(&err),
                "CrackTunes can't see that member right now. Try again in a moment."
            );
        }
    }

    #[test]
    fn an_unusable_economy_has_words() {
        assert_eq!(
            unusable(false, true),
            Some("The economy is off in this server.")
        );
        assert_eq!(
            unusable(false, false),
            Some("The economy is off in this server.")
        );
        assert_eq!(unusable(true, false), Some("The economy needs a database."));
        assert_eq!(unusable(true, true), None);
    }

    fn data_with(enabled: bool) -> Data {
        let data = Data(Arc::new(DataInner::default()));
        data.economy.set_settings(EconomySettings {
            enabled,
            ..EconomySettings::off(G)
        });
        data
    }

    async fn press_on(data: &Data, id: &str) -> Vec<PressOp> {
        let press = FakePress::default();
        respond_press(
            data,
            &press,
            &FakeTransport::default(),
            Some(G),
            UserId::new(5),
            id,
        )
        .await;
        press.ops()
    }

    fn private(text: &str) -> PressOp {
        PressOp::Respond {
            ephemeral: true,
            text: text.to_string(),
        }
    }

    #[tokio::test]
    async fn a_press_after_the_economy_went_off_is_refused_privately() {
        for id in ["econ:back:3", "econ:reveal:3"] {
            let ops = press_on(&data_with(false), id).await;
            // Reveal defers first; its refusal is then a follow-up.
            let want = if id.contains("reveal") {
                vec![
                    PressOp::Acknowledge,
                    PressOp::Followup {
                        ephemeral: true,
                        text: "The economy is off in this server.".into(),
                    },
                ]
            } else {
                vec![private("The economy is off in this server.")]
            };
            assert_eq!(ops, want, "{id}");
        }
    }

    #[tokio::test]
    async fn a_press_without_a_database_is_refused_privately() {
        let ops = press_on(&data_with(true), "econ:back:3").await;
        assert_eq!(ops, vec![private("The economy needs a database.")]);
    }

    #[tokio::test]
    async fn an_unusable_id_is_still_answered() {
        let ops = press_on(&data_with(true), "econ:back:zero").await;
        assert_eq!(ops, vec![private("That card is out of date.")]);
        let press = FakePress::default();
        respond_press(
            &data_with(true),
            &press,
            &FakeTransport::default(),
            None,
            UserId::new(5),
            "econ:back:3",
        )
        .await;
        assert_eq!(press.ops(), vec![private("That card is out of date.")]);
    }

    // ---- against a database ----

    use crate::db::economy::{self as db_e, NewAction};
    use crate::economy::wallet::ActionPayload;
    use sqlx::PgPool;

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    const BUYER: UserId = UserId::new(10);
    const VICTIM: UserId = UserId::new(5);

    async fn world(pool: &PgPool) -> Data {
        let s = EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        };
        db_e::save_settings(pool, &s).await.unwrap();
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert!(crate::economy::load_settings(&data.economy_data(), G).await);
        data
    }

    async fn fund_in(pool: &PgPool, guild: GuildId, user: UserId, credits: i64) {
        db_e::credit_earnings(
            pool,
            guild,
            &[db_e::EarnRow {
                user,
                voice: credits,
                messages: 0,
            }],
        )
        .await
        .unwrap();
    }

    async fn fund(pool: &PgPool, user: UserId, credits: i64) {
        fund_in(pool, G, user, credits).await;
    }

    /// BUYER pranks VICTIM with a rename (landed and running).
    async fn rename(pool: &PgPool, guild: GuildId, signed: bool) -> i64 {
        fund_in(pool, guild, BUYER, 200).await;
        let charged = db_e::charge_for_action(
            pool,
            &NewAction {
                guild,
                kind: ActionKind::Rename,
                buyer: BUYER,
                target: Some(VICTIM),
                price: 120,
                note: None,
                signed,
                payload: &ActionPayload::Rename {
                    name: "Bob".into(),
                    restore: None,
                },
                expires_at: None,
            },
        )
        .await
        .unwrap();
        assert!(
            db_e::activate_action(pool, charged.action_id, ActionKind::Rename, None)
                .await
                .unwrap()
        );
        charged.action_id
    }

    async fn press_as(data: &Data, who: UserId, id: &str) -> (Vec<PressOp>, usize) {
        let press = FakePress::default();
        respond_press(data, &press, &FakeTransport::default(), Some(G), who, id).await;
        let modals = press.modals.lock().unwrap().len();
        (press.ops(), modals)
    }

    macro_rules! db_test {
        ($name:ident, |$pool:ident: PgPool| $body:expr) => {
            #[crack_test_macros::db_test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) {
                $body.await
            }
        };
    }

    db_test!(
        rename_back_on_an_anonymous_prank_says_reveal_first,
        |pool: PgPool| async {
            let data = world(&pool).await;
            let id = rename(&pool, G, false).await;
            let (ops, modals) = press_as(&data, VICTIM, &format!("econ:back:{id}")).await;
            assert_eq!(ops, vec![private("Reveal who did it first.")]);
            assert_eq!(modals, 0, "no form is opened while the buyer is hidden");
        }
    );

    db_test!(
        rename_back_opens_its_form_once_the_buyer_is_known,
        |pool: PgPool| async {
            let data = world(&pool).await;
            let signed = rename(&pool, G, true).await;
            let (ops, modals) = press_as(&data, VICTIM, &format!("econ:back:{signed}")).await;
            assert_eq!(ops, vec![PressOp::Modal(format!("econm:back:{signed}"))]);
            assert_eq!(modals, 1);
            // An unsigned one opens after a reveal.
            let hidden = rename(&pool, G, false).await;
            fund(&pool, VICTIM, 100).await;
            let (ops, _) = press_as(&data, VICTIM, &format!("econ:reveal:{hidden}")).await;
            assert_eq!(ops.len(), 3, "{ops:?}");
            let (ops, modals) = press_as(&data, VICTIM, &format!("econ:back:{hidden}")).await;
            assert_eq!(ops, vec![PressOp::Modal(format!("econm:back:{hidden}"))]);
            assert_eq!(modals, 1);
        }
    );

    db_test!(only_the_target_can_use_the_buttons, |pool: PgPool| async {
        let data = world(&pool).await;
        let id = rename(&pool, G, true).await;
        fund(&pool, BUYER, 500).await;
        let nope = "Only the person it was done to can use these buttons.";
        let (ops, modals) = press_as(&data, BUYER, &format!("econ:back:{id}")).await;
        assert_eq!((ops, modals), (vec![private(nope)], 0));
        // Reveal: deferred, refused, and nothing charged or unmasked.
        let before = db_e::balance(&pool, G, BUYER).await.unwrap();
        let (ops, _) = press_as(&data, BUYER, &format!("econ:reveal:{id}")).await;
        assert_eq!(
            ops,
            vec![
                PressOp::Acknowledge,
                PressOp::Followup {
                    ephemeral: true,
                    text: nope.into()
                }
            ]
        );
        assert_eq!(db_e::balance(&pool, G, BUYER).await.unwrap(), before);
    });

    db_test!(
        reveal_names_the_buyer_privately_and_posts_it_publicly,
        |pool: PgPool| async {
            let data = world(&pool).await;
            let id = rename(&pool, G, false).await;
            fund(&pool, VICTIM, 250).await;
            let (ops, _) = press_as(&data, VICTIM, &format!("econ:reveal:{id}")).await;
            assert_eq!(
                ops,
                vec![
                    PressOp::Acknowledge,
                    PressOp::Followup {
                        ephemeral: true,
                        text: "It was <@10> (rename). You have 150 🪙 credits.".into()
                    },
                    PressOp::Followup {
                        ephemeral: false,
                        text: "🔍 <@5> unmasked the rename: <@10>".into()
                    },
                ]
            );
            assert_eq!(db_e::balance(&pool, G, VICTIM).await.unwrap(), 150);
            // A second press is refused, and costs nothing.
            let (ops, _) = press_as(&data, VICTIM, &format!("econ:reveal:{id}")).await;
            assert_eq!(ops.len(), 2, "{ops:?}");
            assert_eq!(
                ops[1],
                PressOp::Followup {
                    ephemeral: true,
                    text: "You've already unmasked that one.".into()
                }
            );
            assert_eq!(db_e::balance(&pool, G, VICTIM).await.unwrap(), 150);
        }
    );

    db_test!(
        a_paid_reveal_is_told_to_the_economy_log_after_the_replies,
        |pool: PgPool| async {
            let data = world(&pool).await;
            let mut settings = data.economy.settings(G);
            settings.config.log_channel_id = Some(ChannelId::new(77));
            data.economy.set_settings(settings);
            let id = rename(&pool, G, false).await;
            fund(&pool, VICTIM, 250).await;
            let press = FakePress::default();
            let transport = FakeTransport::default();
            respond_press(
                &data,
                &press,
                &transport,
                Some(G),
                VICTIM,
                &format!("econ:reveal:{id}"),
            )
            .await;
            assert_eq!(press.ops().len(), 3);
            assert_eq!(
                transport.ops(),
                vec![crate::messaging::test_support::Op::Send(77)]
            );
            assert_eq!(
                transport.texts(),
                vec!["<@5> unmasked the rename: <@10>".to_string()]
            );
        }
    );

    db_test!(
        a_card_from_another_server_is_out_of_date,
        |pool: PgPool| async {
            let data = world(&pool).await;
            let elsewhere = rename(&pool, GuildId::new(2), true).await;
            let (ops, modals) = press_as(&data, VICTIM, &format!("econ:back:{elsewhere}")).await;
            assert_eq!(
                (ops, modals),
                (vec![private("That card is out of date.")], 0)
            );
            let (ops, _) = press_as(&data, VICTIM, "econ:back:99999").await;
            assert_eq!(ops, vec![private("That card is out of date.")]);
        }
    );

    db_test!(
        rename_back_is_only_for_renames_and_clowns,
        |pool: PgPool| async {
            let data = world(&pool).await;
            fund(&pool, BUYER, 200).await;
            let charged = db_e::charge_for_action(
                &pool,
                &NewAction {
                    guild: G,
                    kind: ActionKind::Mute,
                    buyer: BUYER,
                    target: Some(VICTIM),
                    price: 90,
                    note: None,
                    signed: true,
                    payload: &ActionPayload::Mute,
                    expires_at: None,
                },
            )
            .await
            .unwrap();
            let (ops, modals) =
                press_as(&data, VICTIM, &format!("econ:back:{}", charged.action_id)).await;
            assert_eq!(
                (ops, modals),
                (vec![private("That card is out of date.")], 0)
            );
        }
    );
}
