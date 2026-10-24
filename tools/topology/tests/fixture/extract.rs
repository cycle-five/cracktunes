use crate::common::{fixture_config, fixture_index, fixture_ws};
use rust_topology::digest::Digest;
use rust_topology::extract;
use rust_topology::model::{Generator, Topology};
use rust_topology::workspace::Workspace;
use std::sync::OnceLock;

fn generator() -> Generator {
    Generator {
        tool_hash: "t".into(),
        rust_analyzer: "ra".into(),
        config_hash: "c".into(),
        ra_error_lines: 0,
    }
}

fn topo() -> &'static Topology {
    static T: OnceLock<Topology> = OnceLock::new();
    T.get_or_init(|| {
        let cfg = fixture_config();
        let ws = Workspace::load(&fixture_ws(), &cfg.test_crates).unwrap();
        extract::build(&ws, &cfg, fixture_index(), generator()).unwrap()
    })
}

#[test]
fn topology_roundtrips() {
    let t = topo();
    let text = serde_json::to_string_pretty(t).unwrap();
    let back: Topology = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, t);
}

#[test]
fn digest_ignores_line_shifts() {
    let t = topo();
    let mut shifted = t.clone();
    for u in &mut shifted.units {
        u.start_line += 5;
        u.end_line += 5;
    }
    for e in &mut shifted.edges {
        e.first.line += 5;
    }
    for e in &mut shifted.external {
        e.first.line += 5;
    }
    for r in &mut shifted.reexports {
        r.first.line += 5;
    }
    for w in &mut shifted.warnings {
        if let Some(l) = &mut w.loc {
            l.line += 5;
        }
    }
    for m in shifted.taint.values_mut().flat_map(|m| m.values_mut()) {
        for ev in &mut m.direct {
            ev.first.line += 5;
        }
    }
    for c in shifted
        .cycles
        .by_module
        .iter_mut()
        .chain(&mut shifted.cycles.by_top_module)
    {
        for e in &mut c.edges {
            e.first.line += 5;
        }
    }
    assert_ne!(&shifted, t);
    assert_eq!(Digest::from_topology(&shifted), Digest::from_topology(t));
}

#[test]
fn digest_has_no_loc() {
    let text = serde_json::to_string_pretty(&Digest::from_topology(topo())).unwrap();
    assert!(!text.contains("\"line\""), "{text}");
    assert!(!text.contains("start_line"), "{text}");
}

#[test]
fn build_is_sorted_and_named() {
    let t = topo();
    assert_eq!(t.name, "fixture");
    assert_eq!(t.groups, vec!["discord".to_string()]);
    assert!(t.units.windows(2).all(|w| w[0].id < w[1].id));
    assert!(t.crates.windows(2).all(|w| w[0].id < w[1].id));
    assert_eq!(t.generator.tool_hash, "t");
    assert!(!t.edges.is_empty());
    assert_eq!(t.orphans.len(), 1);
    assert!(
        t.taint["discord"].contains_key(&rust_topology::model::UnitId(
            "appcore::rules::wrapper".into()
        ))
    );
}

mod sample {
    use rust_topology::config::Config;
    use rust_topology::digest::Digest;
    use rust_topology::model::*;
    use rust_topology::summary;
    use std::collections::BTreeMap;

    fn id(s: &str) -> UnitId {
        UnitId(s.into())
    }

    fn loc(line: u32) -> Loc {
        Loc {
            file: "f.rs".into(),
            line,
        }
    }

    fn krate(name: &str, lines: u32, test_only: bool) -> CrateInfo {
        CrateInfo {
            id: name.into(),
            package: name.into(),
            kind: TargetKind::Lib,
            root_file: format!("{name}/lib.rs"),
            test_only,
            lines,
        }
    }

