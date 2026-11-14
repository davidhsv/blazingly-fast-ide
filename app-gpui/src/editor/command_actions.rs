//! The editing commands in the action registry: menus, the keymap and Find Action run them on
//! the window's editor. Each computes its edit from the editor's text and selection
//! ([`Editor::edit_state`]) and the language's facts, and the editor applies it as one undo step.
//!
//! The backend has no syntax tree of the document. The commands that read one get the tree of
//! the document's comments and string literals ([`Editor::literal_tree`]) where they only ask
//! whether a position is code, and the unparsed tree otherwise, which makes them take their
//! text-only path. The comment commands get the unparsed tree: a block comment holding its own
//! closing token takes a parser to confirm, and the highlights cannot tell two adjacent comments
//! from one.

use super::actions::on_editor;
use super::history::EditKind;
use super::view::Editor;
use crate::editor::intelligence::EditorIntelligence;
use crate::editor_commands::column_mode::Block;
use crate::editor_commands::editing::{self, OccurrenceOutcome, OccurrenceSearch};
use crate::editor_commands::tree::SyntaxTree;
use crate::editor_commands::vertical_motion::{self, GoalColumns, VerticalMotion};
use crate::editor_commands::word_completion::{self, ExpandDirection, ExpandOutcome, ExpandSession};
use crate::editor_commands::{
    Edit, EditState, IndentContext, Selection, caret_commands, comments, lines, move_element,
};
use crate::editor_lsp::hint::Tone;
use crate::platform::actions::ActionRegistry;
use crate::workbench;
use gpui_kit::{App, Context};

/// The editor's text version and selection when a command left them: what the command
/// remembered is valid while they are unchanged.
pub(super) type Stamp = (u32, Vec<Selection>, usize);

/// What the commands remember between two invocations. An entry is valid while the editor is as
/// the command left it ([`Stamp`]): any other edit or selection change ends the run.
#[derive(Debug, Default)]
pub(super) struct CommandMemory {
    /// The goal columns of vertical motion and Clone Caret.
    pub(super) goals: Option<(Stamp, GoalColumns)>,
    /// A run of Add Selection for Next Occurrence.
    pub(super) occurrence: Option<(Stamp, OccurrenceSearch)>,
    /// Where the last Comment with Line Comment left the editor.
    pub(super) comment: Option<Stamp>,
    /// Column Selection Mode is on.
    pub(super) column_on: bool,
    /// The block ⇧+arrows extend in Column Selection Mode.
    pub(super) column_block: Option<(Stamp, Block)>,
    /// A run of Cyclic Expand Word (it checks its own validity).
    pub(super) expand: Option<ExpandSession>,
}

/// The value of `slot` when it was remembered at `now`.
pub(super) fn recall<T: Clone>(slot: &Option<(Stamp, T)>, now: &Stamp) -> Option<T> {
    slot.as_ref().filter(|(stamp, _)| stamp == now).map(|(_, value)| value.clone())
}

/// A command as the registry runs it on the window's editor.
type Command = fn(&mut Editor, &mut Context<Editor>);
/// The state of a toggle command on an editor.
type Toggled = fn(&Editor) -> bool;
/// Clone Caret Above or Below.
type CloneCaret = fn(&EditState, &[(u32, u32)]) -> Option<VerticalMotion>;

