use super::*;
use crate::code_insight::usages::{Usage, usage_key};
use crate::code_insight::usages_view::RowKey;
use crate::editor::buffer;
use crate::editor::marks::{MarkKind, MarkLayer};
use crate::everywhere::bookmarks_popup::BookmarksPopup;
use crate::find::popup_view::SEARCH_DEBOUNCE;
use crate::find::session::FindStatus;
use crate::ui::code_preview::CodePreview;
use serde_json::json;

/// The file the Find in Files cases preview: a Rust file, so that its keywords are coloured.
const ORIGINAL: &str = "fn a() {}\nfn order() {}\nfn b() { order() }\n";

/// The shortcut of `key` with the platform's command modifier (⌘ on macOS, Ctrl elsewhere).
fn command(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-{key}") }
}

/// Waits for the popup's search to finish and returns its rows.
fn searched(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
    for _ in 0..3000 {
        cx.executor().advance_clock(SEARCH_DEBOUNCE);
        cx.run_until_parked();
        if popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done && !popup.is_stale()) {
            return popup.read_with(cx, |popup, _| popup.lines());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the search did not finish within thirty seconds");
}

fn find_popup(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<FindPopup> {
    workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open")
}

/// Waits for `preview` to show `path` and returns the editor it shows it in.
#[track_caller]
fn previewed(preview: &Entity<CodePreview>, path: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
    wait_long(cx, "the preview of the file", |cx| {
        preview.read_with(cx, |preview, _| preview.path() == Some(path) && preview.editor().is_some())
    });
    preview.read_with(cx, |preview, _| preview.editor().cloned()).expect("the preview has an editor")
}

/// The ranges `editor` marks as `kind`, each as its 0-based line and its text.
fn marked(editor: &Entity<Editor>, kind: MarkKind, cx: &mut VisualTestContext) -> Vec<(u32, String)> {
    editor.read_with(cx, |editor, _| {
        let rope = editor.rope();
        let marks = editor.marks(MarkLayer::Find).iter().filter(|mark| mark.kind == kind);
        let text = |range: &std::ops::Range<u32>| buffer::slice(rope, range.start, range.end);
        marks.map(|mark| (buffer::line_col(rope, mark.range.start).0, text(&mark.range))).collect()
    })
}

/// Gives the keyboard to `editor`, in the active window.
fn focus(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.run_until_parked();
}

/// Find in Files with `query` searched, its preview on the first result of `path`, which it returns
/// with the popup.
fn find_previewed(
    workbench: &Entity<Workbench>,
    query: &str,
    path: &str,
    cx: &mut VisualTestContext,
) -> (Entity<FindPopup>, Entity<Editor>) {
    run_action(cx, "find.inPath", Some(json!({ "query": query })));
    let popup = find_popup(workbench, cx);
    assert!(!searched(&popup, cx).is_empty(), "{query} is found");
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    let editor = previewed(&preview, path, cx);
    (popup, editor)
}

/// The preview of the selected result is an editor on the file's document, editable, in the editor's
/// colours: the selected line's matches are current, the others marked, the line tinted and the
/// keywords coloured as the editor colours them.
#[gpui_kit::test]
async fn shows_the_file_editable_in_an_editor_view_with_the_matches_highlighted(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let path = text_of(&root.join("src/a.rs"));
    let (_popup, editor) = find_previewed(&workbench, "order", &path, cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.text(), ORIGINAL);
        assert!(!editor.is_read_only(), "the preview edits the document");
        assert_eq!(editor.caret_position(), (1, 0), "the caret is on the selected result's line");
    });
    assert_eq!(marked(&editor, MarkKind::CurrentFindOccurrence, cx), [(1, "order".to_owned())]);
    assert_eq!(marked(&editor, MarkKind::FindOccurrence, cx), [(2, "order".to_owned())]);
    assert_eq!(marked(&editor, MarkKind::CurrentLine, cx), [(1, "fn order() {}".to_owned())]);
    let keyword = cx.update(|_, cx| theme::color(cx, "syn-keyword"));
    wait_long(cx, "the line drawn tinted with its keyword coloured", |cx| {
        editor.read_with(cx, |editor, _| {
            !editor.painted_marks(MarkKind::CurrentLine).is_empty() && editor.painted_color_at(1, 0) == Some(keyword)
        })
    });
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// Not from a spec: what is typed in the preview is in the document the open tab shows, and the save
// shortcut pressed in the preview writes it to disk.
#[gpui_kit::test]
async fn an_edit_in_the_preview_reaches_the_open_tab_and_the_save_shortcut_writes_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let file = root.join("src/a.rs");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path())));
    let tab = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the tab's editor");
    let (_popup, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
    focus(&editor, cx);
    cx.simulate_input("// x ");
    let edited = ORIGINAL.replacen("fn order", "// x fn order", 1);
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), edited);
    wait_long(cx, "the edit in the tab", |cx| tab.read_with(cx, |tab, _| tab.text()) == edited);
    assert_eq!(marked(&editor, MarkKind::CurrentFindOccurrence, cx), [(1, "order".to_owned())], "the mark moves along");
    cx.simulate_keystrokes(&command("s"));
    wait_long(cx, "the save", |_| std::fs::read_to_string(&file).is_ok_and(|text| text == edited));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// Edits made in a preview with no tab of the file outlive the popup: the preview of the file shows them
