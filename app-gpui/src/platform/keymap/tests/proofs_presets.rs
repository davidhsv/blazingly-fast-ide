use super::*;

/// The bindings `km` gives `id`; an action the preset leaves unbound has none.
fn bound<'a>(km: &'a Keymap, id: &str) -> &'a [String] {
    km.get(id).map_or(&[], Vec::as_slice)
}

fn normalized(keys: &[String]) -> Vec<String> {
    keys.iter().map(|key| normalize_binding(key)).collect()
}

/// The actions of `km` bound to `key`, in action order.
fn owners(km: &Keymap, key: &str) -> Vec<String> {
    let key = normalize_binding(key);
    km.iter().filter(|(_, keys)| normalized(keys).contains(&key)).map(|(id, _)| id.clone()).collect()
}

fn keymap(id: &str) -> &'static Keymap {
    preset(id).unwrap_or_else(|| panic!("preset {id} is built in"))
}

/// The Eclipse presets: (id, title, parent IDEA preset, primary modifier).
const ECLIPSE: [(&str, &str, &str, &str); 2] = [
    ("eclipse-windows", "Eclipse", "intellij-windows", "Ctrl"),
    ("eclipse-macos", "Eclipse (macOS)", "intellij-macos", "Meta"),
];

/// Shortcuts shared on purpose, each by actions of separate focused surfaces.
const SHARED: &[&[&str]] = &[
    &["navigate.gotoClass", "terminal.new"],
    &["navigate.recentLocations", "project.open", "debug.jumpToSource", "diff.jumpToSource", "find.popup.openResult"],
    &[
        "navigate.declaration",
        "db.goToDdl",
        "project.open",
        "debug.jumpToSource",
        "diff.jumpToSource",
        "find.popup.openResult",
    ],
    &["navigate.typeHierarchy", "db.editData"],
    &["navigate.fileStructure", "db.goToColumn"],
    &["find.replaceInFile", "agents.findInConversation", "vcs.log.goToRef"],
    &["find.next", "find.popup.nextResult"],
    &["find.previous", "find.popup.previousResult"],
    &["find.usages", "project.findUsages"],
    &["code.quickDocumentation", "db.quickDoc", "debug.setValue", "vcs.changelist.edit"],
    &["code.reformat", "project.reformatCode"],
    &["code.optimizeImports", "project.optimizeImports"],
    &["code.deleteLine", "db.duplicateSource", "project.compareWith", "vcs.showDiff"],
    &["code.moveStatementDown", "find.field.recentSearches", "find.popup.recentSearches"],
    &["code.foldAll", "project.collapseAll"],
    &[
        "refactor.rename",
        "project.rename",
        "db.rename",
        "vcs.changelist.edit",
        "vcs.log.editMessage",
        "vcs.shelf.rename",
    ],
    &["refactor.changeSignature", "db.modifyTable"],
    &["refactor.move", "project.move"],
    &["editor.closeTab", "toolwindow.closeContent"],
    &["run.rerun", "debug.rerun", "find.rerun", "usages.rerun"],
    &[
        "debug.smartStepInto",
        "db.refresh",
        "github.refresh",
        "github.pullRequest.refresh",
        "github.pullRequest.timeline.refresh",
    ],
    &[
        "debug.runToCursor",
        "db.refresh",
        "github.refresh",
        "github.pullRequest.refresh",
        "github.pullRequest.timeline.refresh",
    ],
    &["editor.increaseFontSize", "image.zoomIn"],
    &["editor.decreaseFontSize", "image.zoomOut"],
];

// src/platform/keymaps/eclipse.test.ts › %s › is listed, loads, switches without stale overrides, and can parent a
// custom keymap
/// Each Eclipse preset is listed under its title, replaces another preset's overrides when chosen, and can be the
/// parent of a user keymap whose own overrides apply over it.
#[test]
fn the_eclipse_presets_load_switch_without_stale_overrides_and_parent_a_custom_keymap() {
    for (id, title, _, _) in ECLIPSE {
        let km = keymap(id);
        assert!(PRESET_TITLES.contains(&(id, title)), "{id} is listed as {title}");
        let mut state = KeymapState::new("intellij-macos");
        state.apply_settings(Some(&json!("vscode-macos")), Some(&json!({ "navigate.gotoFile": ["Alt+X"] })), None);
        state.apply_settings(Some(&json!(id)), Some(&json!({})), None);
        assert_eq!(state.preset_id(), id);
        assert_eq!(state.bindings_for("navigate.gotoFile"), bound(km, "navigate.gotoFile"), "{id}");
        let custom = json!({ "custom.eclipse": { "parent": id, "overrides": { "file.save": ["Alt+S"] } } });
        state.apply_settings(Some(&json!("custom.eclipse")), Some(&json!({})), Some(&custom));
        assert_eq!(state.preset_id(), id);
        assert_eq!(state.bindings_for("file.save"), ["Alt+S"], "{id}");
        assert_eq!(state.bindings_for("navigate.gotoFile"), bound(km, "navigate.gotoFile"), "{id}");
    }
}

