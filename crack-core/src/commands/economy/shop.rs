//! `/shop menu|immunity|rename|lock|clown|drag|mute|deafen|jail|mirror|theme|cleanse|reveal`.
use super::admin::{log, log_to};
use super::credits::{gate, live, say};
use crate::commands::music::doplay::{play_core, play_settlement, PlayOutcome, Settlement};
use crate::commands::permissions::cmd_check_music;
use crate::db::economy as db;
use crate::economy::bounty;
use crate::economy::buttons::{card_buttons, carries_buttons};
use crate::economy::cards::{
    card_channel, card_destination, card_text, dedication_card_text, jump_card_text,
};
use crate::economy::discord::LiveEconomyDiscord;
use crate::economy::preflight::{self, report_for, ActionPreflight};
use crate::economy::settings::{
    validate_nick, validate_note, ActionKind, EconomySettings, NickError, NoteError,
};
use crate::economy::shop::{
    ago_words, buy, buy_error_text, buy_immunity, cleanse, preflight_refusal, refusal_text, reveal,
    reveal_choices, BuyError, BuyOutcome, BuyRequest,
};
use crate::economy::shop::{cant_afford, check_purchase, PurchaseFacts};
use crate::economy::target::check_target;
use crate::economy::target::{gather_facts, voice_of, Subject};
use crate::economy::theme::{buy_theme, parse_start, StartError, ThemeRequest};
use crate::economy::wallet::{ActionPayload, ActionState, MoveError};
use crate::messaging::courier;
use crate::messaging::messages::{
    DASH_ECONOMY_PREFLIGHT_UNKNOWN, ECONOMY_CANT_TELL, ECONOMY_DOCS_URL, ECONOMY_IMMUNE_UNTIL,
    ECONOMY_NICK_EMPTY, ECONOMY_NICK_LINK, ECONOMY_NICK_MENTION, ECONOMY_NICK_TOO_LONG,
    ECONOMY_NOTE_EMPTY, ECONOMY_NOTE_LINK, ECONOMY_NOTE_MENTION, ECONOMY_NOTE_TOO_LONG,
    ECONOMY_PLAY_NOT_CHARGED, ECONOMY_PURCHASE_FAILED, ECONOMY_REVEAL_PREFIX,
    ECONOMY_REVEAL_UNMASKED, ECONOMY_REVEAL_WAS, ECONOMY_SHOP_BLOCKED, ECONOMY_SHOP_BOT_BUYER,
    ECONOMY_SHOP_BOUNCED, ECONOMY_SHOP_CLEANSED_PREFIX, ECONOMY_SHOP_CLEANSED_SUFFIX,
    ECONOMY_SHOP_DOCS_LABEL, ECONOMY_SHOP_DONE, ECONOMY_SHOP_IMMUNITY_NOTE, ECONOMY_SHOP_MIRROR_UP,
    ECONOMY_SHOP_NOT_SOLD, ECONOMY_SHOP_TITLE, ECONOMY_THEME_BOUGHT_PREFIX,
    ECONOMY_THEME_BOUGHT_SUFFIX, ECONOMY_THEME_START_MALFORMED, ECONOMY_THEME_START_TOO_LATE,
    ECONOMY_YOU_HAVE,
};
use crate::messaging::render::Rendered;
use crate::messaging::transport::DiscordTransport;
use crate::modlog::event::ModLogEvent;
use crate::{Context, Data, Error};
use chrono::{DateTime, Utc};
use poise::serenity_prelude::{AutocompleteChoice, CreateAutocompleteResponse};
use serenity::all::{ChannelId, GenericChannelId, GuildId, Member, UserId};
use std::borrow::Cow;

/// Why the permission report says a prank can't be sold now, in words;
/// `None` when it is ready. A report that couldn't be checked fails closed,
/// as the purchase does.
fn unmet(report: &[ActionPreflight], kind: ActionKind) -> Option<String> {
    let entry = report.iter().find(|a| a.kind == kind.key())?;
    match &entry.needs {
        Some(gaps) if gaps.is_empty() => None,
        Some(gaps) => Some(gaps.join("; ")),
        None => Some(DASH_ECONOMY_PREFLIGHT_UNKNOWN.to_string()),
    }
}

