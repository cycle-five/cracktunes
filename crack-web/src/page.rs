//! The dashboard's HTML. Two small pages, written by hand: every inserted
//! string goes through [`esc`], and the queue itself is not HTML at all --
//! it is inlined as JSON and drawn by `app.js`, the one renderer.

use crate::{
    backend::GuildEntry,
    history::HistoryPage,
    settings::{EconomySettingsView, ModLogSettingsView, RoomSettingsView},
    view::PageState,
};
use crack_core::{
    messaging::{
        message::REPO_LINK,
        messages::{
            DASH_ECONOMY_ANNOUNCE_CHANNEL, DASH_ECONOMY_ANNOUNCE_STATUS, DASH_ECONOMY_CAMERA,
            DASH_ECONOMY_CAMERA_NOTE, DASH_ECONOMY_COIN, DASH_ECONOMY_COIN_NOTE,
            DASH_ECONOMY_CURRENCY_EMOJI, DASH_ECONOMY_CURRENCY_NAME, DASH_ECONOMY_ENABLED,
            DASH_ECONOMY_GAMBLING, DASH_ECONOMY_GAMBLING_NOTE, DASH_ECONOMY_GUIDE,
            DASH_ECONOMY_IMMUNITY_HOURS, DASH_ECONOMY_IMMUNITY_MODS, DASH_ECONOMY_IMMUNITY_OPTOUT,
            DASH_ECONOMY_IMMUNITY_SHOP, DASH_ECONOMY_JAIL_CHANNEL, DASH_ECONOMY_JAIL_NOTE,
            DASH_ECONOMY_MAX_BET, DASH_ECONOMY_MESSAGE_COOLDOWN, DASH_ECONOMY_MESSAGE_RATE,
            DASH_ECONOMY_NO_CHANNEL_OPTION, DASH_ECONOMY_OPTOUT_COOLDOWN, DASH_ECONOMY_PREFLIGHT,
            DASH_ECONOMY_PREFLIGHT_READY, DASH_ECONOMY_PREFLIGHT_UNKNOWN, DASH_ECONOMY_PRICES,
            DASH_ECONOMY_PRICE_COST, DASH_ECONOMY_PRICE_FOR_SALE, DASH_ECONOMY_RENAME_HOURS,
            DASH_ECONOMY_SHORT_EFFECT, DASH_ECONOMY_TITLE, DASH_ECONOMY_VOICE_RATE,
            DASH_LANDING_DESCRIPTION, DASH_LANDING_TITLE, DASH_MODLOG_CHANNEL, DASH_MODLOG_ENABLED,
            DASH_MODLOG_GUIDE, DASH_MODLOG_NO_CHANNEL_OPTION, DASH_MODLOG_NO_CHANNEL_YET,
            DASH_MODLOG_TITLE, DASH_SETTINGS_ENABLED, DASH_SETTINGS_GRACE, DASH_SETTINGS_GUIDE,
            DASH_SETTINGS_HUB, DASH_SETTINGS_LIMIT, DASH_SETTINGS_LINK,
            DASH_SETTINGS_NO_HUB_OPTION, DASH_SETTINGS_PERMISSIONS, DASH_SETTINGS_PREVIEW,
            DASH_SETTINGS_SAVE, DASH_SETTINGS_SAVED, DASH_SETTINGS_SAVING_FAILED,
            DASH_SETTINGS_TEMPLATE, DASH_SETTINGS_TITLE, DASH_SIGN_IN, ECONOMY_DOCS_URL,
            JOIN_VOICE_FOR_CONTROLS, MODLOG_STATUS_READY, MODLOG_STATUS_UNKNOWN, PATREON_URL,
            PREMIUM_CONTROLS, PREMIUM_HISTORY, ROOM_STATUS_NO_HUB_YET, ROOM_STATUS_READY,
            ROOM_STATUS_UNKNOWN,
        },
    },
    rooms::settings::{render_room_name, GRACE_CHOICES, ROOM_NAME_MAX},
};
use serde::Serialize;
use serenity::all::GuildId;

/// Escape text for HTML element content and double-quoted attributes.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// JSON safe to place inside `<script type="application/json">`. Every `<`
/// becomes `<`, so no string -- a track title of `</script>` included --
/// can end the element. JSON.parse reads it back unchanged.
pub fn inline_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v)
        .expect("the view types serialize infallibly")
        .replace('<', "\\u003c")
}

/// Scripts every page but the history page loads.
const QUEUE_SCRIPTS: &[&str] = &["sortable.min.js", "app.js", "find.js"];
/// The history page: `app.js` for the logout button, `history.js` for the rest.
const HISTORY_SCRIPTS: &[&str] = &["app.js", "history.js"];
/// The settings page: `app.js` for the logout button, `settings.js` for the rest.
const SETTINGS_SCRIPTS: &[&str] = &["app.js", "settings.js"];

