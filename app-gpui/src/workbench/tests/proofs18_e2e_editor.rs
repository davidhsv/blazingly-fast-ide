use super::proofs18_e2e_fixtures::{click_at, open_from_tree, primary};
use super::*;
use crate::editor::find_bar::FindBar;
use crate::editor::marks::MarkLayer;

const BUILD: &str = concat!(
    "import com.acme.orders.model.Order\n",
    "\n",
    "class OrderController(private val service: OrderService) {\n",
    "    fun create(req: Req): OrderDto {\n",
    "        val order: Order = service.create(req)\n",
    "        return order.toDto()\n",
    "    }\n",
    "\n",
    "    fun toDto(): OrderDto = OrderDto()\n",
    "}\n",
);
const SETTINGS: &str = "rootProject.name = \"acme\"\n@PostMapping\nfun list(status: Status) = service.find(status)\n";

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

/// The acme files in a trusted project, open in an active window.
fn acme(cx: &mut TestAppContext) -> Opened<'_> {
    let files = [("build.gradle.kts", BUILD), ("settings.gradle.kts", SETTINGS), ("README.md", "# acme\n")];
    let (folder, root, workbench, cx) = trusted_project(cx, &files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

fn editor_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Editor> {
    workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
}

fn caret_item(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |w, cx| w.status_items(cx)).first().cloned().unwrap_or_default()
}

fn bar_query(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let bar: Entity<FindBar> = workbench.read_with(cx, |w, _| w.find_bar())?;
    Some(bar.read_with(cx, |bar, cx| bar.find_field().read(cx).text().to_owned()))
}

/// Selects what the find field holds and types `text` over it.
fn type_in_bar(workbench: &Entity<Workbench>, text: &str, cx: &mut VisualTestContext) {
    let bar = workbench.read_with(cx, |w, _| w.find_bar()).expect("the bar is open");
    let field = bar.read_with(cx, |bar, _| bar.find_field().clone());
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    field.update(cx, |field, cx| field.select_all(cx));
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Opened from the Project view, the file takes ⌘↓, Enter and typing at its end; ⌘Z takes the typing back.
#[gpui_kit::test]
async fn opens_a_file_from_the_project_tree_and_types_into_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = acme(cx);
    open_from_tree(&workbench, &root.join("build.gradle.kts"), cx);
    let editor = editor_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes(&primary("down"));
    cx.simulate_keystrokes("enter");
    cx.simulate_input("// typed by e2e");
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.text()).contains("// typed by e2e"));
    cx.simulate_keystrokes(&primary("z"));
    cx.run_until_parked();
    assert!(!editor.read_with(cx, |e, _| e.text()).contains("// typed by e2e"), "undo takes the typing back");
    close_backend_project(cx);
}

/// After typing a line in README.md, the Project tree takes the keyboard, the keymap becomes the Windows one and
/// the Project view moves to the right; back in the editor, Delete takes the character after the caret and
/// Backspace the one before.
#[gpui_kit::test]
async fn delete_edits_text_after_the_focused_project_tree_is_remounted(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = acme(cx);
    open_from_tree(&workbench, &root.join("README.md"), cx);
    let editor = editor_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes(&primary("down"));
    cx.simulate_keystrokes("enter");
    cx.simulate_input("delete probe abc");
    cx.run_until_parked();
    let probe_line = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.text()).lines().find(|line| line.starts_with("delete probe")).map(str::to_owned)
    };
    assert_eq!(probe_line(cx).as_deref(), Some("delete probe abc"));

    let tree = project_tree_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
    let keymap = crate::platform::settings::SettingsWrite {
        key: crate::settings_view::keymap::KEYMAP_KEY,
        value: serde_json::json!("intellij-windows"),
    };
    cx.update(|_, cx| crate::platform::settings::set_global("w1", &[keymap], cx)).expect("the keymap is stored");
    workbench.update_in(cx, |w, window, cx| {
        w.with_layout(window, cx, |layout| layout.move_window(PROJECT_TOOL_WINDOW, Slot::Right, None))
    });
    cx.run_until_parked();
    let moved = workbench.read_with(cx, |w, _| w.tool_windows().and_then(|layout| layout.slot_of(PROJECT_TOOL_WINDOW)));
    assert_eq!(moved, Some(Slot::Right));
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(PROJECT_TOOL_WINDOW)), "it shows on the right");

    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes("left delete");
    cx.run_until_parked();
    assert_eq!(probe_line(cx).as_deref(), Some("delete probe ab"));
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(probe_line(cx).as_deref(), Some("delete probe a"));
    close_backend_project(cx);
}