/// The shop's listing: every item that is on, with its price, and the docs
/// link. An item that isn't sold says so, and a prank the permission report
/// (`report`, [`crate::economy::preflight::economy_preflight`]) says can't
/// be done right now also says why (spec 7).
pub fn menu_text(settings: &EconomySettings, report: &[ActionPreflight]) -> String {
    let cfg = &settings.config;
    let mut lines = vec![format!("**{ECONOMY_SHOP_TITLE}**")];
    for kind in ActionKind::ALL {
        let price = cfg.prices.get(*kind);
        if !price.on {
            continue;
        }
        let why = unmet(report, *kind);
        let sold = match kind {
            ActionKind::Immunity => cfg.immunity_shop,
            // Jail needs a channel to send members to.
            ActionKind::Jail => cfg.jail_channel_id.is_some() && why.is_none(),
            _ => why.is_none(),
        };
        if sold {
            lines.push(format!(
                "{}: {} {} {}",
                kind.label(),
                price.price,
                cfg.currency_emoji,
                cfg.currency_name
            ));
        } else if let Some(why) = why {
            lines.push(format!("{}: {ECONOMY_SHOP_NOT_SOLD} ({why})", kind.label()));
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
    say(ctx, buy_error_text(&err, currency), true).await
}

/// Where a purchase's words go: a command's reply, or a button's follow-ups.
#[async_trait::async_trait]
pub(crate) trait Answer: Send + Sync {
    /// Said to the buyer alone.
    async fn private(&self, text: String) -> Result<(), Error>;
    /// Said in the channel; mentions in it ping nobody.
    async fn public(&self, out: Rendered) -> Result<(), Error>;
}

/// A slash command's answers.
#[cfg(not(tarpaulin_include))]
struct CommandAnswer<'a>(Context<'a>);

#[cfg(not(tarpaulin_include))]
#[async_trait::async_trait]
impl Answer for CommandAnswer<'_> {
    async fn private(&self, text: String) -> Result<(), Error> {
        say(self.0, text, true).await
    }
    async fn public(&self, out: Rendered) -> Result<(), Error> {
        courier::reply_rendered(self.0, out, false).await?;
        Ok(())
    }
}

/// Who is buying, where, and how to answer them.
#[cfg(not(tarpaulin_include))]
pub(crate) struct PrankEnv<'a> {
    pub data: &'a Data,
    pub ctx: &'a serenity::all::Context,
    pub guild: GuildId,
    /// The buyer as Discord resolved them (the interaction's member).
    pub buyer: &'a Member,
    /// The channel the command or button was used in.
    pub invoked_in: ChannelId,
    pub answer: &'a dyn Answer,
}

