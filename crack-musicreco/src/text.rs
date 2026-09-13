//! Text comparison shared by resolvers and providers, so "is this the same
//! artist" means one thing everywhere in the crate.

/// Normalize a title or artist name for exact-match comparison:
/// Unicode-lowercase, collapse/trim whitespace, and treat a typographic
/// right single quote the same as an ASCII apostrophe -- MusicBrainz's
/// canonical text favors the former ("Guns N’ Roses"); YouTube titles
/// almost always use the latter.
pub(crate) fn normalize(s: &str) -> String {
    s.replace('\u{2019}', "'")
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalize_folds_case_whitespace_and_curly_apostrophes() {
        assert_eq!(normalize("Queen"), "queen");
        assert_eq!(
            normalize("  Sweet   Child O\u{2019} Mine  "),
            "sweet child o' mine"
        );
        assert_eq!(
            normalize("queen  -  BOHEMIAN Rhapsody"),
            "queen - bohemian rhapsody"
        );
    }
}
