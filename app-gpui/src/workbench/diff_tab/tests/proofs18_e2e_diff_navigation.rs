use super::super::super::tests::click_on;
use super::*;
use crate::vcs_diff::diff_input::{DiffSide, with_chain};
use gpui_kit::Focusable;

/// A diff of `name` under the project, from `before` at HEAD to `after` in the working tree, both given as
/// texts.
fn text_diff(root: &std::path::Path, name: &str, before: &str, after: &str) -> DiffData {
    let path = root.join(name).to_string_lossy().into_owned();
    DiffData::new(&path, DiffSide::text(before, "HEAD"), DiffSide::text(after, "Working tree"))
}

/// Opens the first diff of a chain of `files` (name, text at HEAD, working text) in the preview tab, side by
/// side with nothing collapsed, and waits for its one difference.
fn open_chain(
    workbench: &Entity<Workbench>,
    root: &std::path::Path,
    files: &[(&str, &str, &str)],
    cx: &mut VisualTestContext,
) {
    cx.update(|_, cx| {
        cx.default_global::<UiStorage>().set(DIFF_OPTIONS_KEY, r#"{"mode":"split","collapse":false}"#.to_owned());
    });
    let items: Vec<DiffData> = files.iter().map(|(name, before, after)| text_diff(root, name, before, after)).collect();
    let data = with_chain(&items, 0).expect("the chain has a first file");
    workbench.update_in(cx, |w, window, cx| w.open_diff_tabs(vec![DiffTab { data, preview: true }], window, cx));
    wait_long(cx, "the first diff of the chain", |cx| counter(workbench, cx).as_deref() == Some("1 difference"));
}

/// Gives the keyboard to the viewer of the active diff tab.
fn focus_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let viewer = workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).expect("a diff tab is active");
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.run_until_parked();
}

fn hint(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<&'static str> {
    workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx).and_then(|viewer| viewer.read(cx).nav_hint()))
}

fn viewer_width(cx: &mut VisualTestContext) -> f32 {
    cx.run_until_parked();
    f32::from(cx.debug_bounds("diff-view").expect("the diff is drawn").size.width)
}

/// The active tab's name starts with `name` and its counter says `counter`.
fn at(workbench: &Entity<Workbench>, name: &str, counter_says: &str, cx: &mut VisualTestContext) -> bool {
    selected_tab(workbench, cx).is_some_and(|tab| tab.starts_with(name))
        && counter(workbench, cx).as_deref() == Some(counter_says)
}

const AB: [(&str, &str, &str); 2] =
    [("a.ts", "same\nbefore\ntail\n", "same\nafter\ntail\n"), ("b.ts", "same\nbefore\ntail\n", "same\nafter\ntail\n")];

/// A changed file's diff in the preview tab beside the Project view: Maximize Editor from the toolbar hides the
/// Project view and draws the diff wider, still at "1 difference"; Restore Size brings the Project view back
/// and draws the diff at its width again.
#[gpui_kit::test]
async fn maximizes_a_diff_preview_tab_and_restores_its_previous_layout(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, init_repo);
    open_chain(&workbench, &root, &AB, cx);
    let project_shown =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.is_tool_window_visible(PROJECT_TOOL_WINDOW));
    assert!(project_shown(cx), "the Project view is shown");
    let before = viewer_width(cx);

    click_on(cx, "diff-maximize");
    assert!(!project_shown(cx), "the Project view is hidden");
    let viewer = workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).expect("a diff tab is active");
    assert_eq!(maximize_button(&viewer, cx), Some(("Restore Size", true)));
    assert!(viewer_width(cx) > before + 100.0, "the diff is drawn wider");
    assert_eq!(counter(&workbench, cx).as_deref(), Some("1 difference"));

    click_on(cx, "diff-maximize");
    assert!(project_shown(cx), "the Project view is back");
    assert!((viewer_width(cx) - before).abs() < 2.0, "the diff is drawn at its width again");
    assert_eq!(counter(&workbench, cx).as_deref(), Some("1 difference"));
}