/// Buy a prank (or a mirror, with no target; `target` is the member as
/// Discord resolved them). Validate, gather facts, preflight, buy; then reply
/// privately with the balance, post the card, and log the purchase. A mirror
/// posts nothing public and logs nothing. The slash commands and the Rename
/// them back button both come through here.
#[cfg(not(tarpaulin_include))]
pub(crate) async fn run_prank(
    env: &PrankEnv<'_>,
    kind: ActionKind,
    target: Option<&Member>,
    nick: Option<String>,
    note: Option<String>,
    signed: bool,
) -> Result<(), Error> {
    let PrankEnv {
        data,
        ctx,
        guild,
        answer,
        ..
    } = *env;
    let buyer = env.buyer.user.id;
    if env.buyer.user.bot() {
        return answer.private(ECONOMY_SHOP_BOT_BUYER.to_string()).await;
    }
    let note = match note.as_deref().map(validate_note).transpose() {
        Ok(n) => n,
        Err(e) => return answer.private(note_error_text(e).to_string()).await,
    };
    let nick = match nick.as_deref().map(validate_nick).transpose() {
        Ok(n) => n,
        Err(e) => return answer.private(nick_error_text(e).to_string()).await,
    };
    let cfg = data.economy.settings(guild).config;
    let now = Utc::now();
    let cache = &ctx.cache;
    let buyer_voice = voice_of(cache, guild, buyer);
    fn who(member: &Member, buyer: UserId) -> Subject<'_> {
        Subject {
            member,
            is_self: member.user.id == buyer,
        }
    }
    let me = who(env.buyer, buyer);
    let buyer_facts = match gather_facts(data, cache, guild, me, kind, buyer_voice, now).await {
        Ok(f) => f,
        Err(e) => return answer.private(buy_error_text(&e, &cfg.currency_name)).await,
    };
    // A mirror protects the buyer: their own facts stand in for a target's.
    let facts = match target {
        Some(t) => {
            match gather_facts(data, cache, guild, who(t, buyer), kind, buyer_voice, now).await {
                Ok(f) => f,
                Err(e) => return answer.private(buy_error_text(&e, &cfg.currency_name)).await,
            }
        },
        None => buyer_facts.clone(),
    };
    // Refuse before charging when the bot lacks what the action needs. An
    // action that is off is left to `buy`, which says so.
    if cfg.prices.get(kind).on {
        let channels = involved_channels(kind, facts.in_voice, buyer_voice, cfg.jail_channel_id);
        let perms = preflight::bot_perms(cache, guild, &channels);
        if let Err(r) = preflight_refusal(kind, perms.as_ref(), &channels) {
            return answer.private(refusal_text(&r, &cfg.currency_name)).await;
        }
    }
    let req = BuyRequest {
        guild,
        buyer,
        kind,
        target: target.map(|t| t.user.id),
        nick: nick.clone(),
        note: note.clone(),
        signed,
        now,
    };
    let discord = LiveEconomyDiscord {
        http: ctx.http.clone(),
    };
    let bought = match buy(
        data,
        &discord,
        facts.clone(),
        buyer_facts.clone(),
        buyer_voice,
        req,
    )
    .await
    {
        Ok(b) => b,
        Err(e) => return answer.private(buy_error_text(&e, &cfg.currency_name)).await,
    };
    // A prank that landed collects the bounties on its target. The prank is
    // done whatever this says, so a failed claim is logged and nothing more.
    let claimed = match (target, data.database_pool.as_ref()) {
        (Some(t), Some(pool)) => {
            let t = t.user.id;
            match bounty::collect(pool, guild, t, buyer, kind, bought.outcome).await {
                Ok(total) => total,
                Err(err) => {
                    tracing::warn!(
                        "economy: claiming the bounties on {t} in {guild} failed: {err}"
                    );
                    0
                },
            }
        },
        _ => 0,
    };
    let mut reply = bought_reply_text(
        kind,
        bought.outcome,
        bought.balance,
        &cfg.currency_name,
        &cfg.currency_emoji,
    );
    if claimed > 0 {
        reply.push('\n');
        reply.push_str(&bounty::collected_reply_line(claimed, &cfg.currency_name));
    }
    // Reply first: a slow card or log channel must not time the interaction out.
    let replied = answer.private(reply).await;
    let Some(target) = target.map(|t| t.user.id) else {
        return replied;
    };
    let channel = card_channel(
        kind,
        bought.outcome,
        facts.in_voice,
        buyer_voice,
        cfg.jail_channel_id,
        env.invoked_in,
        cfg.announce_channel_id,
    );
    let mut card = card_text(
        kind,
        target,
        nick.as_deref(),
        bought.until,
        note.as_deref(),
        signed.then_some(buyer),
        bought.outcome,
    );
    if claimed > 0 {
        card.push('\n');
        card.push_str(&bounty::collected_card_line(target));
    }
    let mut rendered = Rendered::text(card);
    if carries_buttons(kind, bought.outcome) {
        rendered = rendered.with_components(card_buttons(bought.action_id));
    }
    let transport = DiscordTransport::of(ctx);
    if let Err(err) =
        courier::post_message(&transport, GenericChannelId::new(channel.get()), &rendered).await
    {
        // The destination may be a channel the bot cannot post in: a public
        // follow-up in the invoking channel needs no channel permissions.
        tracing::warn!(
            "economy: posting the {kind:?} card in {channel} failed, answering in the invoking channel: {err:?}"
        );
        if let Err(err) = answer.public(rendered).await {
            tracing::warn!("economy: the {kind:?} card follow-up failed: {err:?}");
        }
    }
    if let Some(event) = shop_event(
        kind,
        bought.outcome,
        buyer,
        target,
        note,
        signed,
        bought.until,
    ) {
        log_to(env.data, env.ctx, env.guild, event).await;
    }
    replied
}

