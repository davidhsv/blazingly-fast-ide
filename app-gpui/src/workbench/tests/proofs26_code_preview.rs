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

/// The preview's gutter is the plain one of a preview: no change bars, no caret row across it, no
/// bookmarks or run marks columns, and nothing of it is reported to the window.
#[track_caller]
fn assert_plain_gutter(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert!(!editor.gutter_marks_enabled(), "the preview draws no gutter marks");
        assert_eq!((editor.gutter_column("bookmarks", cx), editor.gutter_column("run", cx)), (None, None));
        assert!(editor.lane_marks().is_empty() && editor.painted_gutter_rows().is_empty());
    });
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
    let (_, editor) = find_previewed(&workbench, "order", &path, cx);
    assert_plain_gutter(&editor, cx);
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
    // The gutter has the line numbers alone: no fold chevrons, no icon columns.
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.fold_column_width(cx), gpui_kit::px(0.0), "no fold chevrons");
        assert!(editor.painted_fold_markers().is_empty());
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
    let (_, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
    focus(&editor, cx);
    cx.simulate_input("// x ");
    let edited = ORIGINAL.replacen("fn order", "// x fn order", 1);
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), edited);
    wait_long(cx, "the edit in the tab", |cx| tab.read_with(cx, |tab, _| tab.text()) == edited);
    assert_eq!(marked(&editor, MarkKind::CurrentFindOccurrence, cx), [(1, "order".to_owned())], "the mark moves along");
    cx.simulate_keystrokes(&command("s"));
    wait_long(cx, "the save", |_| std::fs::read_to_string(&file).is_ok_and(|text| text == edited));
    // An edit made in the tab shows in the preview: both show the document's one text, each editor with its
    // own undo history.
    tab.update(cx, |tab, cx| tab.select(0, 0, cx));
    focus(&tab, cx);
    cx.simulate_input("y");
    let both = format!("y{edited}");
    wait_long(cx, "the tab's edit in the preview", |cx| editor.read_with(cx, |editor, _| editor.text()) == both);
    assert_eq!(tab.read_with(cx, |tab, _| tab.text()), both);
    focus(&editor, cx);
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
    let (_, editor) = find_previewed(&workbench, "order", &path, cx);
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

    let (_, reopened) = find_previewed(&workbench, "order", &path, cx);
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
    let (_, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
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
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    wait_long(cx, "the first result previewed", |cx| preview.read_with(cx, |preview, _| preview.editor().is_some()));
    let shown = preview.read_with(cx, |preview, _| preview.path().map(str::to_owned)).expect("a file is shown");
    let (a, b) = (root.join("src/a.txt"), root.join("src/b.txt"));
    let passed_over = if shown == text_of(&a) { b } else { a };
    // Down and back up in one go: the clock does not move between them, so the rest on the second result
    // never ends. A second on the clock is far past the rest (80 ms) its file would be opened after.
    to_query(&popup, cx);
    cx.simulate_keystrokes("down up");
    for _ in 0..20 {
        cx.executor().advance_clock(Duration::from_millis(50));
        cx.run_until_parked();
    }
    assert_eq!(preview.read_with(cx, |preview, _| preview.path().map(str::to_owned)), Some(shown));
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
    assert_plain_gutter(&editor, cx);
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
    assert_plain_gutter(&editor, cx);
    assert!(editor.read_with(cx, |editor, _| editor.is_read_only()), "the usages preview does not edit");
    assert_eq!(
        marked(&editor, MarkKind::FindOccurrence, cx),
        [(0, "shared".to_owned()), (1, "shared".to_owned())],
        "every usage of the file is marked alike"
    );
    assert_eq!(marked(&editor, MarkKind::CurrentLine, cx), [(1, "shared again".to_owned())]);
    close_backend_project(cx);
}

/// Writes Files › Autosave for the window `w1`.
fn set_autosave(on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        crate::platform::settings::set(
            "w1",
            ide_workspace::settings::SettingsScope::Global,
            "files.autoSave",
            &serde_json::Value::from(on),
            cx,
        )
    })
    .expect("the setting is stored");
}

/// Gives the keyboard back to the popup's query field, where the list's keys are.
fn to_query(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let handle = popup.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    cx.run_until_parked();
}

/// How many documents the backend has open for the window's project.
fn open_documents(cx: &mut VisualTestContext) -> usize {
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.project("w1").map(|project| project.documents.open_documents().len()).unwrap_or_default()
}