/// again, and they can still be undone there.
#[gpui_kit::test]
async fn keeps_dirty_text_and_undo_when_the_preview_closes_and_reopens_without_a_tab(cx: &mut TestAppContext) {
    let _home = start(cx);
    // Autosave would save the edits as the preview goes.
    cx.update(|cx| {
        let off = serde_json::Value::from(false);
        crate::platform::settings::set(
            "w1",
            ide_workspace::settings::SettingsScope::Global,
            "files.autoSave",
            &off,
            cx,
        )
        .expect("the setting is stored");
    });
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let file = root.join("src/a.rs");
    let path = text_of(&file);
    let (_popup, editor) = find_previewed(&workbench, "order", &path, cx);
    focus(&editor, cx);
    // Typed without a pause: one step of the undo history.
    cx.simulate_input("edited");
    let edited = ORIGINAL.replacen("fn order", "editedfn order", 1);
    wait_long(cx, "the document to have unsaved edits", |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()), "Escape closes the popup");
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let open_text = |host: &ide_host::Host| host.project("w1").ok().and_then(|p| p.documents.text_of(&file));
    assert_eq!(open_text(&host), Some(edited.clone()), "the document stays open with the edits");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), ORIGINAL, "nothing is saved");

    let (_popup, reopened) = find_previewed(&workbench, "order", &path, cx);
    assert_eq!(reopened.read_with(cx, |editor, _| editor.text()), edited);
    focus(&reopened, cx);
    cx.simulate_keystrokes(&command("z"));
    assert_eq!(reopened.read_with(cx, |editor, _| editor.text()), ORIGINAL);
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// Enter in the preview breaks the line, Backspace edits the text and leaves the query and the results
/// alone, and Escape closes the popup.
#[gpui_kit::test]
async fn enter_inserts_a_newline_editing_keys_stay_out_of_the_results_escape_bubbles(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let (popup, editor) = find_previewed(&workbench, "order", &text_of(&root.join("src/a.rs")), cx);
    let rows = popup.read_with(cx, |popup, _| popup.lines());
    focus(&editor, cx);
    let lines = editor.read_with(cx, |editor, _| editor.line_count());
    cx.simulate_keystrokes("enter");
    assert_eq!(editor.read_with(cx, |editor, _| editor.line_count()), lines + 1);
    cx.simulate_keystrokes("backspace");
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), ORIGINAL);
    popup.read_with(cx, |popup, _| {
        assert_eq!(popup.options().pattern, "order", "the query field did not take the key");
        assert_eq!(popup.lines(), rows, "the results did not take the key");
    });
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()), "Escape reaches the popup");
    close_backend_project(cx);
}

