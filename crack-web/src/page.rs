//! The dashboard's HTML. Two small pages, written by hand: every inserted
//! string goes through [`esc`], and the queue itself is not HTML at all --
//! it is inlined as JSON and drawn by `app.js`, the one renderer.

use crate::{backend::GuildEntry, history::HistoryPage, view::PageState};
use crack_core::messaging::messages::{PATREON_URL, PREMIUM_HISTORY};
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
const QUEUE_SCRIPTS: &[&str] = &["sortable.min.js", "app.js"];
/// The history page: `app.js` for the logout button, `history.js` for the rest.
const HISTORY_SCRIPTS: &[&str] = &["app.js", "history.js"];

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
        format!("<p class=\"links\"><a href=\"/g/{guild_id}/history\">History</a></p>")
    } else {
        String::new()
    };
    layout(
        guild_name,
        &format!(
            "<section id=\"dash\" data-guild=\"{guild_id}\">\
<h1>{name}</h1>{links}<p id=\"badge\" hidden>Reconnecting…</p><p id=\"note\" hidden></p>\
<div id=\"now\"></div><h2>Up next</h2><ol id=\"upcoming\"></ol></section>\
<script type=\"application/json\" id=\"initial\">{json}</script>",
            name = esc(guild_name),
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
<option value=\"web\">dashboard</option><option value=\"bot\">bot</option></select></label>\
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
            },
            false,
        );
        assert_eq!(
            html.matches("</script>").count(),
            3,
            "only the three real closers: {html}"
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
}