fn layout(title: &str, body: &str, scripts: &[&str]) -> String {
    let scripts: String = scripts
        .iter()
        .map(|s| format!("<script src=\"/assets/{s}\"></script>"))
        .collect();
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title><link rel=\"stylesheet\" href=\"/assets/app.css\"></head>\
<body><header><a class=\"brand\" href=\"/\">Crack Tunes</a>\
<button type=\"button\" id=\"logout\">Log out</button></header>\
<main>{body}</main>{scripts}</body></html>",
        title = esc(title),
    )
}

/// `GET /` signed out: what the dashboard is and a way in. It is also the
/// card Discord shows when someone pastes the address (#589), so it carries
/// Open Graph tags with absolute URLs under `origin`. Its own head: no logout
/// button and no scripts.
pub fn landing_page(origin: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title><meta name=\"description\" content=\"{description}\">\
<meta property=\"og:type\" content=\"website\">\
<meta property=\"og:site_name\" content=\"CrackTunes\">\
<meta property=\"og:title\" content=\"{title}\">\
<meta property=\"og:description\" content=\"{description}\">\
<meta property=\"og:url\" content=\"{origin}/\">\
<meta property=\"og:image\" content=\"{origin}/assets/og.webp\">\
<meta property=\"og:image:width\" content=\"1200\">\
<meta property=\"og:image:height\" content=\"630\">\
<meta name=\"twitter:card\" content=\"summary_large_image\">\
<link rel=\"stylesheet\" href=\"/assets/app.css\"></head>\
<body><header><a class=\"brand\" href=\"/\">Crack Tunes</a></header>\
<main><h1>{title}</h1><p>{description}</p>\
<p><a class=\"button\" href=\"/auth/login?return_to=%2F\">{sign_in}</a></p></main>\
</body></html>",
        title = esc(DASH_LANDING_TITLE),
        description = esc(DASH_LANDING_DESCRIPTION),
        origin = esc(origin),
        sign_in = esc(DASH_SIGN_IN),
    )
}

/// `GET /`: the guilds this user can open.
pub fn picker_page(username: &str, guilds: &[GuildEntry]) -> String {
    let items: String = if guilds.is_empty() {
        "<p class=\"empty\">The bot is not playing in any server you are in.</p>".to_owned()
    } else {
        let li: String = guilds
            .iter()
            .map(|g| {
                format!(
                    "<li><a href=\"/g/{id}\">{name}</a>{chan}</li>",
                    id = g.id,
                    name = esc(&g.name),
                    chan = g
                        .channel
                        .as_deref()
                        .map(|c| format!(" <span class=\"chan\">🔊 {}</span>", esc(c)))
                        .unwrap_or_default(),
                )
            })
            .collect();
        format!("<ul class=\"guilds\">{li}</ul>")
    };
    layout(
        "Crack Tunes",
        &format!("<h1>Hi, {}</h1>{items}", esc(username)),
        QUEUE_SCRIPTS,
    )
}

/// `GET /g/{id}`: the queue page. The state is inlined; `app.js` draws it.
/// `history_link` is for managers (Manage Server).
pub fn queue_page(
    guild_name: &str,
    guild_id: GuildId,
    state: &PageState,
    history_link: bool,
) -> String {
    let links = if history_link {
        format!(
            "<p class=\"links\"><a href=\"/g/{guild_id}/history\">History</a> · \
<a href=\"/g/{guild_id}/settings\">{settings}</a></p>",
            settings = esc(DASH_SETTINGS_LINK),
        )
    } else {
        String::new()
    };
    layout(
        guild_name,
        &format!(
            "<section id=\"dash\" data-guild=\"{guild_id}\">\
<h1>{name}</h1>{links}<p id=\"badge\" hidden>Reconnecting…</p><p id=\"note\" hidden></p>\
<div id=\"controls\" hidden>\
<button type=\"button\" id=\"c-pause\"></button>\
<button type=\"button\" id=\"c-skip\">⏭ Skip</button>\
<button type=\"button\" id=\"c-shuffle\">🔀 Shuffle</button>\
<button type=\"button\" id=\"c-repeat\" aria-pressed=\"false\">🔁 Repeat</button>\
</div>\
<p id=\"premium-controls\" hidden>{premium} <a href=\"{patreon}\">CrackTunes Patreon</a></p>\
<p id=\"voice-controls\" hidden>{voice}</p>\
<form id=\"find\" role=\"search\" autocomplete=\"off\">\
<input type=\"search\" id=\"find-q\" name=\"q\" minlength=\"2\" maxlength=\"100\" \
placeholder=\"Find a track\" aria-label=\"Find a track\">\
<button type=\"submit\" id=\"find-go\">Search</button>\
<fieldset id=\"find-src\"><legend>Search in</legend>\
<label class=\"chip\"><input type=\"checkbox\" value=\"queue\" checked> Queue</label>\
<label class=\"chip\"><input type=\"checkbox\" value=\"history\" checked> History</label>\
<label class=\"chip\"><input type=\"checkbox\" value=\"youtube\" checked> YouTube</label>\
<label class=\"chip\"><input type=\"checkbox\" value=\"soundcloud\" checked> SoundCloud</label>\
</fieldset></form><div id=\"results\" aria-live=\"polite\"></div>\
<div id=\"now\"></div><h2>Up next</h2><ol id=\"upcoming\"></ol></section>\
<script type=\"application/json\" id=\"initial\">{json}</script>",
            name = esc(guild_name),
            premium = esc(PREMIUM_CONTROLS),
            patreon = esc(PATREON_URL),
            voice = esc(JOIN_VOICE_FOR_CONTROLS),
            json = inline_json(state),
        ),
        QUEUE_SCRIPTS,
    )
}