    fn unit(name: &str, krate: &str, kind: UnitKind, own: u32, test: bool) -> Unit {
        Unit {
            id: id(name),
            kind,
            krate: krate.into(),
            module: None,
            parent: None,
            file: "f.rs".into(),
            start_line: 1,
            end_line: 2,
            lines: own,
            own_lines: own,
            test,
            features: Default::default(),
            hash: format!("h-{name}"),
        }
    }

    fn taint(state: State, cuts: u32) -> UnitTaint {
        UnitTaint {
            state,
            cuts,
            direct: vec![],
            via: vec![],
        }
    }

    fn edge(from: &str, to: &str, p: Position, count: u32) -> Edge {
        Edge {
            from: id(from),
            to: id(to),
            position: p,
            count,
            first: loc(1),
        }
    }

    fn warn(kind: WarningKind) -> Warning {
        Warning {
            kind,
            message: "m".into(),
            loc: Some(loc(3)),
        }
    }

    pub fn topo() -> Topology {
        let mut discord = BTreeMap::new();
        for (u, s, c) in [
            ("big::a", State::Body, 0),
            ("big::a2", State::Body, 1),
            ("big::b", State::Clean, 0),
            ("big::t", State::Body, 0),
            ("big::M", State::Body, 0),
            ("small::s", State::Signature, 0),
        ] {
            discord.insert(id(u), taint(s, c));
        }
        let mut db = BTreeMap::new();
        db.insert(id("big::a"), taint(State::Inherited, 5));
        Topology {
            generator: Generator {
                tool_hash: "t".into(),
                rust_analyzer: "ra".into(),
                config_hash: "c".into(),
                ra_error_lines: 7,
            },
            name: "sample".into(),
            groups: vec!["discord".into(), "db".into()],
            crates: vec![
                krate("big", 100, false),
                krate("small", 50, false),
                krate("tst", 500, true),
            ],
            units: vec![
                unit("big::a", "big", UnitKind::Function, 10, false),
                unit("big::a2", "big", UnitKind::Function, 20, false),
                unit("big::b", "big", UnitKind::Function, 5, false),
                unit("big::t", "big", UnitKind::Function, 7, true),
                unit("big::M", "big", UnitKind::Module, 1000, false),
                unit("small::s", "small", UnitKind::Function, 3, false),
            ],
            edges: vec![
                edge("big::a", "big::b", Position::Body, 2),
                edge("big::a", "big::b", Position::Body, 3),
                edge("big::a", "big::b", Position::Signature, 1),
            ],
            external: vec![
                ExternalRef {
                    from: id("big::a"),
                    package: "serenity".into(),
                    symbol: "x".into(),
                    position: Position::Body,
                    count: 4,
                    first: loc(1),
                },
                ExternalRef {
                    from: id("big::a"),
                    package: "serenity".into(),
                    symbol: "y".into(),
                    position: Position::Body,
                    count: 5,
                    first: loc(2),
                },
            ],
            reexports: vec![],
            imports: BTreeMap::new(),
            taint: [("discord".to_string(), discord), ("db".to_string(), db)].into(),
            module_lines: BTreeMap::new(),
            cycles: Cycles {
                by_module: vec![
                    Cycle {
                        krate: "big".into(),
                        modules: vec![id("big::z"), id("big::y")],
                        edges: vec![],
                    },
                    Cycle {
                        krate: "big".into(),
                        modules: vec![id("big::c"), id("big::d")],
                        edges: vec![],
                    },
                ],
                by_top_module: vec![],
            },
            wide_structs: vec![WideStruct {
                id: id("big::W"),
                fields: vec!["f".into()],
                uses: [(
                    "f".to_string(),
                    [
                        (id("big::m1"), [id("big::a")].into()),
                        (id("big::m2"), [id("big::b")].into()),
                    ]
                    .into(),
                )]
                .into(),
            }],
            module_groups: BTreeMap::new(),
            orphans: vec![
                Orphan {
                    package: "big".into(),
                    file: "big/o.rs".into(),
                    lines: 4,
                },
                Orphan {
                    package: "big".into(),
                    file: "big/o.rs".into(),
                    lines: 6,
                },
                Orphan {
                    package: "big".into(),
                    file: "big/p.rs".into(),
                    lines: 1,
                },
            ],
            warnings: vec![
                warn(WarningKind::UnparsedFile),
                warn(WarningKind::UnparsedFile),
                warn(WarningKind::UnmappedReference),
            ],
        }
    }

