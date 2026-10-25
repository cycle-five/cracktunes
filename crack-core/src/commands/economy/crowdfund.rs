//! `/crowdfund start|pledge|list`: pool credits for a prank against a member.
use super::admin::{discord_len, fit, log_to};
use super::credits::{gate, say};
use super::shop::{involved_channels, nick_error_text, note_error_text};
use crate::db::economy as db;
use crate::db::economy::ActionRow;
use crate::economy::cards::{
    card_channel, card_destination, fund_card_text, fund_failed_card_text, fund_started_card_text,
};
use crate::economy::crowdfund::{
    execute, not_landed_reason, pledge as pledge_to, start as start_fund, FillFacts, FundRequest,
    NotLanded, PledgeRequest, LIST_MAX_FUNDS,
};
use crate::economy::discord::{EconDiscordError, LiveEconomyDiscord};
use crate::economy::preflight;
use crate::economy::settings::{
    validate_nick, validate_note, ActionKind, EconomyConfig, NickError,
};
use crate::economy::shop::{buy_error_text, preflight_refusal, BuyOutcome};
use crate::economy::target::{gather_facts, Subject};
use crate::messaging::courier;
use crate::messaging::format::CONTENT_MAX;
use crate::messaging::messages::{
    ECONOMY_CANT_TELL, ECONOMY_FUND_FAILED_CARD_SUFFIX, ECONOMY_FUND_FILLED_FAILED,
    ECONOMY_FUND_FILLED_LANDED, ECONOMY_FUND_LIST_ENDS, ECONOMY_FUND_LIST_MORE_PREFIX,
    ECONOMY_FUND_LIST_MORE_SUFFIX, ECONOMY_FUND_LIST_NONE, ECONOMY_FUND_LIST_ON,
    ECONOMY_FUND_LIST_TITLE, ECONOMY_FUND_OF, ECONOMY_FUND_OPEN_HINT, ECONOMY_FUND_PLEDGED,
    ECONOMY_FUND_STARTED, ECONOMY_READ_FAILED, ECONOMY_SHOP_BOT_BUYER, ECONOMY_YOU_HAVE,
};
use crate::messaging::render::Rendered;
use crate::messaging::transport::DiscordTransport;
use crate::modlog::event::ModLogEvent;
use crate::{Context, Data, Error};
use chrono::Utc;
use crack_types::discord::ToRef;
use poise::serenity_prelude::{AutocompleteChoice, CreateAutocompleteResponse};
use serenity::all::{ChannelId, GenericChannelId, GuildId, Member, UserId};
use std::borrow::Cow;

/// The pranks a crowdfund can buy: [`ActionKind::FUNDABLE`] (drag is left
/// out: its destination is the buyer's channel at the moment it runs, and a
/// fund fills at a moment nobody chose).
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum FundKind {
    #[name = "Rename"]
    Rename,
    #[name = "Clown"]
    Clown,
    #[name = "Lock nickname"]
    Lock,
    #[name = "Mute"]
    Mute,
    #[name = "Deafen"]
    Deafen,
    #[name = "Jail"]
    Jail,
}

impl FundKind {
    pub fn kind(self) -> ActionKind {
        match self {
            FundKind::Rename => ActionKind::Rename,
            FundKind::Clown => ActionKind::Clown,
            FundKind::Lock => ActionKind::Lock,
            FundKind::Mute => ActionKind::Mute,
            FundKind::Deafen => ActionKind::Deafen,
            FundKind::Jail => ActionKind::Jail,
        }
    }
}

