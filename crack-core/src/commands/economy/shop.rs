//! `/shop menu|immunity|rename|lock|clown|drag|mute|deafen|jail|mirror|cleanse|reveal`.
use super::admin::log;
use super::credits::{gate, live, say};
use crate::economy::cards::{card_channel, card_text};
use crate::economy::preflight;
use crate::economy::settings::{
    validate_nick, validate_note, ActionKind, EconomySettings, NickError, NoteError,
};
use crate::economy::shop::{
    ago_words, buy, buy_immunity, cleanse, preflight_refusal, refusal_text, reveal, reveal_choices,
    BuyError, BuyOutcome, BuyRequest,
};
use crate::economy::target::{gather_facts, voice_of, Subject};
use crate::messaging::courier;
use crate::messaging::messages::{
    ECONOMY_DOCS_URL, ECONOMY_IMMUNE_UNTIL, ECONOMY_NICK_EMPTY, ECONOMY_NICK_LINK,
    ECONOMY_NICK_MENTION, ECONOMY_NICK_TOO_LONG, ECONOMY_NOTE_EMPTY, ECONOMY_NOTE_LINK,
    ECONOMY_NOTE_MENTION, ECONOMY_NOTE_TOO_LONG, ECONOMY_NO_DATABASE, ECONOMY_PURCHASE_FAILED,
    ECONOMY_REVEAL_PREFIX, ECONOMY_REVEAL_UNMASKED, ECONOMY_REVEAL_WAS, ECONOMY_SHOP_BLOCKED,
    ECONOMY_SHOP_BOT_BUYER, ECONOMY_SHOP_BOUNCED, ECONOMY_SHOP_CLEANSED_PREFIX,
    ECONOMY_SHOP_CLEANSED_SUFFIX, ECONOMY_SHOP_DOCS_LABEL, ECONOMY_SHOP_DONE,
    ECONOMY_SHOP_IMMUNITY_NOTE, ECONOMY_SHOP_MIRROR_UP, ECONOMY_SHOP_NOT_SOLD, ECONOMY_SHOP_TITLE,
    ECONOMY_YOU_HAVE,
};
use crate::messaging::render::Rendered;
use crate::messaging::transport::DiscordTransport;
use crate::modlog::event::ModLogEvent;
use crate::{Context, Data, Error};
use chrono::{DateTime, Utc};
use poise::serenity_prelude::{AutocompleteChoice, CreateAutocompleteResponse};
use serenity::all::{ChannelId, GenericChannelId, User, UserId};
use std::borrow::Cow;

/// The shop's listing: every item that is on, with its price, and the docs link.
pub fn menu_text(settings: &EconomySettings) -> String {
    let cfg = &settings.config;
    let mut lines = vec![format!("**{ECONOMY_SHOP_TITLE}**")];
    for kind in ActionKind::ALL {
        let price = cfg.prices.get(*kind);
        if !price.on {
            continue;
        }
        let sold = match kind {
            ActionKind::Immunity => cfg.immunity_shop,
            // Jail needs a channel to send members to.
            ActionKind::Jail => cfg.jail_channel_id.is_some(),
            _ => true,
        };
        if sold {
            lines.push(format!(
                "{}: {} {} {}",
                kind.label(),
                price.price,
                cfg.currency_emoji,
                cfg.currency_name
            ));
        } else {
            lines.push(format!("{}: {ECONOMY_SHOP_NOT_SOLD}", kind.label()));
        }
        if *kind == ActionKind::Immunity {
            lines.push(ECONOMY_SHOP_IMMUNITY_NOTE.to_string());
        }
    }
    // Angle brackets keep Discord from unfurling the link into a preview.
    lines.push(format!("[{ECONOMY_SHOP_DOCS_LABEL}](<{ECONOMY_DOCS_URL}>)"));
    lines.join("\n")
}

/// The public card announcing a purchase.
pub fn immune_card_text(buyer: UserId, until: DateTime<Utc>) -> String {
    format!(
        "🛡️ <@{buyer}> {ECONOMY_IMMUNE_UNTIL} <t:{}:f>",
        until.timestamp()
    )
}

/// The private follow-up: what is left after buying.
pub fn balance_followup_text(balance: i64, currency: &str, emoji: &str) -> String {
    format!("{ECONOMY_YOU_HAVE} {balance} {emoji} {currency}. {ECONOMY_SHOP_IMMUNITY_NOTE}")
}