/// Buy a prank from a slash command: the gate and the bot check, then the
/// shared purchase.
#[cfg(not(tarpaulin_include))]
async fn prank(
    ctx: Context<'_>,
    kind: ActionKind,
    target: Option<Member>,
    nick: Option<String>,
    note: Option<String>,
    signed: Option<bool>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    // A purchase reads the database several times and calls Discord before
    // its first reply: deferred, the private reply edits this response and
    // the public card falls back to a follow-up.
    ctx.defer_ephemeral().await?;
    // The interaction's own member: roles and nickname, cached or not.
    let Some(buyer) = ctx.author_member().await else {
        return say(ctx, ECONOMY_CANT_TELL, true).await;
    };
    let data = ctx.data();
    let env = PrankEnv {
        data: &data,
        ctx: ctx.serenity_context(),
        guild,
        buyer: &buyer,
        invoked_in: ChannelId::new(ctx.channel_id().get()),
        answer: &CommandAnswer(ctx),
    };
    run_prank(
        &env,
        kind,
        target.as_ref(),
        nick,
        note,
        signed.unwrap_or(false),
    )
    .await
}

/// Change someone's nickname for a while.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn rename(
    ctx: Context<'_>,
    #[description = "Who"] member: Member,
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
    #[description = "Who"] member: Member,
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
    #[description = "Who"] member: Member,
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
    #[description = "Who"] member: Member,
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
    #[description = "Who"] member: Member,
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
    #[description = "Who"] member: Member,
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
    #[description = "Who"] member: Member,
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

/// The words for a refused start time.
pub fn start_error_text(e: StartError) -> &'static str {
    match e {
        StartError::Malformed => ECONOMY_THEME_START_MALFORMED,
        StartError::TooLate => ECONOMY_THEME_START_TOO_LATE,
    }
}

/// The private reply to a bought theme: when it plays, then what is left.
pub fn theme_reply_text(balance: i64, currency: &str, emoji: &str) -> String {
    format!(
        "{ECONOMY_THEME_BOUGHT_PREFIX} {currency} {ECONOMY_THEME_BOUGHT_SUFFIX}\n{}",
        balance_followup_text(balance, currency, emoji)
    )
}

/// Have a song play for someone the next time they join voice.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn theme(
    ctx: Context<'_>,
    #[description = "Who"] member: Member,
    #[description = "A link to the song or video"] link: String,
    #[description = "Where it starts: 83, 1:23 or 1:02:03"] start: Option<String>,
    #[description = "A note shown in the mod log"] note: Option<String>,
    #[description = "Put your name in the mod log"] signed: Option<bool>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    // Reads the database and Discord before the first reply, as a prank does.
    ctx.defer_ephemeral().await?;
    let Some(buyer) = ctx.author_member().await else {
        return say(ctx, ECONOMY_CANT_TELL, true).await;
    };
    let data = ctx.data();
    let cfg = data.economy.settings(guild).config;
    if buyer.user.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    // Both are answered before anything is read or charged.
    let start = match start.as_deref().map(parse_start).transpose() {
        Ok(s) => s.unwrap_or_default(),
        Err(e) => return say(ctx, start_error_text(e), true).await,
    };
    let note = match note.as_deref().map(validate_note).transpose() {
        Ok(n) => n,
        Err(e) => return say(ctx, note_error_text(e), true).await,
    };
    let signed = signed.unwrap_or(false);
    let now = Utc::now();
    let (buyer_id, target) = (buyer.user.id, member.user.id);
    let facts = match gather_facts(
        &data,
        &ctx.serenity_context().cache,
        guild,
        Subject {
            member: &member,
            is_self: target == buyer_id,
        },
        ActionKind::Theme,
        None,
        now,
    )
    .await
    {
        Ok(f) => f,
        Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
    };
    let bought = match buy_theme(
        &data,
        facts,
        ThemeRequest {
            guild,
            buyer: buyer_id,
            target,
            url: link,
            start,
            note: note.clone(),
            signed,
            now,
        },
    )
    .await
    {
        Ok(b) => b,
        Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
    };
    // No card: the theme announces itself when it plays. Reply first, then log.
    let replied = say(
        ctx,
        theme_reply_text(bought.balance, &cfg.currency_name, &cfg.currency_emoji),
        true,
    )
    .await;
    log_to(
        &data,
        ctx.serenity_context(),
        guild,
        ModLogEvent::ShopAction {
            action: ActionKind::Theme,
            buyer: buyer_id,
            target,
            note,
            signed,
            until: bought.until,
            mirrored_from: None,
        },
    )
    .await;
    replied
}

