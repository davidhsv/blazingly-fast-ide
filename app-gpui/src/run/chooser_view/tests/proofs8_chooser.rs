use super::*;

/// The configurations started last follow the pinned ones under Recent, newest first, and are not
/// listed again in their group.
#[gpui_kit::test]
async fn lists_the_configurations_started_last_after_the_pinned_ones_newest_first(cx: &mut TestAppContext) {
    let mut store = pin_fixture();
    store.pinned = vec!["serve".to_owned()];
    store.recent = ["App", "serve", "build"].map(str::to_owned).to_vec();
    let (chooser, _, _, cx) = open(cx, store, ChooserMode::Select);
    assert_eq!(lines(&chooser, cx), ["current", "[Pinned]", "serve", "[Recent]", "App", "build", "edit"]);
}

/// Delete on a detected configuration of any kind asks to delete it; what that saves is no user
/// configuration and the detection suppressed by its id.
#[gpui_kit::test]
async fn chooser_deletion_suppresses_detected_configurations(cx: &mut TestAppContext) {
    let kinds = ["spring-boot", "application", "maven", "gradle", "npm", "pytest", "cargo", "go"];
    let configs = kinds.iter().map(|kind| config(&format!("{kind}:app"), kind, kind)).collect();
    let (chooser, store, events, cx) = open(cx, store_of(configs, None), ChooserMode::Select);
    for kind in kinds {
        let id = format!("{kind}:app");
        move_to(&chooser, kind, cx);
        cx.simulate_keystrokes("delete");
        assert_eq!(events.borrow().last(), Some(&ConfigChooserEvent::Delete(id.clone())), "{kind}");
        let saved = store.update(cx, |store, _| store.delete_config(&id).1);
        assert_eq!(saved, Some((Vec::new(), vec![id.clone()])), "{kind}");
    }
}

/// Delete on a temporary configuration forgets it: the chooser no longer lists it and the store
/// keeps no temporary configuration (nothing is saved for it).
#[gpui_kit::test]
async fn temporary_deletion_does_not_persist_suppression(cx: &mut TestAppContext) {
    let mut store = store_of(vec![config("npm:app", "Detected app", "npm")], Some("npm:app"));
    store.temporary = vec!["npm:app".to_owned()];
    let (chooser, store, events, cx) = open(cx, store, ChooserMode::Select);
    assert_eq!(chooser.read_with(cx, |chooser, _| chooser.selected()).as_deref(), Some("Detected app"));
    cx.simulate_keystrokes("delete");
    assert_eq!(*events.borrow(), [ConfigChooserEvent::Delete("npm:app".into())]);
    cx.run_until_parked();
    assert!(!lines(&chooser, cx).contains(&"Detected app".to_owned()), "{:?}", lines(&chooser, cx));
    assert!(store.read_with(cx, |store, _| store.configs().is_empty() && store.temporary.is_empty()));
}