/// The text the backend's document of `file` holds, while one is open.
fn document_text(file: &std::path::Path, cx: &mut VisualTestContext) -> Option<String> {
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.project("w1").ok().and_then(|project| project.documents.text_of(file))
}

fn unsaved(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |w, cx| w.unsaved_count(cx))
}

// Not from a spec: an edit typed in the preview of a file no tab shows is a document with unsaved
// changes like any other: counted while the preview shows it and after the popup went, and written by
// File › Save All.
#[gpui_kit::test]
async fn save_all_writes_an_edit_made_in_a_preview_of_a_file_without_a_tab(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    set_autosave(false, cx);
    let file = root.join("src/a.rs");
    let before = open_documents(cx);
    let (_, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
    focus(&editor, cx);
    cx.simulate_input("// x ");
    let edited = ORIGINAL.replacen("fn order", "// x fn order", 1);
    wait_long(cx, "the edit to be unsaved", |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
    drop(editor);
    assert_eq!(unsaved(&workbench, cx), 1, "the preview's edit is counted while it shows");
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()));
    assert_eq!(unsaved(&workbench, cx), 1, "and after the popup went");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), ORIGINAL);

    run_action(cx, "file.save", None);
    wait_long(cx, "Save All to write the preview's edit", |cx| {
        unsaved(&workbench, cx) == 0 && std::fs::read_to_string(&file).is_ok_and(|text| text == edited)
    });
    wait_long(cx, "the saved preview's document to be closed", |cx| open_documents(cx) == before);
    close_backend_project(cx);
}

// Not from a spec: with Files › Autosave on, an edit typed in a preview is written when the preview
// lets its file go, whether another result is chosen or the popup closes.
#[gpui_kit::test]
async fn autosave_writes_a_preview_edit_when_another_result_is_chosen_or_the_popup_closes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) =
        trusted_project(cx, &[("src/a.txt", "needle one\n"), ("src/b.txt", "needle two\n")]);
    set_autosave(true, cx);
    let before = open_documents(cx);
    run_action(cx, "find.inPath", Some(json!({ "query": "needle" })));
    let popup = find_popup(&workbench, cx);
    assert_eq!(searched(&popup, cx).len(), 2);
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    wait_long(cx, "a file previewed", |cx| preview.read_with(cx, |preview, _| preview.editor().is_some()));
    let first = preview.read_with(cx, |preview, _| preview.path().map(PathBuf::from)).expect("a file is shown");
    let editor = preview.read_with(cx, |preview, _| preview.editor().cloned()).expect("an editor");
    focus(&editor, cx);
    cx.simulate_input("x");
    let typed = |path: &std::path::Path| std::fs::read_to_string(path).is_ok_and(|text| text.starts_with('x'));
    wait_long(cx, "the edit to be unsaved", |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
    assert!(!typed(&first), "nothing is written while the preview shows the file");
    drop(editor);

    // Another result: the edited file is let go and written.
    to_query(&popup, cx);
    cx.simulate_keystrokes("down");
    wait_long(cx, "the first file to be written", |_| typed(&first));
    wait_long(cx, "the other file previewed", |cx| {
        preview.read_with(cx, |preview, _| preview.path().is_some_and(|path| std::path::Path::new(path) != first))
    });
    let second = preview.read_with(cx, |preview, _| preview.path().map(PathBuf::from)).expect("a file is shown");
    let editor = preview.read_with(cx, |preview, _| preview.editor().cloned()).expect("an editor");
    focus(&editor, cx);
    cx.simulate_input("x");
    wait_long(cx, "the edit to be unsaved", |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));

    // The popup closes: the file shown is let go and written.
    drop((popup, preview));
    cx.simulate_keystrokes("escape");
    drop(editor);
    wait_long(cx, "the second file to be written", |cx| typed(&second) && unsaved(&workbench, cx) == 0);
    wait_long(cx, "the written previews' documents to be closed", |cx| open_documents(cx) == before);
    close_backend_project(cx);
}

