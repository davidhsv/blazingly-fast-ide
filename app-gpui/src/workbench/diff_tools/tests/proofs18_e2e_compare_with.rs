use super::*;
use crate::project_view::context_menu::ProjectCommand;

/// A right click on README.md in the Project view and a click on Compare With… asks for the other file; the
/// file chosen, outside the project, is shown beside README.md in the diff window, named in its title.
#[gpui_kit::test]
async fn compare_with_compares_against_a_file_picked_in_the_dialog(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("README.md", "readme text\n")]);
    let elsewhere = tempfile::tempdir().expect("a temporary folder");
    let other = elsewhere.path().canonicalize().expect("the folder exists").join("other.md");
    std::fs::write(&other, "other text\n").expect("the file is written");
    cx.update(|window, _| window.activate_window());
    let readme = root.join("README.md");
    select_in_tree(&workbench, &readme, cx);
    cx.run_until_parked();
    let selector: &'static str = Box::leak(format!("project-row:{}", text_of(&readme)).into_boxed_str());
    let row = cx.debug_bounds(selector).expect("the row is drawn").center();
    cx.simulate_mouse_down(row, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(row, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("the context menu is open");
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    let index = lines[0]
        .iter()
        .position(|line| line.trim_start_matches("> ") == "Compare With…")
        .unwrap_or_else(|| panic!("Compare With… in {lines:?}"));
    let item: &'static str = Box::leak(format!("menu-row-0-{index}").into_boxed_str());
    let at = cx.debug_bounds(item).expect("the row is drawn").center();
    cx.simulate_click(at, gpui_kit::Modifiers::default());
    wait_long(cx, "the file to compare with is asked for", |cx| cx.did_prompt_for_paths());
    let chosen = other.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![chosen.clone()]));
    wait_long(cx, "both files side by side", |cx| {
        changed_sides(&workbench, cx) == sides(&["readme text"], &["other text"])
    });
    let title = diff_title(&workbench, cx).unwrap_or_default();
    assert!(title.contains("other.md"), "the diff names the chosen file: {title}");
    close_project(cx);
}