/// Pool credits with others for a prank against a member.
#[poise::command(
    category = "Economy",
    slash_command,
    guild_only,
    subcommands("start", "pledge", "list"),
    subcommand_required
)]
pub async fn crowdfund(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// "Pledged 40 🪙 credits (90 of 120). You have 10 🪙 credits." With the
/// fund's fate when this pledge filled it: `Ok` for it landed, `Err(reason)`
/// for it couldn't (and everyone was refunded).
pub fn pledge_reply_text(
    taken: i64,
    total: i64,
    price: i64,
    balance: i64,
    fate: Option<Result<(), &str>>,
    currency: &str,
    emoji: &str,
) -> String {
    let mut text = format!(
        "{ECONOMY_FUND_PLEDGED} {taken} {emoji} {currency} ({total} {ECONOMY_FUND_OF} {price})."
    );
    match fate {
        None => {},
        Some(Ok(())) => text.push_str(&format!(" {ECONOMY_FUND_FILLED_LANDED}")),
        Some(Err(why)) => text.push_str(&format!(
            " {ECONOMY_FUND_FILLED_FAILED} ({why}). {ECONOMY_FUND_FAILED_CARD_SUFFIX}"
        )),
    }
    text.push_str(&format!(
        " {ECONOMY_YOU_HAVE} {balance} {emoji} {currency}."
    ));
    text
}

/// A fund as autocomplete shows it: "Mute on bob: 30/90".
pub fn fund_label(kind: ActionKind, target_name: &str, pledged: i64, price: i64) -> String {
    format!(
        "{} {ECONOMY_FUND_LIST_ON} {target_name}: {pledged}/{price}",
        kind.label()
    )
}

/// Whether a picker label matches what the member has typed: case-insensitive,
/// anywhere in the label. Nothing typed matches everything.
pub fn label_matches(label: &str, typed: &str) -> bool {
    label.to_lowercase().contains(&typed.trim().to_lowercase())
}

/// `/crowdfund list`: each open fund with its target, what is pledged, and
/// when it ends, newest first, at most [`LIST_MAX_FUNDS`], then a line saying
/// how many more; and if even those are too long for Discord, fewer.
pub fn fund_list_text(funds: &[(ActionRow, i64, i64)], cfg: &EconomyConfig) -> String {
    let head = format!("**{ECONOMY_FUND_LIST_TITLE}**");
    if funds.is_empty() {
        return format!("{head}\n{ECONOMY_FUND_LIST_NONE}");
    }
    let lines: Vec<String> = funds
        .iter()
        .take(LIST_MAX_FUNDS)
        .map(|(row, pledged, price)| {
            let mut line = format!(
                "{} {ECONOMY_FUND_LIST_ON} {}: {pledged}/{} {} {}",
                row.kind.label(),
                row.target.map_or_else(String::new, |t| format!("<@{t}>")),
                price,
                cfg.currency_emoji,
                cfg.currency_name
            );
            if let Some(end) = row.expires_at {
                line.push_str(&format!(
                    " · {ECONOMY_FUND_LIST_ENDS} <t:{}:R>",
                    end.timestamp()
                ));
            }
            line
        })
        .collect();
    for keep in (0..=lines.len()).rev() {
        let mut out = vec![head.clone()];
        out.extend(lines[..keep].iter().cloned());
        let left_out = funds.len() - keep;
        if left_out > 0 {
            out.push(format!(
                "{ECONOMY_FUND_LIST_MORE_PREFIX} {left_out} {ECONOMY_FUND_LIST_MORE_SUFFIX}"
            ));
        }
        let text = out.join("\n");
        if discord_len(&text) <= CONTENT_MAX {
            return text;
        }
    }
    fit(&head, CONTENT_MAX)
}

/// What a fill came to, for the private reply.
enum Fate {
    Landed,
    Failed(String),
}

impl Fate {
    fn as_reply(&self) -> Result<(), &str> {
        match self {
            Fate::Landed => Ok(()),
            Fate::Failed(why) => Err(why),
        }
    }
}

/// Refund a filled fund that cannot be run, logging a refund that fails
/// (reconcile finds the row `pending` and refunds it).
async fn refund_failed(pool: &sqlx::PgPool, fund_id: i64) {
    if let Err(e) = db::refund_fund(pool, fund_id, "failed").await {
        tracing::error!("economy: refunding fund {fund_id} failed: {e}");
    }
}

/// Run a fund that just filled (the row is `pending`): read its target, make
/// the checks that need the live server, [`execute`] it, then post the card
/// and the mod log entry. A target who left, or who can't be read, refunds
/// everyone. A card that can't be posted where it belongs falls back to a
/// public follow-up in the invoking channel. The public couldn't-land card
/// never says why; the reason (which may be the target's immunity or
/// opt-out) goes only in the private reply.
async fn run_fill(ctx: Context<'_>, guild: GuildId, fund_id: i64) -> Fate {
    let data = ctx.data();
    let sctx = ctx.serenity_context();
    let cfg = data.economy.settings(guild).config;
    let Some(pool) = data.database_pool.as_ref() else {
        return Fate::Failed(not_landed_reason(&NotLanded::Db, &cfg.currency_name));
    };
    let invoked_in = ChannelId::new(ctx.channel_id().get());
    let row = match db::action_by_id(pool, guild, fund_id).await {
        Ok(Some(r)) => r,
        other => {
            // Every failure ends with a refund, so the words are true.
            tracing::error!("economy: reading the filled fund {fund_id} failed: {other:?}");
            refund_failed(pool, fund_id).await;
            return Fate::Failed(not_landed_reason(&NotLanded::Db, &cfg.currency_name));
        },
    };
    let Some(target) = row.target else {
        refund_failed(pool, fund_id).await;
        return Fate::Failed(not_landed_reason(&NotLanded::Db, &cfg.currency_name));
    };
    let kind = row.kind;
    let member = match guild.member(sctx, target).await {
        Ok(m) => m,
        Err(err) => {
            let err = EconDiscordError::from(err);
            let (reason, why) = if err == EconDiscordError::UnknownMember {
                ("left", NotLanded::Discord(err))
            } else {
                ("failed", NotLanded::Discord(err))
            };
            if let Err(e) = db::refund_fund(pool, fund_id, reason).await {
                tracing::error!("economy: refunding fund {fund_id} failed: {e}");
            }
            let why = not_landed_reason(&why, &cfg.currency_name);
            post(
                ctx,
                kind,
                None,
                &cfg,
                invoked_in,
                fund_failed_card_text(target),
            )
            .await;
            return Fate::Failed(why);
        },
    };
    let now = Utc::now();
    let facts = match gather_facts(
        &data,
        &sctx.cache,
        guild,
        Subject {
            member: &member,
            is_self: false,
        },
        kind,
        None,
        now,
    )
    .await
    {
        Ok(f) => f,
        Err(_) => {
            if let Err(e) = db::refund_fund(pool, fund_id, "failed").await {
                tracing::error!("economy: refunding fund {fund_id} failed: {e}");
            }
            let why = not_landed_reason(&NotLanded::Db, &cfg.currency_name);
            post(
                ctx,
                kind,
                None,
                &cfg,
                invoked_in,
                fund_failed_card_text(target),
            )
            .await;
            return Fate::Failed(why);
        },
    };
    let channels = involved_channels(kind, facts.in_voice, None, cfg.jail_channel_id);
    let perms = preflight::bot_perms(&sctx.cache, guild, &channels);
    let pre = preflight_refusal(kind, perms.as_ref(), &channels);
    let discord = LiveEconomyDiscord {
        http: sctx.http.clone(),
    };
    let target_voice = facts.in_voice;
    match execute(
        &data,
        &discord,
        FillFacts {
            guild,
            target: facts,
            preflight: pre,
        },
        &row,
        &cfg,
        now,
    )
    .await
    {
        Ok(done) => {
            let nick = match &row.payload {
                crate::economy::wallet::ActionPayload::Rename { name, .. } => Some(name.clone()),
                _ => None,
            };
            let card = fund_card_text(
                kind,
                target,
                nick.as_deref(),
                done.until,
                row.note.as_deref(),
                &done.signers,
            );
            post(ctx, kind, Some(target_voice), &cfg, invoked_in, card).await;
            log_to(
                &data,
                sctx,
                guild,
                ModLogEvent::CrowdfundExecuted {
                    action: kind,
                    target: target.to_ref(),
                    contributors: done.contributors.iter().map(ToRef::to_ref).collect(),
                    until: done.until,
                },
            )
            .await;
            Fate::Landed
        },
        Err(why) => {
            let why = not_landed_reason(&why, &cfg.currency_name);
            post(
                ctx,
                kind,
                None,
                &cfg,
                invoked_in,
                fund_failed_card_text(target),
            )
            .await;
            Fate::Failed(why)
        },
    }
}

/// Post a fund's card where the prank's card would go: `landed_voice` is
/// `Some(target's voice)` for a card of a fund that landed, `None` for the
/// couldn't-land card (which goes to the announce channel or where the
/// command ran). Falls back to a public follow-up.
async fn post(
    ctx: Context<'_>,
    kind: ActionKind,
    landed_voice: Option<Option<ChannelId>>,
    cfg: &EconomyConfig,
    invoked_in: ChannelId,
    text: String,
) {
    let channel = match landed_voice {
        Some(voice) => card_channel(
            kind,
            BuyOutcome::Landed,
            voice,
            None,
            cfg.jail_channel_id,
            invoked_in,
            cfg.announce_channel_id,
        ),
        None => card_destination(kind, None, invoked_in, cfg.announce_channel_id),
    };
    let rendered = Rendered::text(text);
    let transport = DiscordTransport::of(ctx.serenity_context());
    if let Err(err) =
        courier::post_message(&transport, GenericChannelId::new(channel.get()), &rendered).await
    {
        tracing::warn!(
            "economy: posting the crowdfund card in {channel} failed, answering in the invoking channel: {err:?}"
        );
        if let Err(err) = courier::reply_rendered(ctx, rendered, false).await {
            tracing::warn!("economy: the crowdfund card follow-up failed: {err:?}");
        }
    }
}

/// Start a crowdfund for a prank against a member.
#[poise::command(slash_command, guild_only)]
// live-Discord glue: a poise command body needs a live `Context` (Discord HTTP, cache, gateway), which no unit test can build.
#[cfg_attr(test, mutants::skip)]
pub async fn start(
    ctx: Context<'_>,
    #[description = "The prank"] action: FundKind,
    #[description = "Who it is against"] member: Member,
    #[description = "Your own pledge, to start it"] pledge: i64,
    #[description = "Their new nickname (for Rename)"] name: Option<String>,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    ctx.defer_ephemeral().await?;
    let Some(starter) = ctx.author_member().await else {
        return say(ctx, ECONOMY_CANT_TELL, true).await;
    };
    let data = ctx.data();
    let cfg = data.economy.settings(guild).config;
    if starter.user.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let kind = action.kind();
    let note = match note.as_deref().map(validate_note).transpose() {
        Ok(n) => n,
        Err(e) => return say(ctx, note_error_text(e), true).await,
    };
    let nick = match (kind, name.as_deref()) {
        (ActionKind::Rename, Some(raw)) => match validate_nick(raw) {
            Ok(n) => Some(n),
            Err(e) => return say(ctx, nick_error_text(e), true).await,
        },
        (ActionKind::Rename, None) => {
            return say(ctx, nick_error_text(NickError::Empty), true).await;
        },
        _ => None,
    };
    let signed = signed.unwrap_or(false);
    let now = Utc::now();
    let (starter_id, target) = (starter.user.id, member.user.id);
    let facts = match gather_facts(
        &data,
        &ctx.serenity_context().cache,
        guild,
        Subject {
            member: &member,
            is_self: target == starter_id,
        },
        kind,
        None,
        now,
    )
    .await
    {
        Ok(f) => f,
        Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
    };
    // The permission check needs the cache. An action that is off is left to
    // `start`, which says so.
    let preflight = if cfg.prices.get(kind).on {
        let channels = involved_channels(kind, facts.in_voice, None, cfg.jail_channel_id);
        let perms = preflight::bot_perms(&ctx.serenity_context().cache, guild, &channels);
        preflight_refusal(kind, perms.as_ref(), &channels)
    } else {
        Ok(())
    };
    let row_nick = nick.clone();
    let log_note = note.clone();
    let opened = match start_fund(
        &data,
        facts,
        FundRequest {
            guild,
            starter: starter_id,
            target,
            kind,
            nick: nick.clone(),
            note,
            signed,
            first_pledge: pledge,
            preflight,
            now,
        },
    )
    .await
    {
        Ok(o) => o,
        Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
    };
    // The mod log always names the starter, whatever the card says.
    log_to(
        &data,
        ctx.serenity_context(),
        guild,
        ModLogEvent::CrowdfundStarted {
            action: kind,
            starter: starter_id.to_ref(),
            target: target.to_ref(),
            note: log_note,
            signed,
        },
    )
    .await;
    let price = i64::from(cfg.prices.get(kind).price);
    let mut reply = ECONOMY_FUND_STARTED.to_string();
    // The share cap keeps a first pledge below the price (a fund costs at
    // least `MIN_FUNDABLE_PRICE`); were one ever to cover it, it fills at once.
    let filled = opened.pledged >= price;
    let fate = if filled {
        Some(run_fill(ctx, guild, opened.action_id).await)
    } else {
        None
    };
    if let Some(balance) = opened.balance {
        let balance = if filled {
            fresh_balance(&data, guild, starter_id, balance).await
        } else {
            balance
        };
        reply.push(' ');
        reply.push_str(&pledge_reply_text(
            opened.pledged,
            opened.pledged,
            price,
            balance,
            fate.as_ref().map(Fate::as_reply),
            &cfg.currency_name,
            &cfg.currency_emoji,
        ));
    }
    if !filled {
        // Tell the room, so others can chip in.
        let card = fund_started_card_text(
            kind,
            target,
            row_nick.as_deref(),
            opened.pledged,
            price,
            &cfg.currency_name,
            signed.then_some(starter_id),
        );
        let here = ChannelId::new(ctx.channel_id().get());
        post(ctx, kind, None, &cfg, here, card).await;
        reply.push(' ');
        reply.push_str(ECONOMY_FUND_OPEN_HINT);
    }
    say(ctx, reply, true).await
}

/// The member's balance now (a refund may have moved it), else `fallback`.
async fn fresh_balance(data: &Data, guild: GuildId, user: UserId, fallback: i64) -> i64 {
    match data.database_pool.as_ref() {
        Some(pool) => db::balance(pool, guild, user).await.unwrap_or(fallback),
        None => fallback,
    }
}

/// The open funds as picker choices: "Mute on bob: 30/90".
async fn fund_autocomplete<'a>(
    ctx: poise::ApplicationContext<'_, Data, Error>,
    partial: &'a str,
) -> CreateAutocompleteResponse<'a> {
    let Some(guild) = ctx.interaction.guild_id else {
        return CreateAutocompleteResponse::new();
    };
    let data = ctx.data();
    let Some(pool) = data.database_pool.as_ref() else {
        return CreateAutocompleteResponse::new();
    };
    if !data.economy.is_enabled(guild) {
        return CreateAutocompleteResponse::new();
    }
    let funds = db::open_funds(pool, guild).await.unwrap_or_default();
    let choices: Vec<AutocompleteChoice<'a>> = funds
        .into_iter()
        .filter_map(|(row, pledged, price)| {
            let target = row.target?;
            let name = ctx
                .serenity_context()
                .cache
                .guild(guild)
                .and_then(|g| g.members.get(&target).map(|m| m.display_name().to_string()))
                .unwrap_or_else(|| target.to_string());
            let label = fit(&fund_label(row.kind, &name, pledged, price), 100);
            label_matches(&label, partial).then_some(())?;
            Some(AutocompleteChoice::new(
                Cow::Owned(label),
                u64::try_from(row.id).ok()?,
            ))
        })
        .take(25)
        .collect();
    CreateAutocompleteResponse::new().set_choices(Cow::Owned(choices))
}