    #[test]
    fn digest_sums_and_names() {
        let d = Digest::from_topology(&topo());
        assert_eq!(d.edges["big::a -> big::b [body]"], 5);
        assert_eq!(d.edges["big::a -> big::b [signature]"], 1);
        assert_eq!(d.edges["big::a -> ext:serenity [body]"], 9);
        assert_eq!(d.edges.len(), 3);
        assert_eq!(d.warnings["unparsed_file"], 2);
        assert_eq!(d.warnings["unmapped_reference"], 1);
        assert_eq!(d.warnings.len(), 2);
        assert_eq!(d.orphans["big/o.rs"], 10);
        assert_eq!(d.orphans["big/p.rs"], 1);
        assert_eq!(d.generator.ra_error_lines, 7);
    }

    #[test]
    fn digest_units_cycles_and_wide() {
        let d = Digest::from_topology(&topo());
        let a = &d.units[&id("big::a")];
        assert_eq!(a.kind, UnitKind::Function);
        assert_eq!((a.hash.as_str(), a.lines, a.test), ("h-big::a", 10, false));
        assert_eq!(a.states["discord"], State::Body);
        assert_eq!(a.states["db"], State::Inherited);
        assert_eq!(d.units[&id("big::b")].states.len(), 1);
        assert!(d.units[&id("big::t")].test);
        assert_eq!(
            d.cycles_by_module,
            vec![
                vec![id("big::c"), id("big::d")],
                vec![id("big::z"), id("big::y")]
            ]
        );
        assert!(d.cycles_by_top_module.is_empty());
        assert_eq!(
            d.wide_structs[&id("big::W")]["f"],
            [id("big::m1"), id("big::m2")].into()
        );
    }

    fn row<'a>(s: &'a str, label: &str) -> Vec<&'a str> {
        s.lines()
            .find(|l| l.trim_start().starts_with(label))
            .unwrap_or_else(|| panic!("no `{label}` row in\n{s}"))
            .split_whitespace()
            .collect()
    }

    #[test]
    fn summary_counts_the_biggest_non_test_crate() {
        let s = summary::render(&topo(), &Config::default());
        // `tst` is bigger but test-only; `small` is smaller; the module, the test
        // unit and other crates' units never count.
        assert!(s.contains("big (non-test)"), "{s}");
        assert_eq!(row(&s, "body"), ["body", "30", "lines", "2", "units"]);
        assert_eq!(row(&s, "clean"), ["clean", "5", "lines", "1", "units"]);
        assert_eq!(
            row(&s, "signature"),
            ["signature", "0", "lines", "0", "units"]
        );
        assert!(s.contains("group discord (headline)"), "{s}");
        assert!(s.contains("group db:"), "{s}");
        assert_eq!(
            row(&s, "orphans:"),
            ["orphans:", "3", "files,", "11", "lines"]
        );
        assert!(s.contains("unparsed_file: 2"), "{s}");
        assert!(s.contains("unmapped_reference: 1"), "{s}");
        assert!(s.contains("rust-analyzer error lines: 7"), "{s}");
        assert!(s.contains("cycles: 2 by module, 0 by top module"), "{s}");
        // seams: a (0 cuts) and a2 (1 cut), cheapest first; tst/clean/module absent.
        let seam_a = s.find("big::a body cuts=0 lines=10").expect("seam a");
        let seam_a2 = s.find("big::a2 body cuts=1 lines=20").expect("seam a2");
        assert!(seam_a < seam_a2);
        assert!(!s.contains("big::t "), "{s}");
        // db inherited-state unit only shows under db.
        assert_eq!(row(&s, "inherited").len(), 5);
    }
}