/// ⌘F in the editor opens the find bar with the keyboard in it; "Order" typed marks its occurrences; Escape
/// closes the bar and gives the editor the keyboard back.
#[gpui_kit::test]
async fn cmd_f_opens_the_find_bar_and_highlights_matches_escape_returns_to_the_editor(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = acme(cx);
    open_from_tree(&workbench, &root.join("build.gradle.kts"), cx);
    let editor = editor_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes(&primary("f"));
    cx.run_until_parked();
    let bar = workbench.read_with(cx, |w, _| w.find_bar()).expect("⌘F opens the bar");
    assert!(cx.update(|window, cx| bar.read(cx).find_field().focus_handle(cx).is_focused(window)));
    cx.simulate_input("Order");
    cx.run_until_parked();
    assert!(!editor.read_with(cx, |e, _| e.marks(MarkLayer::Find).is_empty()), "the matches are marked");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.find_bar().is_none()), "the bar is hidden");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)), "the editor has the keyboard");
    close_backend_project(cx);
}

/// Text selected inside a highlighted find match stays visible: the selection is painted again over the
/// match's background, on the row where the two meet.
#[gpui_kit::test]
async fn the_selection_inside_a_find_match_is_painted_over_the_matchs_background(cx: &mut TestAppContext) {
    use crate::editor::view::PaintedLayer;
    let _home = start(cx);
    let (_folder, root, workbench, cx) = acme(cx);
    open_from_tree(&workbench, &root.join("build.gradle.kts"), cx);
    let editor = editor_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes(&primary("f"));
    cx.run_until_parked();
    cx.simulate_input("Order");
    cx.run_until_parked();
    assert!(!editor.read_with(cx, |e, _| e.marks(MarkLayer::Find).is_empty()), "the matches are marked");
    // "rde" inside the "Order" of "OrderController", on the third line.
    let (from, to) = editor.read_with(cx, |e, _| (e.offset_of(2, 7), e.offset_of(2, 10)));
    editor.update(cx, |e, cx| e.select(from, to, cx));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let layers = editor.read_with(cx, |e, _| e.painted_layers());
    let marks = layers.iter().position(|layer| *layer == PaintedLayer::MarkBackgrounds(2));
    let selected = layers.iter().position(|layer| *layer == PaintedLayer::SelectedText(2));
    assert!(marks.is_some() && selected > marks, "the selection goes over the match: {layers:?}");
    close_backend_project(cx);
}