// src/platform/keymaps/eclipse.test.ts › %s › has valid bindings, no duplicate entries, and no unreviewed new
// conflicts
/// Every Eclipse binding is valid and listed once per action, and two actions share a key only where the IDEA
/// parent already shares it or the pair is a reviewed one of separate surfaces.
#[test]
fn the_eclipse_presets_have_valid_bindings_no_duplicates_and_no_unreviewed_new_conflicts() {
    for (id, _, parent, _) in ECLIPSE {
        let (km, parent) = (keymap(id), keymap(parent));
        for (action, keys) in km {
            assert!(keys.iter().all(|key| is_valid_binding(key)), "{id}: {action}");
            let mut unique = normalized(keys);
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), keys.len(), "{id}: {action} lists a binding twice");
            for key in keys {
                for other in owners(km, key).into_iter().filter(|other| other != action) {
                    let inherited = owners(parent, key);
                    let reviewed =
                        SHARED.iter().any(|group| group.contains(&action.as_str()) && group.contains(&other.as_str()));
                    assert!(
                        (inherited.contains(action) && inherited.contains(&other)) || reviewed,
                        "{id}: {key}: {action} / {other}"
                    );
                }
            }
        }
    }
}

// src/platform/keymaps/eclipse.test.ts › %s › binds %s
/// The Eclipse presets put Open Resource, Open Type, Delete Line, Quick Fix, Organize Imports, Format and Toggle
/// Comment on Eclipse's keys with the platform's primary modifier.
#[test]
fn the_eclipse_presets_bind_eclipses_own_keys() {
    let cases = [
        ("navigate.gotoFile", "Shift+R"),
        ("navigate.gotoClass", "Shift+T"),
        ("code.deleteLine", "D"),
        ("code.showIntentions", "1"),
        ("code.optimizeImports", "Shift+O"),
        ("code.reformat", "Shift+F"),
        ("code.commentLine", "/"),
    ];
    for (id, _, _, modifier) in ECLIPSE {
        let km = keymap(id);
        for (action, suffix) in cases {
            let key = normalize_binding(&format!("{modifier}+{suffix}"));
            assert!(normalized(bound(km, action)).contains(&key), "{id}: {action} is on {key}");
        }
    }
}

// src/platform/keymaps/eclipse.test.ts › %s › preserves official removals and platform-specific bindings
/// The Eclipse presets keep Eclipse's own Go to Declaration and difference keys, leave Move Line, Go to Test, Replace
/// in Files and Push unbound, take Completion from IDEA, and go back and forward with the side buttons.
#[test]
fn the_eclipse_presets_preserve_official_removals_and_platform_specific_bindings() {
    for (id, _, parent, _) in ECLIPSE {
        let (km, parent) = (keymap(id), keymap(parent));
        assert_eq!(bound(km, "navigate.declaration"), ["F3"], "{id}");
        assert_eq!(bound(km, "diff.nextDifference"), ["Ctrl+F7"], "{id}");
        assert_eq!(bound(km, "diff.prevDifference"), ["Shift+F7"], "{id}");
        assert_eq!(bound(km, "code.moveStatementUp"), ["Alt+Up"], "{id}");
        assert_eq!(bound(km, "code.moveStatementDown"), ["Alt+Down"], "{id}");
        for action in ["code.moveLineUp", "code.moveLineDown", "navigate.gotoTest", "find.replaceInPath", "vcs.push"] {
            assert!(km.contains_key(action), "{id}: {action} is listed");
            assert!(bound(km, action).is_empty(), "{id}: {action} is unbound");
        }
        assert_eq!(bound(km, "code.completion"), bound(parent, "code.completion"), "{id}");
        assert!(bound(km, "navigate.back").contains(&"Button4".to_owned()), "{id}");
        assert!(bound(km, "navigate.forward").contains(&"Button5".to_owned()), "{id}");
    }
}

