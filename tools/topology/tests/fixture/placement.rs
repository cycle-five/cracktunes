use crate::common::fixture_placed;
use rust_topology::model::{Position, UnitId};

fn id(s: &str) -> UnitId {
    UnitId(s.into())
}

fn has_edge(from: &str, to: &str, position: Position) -> bool {
    fixture_placed()
        .2
        .edges
        .iter()
        .any(|e| e.from == id(from) && e.to == id(to) && e.position == position)
}

fn has_external(from: &str, package: &str, position: Position) -> bool {
    fixture_placed()
        .2
        .external
        .iter()
        .any(|e| e.from == id(from) && e.package == package && e.position == position)
}

#[test]
fn signature_edge_to_workspace_type() {
    assert!(has_edge(
        "appcore::rules::pick_next",
        "appcore::rules::Track",
        Position::Signature
    ));
}

#[test]
fn body_edge_to_function() {
    assert!(has_edge(
        "appcore::rules::wrapper",
        "appcore::rules::greet",
        Position::Body
    ));
}

#[test]
fn attribute_is_signature_evidence() {
    assert!(has_external(
        "appcore::handlers::cmd",
        "fakecord",
        Position::Signature
    ));
}

#[test]
fn reexport_resolves_to_defining_crate() {
    assert!(has_external(
        "appcore::rules::in_guild",
        "fakecord",
        Position::Signature
    ));
}

#[test]
fn plain_use_is_import_not_ref() {
    let placed = &fixture_placed().1;
    assert!(
        !has_external("appcore::text::label", "fakecord", Position::Signature)
            && !has_external("appcore::text::label", "fakecord", Position::Body)
    );
    assert!(placed.imports[&id("appcore::text")].contains("fakecord"));
}

#[test]
fn path_segment_counts_in_body() {
    assert!(has_external(
        "appcore::text::label_inline",
        "fakecord",
        Position::Body
    ));
}

#[test]
fn emoji_line_maps_to_after_emoji() {
    assert!(has_edge(
        "appcore::text::emoji_user",
        "appcore::text::after_emoji",
        Position::Body
    ));
    assert!(!has_edge(
        "appcore::text::emoji_user",
        "appcore::text::BANNER",
        Position::Body
    ));
}

#[test]
fn bin_reference_prefers_own_crate() {
    assert!(has_edge(
        "appbin[bin]::main",
        "appbin[bin]::run",
        Position::Body
    ));
    assert!(has_edge("appbin[bin]::main", "appbin::run", Position::Body));
}

#[test]
fn field_use_recorded() {
    let graph = &fixture_placed().2;
    assert!(graph.field_uses.contains(&(
        id("appcore::state::Wide"),
        "f3".into(),
        id("appcore::handlers::third")
    )));
}

#[test]
fn pub_use_is_reexport() {
    let placed = &fixture_placed().1;
    assert!(
        placed
            .reexports
            .iter()
            .any(|r| r.module == id("apptypes") && r.target.contains("GuildId"))
    );
}

#[test]
fn fixture_has_no_unmapped_references() {
    let placed = &fixture_placed().1;
    assert_eq!(placed.stats.references_unmapped, 0, "{:?}", placed.warnings);
}

#[test]
fn stats_and_warnings_on_fixture() {
    let placed = &fixture_placed().1;
    assert!(placed.stats.definitions_exact > 50);
    assert_eq!(placed.stats.definitions_unmapped, 0);
    assert!(placed.stats.references > 100);
    assert!(placed.warnings.is_empty(), "{:?}", placed.warnings);
}

#[test]
fn tuple_field_use_edges_to_the_struct() {
    assert!(has_edge(
        "appcore::text::label_inline",
        "fakechars::FixedStr",
        Position::Body
    ));
}

#[test]
fn counts_and_first_location_aggregate() {
    let g = &fixture_placed().2;
    let e = g
        .edges
        .iter()
        .find(|e| {
            e.from == id("appcore::handlers::cmd")
                && e.to == id("appcore::rules::Track")
                && e.position == Position::Body
        })
        .unwrap();
    assert!(e.count >= 2, "{e:?}");
    assert_eq!(e.first.file, "appcore/src/handlers.rs");
    assert_eq!(e.first.line, 19);
}

#[test]
fn no_self_edges_and_sorted() {
    let g = &fixture_placed().2;
    assert!(g.edges.iter().all(|e| e.from != e.to));
    let keys: Vec<_> = g
        .edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone(), e.position))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn field_ref_also_edges_to_owner() {
    assert!(has_edge(
        "appcore::handlers::third",
        "appcore::state::Wide",
        Position::Body
    ));
}

