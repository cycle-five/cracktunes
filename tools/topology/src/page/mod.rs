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

/// The page with its CSS and JS inlined and the model embedded as JSON.
/// Every `</` in the data is written `<\/` so it cannot close the script block.
pub fn render(m: &PageModel) -> String {
    let data = serde_json::to_string(m)
        .expect("a PageModel always serialises")
        .replace("</", "<\\/");
    // The data goes in last: nothing it contains is looked at for placeholders.
    HTML.replace("{{TITLE}}", &escape_html(&format!("{} topology", m.name)))
        .replace("/*{{CSS}}*/", CSS)
        .replace("/*{{JS}}*/", JS)
        .replace("{{DATA}}", &data)
}