/// Every editing command, by action id.
pub(super) const COMMANDS: &[(&str, Command)] = &[
    ("code.commentLine", comment_line),
    ("code.commentBlock", comment_block),
    ("code.duplicateLine", duplicate_line),
    ("code.deleteLine", delete_line),
    ("code.moveStatementUp", |editor, cx| move_statement(editor, true, cx)),
    ("code.moveStatementDown", |editor, cx| move_statement(editor, false, cx)),
    ("code.moveLineUp", |editor, cx| move_line(editor, true, cx)),
    ("code.moveLineDown", |editor, cx| move_line(editor, false, cx)),
    ("code.joinLines", |editor, cx| edit(editor, cx, |state, _| Some(editing::join_lines(state)))),
    ("code.splitLine", |editor, cx| edit(editor, cx, |state, e| caret_commands::split_line(state, Some(&indent(e))))),
    ("code.startNewLine", |editor, cx| edit(editor, cx, |state, e| editing::start_new_line(state, Some(&indent(e))))),
    ("code.startNewLineBefore", |editor, cx| {
        edit(editor, cx, |state, e| editing::start_new_line_before(state, Some(&indent(e))))
    }),
    ("code.toggleCase", |editor, cx| edit(editor, cx, |state, _| Some(editing::toggle_case(state, &unparsed())))),
    ("code.sortLines", |editor, cx| edit(editor, cx, |state, _| lines::sort_lines(state))),
    ("code.reverseLines", |editor, cx| edit(editor, cx, |state, _| lines::reverse_lines(state))),
    ("code.addCaretNextOccurrence", next_occurrence),
    ("code.selectAllOccurrences", |editor, cx| edit(editor, cx, |state, _| editing::select_all_occurrences(state))),
    ("code.unselectOccurrence", unselect_occurrence),
    ("code.addCaretsToLineEnds", |editor, cx| edit(editor, cx, |state, _| editing::add_carets_to_line_ends(state))),
    ("editor.cloneCaretAbove", |editor, cx| clone_caret(editor, vertical_motion::clone_caret_above, cx)),
    ("editor.cloneCaretBelow", |editor, cx| clone_caret(editor, vertical_motion::clone_caret_below, cx)),
    ("editor.matchBrace", |editor, cx| {
        edit(editor, cx, |state, e| caret_commands::cursor_match_brace(state, &e.literal_tree()))
    }),
    ("editor.codeBlockStart", |editor, cx| {
        edit(editor, cx, |state, e| caret_commands::cursor_code_block_start(state, &e.literal_tree()))
    }),
    ("editor.codeBlockEnd", |editor, cx| {
        edit(editor, cx, |state, e| caret_commands::cursor_code_block_end(state, &e.literal_tree()))
    }),
    ("editor.codeBlockStartWithSelection", |editor, cx| {
        edit(editor, cx, |state, e| caret_commands::select_code_block_start(state, &e.literal_tree()))
    }),
    ("editor.codeBlockEndWithSelection", |editor, cx| {
        edit(editor, cx, |state, e| caret_commands::select_code_block_end(state, &e.literal_tree()))
    }),
    ("code.moveElementLeft", |editor, cx| {
        edit(editor, cx, |state, e| move_element::move_element_left(state, &e.literal_tree()))
    }),
    ("code.moveElementRight", |editor, cx| {
        edit(editor, cx, |state, e| move_element::move_element_right(state, &e.literal_tree()))
    }),
    ("code.cyclicExpandWord", |editor, cx| expand_word(editor, ExpandDirection::Backward, cx)),
    ("code.cyclicExpandWordBackward", |editor, cx| expand_word(editor, ExpandDirection::Forward, cx)),
    ("code.pasteAsPlainText", paste_as_plain_text),
    ("code.completeStatement", |editor, cx| {
        edit(editor, cx, |state, e| {
            let facts = &e.facts().language;
            editing::complete_statement(state, &facts.id, facts.comments.line.as_deref(), Some(&indent(e)))
        })
    }),
];

/// The toggle commands: their id, their state on an editor and what switches it.
pub(super) const TOGGLES: &[(&str, Toggled, Command)] = &[
    ("editor.toggleColumnSelection", |editor| editor.memory.column_on, toggle_column_selection),
    ("editor.toggleInsertState", |editor| editor.overwrite, toggle_insert_state),
];

pub fn register(cx: &mut App) {
    let registry = cx.default_global::<ActionRegistry>();
    for &(id, command) in COMMANDS {
        registry.handle(id, move |_, window, cx| on_editor(window, cx, command));
    }
    for &(id, state, command) in TOGGLES {
        registry.handle_toggle(
            id,
            move |window, cx| workbench::active_editor(window, cx).is_some_and(|editor| state(editor.read(cx))),
            move |_, window, cx| on_editor(window, cx, command),
        );
    }
}

/// The tree of a document the backend has not parsed: the commands take their text-only path.
fn unparsed() -> SyntaxTree {
    SyntaxTree::default()
}

