//! The pinned Run tabs' part of which tab a start takes. These cases wait for pinned Run tabs: a
//! session's `pinned` flag and Settings › Run › pin new tabs (`RunSettings::pin_tabs`) do not exist yet;
//! the names used here are the ones the tab-pinning work is expected to add.

use super::*;

fn finished(id: &str, pinned: bool) -> RunSession {
    RunSession { id: id.into(), pinned, ..session(cfg(id, "shell"), None) }
}

/// The tabs as `<session>:<configuration>`, a pinned one marked `*`.
fn tabs(store: &RunStore) -> Vec<String> {
    let tab =
        |session: &RunSession| format!("{}:{}{}", session.id, session.config.id, if session.pinned { "*" } else { "" });
    store.sessions.iter().map(tab).collect()
}

// src/features/run/runTabReuse.test.ts › takes the place of the same configuration's finished tab,
// then the selected one, never a pinned one
#[test]
fn takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one_never_a_pinned_one() {
    let mut store = RunStore::new();
    store.sessions = vec![finished("a", false), finished("b", false), finished("c", true)];
    store.active_session = Some("b".into());
    start(&mut store, &cfg("a", "shell"));
    assert_eq!(tabs(&store), ["a:a", "b:b", "c:c*"]);
    store.active_session = Some("c".into());
    start(&mut store, &cfg("c", "shell"));
    assert_eq!(tabs(&store).len(), 4, "the pinned finished tab of the same configuration is kept");
}

// src/features/run/runTabReuse.test.ts › opens a new tab when every finished tab is pinned, pinned when
// the setting asks
#[test]
fn opens_a_new_tab_when_every_finished_tab_is_pinned_pinned_when_the_setting_asks() {
    let mut store = RunStore::new();
    store.sessions = vec![finished("a", true)];
    store.active_session = Some("a".into());
    let types = RunConfigTypes::new();
    let settings = RunSettings { pin_tabs: true, ..RunSettings::default() };
    let env = StartEnv { types: &types, settings: &settings };
    let (mut start, mut step) = StartRun::start(&cfg("a", "shell"), StartOptions::default(), &mut store, env);
    while let StartStep::SaveAll = step {
        step = start.saved(&mut store, env, 10.0);
    }
    let shown = tabs(&store);
    assert_eq!(shown[0], "a:a*");
    assert!(shown[1].starts_with("run") && shown[1].ends_with(":a*"), "{shown:?}");
}