/// Pledge credits to an open crowdfund.
#[poise::command(slash_command, guild_only)]
pub async fn pledge(
    ctx: Context<'_>,
    #[description = "Which crowdfund"]
    #[autocomplete = "fund_autocomplete"]
    fund: i64,
    #[description = "How many credits"] amount: i64,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    ctx.defer_ephemeral().await?;
    let Some(pledger) = ctx.author_member().await else {
        return say(ctx, ECONOMY_CANT_TELL, true).await;
    };
    let data = ctx.data();
    let cfg = data.economy.settings(guild).config;
    if pledger.user.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let done = match pledge_to(
        &data,
        PledgeRequest {
            guild,
            pledger: pledger.user.id,
            fund,
            amount,
            signed: signed.unwrap_or(false),
        },
    )
    .await
    {
        Ok(d) => d,
        Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
    };
    let p = done.pledged;
    let fate = if p.filled {
        Some(run_fill(ctx, guild, fund).await)
    } else {
        None
    };
    let balance = if p.filled {
        fresh_balance(&data, guild, pledger.user.id, p.balance).await
    } else {
        p.balance
    };
    say(
        ctx,
        pledge_reply_text(
            p.taken,
            p.total,
            p.price,
            balance,
            fate.as_ref().map(Fate::as_reply),
            &cfg.currency_name,
            &cfg.currency_emoji,
        ),
        true,
    )
    .await
}