#[test]
fn bin_root_segment_names_the_lib() {
    let p = &fixture_placed().1;
    assert!(p.refs.iter().any(|r| r.from == id("appbin[bin]::main")
        && r.target == rust_topology::placement::RefTarget::Module(id("appbin"))));
}

#[test]
fn impl_header_is_signature_evidence() {
    assert!(has_external(
        "appcore::handlers::<impl EventHandler for Handler>",
        "fakecord",
        Position::Signature
    ));
    assert!(has_edge(
        "appcore::handlers::<impl EventHandler for Handler>",
        "appcore::handlers::Handler",
        Position::Signature
    ));
}

mod synthetic {
    use super::id;
    use rust_topology::model::{Unit, UnitKind, WarningKind};
    use rust_topology::placement::{Placed, place};
    use rust_topology::scip::{Occurrence, ScipDocument, ScipIndex};
    use rust_topology::structure::{FileInfo, Pos, Span, Structure, UnitDraft};
    use rust_topology::workspace::Workspace;

    pub fn ws() -> Workspace {
        Workspace::load(&crate::common::fixture_ws(), &["apptesting".into()]).unwrap()
    }

    pub fn pos(line: u32, col: u32) -> Pos {
        Pos { line, col }
    }

    pub fn span(start: Pos, end: Pos) -> Span {
        Span { start, end }
    }

    pub fn unit(
        name: &str,
        kind: UnitKind,
        krate: &str,
        file: &str,
        span: Span,
        ident: Option<Pos>,
        body: Option<Span>,
    ) -> UnitDraft {
        UnitDraft {
            unit: Unit {
                id: id(name),
                kind,
                krate: krate.into(),
                module: None,
                parent: None,
                file: file.into(),
                start_line: span.start.line,
                end_line: span.end.line,
                lines: 1,
                own_lines: 1,
                test: false,
                features: Default::default(),
                hash: String::new(),
            },
            ident,
            span,
            body,
        }
    }

    pub fn file(s: &mut Structure, path: &str, krate: &str, module: &str, text: &str) {
        s.files.insert(
            path.into(),
            FileInfo {
                krate: krate.into(),
                module: id(module),
                text: text.into(),
            },
        );
    }

    pub fn occ(line: u32, col: u32, end: u32, symbol: &str, definition: bool) -> Occurrence {
        Occurrence {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: end,
            symbol: symbol.into(),
            definition,
        }
    }

    pub fn doc(path: &str, occurrences: Vec<Occurrence>) -> ScipDocument {
        ScipDocument {
            path: path.into(),
            occurrences,
        }
    }

    pub fn placed(s: &Structure, documents: Vec<ScipDocument>) -> Placed {
        place(s, &ScipIndex { documents }, &ws())
    }

    pub fn target_on_line(p: &Placed, line: u32) -> &RefTarget {
        let hits: Vec<_> = p.refs.iter().filter(|r| r.loc.line == line).collect();
        assert_eq!(hits.len(), 1, "line {line}: {:?}", p.refs);
        &hits[0].target
    }

    pub fn kinds(p: &Placed) -> Vec<WarningKind> {
        p.warnings.iter().map(|w| w.kind).collect()
    }

    pub use rust_topology::model::UnitKind as K;
    pub use rust_topology::placement::RefTarget;
    pub use rust_topology::structure::FieldDef as F;
    pub use rust_topology::structure::Structure as S;
}

