use super::*;

/// A terminal running the plain test shell, focused, with its grid fitted; the home and the folder live as
/// long as the returned guards.
fn focused_shell(cx: &mut TestAppContext) -> (Entity<TerminalView>, &mut VisualTestContext, [tempfile::TempDir; 2]) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        crate::terminal::test_support::isolate_shells(home.path(), cx);
        crate::terminal::test_support::trusted_folder("w1", folder.path(), cx);
    });
    let shell = crate::terminal::test_support::plain_shell(home.path());
    let options = TerminalOptions {
        cwd: ide_core::IpcPath(folder.path().to_path_buf()),
        shell: Some(shell.to_string_lossy().into_owned()),
        args: Vec::new(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
        shell_integration: false,
    };
    let (view, cx) = cx.add_window_view(|_, cx| TerminalView::new("w1", options, cx));
    cx.update(|window, cx| {
        window.activate_window();
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    settle(cx, "the grid is fitted", |cx| view.read_with(cx, |view, _| view.grid.is_some()));
    (view, cx, [home, folder])
}

/// Has the shell read the next line and print it as `GOT[<line>]`, which the echo of the command itself does not
/// hold, so that what reached the program is told apart from what the terminal echoes.
fn print_next_line(cx: &mut VisualTestContext) {
    cx.simulate_input("IFS= read -r x; printf 'G%sT[%s]\\n' O \"$x\"");
    cx.simulate_keystrokes("enter");
}

/// The text of the input method's composition, as the OS hands it to the terminal.
fn compose(view: &Entity<TerminalView>, text: &str, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.replace_and_mark_text_in_range(None, text, None, window, cx));
    cx.run_until_parked();
}

/// What the program printed of the line it read, once it has.
fn printed(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    fn done(line: &str) -> bool {
        line.contains("GOT[") && line.trim_end().ends_with(']')
    }
    settle(cx, "the program prints what it read", |cx| screen(view, cx).lines().any(done));
    let shown = screen(view, cx);
    let line = shown.lines().find(|l| l.contains("GOT[")).unwrap_or_default();
    line[line.find("GOT[").unwrap_or(0)..].trim_end().to_owned()
}

fn marked(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Option<Range<usize>> {
    view.update_in(cx, |view, window, cx| view.marked_text_range(window, cx))
}

/// A composition is shown, not sent: the program gets only the committed text, once, and nothing of what was
/// composed before.
#[gpui_kit::test]
async fn keeps_out_of_real_compositions_and_sends_a_multi_character_commit_once(cx: &mut TestAppContext) {
    let (view, cx, _dirs) = focused_shell(cx);
    print_next_line(cx);
    compose(&view, "e", cx);
    compose(&view, "e\u{301}", cx);
    assert_eq!(marked(&view, cx), Some(0..2), "the composition is held by the terminal");
    cx.simulate_input("éà");
    assert_eq!(marked(&view, cx), None, "the commit ends the composition");
    cx.simulate_keystrokes("enter");
    assert_eq!(printed(&view, cx), "GOT[éà]");
}

/// Text committed without a composition before it (a dead key, an emoji picker) reaches the program.
#[gpui_kit::test]
async fn sends_a_composition_commit_reported_without_compositionstart(cx: &mut TestAppContext) {
    let (view, cx, _dirs) = focused_shell(cx);
    print_next_line(cx);
    assert_eq!(marked(&view, cx), None);
    cx.simulate_input("ü");
    cx.simulate_keystrokes("enter");
    assert_eq!(printed(&view, cx), "GOT[ü]");
}

/// Keys typed one after another reach the program exactly once each, and one input event carrying several
/// characters reaches it whole.
#[gpui_kit::test]
async fn sends_every_key_of_a_fast_run_exactly_once_and_the_whole_text_of_one_event(cx: &mut TestAppContext) {
    let (view, cx, _dirs) = focused_shell(cx);
    print_next_line(cx);
    cx.simulate_keystrokes("a b c");
    cx.simulate_input("xyz");
    cx.simulate_keystrokes("enter");
    assert_eq!(printed(&view, cx), "GOT[abcxyz]");
}

/// A composition the input method keeps through a change of input source (Pinyin to English mid-word) and
/// then commits reaches the program once, as typed.
#[gpui_kit::test]
async fn sends_what_was_typed_once_when_the_input_source_switches_mid_composition(cx: &mut TestAppContext) {
    let (view, cx, _dirs) = focused_shell(cx);
    print_next_line(cx);
    compose(&view, "n", cx);
    compose(&view, "ni", cx);
    assert_eq!(marked(&view, cx), Some(0..2));
    cx.simulate_input("ni");
    assert_eq!(marked(&view, cx), None, "the commit clears the composition");
    cx.simulate_keystrokes("enter");
    assert_eq!(printed(&view, cx), "GOT[ni]");
}