/// The private balance line after a kept paid play.
pub fn paid_play_balance_text(balance: i64, currency: &str, emoji: &str) -> String {
    balance_followup_text(balance, currency, emoji)
}

/// Which of the two paid plays is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaidPlay {
    Dedicate,
    Jump,
}

/// Charge, run `/play`'s own path, then keep the charge or refund it.
///
/// 🔑 No defer before the play (ruling P6): `/play` chooses its reply's
/// visibility, so every check up to the charge reads only the cache and the
/// database.
#[cfg(not(tarpaulin_include))]
async fn paid_play(
    ctx: Context<'_>,
    which: PaidPlay,
    member: Option<Member>,
    song: String,
    note: Option<String>,
    signed: Option<bool>,
) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let Some(buyer) = ctx.author_member().await else {
        return say(ctx, ECONOMY_CANT_TELL, true).await;
    };
    let data = ctx.data();
    let cfg = data.economy.settings(guild).config;
    if buyer.user.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let note = match note.as_deref().map(validate_note).transpose() {
        Ok(n) => n,
        Err(e) => return say(ctx, note_error_text(e), true).await,
    };
    let signed = signed.unwrap_or(false);
    let buyer_id = buyer.user.id;
    let kind = match which {
        PaidPlay::Dedicate => ActionKind::Dedicate,
        PaidPlay::Jump => ActionKind::Jump,
    };
    let target = member.as_ref().map(|m| m.user.id);
    if let Some(member) = member.as_ref() {
        let facts = match gather_facts(
            &data,
            &ctx.serenity_context().cache,
            guild,
            Subject {
                member,
                is_self: member.user.id == buyer_id,
            },
            kind,
            None,
            Utc::now(),
        )
        .await
        {
            Ok(f) => f,
            Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
        };
        if let Err(r) = check_target(kind, &facts, None) {
            return say(ctx, refusal_text(&r, &cfg.currency_name), true).await;
        }
    }
    let price = cfg.prices.get(kind);
    let balance = match db::balance(&pool, guild, buyer_id).await {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!("economy: reading balance of {buyer_id} in {guild} failed: {err}");
            return say(ctx, ECONOMY_PURCHASE_FAILED, true).await;
        },
    };
    if let Err(r) = check_purchase(&PurchaseFacts {
        enabled: data.economy.is_enabled(guild),
        price,
        source_on: true,
        buyer_opted_out: data.economy.is_opted_out(guild, buyer_id),
        already_immune_indefinitely: false,
        balance,
    }) {
        return say(ctx, refusal_text(&r, &cfg.currency_name), true).await;
    }
    let price = i64::from(price.price);
    let payload = match which {
        PaidPlay::Dedicate => ActionPayload::Dedicate { song: song.clone() },
        PaidPlay::Jump => ActionPayload::Jump { song: song.clone() },
    };
    let charged = match db::charge_for_action(
        &pool,
        &db::NewAction {
            guild,
            kind,
            buyer: buyer_id,
            target,
            price,
            note: note.as_deref(),
            signed,
            payload: &payload,
            expires_at: None,
        },
    )
    .await
    {
        Ok(c) => c,
        Err(MoveError::Insufficient { balance }) => {
            let r = cant_afford(price, balance);
            return say(ctx, refusal_text(&r, &cfg.currency_name), true).await;
        },
        Err(err) => {
            tracing::warn!("economy: charging {buyer_id} in {guild} for a play failed: {err:?}");
            return say(ctx, ECONOMY_PURCHASE_FAILED, true).await;
        },
    };
    let mode = match which {
        PaidPlay::Dedicate => None,
        PaidPlay::Jump => Some("next".to_string()),
    };
    let played = play_core(ctx, mode, None, Some(song)).await;
    let verdict = play_settlement(&played.as_ref().map(|o| *o).map_err(|_| ()));
    match verdict {
        Settlement::Refund => {
            if let Err(err) = db::refund_action(&pool, charged.action_id).await {
                // The row stays pending: reconcile refunds it (ruling P5).
                tracing::error!(
                    "economy: refunding action {} for {buyer_id} in {guild} failed: {err}",
                    charged.action_id
                );
            }
            return match played {
                // Poise's handler renders it exactly as `/play` would.
                Err(e) => Err(e),
                Ok(PlayOutcome::NothingQueued | PlayOutcome::Queued) => {
                    say(ctx, ECONOMY_PLAY_NOT_CHARGED, true).await
                },
            };
        },
        Settlement::Keep => {},
    }
    if let Err(err) = db::finish_action(&pool, charged.action_id, ActionState::Done).await {
        tracing::warn!(
            "economy: marking action {} done failed: {err}",
            charged.action_id
        );
    }
    let replied = say(
        ctx,
        paid_play_balance_text(charged.balance, &cfg.currency_name, &cfg.currency_emoji),
        true,
    )
    .await;
    let by = signed.then_some(buyer_id);
    let text = match target {
        Some(t) => dedication_card_text(t, note.as_deref(), by),
        None => jump_card_text(note.as_deref(), by),
    };
    let channel = card_destination(
        kind,
        None,
        ChannelId::new(ctx.channel_id().get()),
        cfg.announce_channel_id,
    );
    let rendered = Rendered::text(text);
    let transport = DiscordTransport::of(ctx.serenity_context());
    if let Err(err) =
        courier::post_message(&transport, GenericChannelId::new(channel.get()), &rendered).await
    {
        tracing::warn!("economy: posting the play card in {channel} failed: {err:?}");
        if let Err(err) = courier::reply_rendered(ctx, rendered, false).await {
            tracing::warn!("economy: the play card follow-up failed: {err:?}");
        }
    }
    if let Some(target) = target {
        log_to(
            &data,
            ctx.serenity_context(),
            guild,
            ModLogEvent::ShopAction {
                action: ActionKind::Dedicate,
                buyer: buyer_id,
                target,
                note,
                signed,
                until: None,
                mirrored_from: None,
            },
        )
        .await;
    }
    replied
}

