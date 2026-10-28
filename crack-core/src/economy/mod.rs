//! Per-server credits: earned in voice and by chatting, spent in the shop.
//! Off by default; see `economy::settings`.
//!
//! The economy itself lives in the `crack-economy` crate, re-exported here so
//! every old `crate::economy::…` path keeps working. What stays in this crate
//! is the glue that needs poise, songbird or the rooms: the shop's buttons,
//! theme playback (`walk_on`) and the target facts (`facts`).

pub use crack_economy::{
    bounty, cards, crowdfund, discord, earn, effects, gamble, immunity, ledger, load_settings,
    lock, mod_cancel, mod_grant, mod_lift, opt_out, preflight, protection, record_process_start,
    registry, save_settings, settings, shop, target, theme, wallet, EconomyData, EconomySaveError,
    GrantError, OptOutError,
};

use crate::Data;
use crack_economy::discord::{LiveEconomyDiscord, LiveShopDiscord};
use crack_rooms::modlog::live::ShopChangeSink;
use serenity::all::{Cache, Http};
use std::sync::Arc;

/// The one live door the shop acts through. Every change it makes reaches the
/// mod log as a literal line, with CrackTunes as the actor and no buyer.
// live-Discord glue: builds the live door from the real `Http` and `Cache`, which no unit test has.
#[cfg_attr(test, mutants::skip)]
pub fn live_discord(data: &Data, http: Arc<Http>, cache: Arc<Cache>) -> LiveShopDiscord {
    let transport = Arc::new(crack_common::transport::DiscordTransport {
        http: http.clone(),
        cache,
    });
    LiveEconomyDiscord::logged(
        http,
        Arc::new(ShopChangeSink {
            data: data.rooms_data(),
            transport,
        }),
    )
}

pub mod buttons;
pub mod facts;
pub mod walk_on;
