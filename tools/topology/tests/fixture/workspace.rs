use crate::common::{fixture_config, fixture_ws};
use rust_topology::model::TargetKind;
use rust_topology::workspace::Workspace;
use std::collections::BTreeSet;

#[test]
fn crate_ids_for_fixture() {
    let ws = Workspace::load(&fixture_ws(), &["apptesting".into()]).unwrap();
    let ids: BTreeSet<_> = ws
        .packages
        .iter()
        .flat_map(|p| p.targets.iter().map(|t| t.crate_id.as_str()))
        .collect();
    for id in [
        "appbin",
        "appbin[bin]",
        "it[test]",
        "appcore",
        "apptypes",
        "fakecord",
        "fakecord_macros",
        "fakechars",
        "apptesting",
    ] {
        assert!(ids.contains(id), "{id}");
    }
    assert!(
        ws.packages
            .iter()
            .find(|p| p.name == "apptesting")
            .unwrap()
            .test_only
    );
    assert!(ws.is_member("appcore"));
    assert!(!ws.is_member("serde"));
}

#[test]
fn target_details() {
    let cfg = fixture_config();
    let ws = Workspace::load(&fixture_ws(), &cfg.test_crates).unwrap();
    let appbin = ws.packages.iter().find(|p| p.name == "appbin").unwrap();
    assert_eq!(appbin.dir, "appbin");
    assert!(!appbin.test_only);
    let by_id = |id: &str| appbin.targets.iter().find(|t| t.crate_id == id).unwrap();
    assert_eq!(by_id("appbin").kind, TargetKind::Lib);
    assert_eq!(by_id("appbin").root_file, "appbin/src/lib.rs");
    assert_eq!(by_id("appbin[bin]").kind, TargetKind::Bin);
    assert_eq!(by_id("appbin[bin]").root_file, "appbin/src/main.rs");
    let mac = ws
        .packages
        .iter()
        .find(|p| p.name == "fakecord-macros")
        .unwrap();
    assert_eq!(mac.targets[0].kind, TargetKind::ProcMacro);
    assert_eq!(cfg.group_of("fakecord-macros"), Some("discord"));
    assert_eq!(ws.name, "ws");
}
