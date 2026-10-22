//! The pure rules and wording behind `/wallet`. Nothing here touches the network or the database.

use super::registry::CoinState;
use super::resolve::RefundReason;
use crate::coin::flow::Outcome;
use crate::db::economy::coin::{WalletOverview, WithdrawalState};
use crate::messaging::messages::{
    COIN_ADDRESS_IS_THE_MINT, COIN_ADDRESS_NOT_BASE58, COIN_ADDRESS_OFF_CURVE,
    COIN_ADDRESS_WRONG_LENGTH, COIN_DEVNET_HINT, COIN_LINK_YOURS, COIN_MINTED, COIN_MINTING,
    COIN_REASON_DEVNET_RESET, COIN_REASON_EXPIRED, COIN_REASON_FAILED_ON_CHAIN,
    COIN_REASON_NEVER_SENT, COIN_REFUNDED, COIN_REFUNDED_BALANCE, COIN_REFUSE_BAD_AMOUNT,
    COIN_REFUSE_NOT_LINKED, COIN_REFUSE_OPTED_OUT, COIN_REFUSE_SPONSOR_CAP, COIN_REFUSE_STARTING,
    COIN_REFUSE_SWITCH_OFF, COIN_REFUSE_TAP_DRY, COIN_REFUSE_UNAVAILABLE, COIN_SHOW_LAST,
    COIN_SHOW_LINKED, COIN_SHOW_MINT, COIN_SHOW_NO_LINK, COIN_SHOW_REASON, COIN_SHOW_TOTAL,
    COIN_STATE_DONE, COIN_STATE_PENDING, COIN_STATE_REFUNDED, COIN_STILL_SETTLING,
};
use crack_coin::Pubkey;
use std::str::FromStr;

/// The authority must hold at least this much (0.01 SOL) to pay for a transfer.
pub const MIN_TAP_LAMPORTS: u64 = 10_000_000;
/// Token accounts we create for one member, per mint.
pub const SPONSORED_ACCOUNTS: i64 = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum AddressRefusal {
    NotBase58,
    WrongLength,
    OffCurve,
    IsTheMint,
}

/// A pasted address, trimmed and checked. Gives the canonical base58 form.
pub fn check_address(raw: &str, mint: Option<&str>) -> Result<String, AddressRefusal> {
    let key = Pubkey::from_str(raw.trim()).map_err(|e| match e {
        crack_coin::Error::Length { .. } => AddressRefusal::WrongLength,
        _ => AddressRefusal::NotBase58,
    })?;
    // A token account (a program derived address) is off the curve: it is not a wallet.
    if !key.is_on_curve() {
        return Err(AddressRefusal::OffCurve);
    }
    let canonical = key.to_string();
    if mint.is_some_and(|m| m.trim() == canonical) {
        return Err(AddressRefusal::IsTheMint);
    }
    Ok(canonical)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Starting,
    Off,
}