/// The language's indentation, as the commands ask for it.
fn indent(editor: &Editor) -> impl Fn(&IndentContext<'_>) -> Option<u32> + '_ {
    |context| editor.facts().indent(context)
}

/// Applies what `command` answers for the editor's state as one undo step; a command that does
/// not apply changes nothing.
fn edit(editor: &mut Editor, cx: &mut Context<Editor>, command: impl Fn(&EditState, &Editor) -> Option<Edit>) {
    editor.edit_with(EditKind::Other, cx, |editor| command(&editor.edit_state(), editor));
}

/// Applies an edit computed once from the current state (a command that also answers what to
/// remember). Answers whether it was applied.
fn apply_once(editor: &mut Editor, edit: Edit, cx: &mut Context<Editor>) -> bool {
    if edit.changes.is_empty() {
        editor.set_selections(edit.selections, edit.main, cx);
        return true;
    }
    editor.apply_edit(&edit.changes, edit.selections, edit.main, cx)
}

fn comment_line(editor: &mut Editor, cx: &mut Context<Editor>) {
    let repeated = editor.memory.comment.as_ref() == Some(&editor.stamp());
    let done = editor.edit_with(EditKind::Other, cx, |editor| {
        let facts = editor.facts();
        let comments_at = |pos: u32| facts.comments_at(pos);
        comments::comment_line(&editor.edit_state(), &comments_at, facts.comment_style, &unparsed(), repeated)
    });
    if done {
        editor.memory.comment = Some(editor.stamp());
    }
}

fn comment_block(editor: &mut Editor, cx: &mut Context<Editor>) {
    edit(editor, cx, |state, editor| {
        let comments_at = |pos: u32| editor.facts().comments_at(pos);
        comments::comment_block(state, &comments_at, &unparsed())
    });
}

fn duplicate_line(editor: &mut Editor, cx: &mut Context<Editor>) {
    edit(editor, cx, |state, _| Some(editing::duplicate_line_or_selection(state, &[])));
}

fn delete_line(editor: &mut Editor, cx: &mut Context<Editor>) {
    edit(editor, cx, |state, _| {
        let edit = editing::delete_line(state, &[]);
        (!edit.changes.is_empty()).then_some(edit)
    });
}

/// Move Statement: without a syntax tree, the lines move.
fn move_statement(editor: &mut Editor, up: bool, cx: &mut Context<Editor>) {
    edit(editor, cx, |state, editor| editing::move_statement(state, &unparsed(), Some(&indent(editor)), up));
}

fn move_line(editor: &mut Editor, up: bool, cx: &mut Context<Editor>) {
    edit(editor, cx, |state, _| editing::move_line(state, up));
}

fn next_occurrence(editor: &mut Editor, cx: &mut Context<Editor>) {
    let run = recall(&editor.memory.occurrence, &editor.stamp());
    let step = editing::next_occurrence(&editor.edit_state(), run);
    if step.outcome == OccurrenceOutcome::NotFound {
        show_hint(editor, "Not found, press again to search from the top", cx);
    }
    if let Some(edit) = step.edit {
        apply_once(editor, edit, cx);
    }
    editor.memory.occurrence = step.run.map(|run| (editor.stamp(), run));
}

fn unselect_occurrence(editor: &mut Editor, cx: &mut Context<Editor>) {
    let run = recall(&editor.memory.occurrence, &editor.stamp());
    if let Some((edit, run)) = editing::unselect_occurrence(&editor.edit_state(), run) {
        apply_once(editor, edit, cx);
        editor.memory.occurrence = Some((editor.stamp(), run));
    }
}

fn clone_caret(editor: &mut Editor, clone: CloneCaret, cx: &mut Context<Editor>) {
    let goals = recall(&editor.memory.goals, &editor.stamp()).unwrap_or_default();
    if let Some(motion) = clone(&editor.edit_state(), &goals) {
        apply_once(editor, motion.edit, cx);
        editor.memory.goals = Some((editor.stamp(), motion.goals));
    }
}

fn expand_word(editor: &mut Editor, direction: ExpandDirection, cx: &mut Context<Editor>) {
    let others = editor.other_texts(cx);
    let session = editor.memory.expand.take();
    match word_completion::cyclic_expand_word(&editor.edit_state(), session, direction, || others) {
        ExpandOutcome::Expanded { edit, session } => {
            if apply_once(editor, edit, cx) {
                editor.memory.expand = Some(session);
            }
        }
        ExpandOutcome::Hint(hint) => show_hint(editor, hint, cx),
    }
}

/// Shows `hint` (English source text) at the caret of `editor`, once the current effects are delivered.
fn show_hint(editor: &Editor, hint: &'static str, cx: &mut Context<Editor>) {
    let intelligence = editor.overlay().and_then(|overlay| overlay.clone().downcast::<EditorIntelligence>().ok());
    let Some(intelligence) = intelligence else { return };
    cx.defer(move |cx| intelligence.update(cx, |this, cx| this.show_hint_at(hint, None, Tone::Info, None, cx)));
}

/// Paste as Plain Text: the clipboard's text as copied, without Reformat on Paste.
fn paste_as_plain_text(editor: &mut Editor, cx: &mut Context<Editor>) {
    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
        editor.paste_text(&text, false, cx);
    }
}