/// The words for a refused note.
pub fn note_error_text(e: NoteError) -> &'static str {
    match e {
        NoteError::Empty => ECONOMY_NOTE_EMPTY,
        NoteError::TooLong => ECONOMY_NOTE_TOO_LONG,
        NoteError::Mention => ECONOMY_NOTE_MENTION,
        NoteError::Link => ECONOMY_NOTE_LINK,
    }
}

/// The words for a refused nickname.
pub fn nick_error_text(e: NickError) -> &'static str {
    match e {
        NickError::Empty => ECONOMY_NICK_EMPTY,
        NickError::TooLong => ECONOMY_NICK_TOO_LONG,
        NickError::Mention => ECONOMY_NICK_MENTION,
        NickError::Link => ECONOMY_NICK_LINK,
    }
}

/// The buyer's private reply after a purchase: what happened, and what is left.
pub fn bought_reply_text(
    kind: ActionKind,
    outcome: BuyOutcome,
    balance: i64,
    currency: &str,
    emoji: &str,
) -> String {
    let lead = match (kind, outcome) {
        (ActionKind::Mirror, _) => ECONOMY_SHOP_MIRROR_UP,
        (_, BuyOutcome::Landed) => ECONOMY_SHOP_DONE,
        (_, BuyOutcome::Bounced) => ECONOMY_SHOP_BOUNCED,
        (_, BuyOutcome::Blocked) => ECONOMY_SHOP_BLOCKED,
    };
    format!("{lead} {ECONOMY_YOU_HAVE} {balance} {emoji} {currency}.")
}

/// The private reply after a cleanse.
pub fn cleansed_reply_text(ended: usize, balance: i64, currency: &str, emoji: &str) -> String {
    format!(
        "{ECONOMY_SHOP_CLEANSED_PREFIX} {ended} {ECONOMY_SHOP_CLEANSED_SUFFIX} {ECONOMY_YOU_HAVE} {balance} {emoji} {currency}."
    )
}

/// The public unmasking. Mentions here ping nobody (cards render with none).
pub fn unmasked_text(presser: UserId, kind: ActionKind, buyer: UserId) -> String {
    format!(
        "{ECONOMY_REVEAL_PREFIX} <@{presser}> {ECONOMY_REVEAL_UNMASKED} {}: <@{buyer}>",
        kind.label().to_lowercase()
    )
}

/// The private reply to a reveal. It names the buyer too, so a failed public
/// post does not lose what was paid for.
pub fn revealed_reply_text(
    kind: ActionKind,
    buyer: UserId,
    balance: i64,
    currency: &str,
    emoji: &str,
) -> String {
    format!(
        "{ECONOMY_REVEAL_WAS} <@{buyer}> ({}). {ECONOMY_YOU_HAVE} {balance} {emoji} {currency}.",
        kind.label().to_lowercase()
    )
}

/// The mod log entry for a purchase. Only an action that changed a member is
/// logged (spec 5.4): a prank a mirror blocked changed nobody, so it has none.
/// A bounce is logged against the buyer, naming whose mirror sent it back.
pub fn shop_event(
    kind: ActionKind,
    outcome: BuyOutcome,
    buyer: UserId,
    target: UserId,
    note: Option<String>,
    signed: bool,
    until: Option<DateTime<Utc>>,
) -> Option<ModLogEvent> {
    let (target, mirrored_from) = match outcome {
        BuyOutcome::Landed => (target, None),
        BuyOutcome::Bounced => (buyer, Some(target)),
        BuyOutcome::Blocked => return None,
    };
    Some(ModLogEvent::ShopAction {
        action: kind,
        buyer,
        target,
        note,
        signed,
        until,
        mirrored_from,
    })
}

/// How a prank is shown in the reveal picker: "Rename \u{b7} 5 minutes ago".
pub fn reveal_label(kind: ActionKind, now: DateTime<Utc>, bought_at: DateTime<Utc>) -> String {
    format!("{} \u{b7} {}", kind.label(), ago_words(now, bought_at))
}

