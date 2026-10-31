use crate::common::{fixture_config, fixture_index, fixture_ws};
use rust_topology::extract;
use rust_topology::model::{Generator, Topology};
use rust_topology::page::{self, model::PageModel};
use rust_topology::workspace::Workspace;
use std::sync::OnceLock;

fn topo() -> &'static Topology {
    static T: OnceLock<Topology> = OnceLock::new();
    T.get_or_init(|| {
        let cfg = fixture_config();
        let ws = Workspace::load(&fixture_ws(), &cfg.test_crates).unwrap();
        let generator = Generator {
            tool_hash: "t".into(),
            rust_analyzer: "ra".into(),
            config_hash: "c".into(),
            ra_error_lines: 0,
        };
        extract::build(&ws, &cfg, fixture_index(), generator).unwrap()
    })
}

fn model() -> PageModel {
    page::model::build(topo(), None, &fixture_config())
}

#[test]
fn model_seams_sorted() {
    let m = model();
    let g = m.seams.iter().find(|g| g.group == "discord").unwrap();
    assert!(!g.seams.is_empty());
    assert!(g.seams.windows(2).all(|w| w[0].cuts <= w[1].cuts));
    assert!(
        g.seams
            .iter()
            .any(|s| m.units[s.index as usize].id.0 == "appcore::rules::wrapper")
    );
}

#[test]
fn model_has_no_source_text() {
    let text = serde_json::to_string(&model()).unwrap();
    assert!(!text.contains("t.id * 2"));
}

#[test]
fn model_overview_and_tests_match_topology() {
    let t = topo();
    let m = model();
    assert_eq!(m.name, "fixture");
    assert_eq!(m.overview.crates.len(), t.crates.len());
    assert_eq!(
        m.overview.total_lines,
        t.crates.iter().map(|c| c.lines).sum::<u32>()
    );
    // Every listed test unit is a test and not clean.
    let g = m.tests.iter().find(|g| g.group == "discord").unwrap();
    for row in &g.units {
        let u = &m.units[row.unit as usize];
        assert!(u.test);
        assert_ne!(row.state, rust_topology::model::State::Clean);
    }
    assert!(
        m.modules
            .iter()
            .any(|g| g.group == "discord" && !g.rows.is_empty())
    );
}

fn html() -> String {
    page::render(&model())
}