// src/platform/keymaps/eclipse.test.ts › %s › leaves undo / redo to the focused component
/// No Eclipse binding takes Undo's or Redo's keys from the focused component; on Windows Ctrl+Y stays free too.
#[test]
fn the_eclipse_presets_leave_undo_and_redo_to_the_focused_component() {
    for (id, _, _, modifier) in ECLIPSE {
        let km = keymap(id);
        for key in [format!("{modifier}+Z"), format!("{modifier}+Shift+Z")] {
            assert!(owners(km, &key).is_empty(), "{id}: {key} is taken by {:?}", owners(km, &key));
        }
        if modifier == "Ctrl" {
            assert!(owners(km, "Ctrl+Y").is_empty(), "{id}: Ctrl+Y is taken by {:?}", owners(km, "Ctrl+Y"));
        }
    }
}

// src/platform/keymaps/eclipse.test.ts › uses the bundled Run / Debug choices rather than assuming Ctrl becomes Cmd
/// Run and Debug are Eclipse's bundled keys on each platform, F11 alone is free on Windows, and no Windows Eclipse
/// binding uses the Windows key.
#[test]
fn the_eclipse_presets_use_the_bundled_run_and_debug_choices() {
    let (windows, mac) = (keymap("eclipse-windows"), keymap("eclipse-macos"));
    let keys = |keys: &[&str]| keys.iter().map(|key| normalize_binding(key)).collect::<Vec<_>>();
    assert_eq!(normalized(bound(windows, "run.debug")), keys(&["Alt+Shift+D", "Shift+F9"]));
    assert_eq!(normalized(bound(windows, "run.run")), keys(&["Alt+Shift+X", "Shift+F10"]));
    assert_eq!(bound(windows, "run.rerun"), ["Ctrl+F11"]);
    assert!(owners(windows, "F11").is_empty(), "{:?}", owners(windows, "F11"));
    assert_eq!(bound(mac, "run.debug"), ["Meta+F11"]);
    assert_eq!(bound(mac, "run.run"), ["Meta+Shift+F11"]);
    assert!(bound(mac, "run.rerun").is_empty());
    assert_eq!(bound(mac, "navigate.navigationBar"), ["Alt+Meta+B"]);
    assert!(windows.values().flatten().all(|key| !key.contains("Meta")));
}

/// The actions binding a key with the Windows (`Meta`) modifier.
fn using_meta(km: &Keymap) -> Vec<&str> {
    km.iter()
        .filter(|(_, keys)| keys.iter().any(|key| key.split(['+', ' ']).any(|part| part == "Meta")))
        .map(|(id, _)| id.as_str())
        .collect()
}

// src/platform/keymaps/windows.test.ts › Windows keymaps › cover the same actions as their macOS presets
/// The Windows IDEA and VS Code presets list exactly the actions of their macOS presets.
#[test]
fn the_windows_keymaps_cover_the_same_actions_as_their_macos_presets() {
    for (windows, mac) in [("intellij-windows", "intellij-macos"), ("vscode-windows", "vscode-macos")] {
        let ids = |id: &str| keymap(id).keys().cloned().collect::<Vec<_>>();
        assert_eq!(ids(windows), ids(mac), "{windows} / {mac}");
    }
}

// src/platform/keymaps/windows.test.ts › Windows keymaps › never bind the Windows key
/// No Windows IDEA or VS Code binding uses the Windows key.
#[test]
fn the_windows_keymaps_never_bind_the_windows_key() {
    assert!(using_meta(keymap("intellij-windows")).is_empty(), "{:?}", using_meta(keymap("intellij-windows")));
    assert!(using_meta(keymap("vscode-windows")).is_empty(), "{:?}", using_meta(keymap("vscode-windows")));
}

// src/platform/keymaps/windows.test.ts › Windows keymaps › IDEA %s → %s
/// IDEA's Windows preset binds Go to File, Find Action, Settings, Run and Comment Line to IDEA's Windows keys.
#[test]
fn the_idea_windows_preset_binds_ideas_windows_keys() {
    let km = keymap("intellij-windows");
    for (id, key) in [
        ("navigate.gotoFile", "Ctrl+Shift+N"),
        ("navigate.findAction", "Ctrl+Shift+A"),
        ("file.settings", "Ctrl+Alt+S"),
        ("run.run", "Shift+F10"),
        ("code.commentLine", "Ctrl+/"),
    ] {
        assert_eq!(bound(km, id), [key], "{id}");
    }
}