/// Queue a song like /play, and say it goes out to someone.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, check = "cmd_check_music")]
pub async fn dedicate(
    ctx: Context<'_>,
    #[description = "Who it goes out to"] member: Member,
    #[description = "Song link or search query"] song: String,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    paid_play(ctx, PaidPlay::Dedicate, Some(member), song, note, signed).await
}

/// Queue a song to play next, like /playnext.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, check = "cmd_check_music")]
pub async fn jump(
    ctx: Context<'_>,
    #[description = "Song link or search query"] song: String,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    paid_play(ctx, PaidPlay::Jump, None, song, note, signed).await
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
        "theme",
        "dedicate",
        "jump",
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
    let settings = ctx.data().economy.settings(guild);
    let (report, _) = report_for(&ctx.serenity_context().cache, guild, &settings.config);
    say(ctx, menu_text(&settings, &report), false).await
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
    // Ends each prank with a Discord call before replying.
    ctx.defer_ephemeral().await?;
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
    // The private reply edits this; the public unmasking is a follow-up.
    ctx.defer_ephemeral().await?;
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

    /// Everything CrackTunes needs, server-wide and in channel 9.
    fn able() -> preflight::BotPerms {
        preflight::BotPerms {
            guild: serenity::all::Permissions::MANAGE_NICKNAMES
                | serenity::all::Permissions::VIEW_AUDIT_LOG
                | serenity::all::Permissions::MOVE_MEMBERS
                | serenity::all::Permissions::MUTE_MEMBERS
                | serenity::all::Permissions::DEAFEN_MEMBERS,
            in_channel: [(ChannelId::new(9), preflight::NEED_CHANNEL)].into(),
        }
    }

    fn menu(s: &EconomySettings, perms: Option<&preflight::BotPerms>) -> String {
        let (report, _) = preflight::economy_preflight(&s.config, perms, |c| format!("<#{c}>"));
        menu_text(s, &report)
    }

    #[test]
    fn menu_lists_on_items_with_prices() {
        let mut s = EconomySettings::off(GuildId::new(1));
        assert_eq!(
            menu(&s, Some(&able())),
            format!(
                "**Shop**\nImmunity: 200 🪙 credits\nProtects you from pranks.\n\
                 Rename: 120 🪙 credits\nLock nickname: 60 🪙 credits\nClown: 30 🪙 credits\n\
                 Drag: 60 🪙 credits\nMute: 90 🪙 credits\nDeafen: 90 🪙 credits\n\
                 Jail: not sold here (Choose a jail channel first.)\nMirror: 150 🪙 credits\n\
                 Cleanse: 80 🪙 credits\nReveal: 100 🪙 credits\n\
                 Walk-on theme: 100 🪙 credits\nDedication: 40 🪙 credits\n\
                 Queue jump: 40 🪙 credits\nBounty: 50 🪙 credits\n\
                 Crowdfund: 10 🪙 credits\n{DOCS}"
            )
        );
        s.config.jail_channel_id = Some(serenity::all::ChannelId::new(9));
        let menu_text = |s: &EconomySettings| menu(s, Some(&able()));
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
    fn menu_marks_what_the_bot_cannot_do_and_says_why() {
        let mut s = EconomySettings::off(GuildId::new(1));
        s.config.jail_channel_id = Some(ChannelId::new(9));
        // No View Audit Log: lock is marked, with the dashboard's words.
        let mut p = able();
        p.guild -= serenity::all::Permissions::VIEW_AUDIT_LOG;
        let text = menu(&s, Some(&p));
        assert!(
            text.contains(
                "Lock nickname: not sold here (Give CrackTunes View Audit Log in Server Settings → Roles)"
            ),
            "{text}"
        );
        assert!(text.contains("Rename: 120 🪙 credits"), "{text}");
        // The jail channel lacks Connect: jail is marked, naming it.
        let mut p = able();
        p.in_channel
            .insert(ChannelId::new(9), serenity::all::Permissions::VIEW_CHANNEL);
        assert!(
            menu(&s, Some(&p)).contains("Jail: not sold here (Give CrackTunes Connect in <#9>)")
        );
        // The cache can't say: every prank is marked (the purchase would be
        // refused too); defences and immunity are still for sale.
        let text = menu(&s, None);
        assert!(
            text.contains("Mute: not sold here (Couldn't check permissions right now.)"),
            "{text}"
        );
        assert!(text.contains("Mirror: 150 🪙 credits"), "{text}");
        assert!(text.contains("Immunity: 200 🪙 credits"), "{text}");
    }

    #[test]
    fn menu_says_immunity_not_sold_when_the_switch_is_off() {
        let mut s = EconomySettings::off(GuildId::new(1));
        s.config.immunity_shop = false;
        let text = menu(&s, Some(&able()));
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
        // Counted in UTF-16 units, as Discord counts: some emoji take 4.
        assert_eq!(
            nick_error_text(NickError::TooLong),
            "That name is too long: 32 characters at most (an emoji counts as 2 or more)."
        );
    }

    #[test]
    fn theme_texts_say_when_it_plays_and_what_is_left() {
        assert_eq!(
            start_error_text(StartError::Malformed),
            "Start time looks like 83, 1:23 or 1:02:03."
        );
        assert_eq!(
            start_error_text(StartError::TooLate),
            "Start time can be at most 12:00:00."
        );
        let text = theme_reply_text(150, "credits", "🪙");
        assert_eq!(
            text,
            format!(
                "Their theme plays the next time they join voice, within 7 days. If it never does, you get your credits back.\n{}",
                balance_followup_text(150, "credits", "🪙")
            )
        );
        assert!(text.contains("You have 150"), "{text}");
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
