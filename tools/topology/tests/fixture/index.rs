use crate::common::fixture_index;

#[test]
fn index_covers_compiled_files() {
    let paths: Vec<&str> = fixture_index()
        .documents
        .iter()
        .map(|d| d.path.as_str())
        .collect();
    for p in [
        "appcore/src/gated.rs",
        "appcore/src/tests.rs",
        "appbin/src/main.rs",
        "appbin/tests/it.rs",
    ] {
        assert!(paths.contains(&p), "missing {p} in {paths:?}");
    }
    assert!(
        !paths.contains(&"appcore/src/orphan.rs"),
        "orphan.rs is not compiled"
    );
}

#[test]
fn definitions_and_references_exist() {
    use rust_topology::symbol::Symbol;
    let doc = fixture_index()
        .documents
        .iter()
        .find(|d| d.path == "appcore/src/rules.rs")
        .unwrap();
    assert!(
        doc.occurrences
            .iter()
            .any(|o| o.definition && o.symbol.ends_with("greet()."))
    );
    assert!(doc.occurrences.iter().any(|o| {
        !o.definition && Symbol::parse(&o.symbol).is_some_and(|s| s.package == "fakecord")
    }));
}