fn data_block(html: &str) -> &str {
    let open = html
        .find(r#"<script type="application/json" id="data">"#)
        .unwrap();
    let rest = &html[open..];
    let start = rest.find('>').unwrap() + 1;
    let end = rest[start..].find("</script").unwrap();
    &rest[start..start + end]
}

#[test]
fn html_embeds_model() {
    let h = html();
    let text = data_block(&h);
    let back: PageModel = serde_json::from_str(text).unwrap();
    assert_eq!(back, model());
}

#[test]
fn html_contract() {
    let h = html();
    assert!(h.contains("<title>fixture topology</title>"));
    assert!(h.contains("prefers-color-scheme: dark"));
    assert!(h.contains(r#":root[data-theme="dark"]"#));
    // The side gutter clears a phone's system bar, as the Artifact contract asks.
    assert!(h.contains("viewport-fit=cover"));
    assert!(h.contains(r#"<meta name="claude:viewport-fit-sides" content="cover">"#));
    assert!(h.contains("padding-left: max(16px, env(safe-area-inset-left, 0px))"));
    assert!(!h.contains("http://"));
    assert!(!h.contains("https://"));
    assert!(!data_block(&h).contains("</"));
    assert!(!data_block(&h).contains("<!--"));
    // Exactly one closing tag for the data script: the one that ends it.
    assert!(h.contains("</script"));
}

#[test]
fn overview_split_adds_up_per_crate() {
    let t = topo();
    let m = model();
    let sum = |keep: &dyn Fn(&str) -> bool| {
        let mut l = (0, 0, 0, 0);
        for (module, x) in &t.module_lines["discord"] {
            let krate = &t.units.iter().find(|u| &u.id == module).unwrap().krate;
            if keep(krate) {
                l = (
                    l.0 + x.clean,
                    l.1 + x.signature,
                    l.2 + x.body,
                    l.3 + x.inherited,
                );
            }
        }
        l
    };
    let tuple = |s: &rust_topology::model::StateLines| (s.clean, s.signature, s.body, s.inherited);
    assert_eq!(tuple(&m.overview.headline), sum(&|_| true));
    assert_eq!(tuple(&m.overview.headline), (73, 16, 8, 3));
    for c in &m.overview.crates {
        match &c.split {
            Some(s) => assert_eq!(tuple(s), sum(&|k| k == c.id), "{}", c.id),
            None => assert!(c.package.starts_with("fakecord"), "{}", c.id),
        }
    }
    let core = m
        .overview
        .crates
        .iter()
        .find(|c| c.id == "appcore")
        .unwrap();
    assert_eq!(tuple(core.split.as_ref().unwrap()), (60, 15, 8, 3));
}

#[test]
fn module_rows_count_units_per_state() {
    let t = topo();
    let m = model();
    let rows = &m
        .modules
        .iter()
        .find(|g| g.group == "discord")
        .unwrap()
        .rows;
    for r in rows {
        let mut want = (0, 0, 0, 0);
        for u in t
            .units
            .iter()
            .filter(|u| !u.test && u.module.as_ref() == Some(&r.module))
        {
            use rust_topology::model::State::*;
            match t.taint["discord"].get(&u.id).map(|x| x.state) {
                Some(Clean) => want.0 += 1,
                Some(Signature) => want.1 += 1,
                Some(Body) => want.2 += 1,
                Some(Inherited) => want.3 += 1,
                None => {},
            }
        }
        let got = (
            r.units.clean,
            r.units.signature,
            r.units.body,
            r.units.inherited,
        );
        assert_eq!(got, want, "{}", r.module.0);
    }
    let rules = rows
        .iter()
        .find(|r| r.module.0 == "appcore::rules")
        .unwrap();
    assert_eq!(
        (
            rules.units.clean,
            rules.units.signature,
            rules.units.body,
            rules.units.inherited
        ),
        (4, 2, 1, 1)
    );
    assert_eq!(rules.lines.signature, 6);
}

#[test]
fn module_unit_counts_leave_out_tests() {
    let t = topo();
    let m = model();
    let tests_mod = m
        .modules
        .iter()
        .find(|g| g.group == "discord")
        .unwrap()
        .rows
        .iter()
        .find(|r| r.module.0 == "appcore::tests")
        .unwrap();
    // `appcore::tests::t` is a test unit in a test module: not counted.
    assert!(
        t.units
            .iter()
            .any(|u| u.id.0 == "appcore::tests::t" && u.test)
    );
    let n = &tests_mod.units;
    assert_eq!(n.clean + n.signature + n.body + n.inherited, 0);
}

#[test]
fn locations_are_interned_and_round_trip() {
    let t = topo();
    let m = model();
    assert!(m.files.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
    for (i, u) in m.units.iter().enumerate() {
        assert_eq!(m.files[u.loc.0 as usize], t.units[i].file);
        assert_eq!(u.loc.1, t.units[i].start_line);
    }
    for (e, te) in m.edges.iter().zip(&t.edges) {
        assert_eq!(m.files[e.first.0 as usize], te.first.file);
        assert_eq!(e.first.1, te.first.line);
    }
    let text = serde_json::to_string(&m.units[0]).unwrap();
    assert!(
        text.contains(&format!("\"loc\":[{},", m.units[0].loc.0)),
        "{text}"
    );
}

#[test]
fn via_is_unit_indices_and_clean_taint_is_the_state_alone() {
    let t = topo();
    let m = model();
    let gi = m.groups.iter().position(|g| g == "discord").unwrap();
    let at = |id: &str| {
        let i = m.units.iter().position(|u| u.id.0 == id).unwrap();
        (i, m.units[i].taint[gi].clone().unwrap())
    };
    let (_, alias) = at("appcore::rules::uses_alias");
    let via: Vec<&str> = alias
        .via
        .iter()
        .map(|&v| m.units[v as usize].id.0.as_str())
        .collect();
    assert_eq!(via, ["apptypes::SharedCtx"]);
    assert_eq!(
        alias.cuts,
        t.taint["discord"][&m.units[at("appcore::rules::uses_alias").0].id].cuts
    );
    let (i, clean) = at("appcore::rules::plain");
    assert_eq!(clean.state, rust_topology::model::State::Clean);
    let json = serde_json::to_string(&m.units[i].taint[gi]).unwrap();
    assert_eq!(json, r#"{"state":"clean"}"#);
    let (_, greet) = at("appcore::rules::greet");
    assert!(!greet.direct.is_empty());
    let first = greet.direct[0].first;
    assert!((first.0 as usize) < m.files.len());
}

#[test]
fn units_know_their_module_and_the_page_knows_the_generator() {
    let m = model();
    let greet = m
        .units
        .iter()
        .find(|u| u.id.0 == "appcore::rules::greet")
        .unwrap();
    let module = &m.units[greet.module.unwrap() as usize];
    assert_eq!(module.id.0, "appcore::rules");
    assert!(module.module.is_some());
    assert_eq!(m.generator.tool_hash, "t");
    assert_eq!(m.generator.rust_analyzer, "ra");
}

#[test]
fn seams_have_no_impl_rows() {
    let m = model();
    for g in &m.seams {
        for s in &g.seams {
            let u = &m.units[s.index as usize];
            assert_ne!(u.kind, rust_topology::model::UnitKind::Impl, "{}", u.id.0);
        }
    }
}

#[test]
fn html_comment_openers_in_data_are_escaped() {
    let mut m = model();
    m.name = "a<!--b</script>c".into();
    let h = page::render(&m);
    let block = data_block(&h);
    assert!(!block.contains("<!--"), "{block}");
    assert!(!block.contains("</"), "{block}");
    assert!(block.contains("\\u003c!--"));
    let back: PageModel = serde_json::from_str(block).unwrap();
    assert_eq!(back.name, "a<!--b</script>c");
    assert_eq!(back, m);
}

#[test]
fn page_size_warning_starts_above_the_limit() {
    assert_eq!(page::size_warning(page::MAX_PAGE_BYTES), None);
    let w = page::size_warning(page::MAX_PAGE_BYTES + 1).unwrap();
    assert!(w.contains("16000001") && w.contains("16000000"), "{w}");
}

#[test]
fn warnings_are_grouped_counted_and_capped() {
    use rust_topology::model::{Loc, Warning, WarningKind};
    let mut t = topo().clone();
    let w = |kind, i: u32| Warning {
        kind,
        message: format!("w{i}"),
        loc: Some(Loc {
            file: "f.rs".into(),
            line: i,
        }),
    };
    t.warnings = (0..120)
        .map(|i| w(WarningKind::UnmappedReference, i))
        .collect();
    t.warnings
        .extend((0..3).map(|i| w(WarningKind::UnparsedFile, i)));
    let m = page::model::build(&t, None, &fixture_config());
    assert_eq!(m.warnings.len(), 2);
    let refs = m
        .warnings
        .iter()
        .find(|g| g.kind == WarningKind::UnmappedReference)
        .unwrap();
    assert_eq!(refs.count, 120);
    assert_eq!(refs.first.len(), 50);
    assert_eq!(refs.first[49].message, "w49");
    let files = m
        .warnings
        .iter()
        .find(|g| g.kind == WarningKind::UnparsedFile)
        .unwrap();
    assert_eq!((files.count, files.first.len()), (3, 3));
}
