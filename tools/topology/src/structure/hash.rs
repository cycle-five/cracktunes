//! Content hash of an item: its token stream without `#[doc]` attributes, so
//! whitespace, comments and documentation never change it.

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use sha2::{Digest, Sha256};

pub(super) fn hash_tokens(ts: TokenStream) -> String {
    let text = strip_docs(ts).to_string();
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn is_doc_group(g: &Group) -> bool {
    g.delimiter() == Delimiter::Bracket
        && matches!(g.stream().into_iter().next(), Some(TokenTree::Ident(i)) if i == "doc")
}

/// Removes every `#[doc …]` and `#![doc …]` at any depth.
fn strip_docs(ts: TokenStream) -> TokenStream {
    let mut out = Vec::new();
    let mut it = ts.into_iter().peekable();
    while let Some(tt) = it.next() {
        match tt {
            TokenTree::Punct(ref p) if p.as_char() == '#' => {
                let mut look = it.clone();
                if matches!(look.peek(), Some(TokenTree::Punct(b)) if b.as_char() == '!') {
                    look.next();
                }
                if matches!(look.peek(), Some(TokenTree::Group(g)) if is_doc_group(g)) {
                    look.next();
                    it = look;
                    continue;
                }
                out.push(tt);
            },
            TokenTree::Group(g) => {
                let mut inner = Group::new(g.delimiter(), strip_docs(g.stream()));
                inner.set_span(g.span());
                out.push(TokenTree::Group(inner));
            },
            other => out.push(other),
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(src: &str) -> String {
        hash_tokens(src.parse().unwrap())
    }

    #[test]
    fn docs_whitespace_and_comments_are_ignored() {
        assert_eq!(h("/// a\nfn f() { 1 }"), h("fn f()   {\n 1 }"));
        assert_eq!(h("#[doc = \" a\"] fn f() {}"), h("/** b */ fn f() {}"));
        assert_eq!(h("fn f() { 1 } // c"), h("fn f() { 1 }"));
    }

    #[test]
    fn docs_are_ignored_at_any_depth() {
        assert_eq!(h("impl A { /// d\n fn f() {} }"), h("impl A { fn f() {} }"));
        assert_eq!(h("#![doc = \"x\"] fn f() {}"), h("fn f() {}"));
        assert_eq!(h("#[doc(hidden)] fn f() {}"), h("fn f() {}"));
    }

    #[test]
    fn code_and_other_attributes_matter() {
        assert_ne!(h("fn f() { 1 }"), h("fn f() { 2 }"));
        assert_ne!(h("#[inline] fn f() {}"), h("fn f() {}"));
        assert_ne!(h("#[docs] fn f() {}"), h("fn f() {}"));
        assert_ne!(h("#[allow(doc)] fn f() {}"), h("fn f() {}"));
        assert_ne!(h("fn f() { # x }"), h("fn f() { x }"));
        assert_ne!(h("fn f() { #! x }"), h("fn f() { ! x }"));
        assert_ne!(h("fn f() { # (doc) }"), h("fn f() { }"));
        // only `#` starts an attribute: a bracket group after other punctuation stays
        assert_ne!(
            h("fn f() { let _ = &[doc, 1]; }"),
            h("fn f() { let _ = ; }")
        );
    }

    #[test]
    fn hash_is_sha256_hex() {
        let x = h("fn f() {}");
        assert_eq!(x.len(), 64);
        assert!(x.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(x, h("fn g() {}"));
    }
}