/// `GET /g/{id}/history`: the first page inlined; `history.js` draws it,
/// filters it, polls for new rows and loads older ones.
pub fn history_page(guild_name: &str, guild_id: GuildId, page: &HistoryPage) -> String {
    const ACTIONS: [&str; 11] = [
        "add", "remove", "move", "skip", "clear", "shuffle", "stop", "pause", "resume", "leave",
        "repeat",
    ];
    let actions: String = ACTIONS
        .iter()
        .map(|a| format!("<option value=\"{a}\">{a}</option>"))
        .collect();
    layout(
        &format!("History · {guild_name}"),
        &format!(
            "<section id=\"history\" data-guild=\"{guild_id}\">\
<h1>History · {name}</h1>\
<p class=\"links\"><a href=\"/g/{guild_id}\">← Queue</a></p>\
<p id=\"badge\" hidden>Reconnecting…</p><p id=\"note\" hidden></p>\
<p id=\"gp-note\" hidden>Entries from the running /gp game are hidden until it ends.</p>\
<div id=\"filters\">\
<label>Action <select id=\"f-action\"><option value=\"\">all</option>{actions}</select></label>\
<label>Source <select id=\"f-source\"><option value=\"\">all</option>\
<option value=\"slash\">slash</option><option value=\"prefix\">prefix</option>\
<option value=\"web\">dashboard</option><option value=\"button\">button</option><option value=\"bot\">bot</option></select></label>\
<label>Since <select id=\"f-since\"><option value=\"\">all</option>\
<option value=\"1h\">1 h</option><option value=\"6h\">6 h</option>\
<option value=\"1d\">1 d</option><option value=\"1w\">1 w</option></select></label>\
<span id=\"f-user\" class=\"chip\" hidden><span id=\"f-user-name\"></span>\
<button type=\"button\" id=\"f-user-clear\" aria-label=\"Clear the member filter\">×</button></span>\
</div><ol id=\"rows\"></ol>\
<button type=\"button\" id=\"older\" hidden>Load older</button>\
<p id=\"premium-note\"{premium_hidden}>{premium} <a href=\"{patreon}\">CrackTunes Patreon</a></p></section>\
<script type=\"application/json\" id=\"initial\">{json}</script>",
            name = esc(guild_name),
            premium_hidden = if page.capped { "" } else { " hidden" },
            premium = esc(PREMIUM_HISTORY),
            patreon = esc(PATREON_URL),
            json = inline_json(page),
        ),
        HISTORY_SCRIPTS,
    )
}

