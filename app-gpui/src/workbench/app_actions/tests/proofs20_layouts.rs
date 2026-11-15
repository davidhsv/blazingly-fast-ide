//! Window › Tool Window Layouts through the window: the sizes a layout keeps, a save under a taken name, and the
//! saved layouts read back from the UI storage.

use super::*;

/// The names of the saved layouts, and the one last saved or restored.
fn saved(cx: &mut VisualTestContext) -> (Vec<String>, Option<String>) {
    cx.update(|_, cx| {
        let layouts = named_layouts(cx);
        (layouts.names().into_iter().map(str::to_owned).collect(), layouts.active().map(str::to_owned))
    })
}

/// Save Current Layout as… under `name`, typed into its prompt.
fn save_as(workbench: &Entity<Workbench>, name: &str, cx: &mut VisualTestContext) {
    run_action(cx, "layout.save", None);
    type_in_prompt(workbench, name, cx);
    click_on(cx, "prompt-button-ok");
}

/// The labels of the rows of the menu of saved layouts that is open.
fn layout_menu(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<String>()).expect("the saved layouts are offered");
    menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| row.label.clone()).collect())
}

// src/workbench/toolwindows/layouts.test.ts › saves and restores visible windows and sizes by name
/// A saved layout keeps the windows that show and the sizes of the sides: the Project view widened a step and the
/// Run window shown come back after the default layout replaced them; restoring a name that is not saved changes
/// nothing.
#[gpui_kit::test]
async fn saves_and_restores_visible_windows_and_sizes_by_name(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
    let visible = |cx: &mut VisualTestContext, id: &str| workbench.read_with(cx, |w, _| w.is_tool_window_visible(id));
    let left = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| w.tool_windows().expect("a project is open").sizes().left)
    };
    assert!(context_is_on(cx, "toolWindowFocus"), "the Project view has the keyboard");
    let default_width = left(cx);
    run_action(cx, "toolwindow.resizeRight", None);
    let wide = left(cx);
    assert!(wide > default_width, "{wide} > {default_width}");
    run_action(cx, "toolwindow.run", None);
    assert!(visible(cx, RUN_TOOL_WINDOW) && visible(cx, "project"));
    save_as(&workbench, "Debugging", cx);
    assert_eq!(saved(cx), (vec!["Debugging".to_owned()], Some("Debugging".to_owned())));

    run_action(cx, "layout.restoreDefault", None);
    assert!(!visible(cx, RUN_TOOL_WINDOW), "the default layout shows no Run window");
    assert_eq!(left(cx), default_width, "the default layout has the default width");

    run_action(cx, "layout.restore", Some(Value::from("Debugging")));
    assert!(visible(cx, RUN_TOOL_WINDOW) && visible(cx, "project"), "the saved windows show again");
    assert_eq!(left(cx), wide, "the saved width is back");
    assert_eq!(saved(cx).1.as_deref(), Some("Debugging"));

    run_action(cx, "layout.restore", Some(Value::from("Missing")));
    assert!(visible(cx, RUN_TOOL_WINDOW) && visible(cx, "project"));
    assert_eq!(left(cx), wide);
    assert_eq!(saved(cx).1.as_deref(), Some("Debugging"), "an unknown name restores nothing");
    close(cx);
}

// src/workbench/toolwindows/layouts.test.ts › replaces a layout of the same name, renames and deletes
/// Saving under a name that is taken replaces that layout in its place: the list keeps one entry per name and the
/// name restores what was saved last. Rename takes a new name and Delete removes the layout, which is then no longer
/// the active one.
#[gpui_kit::test]
async fn replaces_a_layout_of_the_same_name_renames_and_deletes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
    let run_shows =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.is_tool_window_visible(RUN_TOOL_WINDOW));
    save_as(&workbench, "A", cx);
    save_as(&workbench, "B", cx);
    run_action(cx, "toolwindow.run", None);
    assert!(run_shows(cx));
    save_as(&workbench, "A", cx);
    assert_eq!(saved(cx), (vec!["A".to_owned(), "B".to_owned()], Some("A".to_owned())));

    run_action(cx, "layout.restore", Some(Value::from("B")));
    assert!(!run_shows(cx), "B was saved without the Run window");
    run_action(cx, "layout.restore", Some(Value::from("A")));
    assert!(run_shows(cx), "A is the layout saved last under its name");

    // Rename: the first layout is chosen, then named.
    run_action(cx, "layout.rename", None);
    assert_eq!(layout_menu(&workbench, cx), ["A", "B"]);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    type_in_prompt(&workbench, "C", cx);
    click_on(cx, "prompt-button-ok");
    assert_eq!(saved(cx), (vec!["C".to_owned(), "B".to_owned()], Some("C".to_owned())));

    // Delete, once confirmed.
    run_action(cx, "layout.delete", None);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click_on(cx, "prompt-button-ok");
    assert_eq!(saved(cx), (vec!["B".to_owned()], None));
    close(cx);
}

// src/workbench/toolwindows/layouts.test.ts › persists across reloads and ignores malformed entries
/// The saved layouts are read back from the UI storage each time they are offered: the one saved is listed, and
/// stored entries without a name are left out while one whose layout is malformed is offered and restores the
/// default layout.
#[gpui_kit::test]
async fn persists_across_reloads_and_ignores_malformed_entries(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
    save_as(&workbench, "Kept", cx);
    let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(NAMED_LAYOUTS_STORAGE_KEY).map(str::to_owned));
    assert_eq!(NamedLayouts::parse(stored.as_deref()).names(), ["Kept"], "the storage holds it: {stored:?}");
    run_action(cx, "layout.restore", None);
    assert_eq!(layout_menu(&workbench, cx), ["Kept"]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let malformed = r#"{"layouts":[{"name":""},{"nope":1},{"name":"X","layout":5}]}"#;
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(NAMED_LAYOUTS_STORAGE_KEY, malformed.to_owned()));
    run_action(cx, "toolwindow.run", None);
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(RUN_TOOL_WINDOW)));
    run_action(cx, "layout.restore", None);
    assert_eq!(layout_menu(&workbench, cx), ["X"], "the entries without a name are left out");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(saved(cx).1.as_deref(), Some("X"));
    let (project_shows, run_shows) = workbench
        .read_with(cx, |w, _| (w.is_tool_window_visible("project"), w.is_tool_window_visible(RUN_TOOL_WINDOW)));
    assert!(project_shows && !run_shows, "a malformed layout restores the default one");
    close(cx);
}