/// The channels a purchase of `kind` needs the bot to reach, for preflight:
/// the target's voice and the buyer's for a drag, the jail channel for jail.
pub fn involved_channels(
    kind: ActionKind,
    target_voice: Option<ChannelId>,
    buyer_voice: Option<ChannelId>,
    jail: Option<ChannelId>,
) -> Vec<ChannelId> {
    match kind {
        ActionKind::Drag => target_voice.into_iter().chain(buyer_voice).collect(),
        ActionKind::Jail => jail.into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Reply to a purchase error privately.
#[cfg(not(tarpaulin_include))]
async fn say_buy_error(ctx: Context<'_>, err: BuyError, currency: &str) -> Result<(), Error> {
    match err {
        BuyError::Refused(r) => say(ctx, refusal_text(&r, currency), true).await,
        BuyError::NoDatabase => say(ctx, ECONOMY_NO_DATABASE, true).await,
        BuyError::Db | BuyError::Discord(_) | BuyError::Invalid => {
            say(ctx, ECONOMY_PURCHASE_FAILED, true).await
        },
    }
}

/// Buy a prank (or a mirror, with no target). Gate, validate, gather facts,
/// preflight, buy; then reply privately with the balance, post the card, and
/// log the purchase. A mirror posts nothing public and logs nothing.
#[cfg(not(tarpaulin_include))]
async fn prank(
    ctx: Context<'_>,
    kind: ActionKind,
    target: Option<User>,
    nick: Option<String>,
    note: Option<String>,
    signed: Option<bool>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let buyer = ctx.author();
    if buyer.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let note = match note.as_deref().map(validate_note).transpose() {
        Ok(n) => n,
        Err(e) => return say(ctx, note_error_text(e), true).await,
    };
    let nick = match nick.as_deref().map(validate_nick).transpose() {
        Ok(n) => n,
        Err(e) => return say(ctx, nick_error_text(e), true).await,
    };
    let data = ctx.data();
    let cfg = data.economy.settings(guild).config;
    let now = Utc::now();
    let cache = &ctx.serenity_context().cache;
    let buyer_voice = voice_of(cache, guild, buyer.id);
    let gather = |user: &User, is_self: bool| {
        let who = Subject {
            member: user.id,
            is_bot: user.bot(),
            is_self,
        };
        gather_facts(&data, cache, guild, who, kind, buyer_voice, now)
    };
    let buyer_facts = match gather(buyer, true).await {
        Ok(f) => f,
        Err(e) => return say_buy_error(ctx, e, &cfg.currency_name).await,
    };
    // A mirror protects the buyer: their own facts stand in for a target's.
    let facts = match &target {
        Some(t) => match gather(t, t.id == buyer.id).await {
            Ok(f) => f,
            Err(e) => return say_buy_error(ctx, e, &cfg.currency_name).await,
        },
        None => buyer_facts.clone(),
    };
    // Refuse before charging when the bot lacks what the action needs. An
    // action that is off is left to `buy`, which says so.
    if cfg.prices.get(kind).on {
        let channels = involved_channels(kind, facts.in_voice, buyer_voice, cfg.jail_channel_id);
        let perms = preflight::bot_perms(cache, guild, &channels);
        if let Err(r) = preflight_refusal(kind, perms.as_ref(), &channels) {
            return say(ctx, refusal_text(&r, &cfg.currency_name), true).await;
        }
    }
    let signed = signed.unwrap_or(false);
    let req = BuyRequest {
        guild,
        buyer: buyer.id,
        kind,
        target: target.as_ref().map(|t| t.id),
        nick: nick.clone(),
        note: note.clone(),
        signed,
        now,
    };
    let bought = match buy(
        &data,
        &live(ctx),
        facts.clone(),
        buyer_facts.clone(),
        buyer_voice,
        req,
    )
    .await
    {
        Ok(b) => b,
        Err(e) => return say_buy_error(ctx, e, &cfg.currency_name).await,
    };
    // Reply first: a slow card or log channel must not time the interaction out.
    let replied = say(
        ctx,
        bought_reply_text(
            kind,
            bought.outcome,
            bought.balance,
            &cfg.currency_name,
            &cfg.currency_emoji,
        ),
        true,
    )
    .await;
    let Some(target) = target else {
        return replied;
    };
    let channel = card_channel(
        kind,
        bought.outcome,
        facts.in_voice,
        buyer_voice,
        cfg.jail_channel_id,
        ChannelId::new(ctx.channel_id().get()),
        cfg.announce_channel_id,
    );
    let card = card_text(
        kind,
        target.id,
        nick.as_deref(),
        bought.until,
        note.as_deref(),
        signed.then_some(buyer.id),
        bought.outcome,
    );
    let transport = DiscordTransport::of(ctx.serenity_context());
    if let Err(err) = courier::post_message(
        &transport,
        GenericChannelId::new(channel.get()),
        &Rendered::text(card.clone()),
    )
    .await
    {
        // The destination may be a channel the bot cannot post in: a public
        // follow-up in the invoking channel needs no channel permissions.
        tracing::warn!(
            "economy: posting the {kind:?} card in {channel} failed, answering in the invoking channel: {err:?}"
        );
        if let Err(err) = say(ctx, card, false).await {
            tracing::warn!("economy: the {kind:?} card follow-up failed: {err:?}");
        }
    }
    if let Some(event) = shop_event(
        kind,
        bought.outcome,
        buyer.id,
        target.id,
        note,
        signed,
        bought.until,
    ) {
        log(ctx, guild, event).await;
    }
    replied
}

/// Change someone's nickname for a while.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn rename(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "Their new nickname"] name: String,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    prank(
        ctx,
        ActionKind::Rename,
        Some(member),
        Some(name),
        note,
        signed,
    )
    .await
}