pub fn readiness(state: &CoinState) -> Readiness {
    match state {
        CoinState::Starting => Readiness::Starting,
        CoinState::Off(_) => Readiness::Off,
        CoinState::Ready(_) => Readiness::Ready,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum WithdrawRefusal {
    Starting,
    Unavailable,
    SwitchOff,
    OptedOut,
    NotLinked,
    BadAmount,
    TapDry,
    SponsorCapReached,
}

/// The checks that need no I/O, in the order the member should meet them.
pub fn check_withdraw_local(
    readiness: Readiness,
    switch_on: bool,
    opted_out: bool,
    linked: bool,
    amount: i64,
) -> Result<(), WithdrawRefusal> {
    match readiness {
        Readiness::Starting => return Err(WithdrawRefusal::Starting),
        Readiness::Off => return Err(WithdrawRefusal::Unavailable),
        Readiness::Ready => {},
    }
    if !switch_on {
        return Err(WithdrawRefusal::SwitchOff);
    }
    if opted_out {
        return Err(WithdrawRefusal::OptedOut);
    }
    if !linked {
        return Err(WithdrawRefusal::NotLinked);
    }
    if amount < 1 {
        return Err(WithdrawRefusal::BadAmount);
    }
    Ok(())
}

/// The checks that need chain reads. `Ok(true)` means this mint creates the member's token
/// account, which counts against their sponsored accounts.
pub fn check_withdraw_chain(
    tap_lamports: u64,
    has_account: bool,
    sponsored_used: i64,
) -> Result<bool, WithdrawRefusal> {
    if tap_lamports < MIN_TAP_LAMPORTS {
        return Err(WithdrawRefusal::TapDry);
    }
    if has_account {
        return Ok(false);
    }
    if sponsored_used >= SPONSORED_ACCOUNTS {
        return Err(WithdrawRefusal::SponsorCapReached);
    }
    Ok(true)
}

/// `ABCD…WXYZ`: the first four and the last four characters.
pub fn short_address(a: &str) -> String {
    let chars: Vec<char> = a.chars().collect();
    if chars.len() < 8 {
        return a.to_owned();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

pub fn explorer_tx(sig: &str) -> String {
    format!("https://explorer.solana.com/tx/{sig}?cluster=devnet")
}

pub fn explorer_address(a: &str) -> String {
    format!("https://explorer.solana.com/address/{a}?cluster=devnet")
}

pub fn refusal_text(r: &WithdrawRefusal) -> String {
    let text = match r {
        WithdrawRefusal::Starting => COIN_REFUSE_STARTING,
        WithdrawRefusal::Unavailable => COIN_REFUSE_UNAVAILABLE,
        WithdrawRefusal::SwitchOff => COIN_REFUSE_SWITCH_OFF,
        WithdrawRefusal::OptedOut => COIN_REFUSE_OPTED_OUT,
        WithdrawRefusal::NotLinked => COIN_REFUSE_NOT_LINKED,
        WithdrawRefusal::BadAmount => COIN_REFUSE_BAD_AMOUNT,
        WithdrawRefusal::TapDry => COIN_REFUSE_TAP_DRY,
        WithdrawRefusal::SponsorCapReached => COIN_REFUSE_SPONSOR_CAP,
    };
    text.replace("{limit}", &SPONSORED_ACCOUNTS.to_string())
}

pub fn address_refusal_text(r: &AddressRefusal) -> &'static str {
    match r {
        AddressRefusal::NotBase58 => COIN_ADDRESS_NOT_BASE58,
        AddressRefusal::WrongLength => COIN_ADDRESS_WRONG_LENGTH,
        AddressRefusal::OffCurve => COIN_ADDRESS_OFF_CURVE,
        AddressRefusal::IsTheMint => COIN_ADDRESS_IS_THE_MINT,
    }
}

fn refund_reason_text(r: &RefundReason) -> String {
    match r {
        RefundReason::NeverSent => COIN_REASON_NEVER_SENT.to_owned(),
        RefundReason::FailedOnChain(why) => COIN_REASON_FAILED_ON_CHAIN.replace("{why}", why),
        RefundReason::DevnetReset => COIN_REASON_DEVNET_RESET.to_owned(),
        RefundReason::Expired => COIN_REASON_EXPIRED.to_owned(),
        RefundReason::Other(text) => text.clone(),
    }
}

/// The edit that replaces the "Minting…" placeholder. The address is only ever shortened.
pub fn outcome_text(o: &Outcome, amount: i64, address: &str, currency: &str) -> String {
    match o {
        Outcome::Minted { signature } => format!(
            "{}\n{}\n{COIN_DEVNET_HINT}",
            COIN_MINTED
                .replace("{amount}", &amount.to_string())
                .replace("{address}", &short_address(address)),
            explorer_tx(signature)
        ),
        Outcome::Refunded { reason, balance } => {
            let mut text = COIN_REFUNDED
                .replace("{reason}", &refund_reason_text(reason))
                .replace("{amount}", &amount.to_string())
                .replace("{currency}", currency);
            if let Some(balance) = balance {
                text.push(' ');
                text.push_str(
                    &COIN_REFUNDED_BALANCE
                        .replace("{balance}", &balance.to_string())
                        .replace("{currency}", currency),
                );
            }
            text
        },
        Outcome::StillSettling => COIN_STILL_SETTLING.to_owned(),
    }
}

/// The "Minting…" placeholder.
pub fn minting_text(amount: i64, address: &str) -> String {
    COIN_MINTING
        .replace("{amount}", &amount.to_string())
        .replace("{address}", &short_address(address))
}

fn state_label(s: WithdrawalState) -> &'static str {
    match s {
        WithdrawalState::Pending => COIN_STATE_PENDING,
        WithdrawalState::Done => COIN_STATE_DONE,
        WithdrawalState::Refunded => COIN_STATE_REFUNDED,
    }
}

/// `/wallet show`. Only its owner sees it, and the address is still shortened.
pub fn show_text(address: Option<&str>, mint: Option<&str>, overview: &WalletOverview) -> String {
    let mut lines = Vec::new();
    match address {
        // The visible text stays shortened; the full address is only in the link target.
        Some(a) => lines.push(format!(
            "{COIN_SHOW_LINKED} [{}](<{}>)",
            short_address(a),
            explorer_address(a)
        )),
        None => lines.push(format!("{COIN_SHOW_NO_LINK} {COIN_LINK_YOURS}")),
    }
    if let Some(mint) = mint {
        lines.push(format!("{COIN_SHOW_MINT} {}", explorer_address(mint)));
    }
    lines.push(format!("{COIN_SHOW_TOTAL} {}", overview.total_withdrawn));
    if let Some(last) = &overview.last {
        let mut line = format!(
            "{COIN_SHOW_LAST} {} ({})",
            last.amount,
            state_label(last.state)
        );
        if let Some(sig) = &last.signature {
            line.push(' ');
            line.push_str(&explorer_tx(sig));
        }
        lines.push(line);
        if last.state == WithdrawalState::Refunded {
            if let Some(reason) = &last.reason {
                let why = refund_reason_text(&RefundReason::from_db_text(reason));
                lines.push(format!("{COIN_SHOW_REASON} {why}"));
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::economy::coin::{LastWithdrawal, WalletOverview, WithdrawalState};
    use crack_coin::{associated_token_address, Keypair, Pubkey};

    fn owner() -> String {
        Keypair::from_seed([5u8; 32]).pubkey().to_string()
    }

    fn mint() -> String {
        Keypair::from_seed([6u8; 32]).pubkey().to_string()
    }

    #[test]
    fn check_address_accepts_an_owner_key_and_trims() {
        let a = owner();
        assert_eq!(check_address(&a, Some(&mint())), Ok(a.clone()));
        assert_eq!(check_address(&format!(" \n{a}\r\n "), None), Ok(a));
    }

    #[test]
    fn check_address_refuses_the_rest() {
        assert_eq!(check_address("0OIl", None), Err(AddressRefusal::NotBase58));
        // A valid key cut short decodes to fewer than 32 bytes.
        let short = owner()[..40].to_string();
        assert_eq!(
            check_address(&short, None),
            Err(AddressRefusal::WrongLength)
        );
        // A pasted 64-byte secret key is too long, and is never echoed back.
        let secret = crack_coin::Signature([7u8; 64]).to_string();
        assert_eq!(
            check_address(&secret, None),
            Err(AddressRefusal::WrongLength)
        );
        assert!(!address_refusal_text(&AddressRefusal::WrongLength).contains(&secret));
        let m: Pubkey = mint().parse().unwrap();
        let o: Pubkey = owner().parse().unwrap();
        let ata = associated_token_address(&o, &m).to_string();
        assert_eq!(check_address(&ata, None), Err(AddressRefusal::OffCurve));
        assert_eq!(
            check_address(&mint(), Some(&mint())),
            Err(AddressRefusal::IsTheMint)
        );
    }

    #[test]
    fn local_checks_refuse_in_order() {
        let ok = |r, s, o, l, a| check_withdraw_local(r, s, o, l, a);
        assert_eq!(ok(Readiness::Ready, true, false, true, 1), Ok(()));
        assert_eq!(
            ok(Readiness::Starting, true, false, true, 1),
            Err(WithdrawRefusal::Starting)
        );
        assert_eq!(
            ok(Readiness::Off, true, false, true, 1),
            Err(WithdrawRefusal::Unavailable)
        );
        assert_eq!(
            ok(Readiness::Ready, false, false, true, 1),
            Err(WithdrawRefusal::SwitchOff)
        );
        assert_eq!(
            ok(Readiness::Ready, true, true, true, 1),
            Err(WithdrawRefusal::OptedOut)
        );
        assert_eq!(
            ok(Readiness::Ready, true, false, false, 1),
            Err(WithdrawRefusal::NotLinked)
        );
        for amount in [0, -1, i64::MIN] {
            assert_eq!(
                ok(Readiness::Ready, true, false, true, amount),
                Err(WithdrawRefusal::BadAmount)
            );
        }
        assert_eq!(ok(Readiness::Ready, true, false, true, i64::MAX), Ok(()));
        // The first failing check wins.
        assert_eq!(
            ok(Readiness::Off, false, true, false, 0),
            Err(WithdrawRefusal::Unavailable)
        );
        assert_eq!(
            ok(Readiness::Ready, false, true, false, 0),
            Err(WithdrawRefusal::SwitchOff)
        );
        assert_eq!(
            ok(Readiness::Ready, true, true, false, 0),
            Err(WithdrawRefusal::OptedOut)
        );
        assert_eq!(
            ok(Readiness::Ready, true, false, false, 0),
            Err(WithdrawRefusal::NotLinked)
        );
    }

    #[test]
    fn readiness_follows_the_coin_state() {
        use crate::coin::config::CoinOff;
        use crate::coin::registry::CoinState;
        assert_eq!(readiness(&CoinState::Starting), Readiness::Starting);
        assert_eq!(readiness(&CoinState::Off(CoinOff::Mainnet)), Readiness::Off);
        let chain: std::sync::Arc<dyn crate::coin::chain::CoinChain> =
            std::sync::Arc::new(crate::coin::fake::FakeChain::new());
        assert_eq!(readiness(&CoinState::Ready(chain)), Readiness::Ready);
    }

    #[test]
    fn chain_checks() {
        assert_eq!(
            check_withdraw_chain(9_999_999, true, 0),
            Err(WithdrawRefusal::TapDry)
        );
        // The tap is checked first.
        assert_eq!(
            check_withdraw_chain(0, false, 3),
            Err(WithdrawRefusal::TapDry)
        );
        assert_eq!(
            check_withdraw_chain(10_000_000, false, 3),
            Err(WithdrawRefusal::SponsorCapReached)
        );
        assert_eq!(check_withdraw_chain(10_000_000, false, 2), Ok(true));
        assert_eq!(check_withdraw_chain(10_000_000, true, 3), Ok(false));
        assert_eq!(check_withdraw_chain(u64::MAX, true, 0), Ok(false));
    }

    #[test]
    fn short_address_keeps_four_and_four() {
        assert_eq!(short_address("ABCDEFGHIJKLWXYZ"), "ABCD…WXYZ");
        assert_eq!(short_address("ABCDEFGH"), "ABCD…EFGH");
        // Too short to hide anything useful: never print more than it has.
        assert_eq!(short_address("ABC"), "ABC");
    }

    #[test]
    fn outcome_text_links_the_devnet_explorer() {
        let o = Outcome::Minted {
            signature: "SIG123".into(),
        };
        let text = outcome_text(&o, 500, &owner(), "credits");
        assert!(text.contains("https://explorer.solana.com/tx/SIG123?cluster=devnet"));
        assert!(text.contains("500"));
        assert!(text.contains(&short_address(&owner())));
        assert_eq!(
            explorer_address("ADDR"),
            "https://explorer.solana.com/address/ADDR?cluster=devnet"
        );
    }

    #[test]
    fn outcome_texts_never_show_the_full_address() {
        let a = owner();
        let outcomes = [
            Outcome::Minted {
                signature: "S".into(),
            },
            Outcome::StillSettling,
            Outcome::Refunded {
                reason: RefundReason::NeverSent,
                balance: Some(7),
            },
            Outcome::Refunded {
                reason: RefundReason::FailedOnChain("boom".into()),
                balance: None,
            },
            Outcome::Refunded {
                reason: RefundReason::DevnetReset,
                balance: None,
            },
            Outcome::Refunded {
                reason: RefundReason::Expired,
                balance: None,
            },
            Outcome::Refunded {
                reason: RefundReason::Other("hand edit".into()),
                balance: None,
            },
        ];
        for o in &outcomes {
            let text = outcome_text(o, 12, &a, "credits");
            assert!(!text.contains(&a), "{o:?}: {text}");
            assert!(!text.is_empty());
        }
        let refunded = outcome_text(&outcomes[2], 12, &a, "credits");
        assert!(
            refunded.contains("12") && refunded.contains('7'),
            "{refunded}"
        );
        let other = outcome_text(&outcomes[6], 12, &a, "credits");
        assert!(other.contains("hand edit"), "{other}");
    }

    #[test]
    fn show_text_without_a_link_says_how_to_link() {
        let overview = WalletOverview {
            total_withdrawn: 0,
            last: None,
        };
        let text = show_text(None, Some("MINT"), &overview);
        assert!(text.contains("/wallet link"), "{text}");
        let a = owner();
        let with = show_text(Some(&a), Some("MINT"), &overview);
        let link = format!("[{}](<{}>)", short_address(&a), explorer_address(&a));
        assert!(with.contains(&link), "{with}");
        // Outside the link target, the full address never appears.
        assert!(
            !with.replace(&explorer_address(&a), "").contains(&a),
            "{with}"
        );
        assert!(with.contains("MINT"));
    }

    #[test]
    fn show_text_reports_totals_and_the_last_withdrawal() {
        let overview = WalletOverview {
            total_withdrawn: 40,
            last: Some(LastWithdrawal {
                amount: 15,
                state: WithdrawalState::Done,
                signature: Some("SIGX".into()),
                reason: None,
                created_at: chrono::Utc::now(),
            }),
        };
        let text = show_text(Some(&owner()), None, &overview);
        assert!(text.contains("40") && text.contains("15"), "{text}");
        assert!(
            text.contains("cluster=devnet") && text.contains("SIGX"),
            "{text}"
        );
    }

    #[test]
    fn show_text_says_why_the_last_withdrawal_was_refunded() {
        let refunded = |reason: &str| WalletOverview {
            total_withdrawn: 0,
            last: Some(LastWithdrawal {
                amount: 9,
                state: WithdrawalState::Refunded,
                signature: None,
                reason: Some(reason.to_owned()),
                created_at: chrono::Utc::now(),
            }),
        };
        let text = show_text(None, None, &refunded("expired"));
        assert!(text.contains(COIN_REASON_EXPIRED), "{text}");
        let text = show_text(None, None, &refunded("failed on chain: boom"));
        assert!(text.contains("the network rejected it: boom"), "{text}");
        let text = show_text(None, None, &refunded("hand edit"));
        assert!(text.contains("hand edit"), "{text}");
        // A done withdrawal shows no reason line.
        let done = WalletOverview {
            total_withdrawn: 9,
            last: Some(LastWithdrawal {
                amount: 9,
                state: WithdrawalState::Done,
                signature: None,
                reason: Some("stale".into()),
                created_at: chrono::Utc::now(),
            }),
        };
        assert!(!show_text(None, None, &done).contains("stale"));
    }

    #[test]
    fn the_sponsor_cap_text_names_the_constant() {
        let text = refusal_text(&WithdrawRefusal::SponsorCapReached);
        assert!(text.contains(&SPONSORED_ACCOUNTS.to_string()), "{text}");
        assert!(!text.contains("{limit}"));
    }

    #[test]
    fn refusal_texts_are_distinct() {
        use WithdrawRefusal::*;
        let all = [
            Starting,
            Unavailable,
            SwitchOff,
            OptedOut,
            NotLinked,
            BadAmount,
            TapDry,
            SponsorCapReached,
        ];
        let texts: std::collections::HashSet<_> = all.iter().map(refusal_text).collect();
        assert_eq!(texts.len(), all.len());
        use AddressRefusal::*;
        let all = [NotBase58, WrongLength, OffCurve, IsTheMint];
        let texts: std::collections::HashSet<_> = all.iter().map(address_refusal_text).collect();
        assert_eq!(texts.len(), all.len());
    }
}