/// Each file keeps its own find bar, query and caret: a second file has no bar until ⌘F, which starts from the
/// last search without moving its caret; a third starts from the latest search; switching back shows each
/// file's own query with its caret, and a bar closed with Escape leaves the others as they were.
#[gpui_kit::test]
async fn each_file_keeps_its_own_find_bar_query_and_caret(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = acme(cx);
    let open = |cx: &mut VisualTestContext, name: &str| {
        let path = root.join(name);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
        let editor = editor_of(&workbench, cx);
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        cx.run_until_parked();
        editor
    };
    let find = |cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(&primary("f"));
        cx.run_until_parked();
    };

    open(cx, "build.gradle.kts");
    find(cx);
    type_in_bar(&workbench, "Order", cx);
    assert!(caret_item(&workbench, cx).contains("(5 chars)"), "{}", caret_item(&workbench, cx));
    let caret_a = caret_item(&workbench, cx);

    let b = open(cx, "settings.gradle.kts");
    assert_eq!(bar_query(&workbench, cx), None, "another file has no bar");
    let at = SETTINGS.find("@PostMapping").expect("the line") as u32;
    b.update(cx, |e, cx| e.select(at, at, cx));
    cx.run_until_parked();
    let caret_b = caret_item(&workbench, cx);
    find(cx);
    assert_eq!(bar_query(&workbench, cx).as_deref(), Some("Order"), "it starts from the last search");
    assert_eq!(caret_item(&workbench, cx), caret_b, "the caret stays");
    type_in_bar(&workbench, "status", cx);
    assert!(caret_item(&workbench, cx).contains("(6 chars)"), "{}", caret_item(&workbench, cx));
    let caret_b2 = caret_item(&workbench, cx);

    open(cx, "README.md");
    assert_eq!(bar_query(&workbench, cx), None);
    find(cx);
    assert_eq!(bar_query(&workbench, cx).as_deref(), Some("status"), "a third file starts from the latest search");

    open(cx, "build.gradle.kts");
    assert_eq!(bar_query(&workbench, cx).as_deref(), Some("Order"));
    assert_eq!(caret_item(&workbench, cx), caret_a);
    open(cx, "settings.gradle.kts");
    assert_eq!(bar_query(&workbench, cx).as_deref(), Some("status"));
    assert_eq!(caret_item(&workbench, cx), caret_b2);
    let bar = workbench.read_with(cx, |w, _| w.find_bar()).expect("the bar");
    cx.update(|window, cx| window.focus(&bar.read(cx).find_field().focus_handle(cx), cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(bar_query(&workbench, cx), None, "Escape closes this file's bar");
    open(cx, "build.gradle.kts");
    assert_eq!(bar_query(&workbench, cx).as_deref(), Some("Order"), "the other file's bar stays");
    close_backend_project(cx);
}

/// Two files opened from the Project view are two tabs; ⌘W in the editor closes the active one, and
/// build.gradle.kts is left.
#[gpui_kit::test]
async fn opening_several_files_creates_tabs_and_cmd_w_closes_the_active_one(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = acme(cx);
    open_from_tree(&workbench, &root.join("build.gradle.kts"), cx);
    open_from_tree(&workbench, &root.join("settings.gradle.kts"), cx);
    let names = |cx: &mut VisualTestContext| -> Vec<String> {
        workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|tab| tab.name).collect())
    };
    assert_eq!(names(cx).len(), 2);
    let editor = editor_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-w" } else { "ctrl-f4" });
    cx.run_until_parked();
    assert_eq!(names(cx), ["build.gradle.kts"]);
    close_backend_project(cx);
}

/// ⌘R opens the replace bar; OrderDto replaced with OrderView by Replace All leaves no OrderDto and two lines
/// with OrderView, and one undo in the editor brings both OrderDto lines back.
#[gpui_kit::test]
async fn cmd_r_replaces_every_match_in_the_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = acme(cx);
    open_from_tree(&workbench, &root.join("build.gradle.kts"), cx);
    let editor = editor_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes(&primary("r"));
    cx.run_until_parked();
    let bar = workbench.read_with(cx, |w, _| w.find_bar()).expect("⌘R opens the bar");
    assert!(bar.read_with(cx, |bar, _| bar.is_replace()), "it is the replace bar");
    type_in_bar(&workbench, "OrderDto", cx);
    let replace = bar.read_with(cx, |bar, _| bar.replace_field().clone());
    cx.update(|window, cx| window.focus(&replace.focus_handle(cx), cx));
    cx.simulate_input("OrderView");
    cx.run_until_parked();
    click_at(cx, "find-bar-replace-all".to_owned());
    let lines_with = |cx: &mut VisualTestContext, word: &str| {
        editor.read_with(cx, |e, _| e.text()).lines().filter(|line| line.contains(word)).count()
    };
    assert_eq!(lines_with(cx, "OrderDto"), 0);
    assert_eq!(lines_with(cx, "OrderView"), 2);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes(&primary("z"));
    cx.run_until_parked();
    assert_eq!(lines_with(cx, "OrderDto"), 2, "one undo takes the whole replacement back");
    close_backend_project(cx);
}
