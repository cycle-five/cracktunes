//! `docs/metrics.md` and `names::ALL` must agree, in both directions.

use crack_metrics::names;
use std::collections::BTreeSet;

#[test]
fn docs_list_every_metric() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/metrics.md");
    let doc = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {path}: {e}; the metrics doc must exist"));

    // Tokens are maximal runs of [a-z_] after a `cracktunes_` prefix.
    let mut in_doc = BTreeSet::new();
    let mut rest = doc.as_str();
    while let Some(at) = rest.find("cracktunes_") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| !(c.is_ascii_lowercase() || c == '_'))
            .unwrap_or(tail.len());
        in_doc.insert(&tail[..end]);
        rest = &tail[end..];
    }

    for name in names::ALL.iter().filter(|n| n.starts_with("cracktunes_")) {
        assert!(
            in_doc.contains(name),
            "docs/metrics.md does not list {name}"
        );
    }
    for token in &in_doc {
        assert!(
            names::ALL.contains(token),
            "docs/metrics.md mentions {token}, which names.rs does not define"
        );
    }
}
