use super::*;

const DIGITS: std::ops::RangeInclusive<char> = '0'..='9';

/// A file of twelve lines, `line 0` to `line 11`, in the project, open in the editor with the keyboard in it.
fn open_digits_file(
    root: &std::path::Path,
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (PathBuf, String) {
    let file = root.join("src/digits.rs");
    let text: String = (0..12).map(|line| format!("line {line}\n")).collect();
    std::fs::write(&file, &text).expect("the file is written");
    let at = file.clone();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(at, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path())));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    cx.update(|window, cx| {
        window.activate_window();
        let focus = editor.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (file, text)
}

/// Puts the caret of the active editor at the start of `line`.
fn put_caret(workbench: &Entity<Workbench>, line: u32, cx: &mut VisualTestContext) {
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    editor.update(cx, |editor, cx| {
        let at = editor.offset_of(line, 0);
        editor.select(at, at, cx);
    });
    cx.run_until_parked();
}

/// The file and caret of the active editor.
fn caret(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(PathBuf, (u32, u32))> {
    workbench.read_with(cx, |w, cx| {
        w.editor().map(|editor| (editor.read(cx).path().to_path_buf(), editor.read(cx).caret_position()))
    })
}

/// The window's line bookmarks: (file name, line, mnemonic).
fn marks(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, u32, Option<char>)> {
    workbench.read_with(cx, |w, _| {
        let name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
        w.bookmarks().data().items.iter().map(|b| (name(&b.path), b.line, b.mnemonic)).collect()
    })
}

/// The keystroke the window's keymap binds to `id`, as GPUI spells it.
fn key_of(id: &str, cx: &mut VisualTestContext) -> String {
    let binding =
        cx.update(|_, cx| cx.global::<crate::platform::input::Keymap>().0.primary_key_binding(id).map(str::to_owned));
    let binding = binding.unwrap_or_else(|| panic!("the keymap binds {id}"));
    menu::gpui_keystroke(&binding).unwrap_or_else(|| panic!("{binding} of {id} is a key GPUI can press"))
}

/// Each of Toggle Bookmark 0 to 9, pressed on its key on its own line, gives that line a bookmark with its digit and
/// types nothing into the file; run through the registry on the same line it removes that bookmark, and again it
/// puts it back. The ten bookmarks stand side by side, one digit each.
#[gpui_kit::test]
async fn toggle_bookmark_with_a_digit_marks_the_caret_line_with_that_digit_for_every_digit(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    let (_file, text) = open_digits_file(&root, &workbench, cx);
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

    for (line, digit) in (0u32..).zip(DIGITS) {
        let id = format!("navigate.toggleBookmark{digit}");
        let key = key_of(&id, cx);
        put_caret(&workbench, line, cx);
        cx.simulate_keystrokes(&key);
        cx.run_until_parked();
        let here = ("digits.rs".to_owned(), line, Some(digit));
        assert!(marks(&workbench, cx).contains(&here), "{key} ({id}) marks line {line}: {:?}", marks(&workbench, cx));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), text, "{key} types nothing into the file");

        run_action(cx, &id, None);
        assert!(!marks(&workbench, cx).iter().any(|mark| mark.1 == line), "{id} again removes the bookmark of {line}");
        run_action(cx, &id, None);
        assert!(marks(&workbench, cx).contains(&here), "{id} puts the bookmark of line {line} back");
    }
    let mut all = marks(&workbench, cx);
    all.sort();
    let expected: Vec<(String, u32, Option<char>)> =
        (0u32..).zip(DIGITS).map(|(line, digit)| ("digits.rs".to_owned(), line, Some(digit))).collect();
    assert_eq!(all, expected, "one bookmark per digit, each on its own line");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Each of Go to Bookmark 0 to 9, pressed on its key from another file and run through the registry from another
/// line of the same file, puts the caret at the start of the line that has the bookmark with its digit.
#[gpui_kit::test]
async fn go_to_bookmark_with_a_digit_puts_the_caret_on_that_bookmarks_line_for_every_digit(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    let (file, _) = open_digits_file(&root, &workbench, cx);
    // Line `digit + 1` gets bookmark `digit`, so no bookmark is on the line the caret starts from.
    for (line, digit) in (1u32..).zip(DIGITS) {
        put_caret(&workbench, line, cx);
        run_action(cx, &format!("navigate.toggleBookmark{digit}"), None);
    }
    assert_eq!(marks(&workbench, cx).len(), 10, "ten bookmarks: {:?}", marks(&workbench, cx));

    let alpha = root.join("src/alpha.rs");
    for (line, digit) in (1u32..).zip(DIGITS) {
        let id = format!("navigate.gotoBookmark{digit}");
        let key = key_of(&id, cx);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(alpha.clone(), window, cx));
        settle(cx, |cx| caret(&workbench, cx).is_some_and(|(path, _)| path == alpha));
        cx.simulate_keystrokes(&key);
        settle(cx, |cx| caret(&workbench, cx) == Some((file.clone(), (line, 0))));

        put_caret(&workbench, 0, cx);
        assert_eq!(caret(&workbench, cx), Some((file.clone(), (0, 0))));
        run_action(cx, &id, None);
        settle(cx, |cx| caret(&workbench, cx) == Some((file.clone(), (line, 0))));
    }
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
