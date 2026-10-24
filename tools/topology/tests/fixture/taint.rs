use crate::common::{fixture_config, fixture_placed};
use rust_topology::model::{State, State::*, UnitId, UnitTaint};
use rust_topology::taint::{self, Seam};
use std::sync::OnceLock;

fn id(s: &str) -> UnitId {
    UnitId(s.into())
}

fn all() -> &'static taint::Taint {
    static T: OnceLock<taint::Taint> = OnceLock::new();
    T.get_or_init(|| {
        let (s, p, g) = fixture_placed();
        taint::compute(s, p, g, &fixture_config())
    })
}

fn taint(u: &str) -> &'static UnitTaint {
    all().by_group["discord"]
        .get(&id(u))
        .unwrap_or_else(|| panic!("no taint for {u}"))
}

fn state(u: &str) -> State {
    taint(u).state
}

fn seams(group: &str, max: u32) -> Vec<Seam> {
    let units: Vec<_> = fixture_placed()
        .0
        .units
        .iter()
        .map(|d| d.unit.clone())
        .collect();
    taint::seams(&all().by_group[group], &units, max)
}

#[test]
fn fixture_states() {
    for (u, s) in [
        ("appcore::rules::pick_next", Clean),
        ("appcore::rules::score", Clean),
        ("appcore::rules::plain", Clean),
        ("appcore::rules::Track", Clean),
        ("appcore::rules::in_guild", Signature),
        ("appcore::rules::uses_alias", Signature),
        ("appcore::rules::greet", Body),
        ("appcore::rules::wrapper", Inherited),
        (
            "appcore::handlers::<impl EventHandler for Handler>",
            Signature,
        ),
        (
            "appcore::handlers::<Handler as EventHandler>::on_event",
            Signature,
        ),
        ("appcore::handlers::Bot::ping", Clean),
        ("appcore::handlers::cmd", Signature),
        ("appcore::text::label", Clean),
        ("appcore::text::label_inline", Body),
        ("apptypes::SharedCtx", Signature),
        ("apptypes::Plain", Clean),
    ] {
        assert_eq!(state(u), s, "{u}");
    }
}

#[test]
fn alias_taint_is_via() {
    let t = taint("appcore::rules::uses_alias");
    assert!(t.direct.is_empty());
    assert_eq!(t.via, vec![id("apptypes::SharedCtx")]);
    assert_eq!(t.cuts, 1);
}

#[test]
fn cuts_count_lines() {
    assert_eq!(taint("appcore::rules::greet").cuts, 2);
    assert_eq!(taint("appcore::handlers::cmd").cuts, 1);
    assert_eq!(taint("appcore::rules::wrapper").cuts, 1);
}

#[test]
fn seams_exclude_tests_and_sort_by_cuts() {
    let s = seams("discord", 2);
    assert!(s.iter().all(|x| x.unit != id("appcore::tests::t")));
    assert!(s.windows(2).all(|w| w[0].cuts <= w[1].cuts));
    assert!(s.iter().any(|x| x.unit == id("appcore::rules::wrapper")));
}

#[test]
fn test_unit_still_has_a_state() {
    assert_eq!(state("appcore::tests::t"), Body);
}