fn toggle_column_selection(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.memory.column_on = !editor.memory.column_on;
    editor.memory.column_block = None;
    cx.notify();
}

fn toggle_insert_state(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.overwrite = !editor.overwrite;
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::editor_commands::change;
    use crate::editor_commands::testing::{show, state};
    use crate::platform::actions::{ActionRegistry, execute_action_or_throw};
    use crate::platform::input;
    use crate::platform::storage::UiStorage;
    use crate::theme;
    use crate::workbench::Workbench;
    use gpui_kit::{ClipboardItem, Entity, Focusable, TestAppContext, VisualTestContext, px};
    use std::collections::BTreeSet;
    use std::time::Duration;

    /// A single-file window on a file of `name` with `text`, the editor with the keyboard.
    struct Fixture {
        editor: Entity<Editor>,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    async fn open<'a>(cx: &'a mut TestAppContext, name: &str, text: &str) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let file = folder.path().canonicalize().expect("the folder exists").join(name);
        std::fs::write(&file, text).expect("the file is written");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            workbench::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            workbench::menu::init(cx);
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| {
            cx.update(|window, cx| {
                workbench.read(cx).editor().is_some_and(|editor| editor.focus_handle(cx).is_focused(window))
            })
        });
        let editor = workbench.read_with(cx, |workbench, _| workbench.editor().cloned()).expect("an editor");
        (Fixture { editor, _folders: (home, folder) }, cx)
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    fn close(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    impl Fixture {
        /// Makes the document the text of `marked`, with its carets and selections.
        fn set(&self, cx: &mut VisualTestContext, marked: &str) {
            let wanted = state(marked);
            self.editor.update(cx, |editor, cx| {
                let len = utf16(&editor.text());
                if editor.text() != wanted.text() {
                    assert!(editor.apply_edit(&[change(0, len, wanted.text())], Vec::new(), 0, cx), "{marked}");
                }
                editor.set_selections(wanted.selections.clone(), wanted.main, cx);
            });
            cx.run_until_parked();
        }

        /// The text with its carets and selections marked.
        fn shown(&self, cx: &mut VisualTestContext) -> String {
            self.editor.read_with(cx, |editor, _| show(&editor.edit_state()))
        }

        /// Waits for the document's highlights, which tell its comments and string literals.
        fn highlighted(&self, cx: &mut VisualTestContext) {
            settle(cx, |cx| self.editor.read_with(cx, |editor, _| !editor.literal_tree().is_empty()));
        }
    }

    fn utf16(text: &str) -> u32 {
        text.encode_utf16().count() as u32
    }

    /// Runs `id` as a menu or the keymap would, on the window's editor.
    fn run(cx: &mut VisualTestContext, id: &str) {
        cx.update(|window, cx| execute_action_or_throw(id, None, window, cx))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        cx.run_until_parked();
    }

    /// Each case: the actions run in order, the marked text before and after them. The first range
    /// of a marked text is the main one.
    type Case = (&'static [&'static str], &'static str, &'static str);

    /// Runs every case through the registry, then checks that undo takes each back in one step.
    fn check(f: &Fixture, cx: &mut VisualTestContext, cases: &[Case]) {
        for &(ids, before, after) in cases {
            f.set(cx, before);
            for id in ids {
                run(cx, id);
            }
            assert_eq!(f.shown(cx), after, "{ids:?} on {before:?}");
            if f.editor.read_with(cx, |editor, _| editor.text()) != state(before).text() {
                for _ in 0..ids.len() {
                    cx.update(|window, cx| execute_action_or_throw("edit.undo", None, window, cx)).expect("undo runs");
                }
                cx.run_until_parked();
                assert_eq!(f.shown(cx), before, "undo of {ids:?}");
            }
        }
    }

    const LINE_CASES: &[Case] = &[
        (&["code.duplicateLine"], "a|b\nc", "ab\na|b\nc"),
        (&["code.duplicateLine"], "a|b\nc|d", "ab\ncd\na|b\nc|d"),
        (&["code.deleteLine"], "a|b\ncd\n", "c|d\n"),
        (&["code.moveLineDown"], "a|b\ncd", "cd\na|b"),
        (&["code.moveLineUp"], "ab\nc|d", "c|d\nab"),
        (&["code.moveStatementDown"], "a|b\ncd", "cd\na|b"),
        (&["code.moveStatementUp"], "ab\nc|d", "c|d\nab"),
        (&["code.joinLines"], "a|\n  b", "a| b"),
        (&["code.sortLines"], "c|\nb\na", "|a\nb\nc"),
        (&["code.reverseLines"], "a|\nb\nc", "|c\nb\na"),
        (&["code.toggleCase"], "one [two] three", "one [TWO] three"),
        (&["code.splitLine"], "ab|cd", "ab|\ncd"),
        (&["code.startNewLine"], "ab|cd", "abcd\n|"),
        (&["code.startNewLineBefore"], "ab|cd", "|\nabcd"),
        (&["code.completeStatement"], "let a = 1|", "let a = 1;\n|"),
        (&["code.commentLine"], "|let a;\nb", "//let a;\n|b"),
        (&["code.commentLine"], "|a\n|b\nc", "|//a\n|//b\nc"),
        (&["code.commentLine"], "//a|\nb", "a\nb|"),
        (&["code.commentBlock"], "[ab]", "/* [ab] */"),
    ];

    /// The line, statement, case and comment commands edit every range as one undo step.
    #[gpui_kit::test]
    async fn line_and_comment_commands_run_through_the_registry(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", "").await;
        check(&f, cx, LINE_CASES);
        close(cx);
    }

    const CARET_CASES: &[Case] = &[
        (&["code.addCaretNextOccurrence"], "|ab ab", "[ab] ab"),
        (&["code.addCaretNextOccurrence", "code.addCaretNextOccurrence"], "|ab ab", "[ab] [ab]"),
        (
            &["code.addCaretNextOccurrence", "code.addCaretNextOccurrence", "code.unselectOccurrence"],
            "|ab ab",
            "[ab] ab",
        ),
        (&["code.selectAllOccurrences"], "|ab abc ab", "[ab] abc [ab]"),
        (&["code.unselectOccurrence"], "[ab] [ab]", "ab [ab]"),
        (&["code.addCaretsToLineEnds"], "[ab\ncd\nef]", "ab|\ncd|\nef|"),
        (&["editor.cloneCaretBelow"], "a|b\ncd", "a|b\nc|d"),
        (&["editor.cloneCaretAbove"], "ab\nc|d", "a|b\nc|d"),
        (&["editor.cloneCaretBelow", "editor.cloneCaretBelow"], "abc|\nd\nefg", "abc|\nd|\nefg|"),
        (&["editor.matchBrace"], "f|(a)", "f(a)|"),
        (&["editor.codeBlockStart"], "{ a| }", "{| a }"),
        (&["editor.codeBlockEnd"], "{ a| }", "{ a |}"),
        (&["editor.codeBlockStartWithSelection"], "{ a| }", "{[ a] }"),
        (&["editor.codeBlockEndWithSelection"], "{ a| }", "{ a[ ]}"),
        (&["code.moveElementLeft"], "f(a, |b)", "f(|b, a)"),
        (&["code.moveElementRight"], "f(|a, b)", "f(b, |a)"),
        (&["code.cyclicExpandWord"], "alpha al|", "alpha alpha|"),
        (&["code.cyclicExpandWord", "code.cyclicExpandWord"], "alpha al|", "alpha al|"),
        (&["code.cyclicExpandWordBackward"], "al| alpha", "alpha| alpha"),
    ];

    /// The caret, occurrence and navigation commands move or add carets; Cyclic Expand Word cycles back to
    /// the typed prefix.
    #[gpui_kit::test]
    async fn caret_and_occurrence_commands_run_through_the_registry(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", "").await;
        check(&f, cx, CARET_CASES);
        // The keymap's caret click reaches the editor through the window's shortcut dispatch.
        f.set(cx, "|ab ab");
        let at = f.editor.read_with(cx, |e, _| e.position_of(4).expect("painted")) + gpui_kit::point(px(0.0), px(3.0));
        cx.simulate_click(at, gpui_kit::Modifiers { alt: true, shift: true, ..Default::default() });
        assert_eq!(f.shown(cx), "|ab a|b");
        // The run of Add Selection for Next Occurrence ends with any other selection change.
        f.set(cx, "|ab ab ab");
        run(cx, "code.addCaretNextOccurrence");
        f.editor.update(cx, |editor, cx| editor.set_selections(vec![(0, 2)], 0, cx));
        run(cx, "code.addCaretNextOccurrence");
        // Started from a selection, the run matches inside words too.
        assert_eq!(f.shown(cx), "[ab] [ab] ab");
        close(cx);
    }

    /// Paste as Plain Text pastes the clipboard as copied, without Reformat on Paste.
    #[gpui_kit::test]
    async fn paste_as_plain_text_keeps_the_copied_indentation(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", "").await;
        f.set(cx, "fn f() {\n    |\n}");
        cx.write_to_clipboard(ClipboardItem::new_string("a\n        b".to_owned()));
        run(cx, "code.pasteAsPlainText");
        assert_eq!(f.shown(cx), "fn f() {\n    a\n        b|\n}");
        close(cx);
    }

    /// Column Selection Mode makes ⇧+arrows select a block; Toggle Insert/Overwrite makes typing replace.
    #[gpui_kit::test]
    async fn toggles_switch_column_selection_and_overwrite(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "notes.txt", "").await;
        let toggled = |cx: &mut VisualTestContext, id: &str| {
            cx.update(|window, cx| {
                let action = cx.global::<ActionRegistry>().get(id).expect("registered");
                action.is_toggled(window, cx)
            })
        };
        f.set(cx, "a|bc\ndef");
        assert_eq!(toggled(cx, "editor.toggleColumnSelection"), Some(false));
        run(cx, "editor.toggleColumnSelection");
        assert_eq!(toggled(cx, "editor.toggleColumnSelection"), Some(true));
        cx.simulate_keystrokes("shift-down shift-right");
        assert_eq!(f.shown(cx), "a[b]c\nd[e]f");
        run(cx, "editor.toggleColumnSelection");
        f.set(cx, "a|bcd");
        run(cx, "editor.toggleInsertState");
        assert_eq!(toggled(cx, "editor.toggleInsertState"), Some(true));
        cx.simulate_input("XY");
        assert_eq!(f.shown(cx), "aXY|d");
        run(cx, "editor.toggleInsertState");
        cx.simulate_input("Z");
        assert_eq!(f.shown(cx), "aXYZ|d");
        close(cx);
    }

    /// Every id the registry runs here: the case tables and the tests that run actions one by one.
    #[test]
    fn every_command_is_run_through_the_registry_by_a_test() {
        let mut run: BTreeSet<&str> = [LINE_CASES, CARET_CASES]
            .iter()
            .flat_map(|cases| cases.iter())
            .flat_map(|case| case.0.iter().copied())
            .collect();
        run.extend(["code.pasteAsPlainText", "editor.toggleColumnSelection", "editor.toggleInsertState"]);
        let registered: BTreeSet<&str> =
            COMMANDS.iter().map(|(id, _)| *id).chain(TOGGLES.iter().map(|(id, _, _)| *id)).collect();
        assert_eq!(run, registered);
    }

    /// Enter indents (between brackets the closer goes to a line of its own), Tab indents to the next stop
    /// and Backspace in leading whitespace goes back to the proper indentation.
    #[gpui_kit::test]
    async fn enter_tab_and_backspace_follow_the_languages_indentation(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", "").await;
        f.set(cx, "fn f() {|}");
        cx.simulate_keystrokes("enter");
        assert_eq!(f.shown(cx), "fn f() {\n    |\n}");
        f.set(cx, "|a\n|b");
        cx.simulate_keystrokes("tab");
        assert_eq!(f.shown(cx), "    |a\n    |b");
        f.set(cx, "fn f() {\n        |x\n}");
        cx.simulate_keystrokes("backspace");
        assert_eq!(f.shown(cx), "fn f() {\n    |x\n}");
        // At the proper indentation the line joins the one above.
        cx.simulate_keystrokes("backspace");
        assert_eq!(f.shown(cx), "fn f() {|x\n}");
        close(cx);
    }

    /// Enter inside a line comment continues the comment, and a quote typed before the closing quote of a
    /// string steps over it: both read the comments and strings of the document's highlights.
    #[gpui_kit::test]
    async fn enter_continues_a_line_comment_and_a_closing_quote_is_stepped_over(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", "// one two\nlet s = \"ab\";\n").await;
        f.highlighted(cx);
        f.editor.update(cx, |editor, cx| editor.select(7, 7, cx));
        cx.simulate_keystrokes("enter");
        assert_eq!(f.shown(cx), "// one \n// |two\nlet s = \"ab\";\n");
        f.highlighted(cx);
        let quote = f.editor.read_with(cx, |editor, _| editor.text().find("\";").expect("the closing quote")) as u32;
        f.editor.update(cx, |editor, cx| editor.select(quote, quote, cx));
        cx.simulate_input("\"");
        assert_eq!(f.shown(cx), "// one \n// two\nlet s = \"ab\"|;\n");
        close(cx);
    }

    /// With Smart Keys › CamelHumps on, ⌥→ and ⌥⌫ stop at the humps of a word.
    #[gpui_kit::test]
    async fn word_movement_stops_at_humps_when_the_setting_asks(cx: &mut TestAppContext) {
        use crate::platform::settings::{SettingsWrite, set_global};
        let (f, cx) = open(cx, "main.rs", "").await;
        f.set(cx, "|fooBar baz");
        cx.simulate_keystrokes("alt-right");
        assert_eq!(f.shown(cx), "fooBar| baz");
        let write = [SettingsWrite { key: "editor.smartKeys.camelHumps", value: serde_json::json!(true) }];
        cx.update(|_, cx| set_global("w1", &write, cx)).expect("the setting is stored");
        settle(cx, |cx| f.editor.read_with(cx, |editor, _| editor.facts().camel_humps));
        f.set(cx, "|fooBar baz");
        cx.simulate_keystrokes("alt-right");
        assert_eq!(f.shown(cx), "foo|Bar baz");
        cx.simulate_keystrokes("end alt-left alt-backspace");
        assert_eq!(f.shown(cx), "foo|baz");
        close(cx);
    }

    /// Copy without a selection copies the caret line; pasting it at a caret puts it above the caret's line.
    #[gpui_kit::test]
    async fn a_line_wise_copy_pastes_above_the_caret_line(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "notes.txt", "").await;
        f.set(cx, "a|b\ncd");
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("ab"));
        f.editor.update(cx, |editor, cx| editor.select(4, 4, cx));
        cx.simulate_keystrokes("cmd-v");
        assert_eq!(f.shown(cx), "ab\nab\nc|d");
        // Cut without a selection takes the caret line with its line break.
        cx.simulate_keystrokes("cmd-x");
        assert_eq!(f.shown(cx), "ab\nab\n|");
        close(cx);
    }
}