/// `GET /g/{id}/settings`: every rooms option, server-rendered with escaped
/// values, and the permissions CrackTunes is missing. `settings.js` saves it.
pub fn settings_page(
    guild_name: &str,
    guild_id: GuildId,
    view: &RoomSettingsView,
    modlog: &ModLogSettingsView,
    economy: &EconomySettingsView,
) -> String {
    let check = |on: bool| if on { " checked" } else { "" };
    let price_rows: String = economy
        .prices
        .iter()
        .map(|p| {
            // The hint rides in the label cell, so the script's row-by-row
            // read of prices never meets a row without an input.
            let hint = p.hint.as_deref().map_or_else(String::new, |h| {
                format!("<br><small class=\"e-hint\">{}</small>", esc(h))
            });
            format!(
                "<tr data-kind=\"{kind}\"><td>{label}{hint}</td>\
<td><input type=\"checkbox\" class=\"e-price-on\" aria-label=\"{for_sale}\"{on}></td>\
<td><input type=\"number\" class=\"e-price\" aria-label=\"{cost}\" min=\"1\" max=\"1000000\" value=\"{price}\"></td></tr>",
                kind = esc(&p.kind),
                label = esc(&p.label),
                for_sale = esc(DASH_ECONOMY_PRICE_FOR_SALE),
                cost = esc(DASH_ECONOMY_PRICE_COST),
                on = check(p.on),
                price = p.price,
            )
        })
        .collect();
    let channel_options =
        |list: &[crate::settings::ChannelOption], chosen: &Option<String>| -> String {
            list.iter()
                .map(|c| {
                    format!(
                        "<option value=\"{id}\"{sel}>{name}</option>",
                        id = esc(&c.id),
                        sel = if chosen.as_deref() == Some(c.id.as_str()) {
                            " selected"
                        } else {
                            ""
                        },
                        name = esc(&c.name),
                    )
                })
                .collect()
        };
    let jail_options = channel_options(&economy.voice_channels, &economy.jail_channel);
    let announce_options = channel_options(&economy.text_channels, &economy.announce_channel);
    let lines = |needs: &Option<Vec<String>>| match needs {
        None => esc(DASH_ECONOMY_PREFLIGHT_UNKNOWN),
        Some(l) if l.is_empty() => esc(DASH_ECONOMY_PREFLIGHT_READY),
        Some(l) => l.iter().map(|f| esc(f)).collect::<Vec<_>>().join("; "),
    };
    let preflight_rows: String = economy
        .preflight
        .iter()
        .map(|a| {
            format!(
                "<li data-kind=\"{kind}\"><strong>{label}</strong>: <span>{text}</span></li>",
                kind = esc(&a.kind),
                label = esc(&a.label),
                text = lines(&a.needs),
            )
        })
        .collect();
    let announce_row = match (&economy.announce_channel, &economy.announce_findings) {
        (None, _) => String::new(),
        (Some(_), found) => format!(
            "<li data-kind=\"announce\"><strong>{label}</strong>: <span>{text}</span></li>",
            label = esc(DASH_ECONOMY_ANNOUNCE_STATUS),
            text = lines(found),
        ),
    };
    // The switch exists only in a coin build; without it the script sends no
    // `coin_withdrawals` and the stored value stays.
    let coin_row = if economy.coin_build {
        format!(
            "<label class=\"check\"><input type=\"checkbox\" id=\"e-coin\"{on}> {text}</label>\
<p id=\"e-coin-note\">{note}</p>",
            on = check(economy.coin_withdrawals),
            text = esc(DASH_ECONOMY_COIN),
            note = esc(DASH_ECONOMY_COIN_NOTE),
        )
    } else {
        String::new()
    };
    let economy_html = format!(
        "<h2>{title}</h2>\
<form id=\"economy-form\" autocomplete=\"off\">\
<label class=\"check\"><input type=\"checkbox\" id=\"e-enabled\"{enabled}> {enabled_text}</label>\
<label>{name_text} <input type=\"text\" id=\"e-name\" maxlength=\"24\" value=\"{name}\"></label>\
<label>{emoji_text} <input type=\"text\" id=\"e-emoji\" maxlength=\"64\" value=\"{emoji}\"></label>\
<label>{voice_text} <input type=\"number\" id=\"e-voice\" min=\"0\" max=\"100\" value=\"{voice}\"></label>\
<label class=\"check\"><input type=\"checkbox\" id=\"e-camera\"{camera}> {camera_text}</label>\
<p id=\"e-camera-note\">{camera_note}</p>\
<label>{message_text} <input type=\"number\" id=\"e-message\" min=\"0\" max=\"100\" value=\"{message}\"></label>\
<label>{cooldown_text} <input type=\"number\" id=\"e-cooldown\" min=\"10\" max=\"3600\" value=\"{cooldown}\"></label>\
<label>{rename_hours_text} <input type=\"number\" id=\"e-rename-hours\" min=\"1\" max=\"168\" value=\"{rename_hours}\"></label>\
<label>{short_text} <input type=\"number\" id=\"e-short\" min=\"1\" max=\"60\" value=\"{short}\"></label>\
<label>{jail_text} <select id=\"e-jail\"><option value=\"\">{no_channel}</option>{jail_options}</select></label>\
<p id=\"e-jail-note\">{jail_note}</p>\
<label>{announce_text} <select id=\"e-announce\"><option value=\"\">{no_channel}</option>{announce_options}</select></label>\
<h3>{prices_text}</h3><table id=\"e-prices\"><tbody>{price_rows}</tbody></table>\
<label class=\"check\"><input type=\"checkbox\" id=\"e-optout\"{optout}> {optout_text}</label>\
<label class=\"check\"><input type=\"checkbox\" id=\"e-shop\"{shop}> {shop_text}</label>\
<label class=\"check\"><input type=\"checkbox\" id=\"e-mods\"{mods}> {mods_text}</label>\
<label>{optout_days_text} <input type=\"number\" id=\"e-optout-days\" min=\"0\" max=\"90\" value=\"{optout_days}\"></label>\
<label>{hours_text} <input type=\"number\" id=\"e-hours\" min=\"1\" max=\"720\" value=\"{hours}\"></label>\
<label class=\"check\"><input type=\"checkbox\" id=\"e-gambling\"{gambling}> {gambling_text}</label>\
<p id=\"e-gambling-note\">{gambling_note}</p>\
<label>{max_bet_text} <input type=\"number\" id=\"e-max-bet\" min=\"1\" max=\"1000000\" value=\"{max_bet}\"></label>\
{coin_row}\
<button type=\"submit\" id=\"e-save\">{save}</button>\
<p id=\"e-note\" role=\"status\" hidden></p></form>\
<h3>{preflight_text}</h3><ul id=\"e-preflight\" data-ready=\"{ready_text}\" data-unknown=\"{unknown_text}\" \
data-announce=\"{announce_status}\">{preflight_rows}{announce_row}</ul>\
<p class=\"links\"><a href=\"{guide}\">{guide_text}</a></p>",
        title = esc(DASH_ECONOMY_TITLE),
        coin_row = coin_row,
        enabled = check(economy.enabled),
        enabled_text = esc(DASH_ECONOMY_ENABLED),
        name_text = esc(DASH_ECONOMY_CURRENCY_NAME),
        name = esc(&economy.currency_name),
        emoji_text = esc(DASH_ECONOMY_CURRENCY_EMOJI),
        emoji = esc(&economy.currency_emoji),
        voice_text = esc(DASH_ECONOMY_VOICE_RATE),
        voice = economy.voice_credits_per_minute,
        camera = check(economy.voice_requires_camera),
        camera_text = esc(DASH_ECONOMY_CAMERA),
        camera_note = esc(DASH_ECONOMY_CAMERA_NOTE),
        message_text = esc(DASH_ECONOMY_MESSAGE_RATE),
        message = economy.message_credits,
        cooldown_text = esc(DASH_ECONOMY_MESSAGE_COOLDOWN),
        cooldown = economy.message_cooldown_seconds,
        prices_text = esc(DASH_ECONOMY_PRICES),
        optout = check(economy.immunity_optout),
        optout_text = esc(DASH_ECONOMY_IMMUNITY_OPTOUT),
        shop = check(economy.immunity_shop),
        shop_text = esc(DASH_ECONOMY_IMMUNITY_SHOP),
        mods = check(economy.immunity_mods),
        mods_text = esc(DASH_ECONOMY_IMMUNITY_MODS),
        optout_days_text = esc(DASH_ECONOMY_OPTOUT_COOLDOWN),
        optout_days = economy.optout_cooldown_days,
        hours_text = esc(DASH_ECONOMY_IMMUNITY_HOURS),
        hours = economy.bought_immunity_hours,
        gambling = check(economy.gambling_enabled),
        gambling_text = esc(DASH_ECONOMY_GAMBLING),
        gambling_note = esc(DASH_ECONOMY_GAMBLING_NOTE),
        max_bet_text = esc(DASH_ECONOMY_MAX_BET),
        max_bet = economy.max_bet,
        rename_hours_text = esc(DASH_ECONOMY_RENAME_HOURS),
        rename_hours = economy.rename_hours,
        short_text = esc(DASH_ECONOMY_SHORT_EFFECT),
        short = economy.short_effect_minutes,
        jail_text = esc(DASH_ECONOMY_JAIL_CHANNEL),
        no_channel = esc(DASH_ECONOMY_NO_CHANNEL_OPTION),
        jail_options = jail_options,
        jail_note = esc(DASH_ECONOMY_JAIL_NOTE),
        announce_text = esc(DASH_ECONOMY_ANNOUNCE_CHANNEL),
        announce_options = announce_options,
        preflight_text = esc(DASH_ECONOMY_PREFLIGHT),
        ready_text = esc(DASH_ECONOMY_PREFLIGHT_READY),
        unknown_text = esc(DASH_ECONOMY_PREFLIGHT_UNKNOWN),
        announce_status = esc(DASH_ECONOMY_ANNOUNCE_STATUS),
        preflight_rows = preflight_rows,
        announce_row = announce_row,
        save = esc(DASH_SETTINGS_SAVE),
        guide = esc(ECONOMY_DOCS_URL),
        guide_text = esc(DASH_ECONOMY_GUIDE),
    );
    let log_channels: String = modlog
        .text_channels
        .iter()
        .map(|c| {
            format!(
                "<option value=\"{id}\"{sel}>{name}</option>",
                id = esc(&c.id),
                sel = if modlog.channel.as_deref() == Some(c.id.as_str()) {
                    " selected"
                } else {
                    ""
                },
                name = esc(&c.name),
            )
        })
        .collect();
    let log_findings = match &modlog.findings {
        None if modlog.channel.is_none() => {
            format!("<p>{}</p>", esc(DASH_MODLOG_NO_CHANNEL_YET))
        },
        None => format!("<p>{}</p>", esc(MODLOG_STATUS_UNKNOWN)),
        Some(list) if list.is_empty() => format!("<p>{}</p>", esc(MODLOG_STATUS_READY)),
        Some(list) => {
            let li: String = list
                .iter()
                .map(|f| format!("<li>{}</li>", esc(f)))
                .collect();
            format!("<ul>{li}</ul>")
        },
    };
    let log_visible = modlog.visibility.as_deref().unwrap_or_default();
    let hubs: String = view
        .voice_channels
        .iter()
        .map(|c| {
            format!(
                "<option value=\"{id}\"{sel}>{name}</option>",
                id = esc(&c.id),
                sel = if view.hub.as_deref() == Some(c.id.as_str()) {
                    " selected"
                } else {
                    ""
                },
                name = esc(&c.name),
            )
        })
        .collect();
    let graces: String = GRACE_CHOICES
        .iter()
        .map(|g| {
            format!(
                "<option value=\"{g}\"{sel}>{g} s</option>",
                sel = if *g == view.empty_grace_secs {
                    " selected"
                } else {
                    ""
                },
            )
        })
        .collect();
    let findings = match &view.findings {
        None if view.hub.is_none() => format!("<p>{}</p>", esc(ROOM_STATUS_NO_HUB_YET)),
        None => format!("<p>{}</p>", esc(ROOM_STATUS_UNKNOWN)),
        Some(list) if list.is_empty() => format!("<p>{}</p>", esc(ROOM_STATUS_READY)),
        Some(list) => {
            let li: String = list
                .iter()
                .map(|f| format!("<li>{}</li>", esc(f)))
                .collect();
            format!("<ul>{li}</ul>")
        },
    };
    layout(
        &format!("{DASH_SETTINGS_TITLE} · {guild_name}"),
        &format!(
            "<section id=\"settings\" data-guild=\"{guild_id}\" data-saved=\"{saved}\" \
data-offline=\"{offline}\">\
<h1>{title} · {name}</h1>\
<p class=\"links\"><a href=\"/g/{guild_id}\">← Queue</a> · <a href=\"{guide}\">{guide_text}</a></p>\
<form id=\"rooms-form\" autocomplete=\"off\">\
<label class=\"check\"><input type=\"checkbox\" id=\"s-enabled\"{enabled}> {enabled_text}</label>\
<label>{hub_text} <select id=\"s-hub\"><option value=\"\">{none}</option>{hubs}</select></label>\
<label>{template_text} <input type=\"text\" id=\"s-template\" maxlength=\"{max}\" value=\"{template}\"></label>\
<p>{preview_text} <output id=\"s-preview\">{preview}</output></p>\
<label>{limit_text} <input type=\"number\" id=\"s-limit\" min=\"0\" max=\"99\" value=\"{limit}\"></label>\
<label>{grace_text} <select id=\"s-grace\">{graces}</select></label>\
<button type=\"submit\" id=\"s-save\">{save}</button>\
<p id=\"s-note\" role=\"status\" hidden></p></form>\
<h2>{permissions}</h2><div id=\"s-findings\" data-ready=\"{ready}\" data-no-hub=\"{no_hub}\" \
data-unknown=\"{unknown}\">{findings}</div>\
<h2>{log_title}</h2>\
<p class=\"links\"><a href=\"{log_guide}\">{log_guide_text}</a></p>\
<form id=\"modlog-form\" autocomplete=\"off\">\
<label class=\"check\"><input type=\"checkbox\" id=\"m-enabled\"{log_on}> {log_on_text}</label>\
<label>{log_channel_text} <select id=\"m-channel\"><option value=\"\">{log_none}</option>{log_channels}</select></label>\
<p id=\"m-visible\">{log_visible}</p>\
<button type=\"submit\" id=\"m-save\">{save}</button>\
<p id=\"m-note\" role=\"status\" hidden></p></form>\
<h3>{permissions}</h3><div id=\"m-findings\" data-ready=\"{log_ready}\" data-no-channel=\"{log_no_channel}\" \
data-unknown=\"{log_unknown}\">{log_findings}</div>{economy_html}</section>",
            saved = esc(DASH_SETTINGS_SAVED),
            offline = esc(DASH_SETTINGS_SAVING_FAILED),
            title = esc(DASH_SETTINGS_TITLE),
            name = esc(guild_name),
            guide = esc(&format!("{REPO_LINK}blob/master/docs/rooms-and-mod-log.md")),
            guide_text = esc(DASH_SETTINGS_GUIDE),
            enabled = if view.enabled { " checked" } else { "" },
            enabled_text = esc(DASH_SETTINGS_ENABLED),
            hub_text = esc(DASH_SETTINGS_HUB),
            none = esc(DASH_SETTINGS_NO_HUB_OPTION),
            template_text = esc(DASH_SETTINGS_TEMPLATE),
            max = ROOM_NAME_MAX,
            template = esc(&view.name_template),
            preview_text = esc(DASH_SETTINGS_PREVIEW),
            preview = esc(&render_room_name(&view.name_template, "Saucey")),
            limit_text = esc(DASH_SETTINGS_LIMIT),
            limit = view.default_limit,
            grace_text = esc(DASH_SETTINGS_GRACE),
            save = esc(DASH_SETTINGS_SAVE),
            permissions = esc(DASH_SETTINGS_PERMISSIONS),
            ready = esc(ROOM_STATUS_READY),
            no_hub = esc(ROOM_STATUS_NO_HUB_YET),
            unknown = esc(ROOM_STATUS_UNKNOWN),
            log_title = esc(DASH_MODLOG_TITLE),
            log_guide = esc(&format!(
                "{REPO_LINK}blob/master/docs/rooms-and-mod-log.md#mod-log"
            )),
            log_guide_text = esc(DASH_MODLOG_GUIDE),
            log_on = if modlog.enabled { " checked" } else { "" },
            log_on_text = esc(DASH_MODLOG_ENABLED),
            log_channel_text = esc(DASH_MODLOG_CHANNEL),
            log_none = esc(DASH_MODLOG_NO_CHANNEL_OPTION),
            log_visible = esc(log_visible),
            log_ready = esc(MODLOG_STATUS_READY),
            log_no_channel = esc(DASH_MODLOG_NO_CHANNEL_YET),
            log_unknown = esc(MODLOG_STATUS_UNKNOWN),
            economy_html = economy_html,
        ),
        SETTINGS_SCRIPTS,
    )
}