// Not from a spec: closing the project with an edit made in a preview whose popup went asks about it;
// Save All writes it, and every document the previews opened is closed with the project.
#[gpui_kit::test]
async fn the_close_question_counts_a_preview_edit_and_save_all_writes_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    cx.update(|window, _| window.activate_window());
    set_autosave(false, cx);
    let file = root.join("src/a.rs");
    let (_, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
    focus(&editor, cx);
    cx.simulate_input("// x ");
    wait_long(cx, "the edit to be unsaved", |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
    drop(editor);
    cx.simulate_keystrokes("escape");

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    let (title, body) = cx.pending_prompt().expect("the unsaved question is asked");
    assert_eq!(title, "Unsaved Changes");
    let project = root.file_name().expect("a name").to_string_lossy().into_owned();
    assert_eq!(body, format!("1 file has unsaved changes in {project}. Save before closing?"));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(document_text(&file, cx), Some(ORIGINAL.replacen("fn order", "// x fn order", 1)), "Cancel keeps it");

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    cx.simulate_prompt_answer("Save All");
    let edited = ORIGINAL.replacen("fn order", "// x fn order", 1);
    wait_long(cx, "the project to close with the edit written", |cx| {
        workbench.read_with(cx, |w, _| w.project_tree().is_none())
            && std::fs::read_to_string(&file).is_ok_and(|text| text == edited)
    });
    assert!(cx.update(|_, cx| crate::ui::code_preview::preview_editors("w1", cx).is_empty()), "nothing is held");
}

// Not from a spec: Don't Save on the close question drops a preview's edit as it drops a tab's: the
// file keeps what it had on disk, and nothing of the preview is left counted for the window.
#[gpui_kit::test]
async fn dont_save_on_the_close_question_leaves_the_file_and_lets_the_preview_go(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    cx.update(|window, _| window.activate_window());
    let file = root.join("src/a.rs");
    let (_, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
    focus(&editor, cx);
    cx.simulate_input("// x ");
    wait_long(cx, "the edit to be unsaved", |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
    drop(editor);
    // The popup stays open: the question counts the preview it shows.
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    let (title, _) = cx.pending_prompt().expect("the unsaved question is asked");
    assert_eq!(title, "Unsaved Changes");
    cx.simulate_prompt_answer("Don't Save");
    wait_long(cx, "the project to close", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_none()));
    for _ in 0..20 {
        cx.executor().advance_clock(Duration::from_millis(50));
        cx.run_until_parked();
    }
    assert_eq!(std::fs::read_to_string(&file).expect("read"), ORIGINAL, "Don't Save holds with autosave on");
    assert_eq!(unsaved(&workbench, cx), 0);
}

/// Find in Files in Replace mode on `notes.txt` ("foo one\nbar\n"), its only result replaced by "qux" with
/// Enter while the preview shows the file. Returns the file.
fn replace_previewed(workbench: &Entity<Workbench>, root: &std::path::Path, cx: &mut VisualTestContext) -> PathBuf {
    let file = root.join("notes.txt");
    run_action(cx, "find.replaceInPath", Some(json!({ "query": "foo" })));
    let popup = find_popup(workbench, cx);
    popup.update(cx, |popup, cx| popup.set_replacement("qux", cx));
    assert_eq!(searched(&popup, cx).len(), 1);
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    previewed(&preview, &text_of(&file), cx);
    to_query(&popup, cx);
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the replacement in the document", |cx| {
        document_text(&file, cx).as_deref() == Some("qux one\nbar\n")
    });
    file
}

// Not from a spec: Replace on the file the preview holds replaces the text of its document and leaves it
// unsaved (Replace saves only the files it opened itself); with Files › Autosave off it is counted as
// unsaved, the disk unchanged, until Save All writes it.
#[gpui_kit::test]
async fn replace_on_the_previewed_file_leaves_it_unsaved_and_counted(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "foo one\nbar\n")]);
    set_autosave(false, cx);
    let file = replace_previewed(&workbench, &root, cx);
    wait_long(cx, "the document to be counted unsaved", |cx| unsaved(&workbench, cx) == 1);
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "foo one\nbar\n");
    cx.simulate_keystrokes("escape");
    assert_eq!(unsaved(&workbench, cx), 1, "still counted once the popup went");
    run_action(cx, "file.save", None);
    wait_long(cx, "the disk to agree with the document", |cx| {
        unsaved(&workbench, cx) == 0 && std::fs::read_to_string(&file).is_ok_and(|text| text == "qux one\nbar\n")
    });
    close_backend_project(cx);
}

