//! Member-facing economy commands: `/credits`, `/coinflip`, `/shop` and `/economy`.

pub mod admin;
pub mod bounty;
pub mod coinflip;
pub mod credits;
pub mod crowdfund;
pub mod shop;
#[cfg(feature = "crack-coin")]
pub mod wallet;

/// Every economy command.
pub fn commands() -> Vec<crate::Command> {
    #[cfg_attr(not(feature = "crack-coin"), expect(unused_mut))]
    let mut v = vec![
        credits::credits(),
        coinflip::coinflip(),
        shop::shop(),
        admin::economy(),
        bounty::bounty(),
        crowdfund::crowdfund(),
    ];
    #[cfg(feature = "crack-coin")]
    v.push(wallet::wallet());
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn economy_commands_are_guild_only_slash_commands() {
        let cmds = super::commands();
        let names: Vec<_> = cmds.iter().map(|c| c.name.as_ref()).collect();
        let mut want = vec![
            "credits",
            "coinflip",
            "shop",
            "economy",
            "bounty",
            "crowdfund",
        ];
        // /wallet exists only in a coin build.
        if crate::COIN_BUILD {
            want.push("wallet");
        }
        assert_eq!(names, want);
        assert!(cmds
            .iter()
            .all(|c| c.guild_only && c.slash_action.is_some()));
        let credits = &cmds[0];
        let mut subs: Vec<_> = credits
            .subcommands
            .iter()
            .map(|s| s.name.as_ref())
            .collect();
        subs.sort();
        assert_eq!(
            subs,
            ["balance", "gift", "history", "optin", "optout", "top"]
        );
    }

    fn subs(c: &crate::Command) -> Vec<&str> {
        let mut subs: Vec<_> = c.subcommands.iter().map(|s| s.name.as_ref()).collect();
        subs.sort();
        subs
    }

    #[test]
    fn commands_registry_has_the_new_subcommands() {
        let cmds = super::commands();
        let shop = &cmds[2];
        let mut want = [
            "menu", "immunity", "rename", "lock", "clown", "drag", "mute", "deafen", "jail",
            "mirror", "cleanse", "reveal", "theme", "dedicate", "jump",
        ];
        want.sort();
        assert_eq!(subs(shop), want);
        assert_eq!(shop.subcommands.len(), 15);
        // The paid plays run the music check, and the game blocks them by
        // their qualified names.
        for name in ["dedicate", "jump"] {
            let sub = shop.subcommands.iter().find(|c| c.name == name).unwrap();
            assert!(!sub.checks.is_empty(), "{name} must carry cmd_check_music");
            let qualified = format!("shop {name}");
            assert!(
                crate::commands::music::gp::GP_BLOCKED_COMMANDS.contains(&qualified.as_str()),
                "{qualified}"
            );
        }
        // A paid play is signed by nature: no `signed` to leave off.
        let params_of = |name: &str| -> Vec<String> {
            let sub = shop.subcommands.iter().find(|c| c.name == name).unwrap();
            sub.parameters.iter().map(|p| p.name.to_string()).collect()
        };
        assert_eq!(params_of("dedicate"), ["member", "song", "note"]);
        assert_eq!(params_of("jump"), ["song", "note"]);
        // The theme's note and signature show on its public card.
        let theme = shop.subcommands.iter().find(|c| c.name == "theme").unwrap();
        let described: Vec<_> = theme
            .parameters
            .iter()
            .map(|p| (p.name.as_ref(), p.description.as_deref()))
            .collect();
        assert_eq!(
            described[3..],
            [
                ("note", Some("A note shown on the card")),
                ("signed", Some("Put your name on the card"))
            ]
        );
        let economy = &cmds[3];
        assert_eq!(
            subs(economy),
            ["adjust", "cancel", "grant", "lift", "status"]
        );
        // Every one is a guild-only slash command.
        assert!(shop
            .subcommands
            .iter()
            .chain(&economy.subcommands)
            .all(|c| c.guild_only && c.slash_action.is_some()));
        // Reveal offers its picker.
        let reveal = shop
            .subcommands
            .iter()
            .find(|c| c.name == "reveal")
            .unwrap();
        assert!(reveal
            .parameters
            .iter()
            .all(|p| p.autocomplete_callback.is_some()));
        assert_eq!(reveal.parameters.len(), 1);
        // /bounty places and lists, and place takes the member and the stake.
        let bounty = &cmds[4];
        assert_eq!(subs(bounty), ["list", "place"]);
        assert!(bounty
            .subcommands
            .iter()
            .all(|c| c.guild_only && c.slash_action.is_some()));
        let place = bounty
            .subcommands
            .iter()
            .find(|c| c.name == "place")
            .unwrap();
        let params: Vec<_> = place.parameters.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(params, ["member", "amount", "note", "signed"]);
        // /crowdfund starts, pledges and lists; start's choices are exactly
        // the fundable pranks, and pledge offers its picker.
        let fund = &cmds[5];
        assert_eq!(subs(fund), ["list", "pledge", "start"]);
        assert!(fund
            .subcommands
            .iter()
            .all(|c| c.guild_only && c.slash_action.is_some()));
        let start = fund.subcommands.iter().find(|c| c.name == "start").unwrap();
        let params: Vec<_> = start.parameters.iter().map(|p| p.name.as_ref()).collect();
        // The starter's pledge is required (Discord lists required options
        // first): opening a fund is never free.
        assert_eq!(
            params,
            ["action", "member", "pledge", "name", "note", "signed"]
        );
        assert!(start.parameters[2].required);
        assert!(start.parameters[3..].iter().all(|p| !p.required));
        let choices: Vec<_> = start.parameters[0]
            .choices
            .iter()
            .map(|c| c.name.as_ref())
            .collect();
        let want: Vec<_> = crate::economy::settings::ActionKind::FUNDABLE
            .iter()
            .map(|k| k.label())
            .collect();
        assert_eq!(choices, want);
        let pledge = fund
            .subcommands
            .iter()
            .find(|c| c.name == "pledge")
            .unwrap();
        let params: Vec<_> = pledge.parameters.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(params, ["fund", "amount", "signed"]);
        assert!(pledge.parameters[0].autocomplete_callback.is_some());
    }

    #[cfg(feature = "crack-coin")]
    #[test]
    fn wallet_has_its_four_subcommands() {
        let cmds = super::commands();
        let wallet = cmds.iter().find(|c| c.name == "wallet").unwrap();
        assert_eq!(subs(wallet), ["link", "show", "unlink", "withdraw"]);
        assert!(wallet
            .subcommands
            .iter()
            .all(|c| c.guild_only && c.slash_action.is_some()));
        let withdraw = wallet
            .subcommands
            .iter()
            .find(|c| c.name == "withdraw")
            .unwrap();
        assert_eq!(withdraw.parameters.len(), 1);
        assert_eq!(withdraw.parameters[0].name, "amount");
    }
}
