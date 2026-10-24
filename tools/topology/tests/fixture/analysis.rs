use crate::common::{fixture_config, fixture_placed};
use rust_topology::analysis::{cycles, groups, wide};
use rust_topology::model::UnitId;

fn id(s: &str) -> UnitId {
    UnitId(s.into())
}

#[test]
fn ring_cycle_found() {
    let (s, _, g) = fixture_placed();
    let c = cycles::compute(s, g);
    for list in [&c.by_module, &c.by_top_module] {
        let ring = list
            .iter()
            .find(|c| c.modules.contains(&id("appcore::ring_a")))
            .expect("ring cycle");
        assert_eq!(ring.krate, "appcore");
        assert_eq!(
            ring.modules,
            vec![id("appcore::ring_a"), id("appcore::ring_b")]
        );
        let pairs: Vec<_> = ring
            .edges
            .iter()
            .map(|e| (&e.from.0[..], &e.to.0[..]))
            .collect();
        assert_eq!(
            pairs,
            [
                ("appcore::ring_a", "appcore::ring_b"),
                ("appcore::ring_b", "appcore::ring_a")
            ]
        );
        assert!(ring.edges.iter().all(|e| e.count >= 1));
    }
}

#[test]
fn no_cycle_between_rules_and_handlers() {
    let (s, _, g) = fixture_placed();
    let c = cycles::compute(s, g);
    for cy in c.by_module.iter().chain(&c.by_top_module) {
        assert!(
            !(cy.modules.contains(&id("appcore::rules"))
                && cy.modules.contains(&id("appcore::handlers")))
        );
        // The crate root never joins a cycle just through `crate::` paths.
        assert!(!cy.modules.contains(&id("appcore")), "{:?}", cy.modules);
    }
}

#[test]
fn wide_struct_matrix() {
    let (s, _, g) = fixture_placed();
    let w = wide::compute(s, g, fixture_config().wide_struct_min_fields);
    assert_eq!(w.len(), 1, "{w:?}");
    let w = &w[0];
    assert_eq!(w.id, id("appcore::state::Wide"));
    assert_eq!(w.fields.len(), 10);
    let third = &w.uses["f3"];
    assert_eq!(
        third[&id("appcore::handlers")],
        [id("appcore::handlers::third")].into()
    );
    assert!(w.uses["f1"].contains_key(&id("appcore::state")));
    assert!(w.uses["f2"].contains_key(&id("appcore::state")));
}

#[test]
fn handlers_reach_discord() {
    let (s, p, _) = fixture_placed();
    let m = groups::module_groups(s, p, &fixture_config());
    assert!(m[&id("appcore::handlers")]["discord"] > 0);
    assert!(
        !m.get(&id("appcore::state"))
            .is_some_and(|g| g.contains_key("discord"))
    );
}
