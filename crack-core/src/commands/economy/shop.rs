//! `/shop menu|immunity`.
use super::credits::{gate, say};
use crate::economy::settings::{ActionKind, EconomySettings};
use crate::economy::shop::{buy_immunity, refusal_text, BuyError};
use crate::messaging::messages::{
    ECONOMY_DOCS_URL, ECONOMY_IMMUNE_UNTIL, ECONOMY_NO_DATABASE, ECONOMY_PURCHASE_FAILED,
    ECONOMY_SHOP_BOT_BUYER, ECONOMY_SHOP_DOCS_LABEL, ECONOMY_SHOP_IMMUNITY_NOTE,
    ECONOMY_SHOP_NOT_SOLD, ECONOMY_SHOP_TITLE, ECONOMY_YOU_HAVE,
};
use crate::{Context, Error};
use chrono::{DateTime, Utc};
use serenity::all::UserId;

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

/// What the shop sells and for how much.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Economy",
    slash_command,
    guild_only,
    subcommands("menu", "immunity"),
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
        Err(BuyError::Refused(r)) => say(ctx, refusal_text(&r, &cfg.currency_name), true).await,
        Err(BuyError::NoDatabase) => say(ctx, ECONOMY_NO_DATABASE, true).await,
        Err(BuyError::Db) => say(ctx, ECONOMY_PURCHASE_FAILED, true).await,
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
}
