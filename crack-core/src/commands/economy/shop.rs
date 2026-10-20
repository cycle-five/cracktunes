//! `/shop menu|immunity`.
use super::credits::{gate, say};
use crate::economy::settings::{ActionKind, EconomySettings};
use crate::economy::shop::{buy_immunity, refusal_text, BuyError};
use crate::messaging::messages::{
    ECONOMY_DOCS_URL, ECONOMY_IMMUNE_UNTIL, ECONOMY_NO_DATABASE, ECONOMY_PURCHASE_FAILED,
    ECONOMY_SHOP_BOT_BUYER, ECONOMY_SHOP_DOCS_LABEL, ECONOMY_SHOP_NOT_SOLD, ECONOMY_SHOP_TITLE,
    ECONOMY_YOU_HAVE,
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
    }
    lines.push(format!("[{ECONOMY_SHOP_DOCS_LABEL}]({ECONOMY_DOCS_URL})"));
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
    format!("{ECONOMY_YOU_HAVE} {balance} {emoji} {currency}.")
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

/// Buy immunity from pranks for a while.
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
    let currency = ctx.data().economy.settings(guild).config.currency_name;
    match buy_immunity(&ctx.data(), guild, buyer.id, Utc::now()).await {
        Ok(bought) => {
            say(ctx, immune_card_text(buyer.id, bought.until), false).await?;
            let cfg = ctx.data().economy.settings(guild).config;
            say(
                ctx,
                balance_followup_text(bought.balance, &cfg.currency_name, &cfg.currency_emoji),
                true,
            )
            .await
        },
        Err(BuyError::Refused(r)) => say(ctx, refusal_text(&r, &currency), true).await,
        Err(BuyError::NoDatabase) => say(ctx, ECONOMY_NO_DATABASE, true).await,
        Err(BuyError::Db) => say(ctx, ECONOMY_PURCHASE_FAILED, true).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::GuildId;

    #[test]
    fn menu_lists_on_items_with_prices() {
        let s = EconomySettings::off(GuildId::new(1));
        assert_eq!(
            menu_text(&s),
            "**Shop**\nImmunity: 200 🪙 credits\n\
             [How the economy works](https://github.com/cycle-five/cracktunes/blob/master/docs/economy.md)"
        );
        let mut s = s;
        let mut p = ActionKind::Immunity.default_price();
        p.on = false;
        s.config.prices.set(ActionKind::Immunity, p);
        assert!(
            !menu_text(&s).contains("Immunity"),
            "an item that is off is not listed"
        );
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
            "You have 40 🪙 credits."
        );
    }

    #[test]
    fn menu_says_immunity_not_sold_when_the_switch_is_off() {
        let mut s = EconomySettings::off(GuildId::new(1));
        s.config.immunity_shop = false;
        assert_eq!(
            menu_text(&s),
            "**Shop**\nImmunity: not sold here\n\
             [How the economy works](https://github.com/cycle-five/cracktunes/blob/master/docs/economy.md)"
        );
    }
}