// src/platform/keymaps/windows.test.ts › Windows keymaps › VS Code reads ⌘ as Ctrl
/// The VS Code Windows preset spells VS Code's ⌘ keys and chords with Ctrl.
#[test]
fn the_vscode_windows_preset_reads_cmd_as_ctrl() {
    let km = keymap("vscode-windows");
    assert_eq!(bound(km, "navigate.gotoFile"), ["Ctrl+P"]);
    assert_eq!(bound(km, "code.quickDocumentation"), ["Ctrl+K Ctrl+I"]);
}

// src/platform/keymaps/windows.test.ts › host modifier helpers › macOS keeps ⌘
/// On macOS the default preset is IDEA's macOS one and ⌘ stays ⌘ in shortcuts, bindings and strokes.
#[test]
fn macos_keeps_cmd() {
    assert_eq!(default_preset(true), "intellij-macos");
    assert_eq!(format_binding("Meta+Shift+O", true), "⇧⌘O");
    assert_eq!(host_binding("Meta+C", true), "Meta+C");
    assert_eq!(mac_stroke("Ctrl+C", true), "Ctrl+C");
}

// src/platform/keymaps/windows.test.ts › host modifier helpers › Windows uses Ctrl and spells shortcuts out
/// Off macOS the default preset is IDEA's Windows one, ⌘ is read as Ctrl, and shortcuts are spelled out with
/// commas between strokes.
#[test]
fn windows_uses_ctrl_and_spells_shortcuts_out() {
    assert_eq!(default_preset(false), "intellij-windows");
    assert_eq!(format_binding("Ctrl+Shift+N", false), "Ctrl+Shift+N");
    assert_eq!(format_binding("Ctrl+K Ctrl+I", false), "Ctrl+K, Ctrl+I");
    assert_eq!(format_binding("Shift Shift", false), "Shift, Shift");
    assert_eq!(host_binding("Meta+Shift+C", false), "Ctrl+Shift+C");
    assert_eq!(mac_stroke("Ctrl+Shift+C", false), "Shift+Meta+C");
}

// src/platform/keymaps/navExtras.test.ts › IDEA macOS bindings for navigation and editor extras › ⇧⌘T is Go to Test
// only; Reopen Closed Tab has no default (VS Code keeps it)
/// In IDEA's macOS preset ⇧⌘T is Go to Test alone and Reopen Closed Tab is unbound; VS Code's keeps ⇧⌘T for it.
#[test]
fn shift_cmd_t_is_go_to_test_only_and_reopen_closed_tab_has_no_idea_default() {
    let (idea, vscode) = (keymap("intellij-macos"), keymap("vscode-macos"));
    let raw_owners: Vec<&str> = idea
        .iter()
        .filter(|(_, keys)| keys.iter().any(|key| key == "Meta+Shift+T"))
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(raw_owners, ["navigate.gotoTest"]);
    assert!(idea.contains_key("editor.reopenClosedTab") && bound(idea, "editor.reopenClosedTab").is_empty());
    assert_eq!(bound(vscode, "editor.reopenClosedTab"), ["Meta+Shift+T"]);
}

// src/platform/keymaps/navExtras.test.ts › IDEA macOS bindings for navigation and editor extras › %s → %s
/// IDEA's macOS preset binds the navigation and editor extras to IDEA's keys, and VS Code's lists each of them.
#[test]
fn the_idea_macos_preset_binds_the_navigation_and_editor_extras() {
    let (idea, vscode) = (keymap("intellij-macos"), keymap("vscode-macos"));
    for (id, key) in [
        ("navigate.lastEditLocation", "Meta+Shift+Backspace"),
        ("navigate.superMethod", "Meta+U"),
        ("navigate.nextMethod", "Ctrl+Shift+Down"),
        ("navigate.previousMethod", "Ctrl+Shift+Up"),
        ("navigate.navigationBar", "Meta+Up"),
        ("code.pasteFromHistory", "Meta+Shift+V"),
        ("code.unwrap", "Meta+Shift+Delete"),
        ("code.foldSelection", "Meta+."),
        ("editor.toggleColumnSelection", "Meta+Shift+8"),
    ] {
        assert!(bound(idea, id).contains(&key.to_owned()), "{id} is on {key}");
        assert!(vscode.contains_key(id), "VS Code lists {id}");
    }
}
