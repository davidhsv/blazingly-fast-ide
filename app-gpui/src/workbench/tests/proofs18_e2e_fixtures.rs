use super::*;

/// A label of a menu row as `PopupMenu::lines` gives it, without the mark of the selected row.
fn bare(line: &str) -> &str {
    line.strip_prefix("> ").unwrap_or(line)
}

/// A selector built at run time, as `debug_bounds` wants it.
pub(super) fn leaked(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

/// Clicks the drawn element `selector`, built at run time.
pub(super) fn click_at(cx: &mut VisualTestContext, selector: String) {
    cx.run_until_parked();
    let selector = leaked(selector);
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// A double click on the drawn element `selector`: one press with a click count of two.
pub(super) fn double_click_at(cx: &mut VisualTestContext, selector: String) {
    cx.run_until_parked();
    let selector = leaked(selector);
    let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    let (button, modifiers) = (gpui_kit::MouseButton::Left, gpui_kit::Modifiers::default());
    cx.simulate_event(gpui_kit::MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(gpui_kit::MouseUpEvent { position, modifiers, button, click_count: 2 });
    cx.run_until_parked();
}

/// A right click on the drawn element `selector`.
pub(super) fn right_click_at(cx: &mut VisualTestContext, selector: String) {
    cx.run_until_parked();
    let selector = leaked(selector);
    let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(position, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// Selects `path` in the Project view and right-clicks its row: the window's context menu opens.
pub(super) fn right_click_in_tree(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {
    select_in_tree(workbench, path, cx);
    right_click_at(cx, format!("project-row:{}", text_of(path)));
}

/// The labels of the rows of menu level `depth` of the window's open menu of commands `C`.
pub(super) fn menu_level<C: Clone + 'static>(
    workbench: &Entity<Workbench>,
    depth: usize,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    cx.run_until_parked();
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<C>()).expect("a menu is open");
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    lines.get(depth).map(|level| level.iter().map(|line| bare(line).to_owned()).collect()).unwrap_or_default()
}

/// Clicks the rows labelled `path` one level after the other in the window's open menu of commands
/// `C`: a click on a row with a submenu opens it, a click on the last one chooses it. Each row's
/// place is read once its level is drawn.
pub(super) fn click_menu_path<C: Clone + 'static>(
    workbench: &Entity<Workbench>,
    path: &[&str],
    cx: &mut VisualTestContext,
) {
    for (depth, label) in path.iter().enumerate() {
        click_menu_row::<C>(workbench, depth, label, cx);
    }
}

/// Clicks the row labelled `label` of the open level `depth` of the window's menu of commands `C`.
pub(super) fn click_menu_row<C: Clone + 'static>(
    workbench: &Entity<Workbench>,
    depth: usize,
    label: &str,
    cx: &mut VisualTestContext,
) {
    let rows = menu_level::<C>(workbench, depth, cx);
    let index = rows.iter().position(|row| row == label).unwrap_or_else(|| panic!("{label} in {rows:?}"));
    click_at(cx, format!("menu-row-{depth}-{index}"));
}

/// Shift pressed and released twice, as the window's root view receives it.
pub(super) fn double_shift(cx: &mut VisualTestContext) {
    let shift = gpui_kit::Modifiers { shift: true, ..gpui_kit::Modifiers::default() };
    for _ in 0..2 {
        cx.simulate_modifiers_change(shift);
        cx.simulate_modifiers_change(gpui_kit::Modifiers::default());
    }
    cx.run_until_parked();
}

/// The platform's primary modifier with `key`: ⌘ on macOS, Ctrl elsewhere.
pub(super) fn primary(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-{key}") }
}

/// Opens `path` of the project from the Project view: the row selected, then Enter.
pub(super) fn open_from_tree(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {
    select_in_tree(workbench, path, cx);
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
}