// Not from a spec: with Files › Autosave on, the file Replace changed under the preview ends written: the
// disk and the document agree and nothing is left unsaved.
#[gpui_kit::test]
async fn replace_on_the_previewed_file_ends_written_with_autosave_on(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "foo one\nbar\n")]);
    set_autosave(true, cx);
    let file = replace_previewed(&workbench, &root, cx);
    cx.simulate_keystrokes("escape");
    wait_long(cx, "the disk to agree with the document", |cx| {
        unsaved(&workbench, cx) == 0 && std::fs::read_to_string(&file).is_ok_and(|text| text == "qux one\nbar\n")
    });
    assert_eq!(document_text(&file, cx), None, "the preview's document is closed with the popup");
    close_backend_project(cx);
}

// Not from a spec: a file that changes on disk while the preview holds it with unsaved edits is
// handled as for a tab: the user is told, the edits stay, and the disk keeps what was written there.
#[gpui_kit::test]
async fn a_file_changed_on_disk_under_an_edited_preview_is_told_not_overwritten(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    set_autosave(false, cx);
    let file = root.join("src/a.rs");
    let (_, editor) = find_previewed(&workbench, "order", &text_of(&file), cx);
    focus(&editor, cx);
    cx.simulate_input("// x ");
    wait_long(cx, "the edit to be unsaved", |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
    // A later write with another length: the change is seen whatever the clock's resolution.
    std::fs::write(&file, "fn changed_outside() {}\n").expect("the file is written");
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.project("w1").expect("the project is open").documents.on_disk_changed(std::slice::from_ref(&file));
    wait_for_notice(&workbench, "a.rs changed on disk", cx);
    let edited = ORIGINAL.replacen("fn order", "// x fn order", 1);
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), edited);
    assert_eq!(document_text(&file, cx), Some(edited));
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "fn changed_outside() {}\n");
    assert_eq!(unsaved(&workbench, cx), 1);
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// Not from a spec: the documents a preview opens are closed again once it lets them go and nothing else
// holds them: after walking through results and closing the popup the backend has as many documents
// open as before.
#[gpui_kit::test]
async fn the_documents_a_preview_opened_are_closed_when_it_lets_them_go(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) =
        trusted_project(cx, &[("src/a.txt", "needle one\n"), ("src/b.txt", "needle two\n")]);
    let before = open_documents(cx);
    run_action(cx, "find.inPath", Some(json!({ "query": "needle" })));
    let popup = find_popup(&workbench, cx);
    assert_eq!(searched(&popup, cx).len(), 2);
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    wait_long(cx, "a file previewed", |cx| preview.read_with(cx, |preview, _| preview.editor().is_some()));
    let first = preview.read_with(cx, |preview, _| preview.path().map(str::to_owned));
    cx.simulate_keystrokes("down");
    wait_long(cx, "the other file previewed", |cx| {
        preview.read_with(cx, |preview, _| preview.editor().is_some() && preview.path().map(str::to_owned) != first)
    });
    wait_long(cx, "one document open for the preview", |cx| open_documents(cx) == before + 1);
    drop((popup, preview));
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()));
    wait_long(cx, "the preview's document to be closed", |cx| open_documents(cx) == before);
    close_backend_project(cx);
}

// Not from a spec: an item of the TODO tool window passed over before its file opened leaves no
// document open: the backend's open documents are those the preview shows.
#[gpui_kit::test]
async fn a_todo_item_passed_over_before_its_file_opens_leaves_no_document_open(cx: &mut TestAppContext) {
    use crate::features_a::todo_view::TodoView;
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("src/a.rs", "// TODO first\n"), ("src/b.rs", "// TODO second\n")]);
    run_action(cx, "toolwindow.todo", None);
    let view: Entity<TodoView> =
        workbench.read_with(cx, |w, _| w.todo_window_view()).expect("the TODO tool window is shown");
    settle(cx, |cx| !view.update(cx, |view, cx| view.lines(cx)).is_empty());
    let before = open_documents(cx);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    // Both in one turn: the first file's item is passed over while its file is being opened.
    view.update(cx, |view, cx| {
        view.select(&text_of(&a), cx);
        view.select(&text_of(&b), cx);
    });
    wait_long(cx, "the second item previewed", |cx| {
        view.read_with(cx, |view, _| view.preview().cloned())
            .and_then(|pane| pane.downcast::<Editor>().ok())
            .is_some_and(|editor| editor.read_with(cx, |editor, _| editor.path() == b.as_path()))
    });
    wait_long(cx, "only the shown file's document open", |cx| open_documents(cx) == before + 1);
    assert!(document_text(&a, cx).is_none(), "the passed-over file is not open");
    close_backend_project(cx);
}