/// Lock someone's nickname as it is.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn lock(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    prank(ctx, ActionKind::Lock, Some(member), None, note, signed).await
}

/// Add a clown to someone's name for a while.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn clown(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    prank(ctx, ActionKind::Clown, Some(member), None, note, signed).await
}

/// Move someone in voice to your channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn drag(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    prank(ctx, ActionKind::Drag, Some(member), None, note, signed).await
}

/// Server-mute someone in voice for a few minutes.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn mute(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    prank(ctx, ActionKind::Mute, Some(member), None, note, signed).await
}

/// Server-deafen someone in voice for a few minutes.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn deafen(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    prank(ctx, ActionKind::Deafen, Some(member), None, note, signed).await
}

/// Send someone in voice to the jail channel for a few minutes.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn jail(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    prank(ctx, ActionKind::Jail, Some(member), None, note, signed).await
}

/// Send the next prank aimed at you back at whoever bought it.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn mirror(ctx: Context<'_>) -> Result<(), Error> {
    prank(ctx, ActionKind::Mirror, None, None, None, None).await
}

/// What the shop sells and for how much.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Economy",
    slash_command,
    guild_only,
    subcommands(
        "menu",
        "immunity",
        "rename",
        "lock",
        "clown",
        "drag",
        "mute",
        "deafen",
        "jail",
        "mirror",
        "cleanse_command",
        "reveal_command"
    ),
    subcommand_required
)]
pub async fn shop(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// List what is for sale and the prices.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn menu(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    say(ctx, menu_text(&ctx.data().economy.settings(guild)), false).await
}

/// Buy immunity from pranks.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn immunity(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let buyer = ctx.author();
    if buyer.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let cfg = ctx.data().economy.settings(guild).config;
    match buy_immunity(&ctx.data(), guild, buyer.id, Utc::now()).await {
        Ok(bought) => {
            say(ctx, immune_card_text(buyer.id, bought.until), false).await?;
            say(
                ctx,
                balance_followup_text(bought.balance, &cfg.currency_name, &cfg.currency_emoji),
                true,
            )
            .await
        },
        Err(e) => say_buy_error(ctx, e, &cfg.currency_name).await,
    }
}

/// End every prank running on you now.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, rename = "cleanse")]
pub async fn cleanse_command(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let buyer = ctx.author();
    if buyer.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let cfg = ctx.data().economy.settings(guild).config;
    match cleanse(&ctx.data(), &live(ctx), guild, buyer.id, Utc::now()).await {
        Ok(done) => {
            let replied = say(
                ctx,
                cleansed_reply_text(
                    done.ended,
                    done.balance,
                    &cfg.currency_name,
                    &cfg.currency_emoji,
                ),
                true,
            )
            .await;
            log(
                ctx,
                guild,
                ModLogEvent::ShopCancelled {
                    by: buyer.id,
                    target: buyer.id,
                },
            )
            .await;
            replied
        },
        Err(e) => say_buy_error(ctx, e, &cfg.currency_name).await,
    }
}