/// A plain page for 404/503 and similar.
pub fn message_page(title: &str, text: &str) -> String {
    layout(
        title,
        &format!("<h1>{}</h1><p>{}</p>", esc(title), esc(text)),
        QUEUE_SCRIPTS,
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::view::{QueueView, TrackView};
    use crack_core::economy::settings::EconomySettings;
    use uuid::Uuid;

    #[test]
    fn esc_covers_the_five() {
        assert_eq!(
            esc(r#"<a href="x">'&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn a_hostile_title_cannot_end_the_inlined_json() {
        let hostile = "</script><img src=x onerror=alert(1)>";
        let view = QueueView::Playing {
            now: TrackView {
                id: Uuid::from_u128(1),
                title: hostile.to_owned(),
                url: None,
                duration_secs: None,
                requester: None,
            },
            upcoming: vec![],
            rev: 1,
            paused: false,
            looping: false,
        };
        let html = queue_page(
            "G",
            GuildId::new(5),
            &PageState {
                view: &view,
                can_control: false,
                plan: crate::view::PlanView::Free,
            },
            false,
        );
        assert_eq!(
            html.matches("</script>").count(),
            4,
            "only the four real closers: {html}"
        );
        assert!(!html.contains("<img"));
        // And the JSON still parses back to the same title.
        let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
        let end = start + html[start..].find("</script>").unwrap();
        #[derive(serde::Deserialize)]
        struct State {
            view: QueueView,
        }
        let parsed: State = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(parsed.view, view);
    }

    /// The origin comes from the environment, but it lands in attributes:
    /// it is escaped like everything else.
    #[test]
    fn the_landing_page_escapes_its_origin() {
        let html = landing_page("https://x\"><script>");
        assert!(!html.contains("<script>"), "{html}");
        assert!(
            html.contains(r#"content="https://x&quot;&gt;&lt;script&gt;/assets/og.webp""#),
            "{html}"
        );
    }

    #[test]
    fn guild_and_user_names_are_escaped() {
        let html = picker_page(
            "<b>me</b>",
            &[GuildEntry {
                id: GuildId::new(5),
                name: "<i>G</i>".into(),
                channel: Some("<u>c</u>".into()),
            }],
        );
        assert!(!html.contains("<b>me") && !html.contains("<i>G") && !html.contains("<u>c"));
        assert!(html.contains("href=\"/g/5\""));
    }

    #[test]
    fn price_hints_render_under_their_row_escaped() {
        let rooms = RoomSettingsView {
            enabled: false,
            hub: None,
            name_template: String::new(),
            default_limit: 0,
            empty_grace_secs: 0,
            voice_channels: vec![],
            findings: None,
        };
        let modlog = ModLogSettingsView {
            enabled: false,
            channel: None,
            text_channels: vec![],
            findings: None,
            visibility: None,
        };
        let mut economy =
            EconomySettingsView::from_settings(&EconomySettings::off(GuildId::new(5)), false);
        let bounty = economy
            .prices
            .iter_mut()
            .find(|p| p.kind == "bounty")
            .unwrap();
        assert_eq!(bounty.hint.as_deref(), Some("Minimum stake."));
        bounty.hint = Some("<b>Minimum</b> stake.".into());
        let html = settings_page("G", GuildId::new(5), &rooms, &modlog, &economy);
        assert!(
            html.contains(
                "<td>Bounty<br><small class=\"e-hint\">&lt;b&gt;Minimum&lt;/b&gt; stake.</small></td>"
            ),
            "{html}"
        );
        assert!(!html.contains("<b>Minimum"));
        // Rows with no hint stay as they were.
        assert!(html.contains("<td>Rename</td>"), "{html}");
        assert_eq!(html.matches("class=\"e-hint\"").count(), 3);
    }

    #[test]
    fn the_camera_checkbox_renders_in_every_build() {
        let rooms = RoomSettingsView {
            enabled: false,
            hub: None,
            name_template: String::new(),
            default_limit: 0,
            empty_grace_secs: 0,
            voice_channels: vec![],
            findings: None,
        };
        let modlog = ModLogSettingsView {
            enabled: false,
            channel: None,
            text_channels: vec![],
            findings: None,
            visibility: None,
        };
        let page = |on: bool| {
            let mut s = EconomySettings::off(GuildId::new(5));
            s.config.voice_requires_camera = on;
            let economy = EconomySettingsView::from_settings(&s, false);
            settings_page("G", GuildId::new(5), &rooms, &modlog, &economy)
        };
        let off = page(false);
        assert!(
            off.contains("<input type=\"checkbox\" id=\"e-camera\"> "),
            "{off}"
        );
        assert!(off.contains(&*esc(DASH_ECONOMY_CAMERA_NOTE)), "{off}");
        let on = page(true);
        assert!(
            on.contains("<input type=\"checkbox\" id=\"e-camera\" checked> "),
            "{on}"
        );
    }

    #[test]
    fn the_coin_checkbox_renders_only_in_a_coin_build() {
        let rooms = RoomSettingsView {
            enabled: false,
            hub: None,
            name_template: String::new(),
            default_limit: 0,
            empty_grace_secs: 0,
            voice_channels: vec![],
            findings: None,
        };
        let modlog = ModLogSettingsView {
            enabled: false,
            channel: None,
            text_channels: vec![],
            findings: None,
            visibility: None,
        };
        let page = |coin_build: bool, on: bool| {
            let mut s = EconomySettings::off(GuildId::new(5));
            s.config.coin_withdrawals = on;
            let economy = EconomySettingsView::from_settings(&s, coin_build);
            settings_page("G", GuildId::new(5), &rooms, &modlog, &economy)
        };
        let lean = page(false, true);
        assert!(!lean.contains("id=\"e-coin\""), "{lean}");
        assert!(!lean.contains(DASH_ECONOMY_COIN_NOTE));
        let coin = page(true, false);
        assert!(
            coin.contains("<input type=\"checkbox\" id=\"e-coin\"> "),
            "{coin}"
        );
        assert!(coin.contains(DASH_ECONOMY_COIN_NOTE));
        let checked = page(true, true);
        assert!(
            checked.contains("<input type=\"checkbox\" id=\"e-coin\" checked> "),
            "{checked}"
        );
    }

    #[test]
    fn the_premium_note_is_shown_only_when_capped() {
        let note = format!(
            "<p id=\"premium-note\">{} <a href=\"{}\">CrackTunes Patreon</a></p>",
            esc(PREMIUM_HISTORY),
            esc(PATREON_URL)
        );
        let capped = HistoryPage {
            capped: true,
            ..HistoryPage::default()
        };
        let html = history_page("S", GuildId::new(5), &capped);
        assert!(html.contains(&note), "{html}");

        let open = HistoryPage::default();
        let html = history_page("S", GuildId::new(5), &open);
        assert!(html.contains("<p id=\"premium-note\" hidden>"), "{html}");
    }

    #[test]
    fn the_queue_page_carries_the_find_panel_and_its_script() {
        let view = QueueView::Idle;
        let state = PageState {
            view: &view,
            can_control: false,
            plan: crate::view::PlanView::Free,
        };
        let html = queue_page("G", GuildId::new(5), &state, false);
        for needle in [
            "<form id=\"find\"",
            "<input type=\"search\" id=\"find-q\" name=\"q\" minlength=\"2\" maxlength=\"100\"",
            "id=\"find-src\"",
            "value=\"queue\"",
            "value=\"history\"",
            "value=\"youtube\"",
            "value=\"soundcloud\"",
            "<div id=\"results\"",
            "<script src=\"/assets/find.js\"></script>",
        ] {
            assert!(html.contains(needle), "missing {needle}: {html}");
        }
        // find.js runs after app.js, which it reads the live state from.
        assert!(html.find("app.js").unwrap() < html.find("find.js").unwrap());
    }

    #[test]
    fn the_queue_page_carries_the_controls_and_their_premium_note() {
        let view = QueueView::Idle;
        let html = queue_page(
            "G",
            GuildId::new(5),
            &PageState {
                view: &view,
                can_control: false,
                plan: crate::view::PlanView::Free,
            },
            false,
        );
        for id in ["c-pause", "c-skip", "c-shuffle", "c-repeat"] {
            assert!(
                html.contains(&format!("<button type=\"button\" id=\"{id}\"")),
                "{id}: {html}"
            );
        }
        assert!(html.contains("<div id=\"controls\" hidden>"), "{html}");
        let note = format!(
            "<p id=\"premium-controls\" hidden>{} <a href=\"{}\">CrackTunes Patreon</a></p>",
            esc(PREMIUM_CONTROLS),
            esc(PATREON_URL)
        );
        assert!(html.contains(&note), "{html}");
        let voice = format!(
            "<p id=\"voice-controls\" hidden>{}</p>",
            esc(JOIN_VOICE_FOR_CONTROLS)
        );
        assert!(html.contains(&voice), "{html}");
        // Between the note and the now-playing card.
        let at = |needle: &str| html.find(needle).unwrap();
        assert!(at("id=\"note\"") < at("id=\"controls\""));
        assert!(at("id=\"premium-controls\"") < at("id=\"now\""));
        assert!(at("id=\"voice-controls\"") < at("id=\"now\""));
    }
}