// Not from a spec: a 5 MB file previews without holding up the dialog: what is typed in the query while
// the file opens lands at once, and the preview ends on the file's last line. The window thread's longest
// stall while the file opens and the time to the preview are printed for the record (`--nocapture`).
#[gpui_kit::test]
async fn a_large_file_previews_while_typing_in_the_query_still_lands(cx: &mut TestAppContext) {
    let _home = start(cx);
    let mut big = "filler line of text that is not searched for\n".repeat(5 * 1024 * 1024 / 46);
    big.push_str("the needle is here\n");
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("big.txt", &big)]);
    let file = text_of(&root.join("big.txt"));
    run_action(cx, "find.inPath", Some(json!({ "query": "needle" })));
    let popup = find_popup(&workbench, cx);
    assert_eq!(searched(&popup, cx).len(), 1);
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    let shown = |cx: &mut VisualTestContext| {
        preview.read_with(cx, |preview, _| preview.path() == Some(file.as_str()) && preview.editor().is_some())
    };
    let shown_before_typing = shown(cx);
    let typing = std::time::Instant::now();
    to_query(&popup, cx);
    cx.simulate_keystrokes("end");
    cx.simulate_input(" is");
    let typed = typing.elapsed();
    assert_eq!(popup.read_with(cx, |popup, _| popup.options().pattern.clone()), "needle is");
    // The longest single turn of the window's thread until the preview shows the file.
    let (opening, mut worst) = (std::time::Instant::now(), Duration::ZERO);
    for _ in 0..3000 {
        cx.executor().advance_clock(Duration::from_millis(20));
        let turn = std::time::Instant::now();
        cx.run_until_parked();
        worst = worst.max(turn.elapsed());
        if shown(cx) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(shown(cx), "the large file is previewed");
    let opened = opening.elapsed();
    let editor = preview.read_with(cx, |preview, _| preview.editor().cloned()).expect("an editor");
    let last = editor.read_with(cx, |editor, _| editor.line_count()) - 2;
    assert_eq!(editor.read_with(cx, |editor, _| editor.line_text(last)), "the needle is here");
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret_position().0), last, "the preview is on the result");
    eprintln!(
        "5 MB preview: shown before typing {shown_before_typing}, typing three keys {typed:?}, \
         preview shown after {opened:?}, longest turn of the window thread {worst:?}"
    );
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// Not from a spec: while the preview has the keyboard the shortcut of an editing action (Duplicate Line)
// stays out of the tab behind the popup, and a shortcut that is not an editing one (Save All) still runs.
#[gpui_kit::test]
async fn editing_shortcuts_stay_in_the_preview_and_the_others_still_run(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL), ("notes.txt", "notes\n")]);
    set_autosave(false, cx);
    let notes = root.join("notes.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(notes.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == notes.as_path())));
    let tab = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the tab's editor");
    focus(&tab, cx);
    cx.simulate_input("my ");
    let (_, editor) = find_previewed(&workbench, "order", &text_of(&root.join("src/a.rs")), cx);
    focus(&editor, cx);
    cx.simulate_keystrokes(&command("d"));
    assert_eq!(tab.read_with(cx, |tab, _| tab.text()), "my notes\n", "Duplicate Line left the tab alone");
    assert_eq!(std::fs::read_to_string(&notes).expect("read"), "notes\n");
    cx.simulate_keystrokes(&command("s"));
    wait_long(cx, "Save All from the preview", |_| std::fs::read_to_string(&notes).is_ok_and(|t| t == "my notes\n"));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// Not from a spec: the previewed line is tinted when it is empty too: a bookmark on an empty line shows
// that line's row tinted.
#[gpui_kit::test]
async fn an_empty_previewed_line_is_tinted(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", "fn a() {}\n\nfn b() {}\n")]);
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
    assert_eq!(marked(&editor, MarkKind::CurrentLine, cx), [(1, String::new())]);
    wait_long(cx, "the empty line drawn tinted", |cx| {
        editor.read_with(cx, |editor, _| !editor.painted_marks(MarkKind::CurrentLine).is_empty())
    });
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}