/// See the crowdfunds that are open.
#[poise::command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let cfg = ctx.data().economy.settings(guild).config;
    match db::open_funds(&pool, guild).await {
        Ok(funds) => say(ctx, fund_list_text(&funds, &cfg), true).await,
        Err(err) => {
            tracing::warn!("economy: listing the funds in {guild} failed: {err}");
            say(ctx, ECONOMY_READ_FAILED, true).await
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::wallet::{ActionPayload, ActionState};
    use chrono::DateTime;

    fn row(id: i64, kind: ActionKind, target: u64) -> ActionRow {
        ActionRow {
            id,
            kind,
            buyer: UserId::new(10),
            target: Some(UserId::new(target)),
            payload: ActionPayload::Mute,
            state: ActionState::Funding,
            expires_at: DateTime::from_timestamp(1_800_000_000, 0),
            note: None,
            signed: false,
            revealed_at: None,
        }
    }

    #[test]
    fn fund_kinds_are_exactly_the_fundable_pranks() {
        let kinds = <FundKind as poise::ChoiceParameter>::list();
        let names: Vec<&str> = kinds.iter().map(|c| c.name.as_ref()).collect();
        let want: Vec<&str> = ActionKind::FUNDABLE.iter().map(|k| k.label()).collect();
        assert_eq!(names, want);
        for (i, k) in ActionKind::FUNDABLE.iter().enumerate() {
            let fk = <FundKind as poise::ChoiceParameter>::from_index(i).unwrap();
            assert_eq!(fk.kind(), *k);
        }
        assert!(!names.contains(&"Drag"));
    }

    #[test]
    fn pledge_replies_say_the_fate_of_a_filled_fund() {
        let r = |fate| pledge_reply_text(40, 90, 120, 10, fate, "credits", "🪙");
        assert_eq!(
            r(None),
            "Pledged 40 🪙 credits (90 of 120). You have 10 🪙 credits."
        );
        assert_eq!(
            r(Some(Ok(()))),
            "Pledged 40 🪙 credits (90 of 120). The fund is full, and it landed. You have 10 🪙 credits."
        );
        assert_eq!(
            r(Some(Err("they aren't in voice"))),
            "Pledged 40 🪙 credits (90 of 120). The fund is full, but it couldn't land (they aren't in voice). Everyone was refunded. You have 10 🪙 credits."
        );
    }

    #[test]
    fn fund_labels_and_the_list_read_plainly() {
        assert_eq!(
            fund_label(ActionKind::Mute, "bob", 30, 90),
            "Mute on bob: 30/90"
        );
        let cfg = EconomyConfig::default();
        assert_eq!(
            fund_list_text(&[], &cfg),
            "**Open crowdfunds**\nNo crowdfunds are open."
        );
        let list = fund_list_text(
            &[
                (row(2, ActionKind::Mute, 20), 30, 50),
                (row(1, ActionKind::Rename, 21), 5, 120),
            ],
            &cfg,
        );
        assert_eq!(
            list,
            "**Open crowdfunds**\nMute on <@20>: 30/50 🪙 credits · ends <t:1800000000:R>\nRename on <@21>: 5/120 🪙 credits · ends <t:1800000000:R>"
        );
    }

    #[test]
    fn the_picker_filters_on_what_is_typed() {
        let label = "Mute on Bob: 30/90";
        assert!(label_matches(label, ""));
        assert!(label_matches(label, "bob"));
        assert!(label_matches(label, "  MUTE "));
        assert!(!label_matches(label, "alice"));
        assert!(!label_matches(label, "rename"));
    }

    #[test]
    fn the_fund_list_caps_and_fits() {
        let cfg = EconomyConfig::default();
        let many: Vec<_> = (0..20)
            .map(|i| (row(i, ActionKind::Mute, 100 + i as u64), 1, 90))
            .collect();
        let list = fund_list_text(&many, &cfg);
        assert_eq!(list.lines().count(), 1 + LIST_MAX_FUNDS + 1);
        assert!(list.ends_with("…and 5 more"), "{list}");
        // A very long currency name pushes lines out, and it still fits.
        let wide = EconomyConfig {
            currency_name: "x".repeat(200),
            ..EconomyConfig::default()
        };
        let list = fund_list_text(&many, &wide);
        assert!(discord_len(&list) <= CONTENT_MAX);
        assert!(list.contains("more"), "{list}");
    }
}