/// The presser's last ten pranks they could unmask, as picker choices.
#[cfg(not(tarpaulin_include))]
async fn reveal_autocomplete<'a>(
    ctx: poise::ApplicationContext<'_, Data, Error>,
    _partial: &'a str,
) -> CreateAutocompleteResponse<'a> {
    let Some(guild) = ctx.interaction.guild_id else {
        return CreateAutocompleteResponse::new();
    };
    if !ctx.data().economy.is_enabled(guild) {
        return CreateAutocompleteResponse::new();
    }
    let now = Utc::now();
    let choices: Vec<AutocompleteChoice<'a>> =
        reveal_choices(&ctx.data(), guild, ctx.interaction.user.id, 10)
            .await
            .into_iter()
            .filter_map(|c| {
                Some(AutocompleteChoice::new(
                    Cow::Owned(reveal_label(c.row.kind, now, c.bought_at)),
                    u64::try_from(c.row.id).ok()?,
                ))
            })
            .collect();
    CreateAutocompleteResponse::new().set_choices(Cow::Owned(choices))
}

/// Unmask who bought an unsigned prank aimed at you.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, rename = "reveal")]
pub async fn reveal_command(
    ctx: Context<'_>,
    #[description = "Which one (default: the most recent)"]
    #[autocomplete = "reveal_autocomplete"]
    action: Option<i64>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let presser = ctx.author();
    if presser.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let cfg = ctx.data().economy.settings(guild).config;
    match reveal(&ctx.data(), guild, presser.id, action).await {
        Ok(done) => {
            let replied = say(
                ctx,
                revealed_reply_text(
                    done.kind,
                    done.buyer,
                    done.balance,
                    &cfg.currency_name,
                    &cfg.currency_emoji,
                ),
                true,
            )
            .await;
            // The private reply already names the buyer: a failed public post
            // loses nothing.
            if let Err(err) =
                say(ctx, unmasked_text(presser.id, done.kind, done.buyer), false).await
            {
                tracing::warn!("economy: the public unmasking failed: {err:?}");
            }
            replied
        },
        Err(e) => say_buy_error(ctx, e, &cfg.currency_name).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::GuildId;

    const DOCS: &str = "[How the economy works](<https://github.com/cycle-five/cracktunes/blob/master/docs/economy.md>)";

    #[test]
    fn menu_lists_on_items_with_prices() {
        let mut s = EconomySettings::off(GuildId::new(1));
        assert_eq!(
            menu_text(&s),
            format!(
                "**Shop**\nImmunity: 200 🪙 credits\nProtects you from pranks.\n\
                 Rename: 120 🪙 credits\nLock nickname: 60 🪙 credits\nClown: 30 🪙 credits\n\
                 Drag: 60 🪙 credits\nMute: 90 🪙 credits\nDeafen: 90 🪙 credits\n\
                 Jail: not sold here\nMirror: 150 🪙 credits\nCleanse: 80 🪙 credits\n\
                 Reveal: 100 🪙 credits\n{DOCS}"
            )
        );
        s.config.jail_channel_id = Some(serenity::all::ChannelId::new(9));
        assert!(menu_text(&s).contains("Jail: 150 🪙 credits"));
        let mut p = ActionKind::Immunity.default_price();
        p.on = false;
        s.config.prices.set(ActionKind::Immunity, p);
        assert!(
            !menu_text(&s).contains("Immunity"),
            "an item that is off is not listed"
        );
        let mut p = ActionKind::Clown.default_price();
        p.on = false;
        s.config.prices.set(ActionKind::Clown, p);
        assert!(!menu_text(&s).contains("Clown"));
        assert!(!menu_text(&s).contains("next update"));
    }

    #[test]
    fn purchase_card_and_followup_text() {
        let until = chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000, 0).unwrap();
        assert_eq!(
            immune_card_text(UserId::new(7), until),
            "🛡️ <@7> is immune until <t:1700000000:f>"
        );
        assert_eq!(
            balance_followup_text(40, "credits", "🪙"),
            "You have 40 🪙 credits. Protects you from pranks."
        );
    }

    #[test]
    fn menu_says_immunity_not_sold_when_the_switch_is_off() {
        let mut s = EconomySettings::off(GuildId::new(1));
        s.config.immunity_shop = false;
        let text = menu_text(&s);
        assert!(
            text.starts_with(
                "**Shop**\nImmunity: not sold here\nProtects you from pranks.\nRename: 120"
            ),
            "{text}"
        );
        assert!(!text.contains("next update"));
    }

    #[test]
    fn note_and_nick_errors_have_their_own_words() {
        let notes: std::collections::HashSet<_> = [
            NoteError::Empty,
            NoteError::TooLong,
            NoteError::Mention,
            NoteError::Link,
        ]
        .map(note_error_text)
        .into();
        assert_eq!(notes.len(), 4);
        let nicks: std::collections::HashSet<_> = [
            NickError::Empty,
            NickError::TooLong,
            NickError::Mention,
            NickError::Link,
        ]
        .map(nick_error_text)
        .into();
        assert_eq!(nicks.len(), 4);
        assert_eq!(
            nick_error_text(NickError::Mention),
            "The nickname can't mention anyone."
        );
    }

    #[test]
    fn reply_texts_say_what_happened_and_what_is_left() {
        let say = |k, o| bought_reply_text(k, o, 40, "credits", "🪙");
        assert_eq!(
            say(ActionKind::Rename, BuyOutcome::Landed),
            "Done. You have 40 🪙 credits."
        );
        assert_eq!(
            say(ActionKind::Rename, BuyOutcome::Bounced),
            "Their mirror sent it back at you. You have 40 🪙 credits."
        );
        assert_eq!(
            say(ActionKind::Mute, BuyOutcome::Blocked),
            "Their mirror blocked it, and you were still charged. You have 40 🪙 credits."
        );
        assert_eq!(
            say(ActionKind::Mirror, BuyOutcome::Landed),
            "Your mirror is up. Nobody can see it. You have 40 🪙 credits."
        );
        assert_eq!(
            cleansed_reply_text(2, 70, "credits", "🪙"),
            "Cleansed. Ended 2 running on you. You have 70 🪙 credits."
        );
    }

    #[test]
    fn unmasking_names_both_and_the_picker_reads_in_words() {
        assert_eq!(
            unmasked_text(UserId::new(1), ActionKind::Lock, UserId::new(2)),
            "🔍 <@1> unmasked the lock nickname: <@2>"
        );
        let now = chrono::TimeZone::timestamp_opt(&Utc, 1_800_000_000, 0).unwrap();
        let label = reveal_label(ActionKind::Rename, now, now - chrono::Duration::minutes(5));
        assert_eq!(label, "Rename \u{b7} 5 minutes ago");
        assert!(!label.contains("<t:"));
    }

    #[test]
    fn preflight_looks_at_the_channels_the_action_touches() {
        let (t, b, j) = (
            Some(ChannelId::new(1)),
            Some(ChannelId::new(2)),
            Some(ChannelId::new(3)),
        );
        let ids = |v: Vec<ChannelId>| v.iter().map(|c| c.get()).collect::<Vec<_>>();
        assert_eq!(ids(involved_channels(ActionKind::Drag, t, b, j)), [1, 2]);
        assert_eq!(ids(involved_channels(ActionKind::Drag, None, b, j)), [2]);
        assert_eq!(ids(involved_channels(ActionKind::Jail, t, b, j)), [3]);
        assert!(involved_channels(ActionKind::Jail, t, b, None).is_empty());
        assert!(involved_channels(ActionKind::Mute, t, b, j).is_empty());
    }

    #[test]
    fn the_private_reveal_names_the_buyer_too() {
        assert_eq!(
            revealed_reply_text(ActionKind::Mute, UserId::new(2), 40, "credits", "🪙"),
            "It was <@2> (mute). You have 40 🪙 credits."
        );
    }

    #[test]
    fn only_actions_that_changed_a_member_are_logged() {
        let (b, t) = (UserId::new(1), UserId::new(2));
        let log = |o| shop_event(ActionKind::Rename, o, b, t, Some("gg".into()), false, None);
        let Some(ModLogEvent::ShopAction {
            buyer,
            target,
            mirrored_from,
            ..
        }) = log(BuyOutcome::Landed)
        else {
            panic!("a landed prank is logged");
        };
        assert_eq!((buyer, target, mirrored_from), (b, t, None));
        // A bounce changed the buyer, and names whose mirror sent it back.
        let Some(ModLogEvent::ShopAction {
            target,
            mirrored_from,
            ..
        }) = log(BuyOutcome::Bounced)
        else {
            panic!("a bounce is logged");
        };
        assert_eq!((target, mirrored_from), (b, Some(t)));
        // A blocked one changed nobody.
        assert_eq!(log(BuyOutcome::Blocked), None);
    }
}
