//! The `page` command: one self-contained HTML review page.

pub mod model;

use model::PageModel;

const HTML: &str = include_str!("assets/page.html");
const CSS: &str = include_str!("assets/page.css");
const JS: &str = include_str!("assets/page.js");

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The largest page the host will serve.
pub const MAX_PAGE_BYTES: usize = 16_000_000;

/// A warning for a page too big to be served, `None` when it fits.
pub fn size_warning(bytes: usize) -> Option<String> {
    (bytes > MAX_PAGE_BYTES).then(|| {
        format!("page is {bytes} bytes, over the {MAX_PAGE_BYTES} byte limit; it may not be served")
    })
}

/// The page with its CSS and JS inlined and the model embedded as JSON.
/// Every `</` in the data is written `<\/` and every `<!--` is written with the JSON
/// escape `\u003c`, so nothing in the data can close the script block or open a comment.
pub fn render(m: &PageModel) -> String {
    let data = serde_json::to_string(m)
        .expect("a PageModel always serialises")
        .replace("</", "<\\/")
        .replace("<!--", "\\u003c!--");
    // The data goes in last: nothing it contains is looked at for placeholders.
    HTML.replace("{{TITLE}}", &escape_html(&format!("{} topology", m.name)))
        .replace("/*{{CSS}}*/", CSS)
        .replace("/*{{JS}}*/", JS)
        .replace("{{DATA}}", &data)
}
