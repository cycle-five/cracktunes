//! The dashboard's HTML. Two small pages, written by hand: every inserted
//! string goes through [`esc`], and the queue itself is not HTML at all --
//! it is inlined as JSON and drawn by `app.js`, the one renderer.

use crate::{backend::GuildEntry, view::PageState};
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

fn layout(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title><link rel=\"stylesheet\" href=\"/assets/app.css\"></head>\
<body><header><a class=\"brand\" href=\"/\">Crack Tunes</a>\
<button type=\"button\" id=\"logout\">Log out</button></header>\
<main>{body}</main><script src=\"/assets/sortable.min.js\"></script>\
<script src=\"/assets/app.js\"></script></body></html>",
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
    )
}

/// `GET /g/{id}`: the queue page. The state is inlined; `app.js` draws it.
pub fn queue_page(guild_name: &str, guild_id: GuildId, state: &PageState) -> String {
    layout(
        guild_name,
        &format!(
            "<section id=\"dash\" data-guild=\"{guild_id}\">\
<h1>{name}</h1><p id=\"badge\" hidden>Reconnecting…</p><p id=\"note\" hidden></p>\
<div id=\"now\"></div><h2>Up next</h2><ol id=\"upcoming\"></ol></section>\
<script type=\"application/json\" id=\"initial\">{json}</script>",
            name = esc(guild_name),
            json = inline_json(state),
        ),
    )
}

/// A plain page for 404/503 and similar.
pub fn message_page(title: &str, text: &str) -> String {
    layout(
        title,
        &format!("<h1>{}</h1><p>{}</p>", esc(title), esc(text)),
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
        };
        let html = queue_page(
            "G",
            GuildId::new(5),
            &PageState {
                view: &view,
                can_control: false,
            },
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
}