/// A macro-generated definition has no ident of its own and lands in the
/// innermost unit (a module unit becomes a `Module` target); one outside every
/// unit is unmapped; modules and fields map exactly.
#[test]
fn contained_and_unmapped_definitions() {
    use synthetic::*;
    let f = "appcore/src/x.rs";
    let mut s = S::default();
    s.units.push(unit(
        "appcore::x",
        K::Module,
        "appcore",
        f,
        span(pos(1, 0), pos(11, 0)),
        None,
        None,
    ));
    s.units.push(unit(
        "appcore::x::S",
        K::Struct,
        "appcore",
        f,
        span(pos(3, 0), pos(6, 0)),
        Some(pos(3, 11)),
        None,
    ));
    s.fields.push(F {
        owner: id("appcore::x::S"),
        name: "a".into(),
        file: f.into(),
        ident: pos(4, 4),
    });
    file(
        &mut s,
        f,
        "appcore",
        "appcore::x",
        "\n\npub struct S {\n    a: u32,\n}\n\n\n\n\n\n",
    );
    let sym = |d: &str| format!("rust-analyzer cargo appcore 0.1.0 {d}");
    let p = placed(
        &s,
        vec![doc(
            f,
            vec![
                occ(0, 0, 1, &sym("x/"), true),
                occ(2, 11, 12, &sym("x/S#"), true),
                occ(3, 4, 5, &sym("x/S#a."), true),
                occ(3, 7, 8, &sym("x/S#Variant."), true),
                occ(7, 0, 1, &sym("x/generated()."), true),
                occ(40, 0, 1, &sym("x/gone()."), true),
                occ(8, 0, 1, &sym("x/S#Variant."), false),
                occ(9, 0, 1, &sym("x/generated()."), false),
                occ(10, 0, 1, &sym("x/never()."), false),
                occ(6, 0, 1, &sym("x/S#a."), false),
            ],
        )],
    );
    assert_eq!(p.stats.definitions_exact, 3);
    assert_eq!(p.stats.definitions_contained, 2);
    assert_eq!(p.stats.definitions_unmapped, 1);
    assert_eq!(p.stats.references, 4);
    assert_eq!(p.stats.references_unmapped, 1);
    assert_eq!(target_on_line(&p, 9), &RefTarget::Unit(id("appcore::x::S")));
    assert_eq!(target_on_line(&p, 10), &RefTarget::Module(id("appcore::x")));
    assert_eq!(
        target_on_line(&p, 7),
        &RefTarget::Field {
            owner: id("appcore::x::S"),
            field: "a".into()
        }
    );
    assert_eq!(
        kinds(&p),
        vec![
            rust_topology::model::WarningKind::UnmappedReference,
            rust_topology::model::WarningKind::UnmappedDefinition
        ]
    );
}