/// A document that cannot be written (here a file without write permission) is previewed read-only:
/// typing changes nothing and leaves it unmodified.
#[gpui_kit::test]
async fn keeps_file_documents_read_only(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let file = root.join("src/a.rs");
    let mut permissions = std::fs::metadata(&file).expect("the file exists").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions).expect("the file is made read-only");
    let (_popup, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
    assert!(editor.read_with(cx, |editor, _| editor.is_read_only()));
    focus(&editor, cx);
    cx.simulate_input("bad");
    cx.simulate_keystrokes("enter");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.text(), ORIGINAL);
        assert!(!editor.is_dirty());
    });
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// Not from a spec: walking past a result before its file is opened opens only the file the selection
// rests on; the file passed over is never opened.
#[gpui_kit::test]
async fn a_result_passed_over_before_its_file_opens_is_dropped(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("src/a.txt", "needle one\n"), ("src/b.txt", "needle two\n")]);
    run_action(cx, "find.inPath", Some(json!({ "query": "needle" })));
    let popup = find_popup(&workbench, cx);
    assert_eq!(searched(&popup, cx).len(), 2);
    // The search ends with the first result selected and its file not opened yet: the selection moves on
    // before the preview's rest is over.
    cx.simulate_keystrokes("down");
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    wait_long(cx, "a file previewed", |cx| preview.read_with(cx, |preview, _| preview.editor().is_some()));
    let shown = preview.read_with(cx, |preview, _| preview.path().map(str::to_owned)).expect("a file is shown");
    let (a, b) = (root.join("src/a.txt"), root.join("src/b.txt"));
    let passed_over = if shown == text_of(&a) { b } else { a };
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let project = host.project("w1").expect("the project is open");
    assert!(project.documents.text_of(&passed_over).is_none(), "{} was not opened", passed_over.display());
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// The Bookmarks popup previews the selected bookmark's file in an editor on its document, on the
/// bookmarked line.
#[gpui_kit::test]
async fn the_bookmarks_popup_previews_the_bookmarked_line_in_an_editor(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let path = text_of(&root.join("src/a.rs"));
    run_action(cx, "navigate.toggleBookmark", Some(json!({ "path": path, "line": 1 })));
    run_action(cx, "navigate.showBookmarks", None);
    let popup = workbench
        .read_with(cx, |w, _| w.popup.as_ref().and_then(|popup| popup.view.clone().downcast::<BookmarksPopup>().ok()))
        .expect("the Bookmarks popup is open");
    let on_mark = |cx: &mut VisualTestContext| {
        popup.read_with(cx, |popup, _| popup.preview_target().map(|(at, line)| (at.to_owned(), line)))
            == Some((path.clone(), Some(1)))
    };
    for _ in 0..4 {
        if on_mark(cx) {
            break;
        }
        cx.simulate_keystrokes("down");
    }
    assert!(on_mark(cx), "the bookmark is selected");
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    let editor = previewed(&preview, &path, cx);
    assert!(!editor.read_with(cx, |editor, _| editor.is_read_only()));
    assert_eq!(marked(&editor, MarkKind::CurrentLine, cx), [(1, "fn order() {}".to_owned())]);
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret_position()), (1, 0));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// The Find Usages window previews the selected usage's file read-only, every usage of the file marked
/// and the usage's line tinted.
#[gpui_kit::test]
async fn the_usages_preview_marks_every_usage_of_the_file_read_only(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("notes/a.txt", "shared value\n"), ("notes/b.txt", "use shared\nshared again\n")]);
    cx.update(|window, _| window.activate_window());
    let a = root.join("notes/a.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(a.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    run_action(cx, "find.usages", None);
    let view = workbench.read_with(cx, |w, _| w.usages_view()).expect("the Find Usages window is shown");
    settle(cx, |cx| view.read_with(cx, |view, _| view.search().is_some_and(|search| !search.loading)));
    let usages: Vec<Usage> = view.read_with(cx, |view, _| {
        let groups = view.search().map(|search| search.groups.as_slice()).unwrap_or_default();
        groups.iter().flat_map(|group| group.usages.clone()).collect()
    });
    let b = text_of(&root.join("notes/b.txt"));
    let second = usages.iter().find(|usage| usage.path == b && usage.line == 1).expect("the usage on b's second line");
    view.update(cx, |view, cx| view.select_row(RowKey::Usage(usage_key(second)), cx));
    wait_long(cx, "the preview pane", |cx| {
        view.read_with(cx, |view, _| view.preview().cloned()).is_some_and(|pane| pane.downcast::<CodePreview>().is_ok())
    });
    let pane = view.read_with(cx, |view, _| view.preview().cloned()).expect("the preview pane");
    let Ok(preview) = pane.downcast::<CodePreview>() else { panic!("the pane is a code preview") };
    let editor = previewed(&preview, &b, cx);
    assert!(editor.read_with(cx, |editor, _| editor.is_read_only()), "the usages preview does not edit");
    assert_eq!(
        marked(&editor, MarkKind::FindOccurrence, cx),
        [(0, "shared".to_owned()), (1, "shared".to_owned())],
        "every usage of the file is marked alike"
    );
    assert_eq!(marked(&editor, MarkKind::CurrentLine, cx), [(1, "shared again".to_owned())]);
    close_backend_project(cx);
}