/// The diffs of a.ts and b.ts in a chain, in the preview tab: Next Difference from the toolbar goes to a.ts's one
/// difference, again says that pressing again goes to the next file, and F7 then shows b.ts at its difference in
/// the preview tab. Previous Difference says the same of the previous file, and pressed again shows a.ts.
#[gpui_kit::test]
async fn continues_between_changed_files_from_toolbar_buttons_and_the_keymap(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, init_repo);
    open_chain(&workbench, &root, &AB, cx);

    click_on(cx, "diff-next");
    wait_long(cx, "on the difference", |cx| at(&workbench, "a.ts", "1 of 1 differences", cx));
    click_on(cx, "diff-next");
    assert_eq!(hint(&workbench, cx), Some("Press again to go to the next file"));
    assert!(at(&workbench, "a.ts", "1 of 1 differences", cx), "still a.ts");
    // The diff has the keyboard: F7 is Next Difference.
    focus_diff(&workbench, cx);
    cx.simulate_keystrokes("f7");
    wait_long(cx, "b.ts's diff", |cx| at(&workbench, "b.ts", "1 of 1 differences", cx));
    assert_eq!(diff_tabs(&workbench, cx).len(), 1, "b.ts took the preview tab");

    click_on(cx, "diff-previous");
    assert_eq!(hint(&workbench, cx), Some("Press again to go to the previous file"));
    click_on(cx, "diff-previous");
    wait_long(cx, "a.ts's diff again", |cx| at(&workbench, "a.ts", "1 of 1 differences", cx));
}

/// a.ts, a binary file, an unchanged one and z.ts in a chain, with the left side maximized and every tool window
/// hidden: F7 twice from a.ts's difference goes to z.ts past the binary and unchanged files, a third F7 stays;
/// ⇧F7 twice comes back to a.ts; Restore Size brings back the Project view with the left side maximized again.
#[gpui_kit::test]
async fn skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, init_repo);
    open_chain(&workbench, &root, &AB, cx);
    let files = [
        ("a.ts", "before\n", "after\n"),
        ("binary.dat", "before\n", "\0binary"),
        ("unchanged.ts", "before\n", "before\n"),
        ("z.ts", "before\n", "after\n"),
    ];
    open_chain(&workbench, &root, &files, cx);
    workbench.update_in(cx, |w, window, cx| {
        w.with_layout(window, cx, |layout| layout.toggle_maximize(PROJECT_TOOL_WINDOW));
        w.with_layout(window, cx, ToolWindowLayout::hide_all);
    });
    cx.run_until_parked();
    assert!(visible_tool_windows(&workbench, cx).is_empty());
    // The caret on the first line, inside a.ts's only difference.
    let viewer = workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).expect("a diff tab is active");
    viewer.update(cx, |viewer, cx| viewer.reveal_line(Side::B, 0, cx));
    focus_diff(&workbench, cx);

    cx.simulate_keystrokes("f7 f7");
    wait_long(cx, "z.ts", |cx| at(&workbench, "z.ts", "1 of 1 differences", cx));
    cx.simulate_keystrokes("f7");
    cx.run_until_parked();
    assert!(at(&workbench, "z.ts", "1 of 1 differences", cx), "z.ts is the last file with changes");
    cx.simulate_keystrokes("shift-f7 shift-f7");
    wait_long(cx, "a.ts", |cx| at(&workbench, "a.ts", "1 of 1 differences", cx));
    assert_eq!(diff_tabs(&workbench, cx).len(), 1, "each file took the preview tab");
    assert!(cx.debug_bounds("diff-toolbar").is_some(), "the diff's toolbar is drawn before Restore Size");

    click_on(cx, "diff-maximize");
    assert_eq!(maximized_side(&workbench, cx), Some(Anchor::Left));
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(PROJECT_TOOL_WINDOW)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("diff-toolbar").is_none(), "the maximized Project view covers the diff's toolbar");
    let root_row: &'static str = Box::leak(format!("project-row:{}", root.to_string_lossy()).into_boxed_str());
    assert!(cx.debug_bounds(root_row).is_some(), "the Project tree is drawn");
}