/// A lib and a bin of one package share symbols: the source text decides.
#[test]
fn lib_and_bin_candidates_follow_the_source_text() {
    use synthetic::*;
    let (lib, bin) = ("appbin/src/lib.rs", "appbin/src/main.rs");
    let mut s = S::default();
    s.units.push(unit(
        "appbin",
        K::Module,
        "appbin",
        lib,
        span(pos(1, 0), pos(2, 0)),
        None,
        None,
    ));
    s.units.push(unit(
        "appbin::run",
        K::Function,
        "appbin",
        lib,
        span(pos(1, 0), pos(2, 0)),
        Some(pos(1, 7)),
        None,
    ));
    s.units.push(unit(
        "appbin[bin]",
        K::Module,
        "appbin[bin]",
        bin,
        span(pos(1, 0), pos(7, 0)),
        None,
        None,
    ));
    s.units.push(unit(
        "appbin[bin]::run",
        K::Function,
        "appbin[bin]",
        bin,
        span(pos(1, 0), pos(2, 0)),
        Some(pos(1, 3)),
        None,
    ));
    s.units.push(unit(
        "appbin[bin]::main",
        K::Function,
        "appbin[bin]",
        bin,
        span(pos(2, 0), pos(7, 0)),
        Some(pos(2, 3)),
        Some(span(pos(2, 10), pos(7, 0))),
    ));
    file(&mut s, lib, "appbin", "appbin", "pub fn run() {}\n");
    file(
        &mut s,
        bin,
        "appbin[bin]",
        "appbin[bin]",
        "fn run() {}\nfn main() {\n    run();\n    appbin::run();\n    crate::run();\n}\n",
    );
    let sym = |d: &str| format!("rust-analyzer cargo appbin 0.1.0 {d}");
    let p = placed(
        &s,
        vec![
            doc(
                lib,
                vec![
                    occ(0, 0, 1, &sym("crate/"), true),
                    occ(0, 7, 10, &sym("run()."), true),
                ],
            ),
            doc(
                bin,
                vec![
                    occ(0, 0, 1, &sym("crate/"), true),
                    occ(0, 3, 6, &sym("run()."), true),
                    occ(1, 3, 7, &sym("main()."), true),
                    occ(2, 4, 7, &sym("run()."), false),
                    occ(3, 4, 10, &sym("crate/"), false),
                    occ(3, 12, 15, &sym("run()."), false),
                    occ(4, 4, 9, &sym("crate/"), false),
                    occ(4, 11, 14, &sym("run()."), false),
                ],
            ),
        ],
    );
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    // Line 4 and 5 each hold a crate-root segment and a function.
    let on = |line, module: bool| -> String {
        let hits: Vec<_> = p
            .refs
            .iter()
            .filter(|r| r.loc.line == line && matches!(r.target, RefTarget::Module(_)) == module)
            .collect();
        assert_eq!(hits.len(), 1, "line {line}: {:?}", p.refs);
        match &hits[0].target {
            RefTarget::Unit(u) | RefTarget::Module(u) => u.0.clone(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(on(3, false), "appbin[bin]::run");
    assert_eq!(on(4, true), "appbin");
    assert_eq!(on(4, false), "appbin::run");
    assert_eq!(on(5, true), "appbin[bin]");
    assert_eq!(on(5, false), "appbin[bin]::run");
}

#[test]
fn outside_tree_and_unindexed_files_warn() {
    use synthetic::*;
    let mut s = S::default();
    file(
        &mut s,
        "appcore/src/lonely.rs",
        "appcore",
        "appcore::lonely",
        "",
    );
    let p = placed(
        &s,
        vec![
            doc(
                "elsewhere.rs",
                vec![
                    occ(0, 0, 1, "rust-analyzer cargo appcore 0.1.0 f().", false),
                    occ(0, 0, 1, "local 1", false),
                ],
            ),
            doc("only_locals.rs", vec![occ(0, 0, 1, "local 2", true)]),
        ],
    );
    assert_eq!(
        kinds(&p),
        vec![
            rust_topology::model::WarningKind::UnindexedFile,
            rust_topology::model::WarningKind::OccurrenceOutsideTree
        ]
    );
    assert!(
        p.warnings[1]
            .message
            .starts_with("1 occurrences in elsewhere.rs")
    );
    assert_eq!(p.stats.references, 0);
}

#[test]
fn glob_reexport_is_kept_but_path_prefix_is_not() {
    use rust_topology::structure::UseSpan;
    use synthetic::*;
    let f = "appcore/src/x.rs";
    let mut s = S::default();
    s.units.push(unit(
        "appcore::x",
        K::Module,
        "appcore",
        f,
        span(pos(1, 0), pos(4, 0)),
        None,
        None,
    ));
    for line in [1, 2] {
        s.uses.push(UseSpan {
            module: id("appcore::x"),
            file: f.into(),
            span: span(pos(line, 0), pos(line + 1, 0)),
            public: true,
        });
    }
    file(
        &mut s,
        f,
        "appcore",
        "appcore::x",
        "pub use serde::*;\npub use serde::Thing;\n\n",
    );
    let sym = |d: &str| format!("rust-analyzer cargo serde 1.0.0 {d}");
    let p = placed(
        &s,
        vec![doc(
            f,
            vec![
                occ(0, 8, 13, &sym("crate/"), false),
                occ(1, 8, 13, &sym("crate/"), false),
                occ(1, 15, 20, &sym("Thing#"), false),
            ],
        )],
    );
    let got: Vec<_> = p
        .reexports
        .iter()
        .map(|r| (r.target.as_str(), r.first.line))
        .collect();
    assert_eq!(got, vec![("serde Thing#", 2), ("serde crate/", 1)]);
}

#[test]
fn same_crate_duplicates_are_counted_and_resolved_deterministically() {
    use synthetic::*;
    let f = "appcore/src/x.rs";
    let build = |reverse: bool| {
        let mut s = S::default();
        s.units.push(unit(
            "appcore::x",
            K::Module,
            "appcore",
            f,
            span(pos(1, 0), pos(5, 0)),
            None,
            None,
        ));
        for (name, col) in [("appcore::x::B", 4), ("appcore::x::A", 14)] {
            s.units.push(unit(
                name,
                K::Function,
                "appcore",
                f,
                span(pos(2, col - 4), pos(2, col + 6)),
                Some(pos(2, col)),
                None,
            ));
        }
        file(
            &mut s,
            f,
            "appcore",
            "appcore::x",
            "\nfn b() {} fn a() {}\n\nx;\n",
        );
        let sym = "rust-analyzer cargo appcore 0.1.0 x/dup().";
        let mut defs = vec![occ(1, 4, 5, sym, true), occ(1, 14, 15, sym, true)];
        if reverse {
            defs.reverse();
        }
        defs.push(occ(3, 0, 1, sym, false));
        placed(&s, vec![doc(f, defs)])
    };
    for reverse in [false, true] {
        let p = build(reverse);
        assert_eq!(p.stats.definitions_ambiguous, 1);
        assert_eq!(
            kinds(&p),
            vec![rust_topology::model::WarningKind::AmbiguousDefinition]
        );
        assert_eq!(target_on_line(&p, 4), &RefTarget::Unit(id("appcore::x::A")));
    }
}

#[test]
fn malformed_symbols_are_counted_and_locals_are_not() {
    use synthetic::*;
    let f = "appcore/src/x.rs";
    let mut s = S::default();
    s.units.push(unit(
        "appcore::x",
        K::Module,
        "appcore",
        f,
        span(pos(1, 0), pos(3, 0)),
        None,
        None,
    ));
    file(&mut s, f, "appcore", "appcore::x", "x\nx\n");
    let p = placed(
        &s,
        vec![doc(
            f,
            vec![
                occ(0, 0, 1, "rust-analyzer cargo appcore", true),
                occ(1, 0, 1, "garbage", false),
                occ(1, 0, 1, "local 4", false),
                occ(0, 0, 1, "local 5", true),
            ],
        )],
    );
    assert_eq!(p.stats.symbols_unparsed, 2);
    assert_eq!(p.stats.references, 0);
    assert_eq!(
        kinds(&p),
        vec![rust_topology::model::WarningKind::UnparsedSymbol; 2]
    );
    assert_eq!(p.warnings[0].loc.as_ref().unwrap().line, 1);
    assert_eq!(p.warnings[1].loc.as_ref().unwrap().line, 2);
}

#[test]
fn output_is_sorted() {
    let p = &fixture_placed().1;
    assert!(p.refs.windows(2).all(|w| w[0] <= w[1]));
    assert!(
        p.reexports
            .windows(2)
            .all(|w| (&w[0].module, &w[0].target) <= (&w[1].module, &w[1].target))
    );
    let s = synthetic_unsorted();
    let files: Vec<_> = s
        .warnings
        .iter()
        .map(|w| w.loc.as_ref().unwrap().file.as_str())
        .collect();
    assert_eq!(files, vec!["aa.rs", "zz.rs"]);
}

/// Occurrences fed in reverse order still produce sorted warnings.
fn synthetic_unsorted() -> rust_topology::placement::Placed {
    use synthetic::*;
    let mut s = S::default();
    file(&mut s, "appcore/src/y.rs", "appcore", "appcore::y", "");
    let p = placed(
        &s,
        vec![
            doc(
                "zz.rs",
                vec![occ(
                    0,
                    0,
                    1,
                    "rust-analyzer cargo appcore 0.1.0 f().",
                    false,
                )],
            ),
            doc(
                "aa.rs",
                vec![occ(
                    0,
                    0,
                    1,
                    "rust-analyzer cargo appcore 0.1.0 g().",
                    false,
                )],
            ),
        ],
    );
    // UnindexedFile for y.rs sorts after aa.rs / zz.rs? Keep only the two outside-tree ones.
    rust_topology::placement::Placed {
        warnings: p
            .warnings
            .into_iter()
            .filter(|w| w.kind == rust_topology::model::WarningKind::OccurrenceOutsideTree)
            .collect(),
        ..p
    }
}

/// `Self` is the impl block and a derived method has no source definition;
/// neither has an occurrence of its own to map to, so both mean the self type
/// (when it is defined in the impl's module); a self type from elsewhere still
/// warns instead of being guessed.
#[test]
fn impl_symbols_mean_their_self_type() {
    use synthetic::*;
    let f = "appcore/src/x.rs";
    let mut s = S::default();
    s.units.push(unit(
        "appcore::x",
        K::Module,
        "appcore",
        f,
        span(pos(1, 0), pos(11, 0)),
        None,
        None,
    ));
    s.units.push(unit(
        "appcore::x::S",
        K::Struct,
        "appcore",
        f,
        span(pos(3, 0), pos(4, 0)),
        Some(pos(3, 11)),
        None,
    ));
    file(
        &mut s,
        f,
        "appcore",
        "appcore::x",
        "\n\npub struct S;\n\n\n\n\n\n\n\n",
    );
    let sym = |d: &str| format!("rust-analyzer cargo appcore 0.1.0 {d}");
    let p = placed(
        &s,
        vec![doc(
            f,
            vec![
                occ(0, 0, 1, &sym("x/"), true),
                occ(2, 11, 12, &sym("x/S#"), true),
                occ(5, 0, 1, &sym("x/impl#[S]"), false),
                occ(6, 0, 1, &sym("x/impl#[S][`PartialEq<Self>`]eq()."), false),
                occ(7, 0, 1, &sym("x/impl#[Elsewhere]"), false),
            ],
        )],
    );
    assert_eq!(p.stats.references, 3);
    assert_eq!(p.stats.references_unmapped, 1);
    assert_eq!(target_on_line(&p, 6), &RefTarget::Unit(id("appcore::x::S")));
    assert_eq!(target_on_line(&p, 7), &RefTarget::Unit(id("appcore::x::S")));
    assert_eq!(
        kinds(&p),
        vec![rust_topology::model::WarningKind::UnmappedReference]
    );
}
