//! The editor commands that read more than the editor's text and selection: folding (the backend's
//! fold ranges), Extend / Shrink Selection (the backend's syntax ranges around the caret), Auto-Indent
//! Lines (the language's indentation), Scroll to Center and the editor's font size.
//!
//! The fold ranges are asked of the backend whenever the text changes and kept on the editor
//! ([`CoreState::folds`]), mapped through the edits until the backend answers for the new text, so
//! the folding commands act at once.

use super::actions::on_editor;
use super::buffer;
use super::folds::{Folds, toggle_fold};
use super::history::EditKind;
use super::nav_history::{SelectionStack, SimpleRange};
use super::view::Editor;
use crate::backend::Backend;
use crate::editor_commands::collapse_block::collapse_block;
use crate::editor_commands::extend_selection::{BackendChain, reusable_chain};
use crate::editor_commands::folding::{self, FoldEdit};
use crate::editor_commands::{Edit, EditState, IndentContext, SelectionExt, change, leading_space, map_after};
use crate::features_b::viewers::font_zoom::{MAX_EDITOR_ZOOM, MIN_EDITOR_ZOOM};
use crate::platform::actions::ActionRegistry;
use gpui_kit::{App, Context};

/// What the commands of this module keep on an editor.
#[derive(Debug, Default)]
pub struct CoreState {
    /// The fold ranges the backend offers and the folded regions.
    pub(super) folds: Folds,
    /// Counts fold requests, so that an answer a newer request overtook is dropped.
    fold_request: u64,
    /// One Extend / Shrink Selection stack per selection range, by index.
    stacks: Vec<SelectionStack>,
    /// The backend's syntax ranges of the last Extend Selection of the main range.
    chain: Option<BackendChain>,
    /// Extend Selection presses waiting for the backend's ranges: they run in order once it answers.
    queued_extends: u32,
    /// The font zoom, in px over the configured size.
    pub(super) font_zoom: i32,
}

/// A folding command: the folds to add and remove for the editor's state.
type FoldCommand = fn(&EditState, &Editor) -> Option<FoldEdit>;

/// Every folding command, by action id.
pub(super) const FOLD_COMMANDS: &[(&str, FoldCommand)] = &[
    ("code.fold", |state, editor| folding::fold_at_caret(state, editor.core.folds.context())),
    ("code.unfold", |state, editor| folding::unfold_at_caret(state, editor.core.folds.folded())),
    ("code.foldToggle", |state, editor| {
        let effects = toggle_fold(state, &editor.core.folds);
        (!effects.is_empty()).then_some(FoldEdit { effects, selection: None })
    }),
    ("code.foldAll", |state, editor| folding::collapse_all(state, editor.core.folds.context())),
    ("code.unfoldAll", |state, editor| folding::expand_all(state, editor.core.folds.folded())),
    ("code.collapseRecursively", |state, editor| folding::collapse_recursively(state, editor.core.folds.context())),
    ("code.expandRecursively", |state, editor| folding::expand_recursively(state, editor.core.folds.context())),
    ("code.foldSelection", |state, editor| folding::fold_selection(state, editor.core.folds.context())),
    ("code.collapseBlock", |state, editor| collapse_block(state, &editor.literal_tree(), editor.core.folds.folded())),
];

/// A command run on the window's editor.
type Command = fn(&mut Editor, &mut Context<Editor>);

/// The other commands of this module, by action id.
pub(super) const COMMANDS: &[(&str, Command)] = &[
    ("code.extendSelection", extend_selection),
    ("code.shrinkSelection", shrink_selection),
    ("code.autoIndentLines", auto_indent_lines),
    ("editor.scrollToCenter", |editor, cx| editor.scroll_caret_to_center(cx)),
    ("editor.increaseFontSize", |editor, cx| step_font_zoom(editor, 1, cx)),
    ("editor.decreaseFontSize", |editor, cx| step_font_zoom(editor, -1, cx)),
];

pub fn register(cx: &mut App) {
    let registry = cx.default_global::<ActionRegistry>();
    for &(id, command) in FOLD_COMMANDS {
        registry.handle(id, move |_, window, cx| on_editor(window, cx, |editor, cx| run_fold(editor, command, cx)));
    }
    for &(id, command) in COMMANDS {
        registry.handle(id, move |_, window, cx| on_editor(window, cx, command));
    }
}

impl Editor {
    /// The lines on screen, in order: every line that no fold hides.
    pub fn shown_lines(&self) -> Vec<u32> {
        self.core.folds.shown_lines(self.line_count())
    }

    /// The folded regions, in document order (UTF-16 offsets).
    pub fn folded_regions(&self) -> &[(u32, u32)] {
        self.core.folds.folded()
    }

    /// Whether the backend's fold ranges are those of the current text.
    pub fn folds_are_current(&self) -> bool {
        self.core.folds.answered() == Some(self.version())
    }

    /// The font zoom, in px over the configured size.
    pub fn font_zoom(&self) -> i32 {
        self.core.font_zoom
    }

    /// Sets the font zoom, kept within the limits. Answers whether it changed.
    pub fn set_font_zoom(&mut self, zoom: i32, cx: &mut Context<Self>) -> bool {
        let zoom = zoom.clamp(MIN_EDITOR_ZOOM, MAX_EDITOR_ZOOM);
        if zoom == self.core.font_zoom {
            return false;
        }
        self.core.font_zoom = zoom;
        cx.notify();
        true
    }
}

/// How long the text must be still before its fold ranges are asked again.
const FOLD_DELAY: std::time::Duration = std::time::Duration::from_millis(40);

/// Asks the backend for the fold ranges of the current text once it has been still for a moment; the
/// answer is kept unless the text moved on or a newer request was made meanwhile.
pub(super) fn refresh_folds(editor: &mut Editor, cx: &mut Context<Editor>) {
    if !cx.has_global::<Backend>() {
        return;
    }
    editor.core.fold_request += 1;
    let request = editor.core.fold_request;
    cx.spawn(async move |editor, cx| {
        cx.background_executor().timer(FOLD_DELAY).await;
        let asked = editor.update(cx, |editor: &mut Editor, cx| {
            (editor.core.fold_request == request).then(|| {
                let host = cx.global::<Backend>().host.clone();
                let (doc, label) = (editor.doc(), editor.window_label().to_owned());
                let work = async move { ide_host::ops::doc::doc_folding(&host, &label, doc).await };
                (editor.version(), doc, cx.global::<Backend>().call(work))
            })
        });
        let Ok(Some((version, doc, work))) = asked else { return };
        let ranges = match work.await {
            Ok(Ok(ranges)) => ranges,
            Ok(Err(error)) => {
                tracing::debug!("no fold ranges for document {doc}: {}", error.message);
                return;
            }
            Err(error) => {
                tracing::debug!("no fold ranges for document {doc}: {error:#}");
                return;
            }
        };
        let _ = editor.update(cx, |editor: &mut Editor, cx| {
            if editor.core.fold_request != request || editor.version() != version {
                return;
            }
            let state = editor.edit_state();
            let mut folds = std::mem::take(&mut editor.core.folds);
            folds.set_backend(&state, &ranges, &|pos: u32| editor.facts().comments_at(pos), version);
            editor.core.folds = folds;
            cx.notify();
        });
    })
    .detach();
}

/// Applies what a folding command answers: the folds change, then the selection moves where the
/// command says. A caret a new fold hides goes to the fold's start, where the placeholder is.
fn run_fold(editor: &mut Editor, command: FoldCommand, cx: &mut Context<Editor>) {
    let state = editor.edit_state();
    let Some(edit) = command(&state, editor) else { return };
    let changed = editor.core.folds.apply(&state, &edit.effects);
    let (ranges, main) = match edit.selection {
        Some(selection) => (selection.selections, selection.main),
        None => (state.selections.clone(), state.main),
    };
    let folded = editor.core.folds.folded().to_vec();
    let outside = |pos: u32| {
        folded.iter().filter(|(from, to)| *from < pos && pos < *to).map(|(from, _)| *from).min().unwrap_or(pos)
    };
    let ranges: Vec<(u32, u32)> = ranges.iter().map(|range| (outside(range.anchor()), outside(range.head()))).collect();
    if ranges != state.selections || main != state.main {
        editor.set_selections(ranges, main, cx);
    }
    if changed {
        cx.notify();
    }
}

/// Selections as the stacks read them.
fn simple(range: (u32, u32)) -> SimpleRange {
    SimpleRange { from: range.start(), to: range.end() }
}

/// The ranges around `current` that need no backend: the word, the comment or string literal and
/// its inside, the line from its indentation, the whole lines, the document.
fn text_candidates(editor: &Editor, current: SimpleRange) -> Vec<SimpleRange> {
    let rope = editor.rope();
    let mut out = Vec::new();
    let word = buffer::word_at(rope, current.from);
    if word.end > word.start {
        out.push(SimpleRange { from: word.start, to: word.end });
    }
    let tree = editor.literal_tree();
    let nodes = (0..tree.len()).map(|index| tree.node(index));
    for node in nodes.filter(|node| node.from <= current.from && current.to <= node.to) {
        out.push(SimpleRange { from: node.from, to: node.to });
        if node.to - node.from > 2 {
            out.push(SimpleRange { from: node.from + 1, to: node.to - 1 });
        }
    }
    let first = buffer::line_col(rope, current.from).0;
    let last = buffer::line_col(rope, current.to).0;
    let line = buffer::line_range(rope, first);
    let end = buffer::line_range(rope, last).end;
    let indent = crate::editor_commands::utf16_len(leading_space(&editor.line_text(first)));
    out.push(SimpleRange { from: line.start + indent, to: end });
    out.push(SimpleRange { from: line.start, to: end });
    out.push(SimpleRange { from: 0, to: buffer::len_utf16(rope) });
    out
}

/// The backend's syntax ranges as offsets of the current text.
fn chain_ranges(editor: &Editor, ranges: &[ide_core::Range]) -> Vec<(u32, u32)> {
    ranges
        .iter()
        .map(|range| {
            let (start, end) = (range.start, range.end);
            (editor.offset_of(start.line, start.col), editor.offset_of(end.line, end.col))
        })
        .collect()
}

/// Extend Selection (⌥↑): every range grows to the smallest syntax range strictly around it, as the
/// backend's tree has them for the main range and the text has them for every range. The backend
/// is asked once per run of presses at one place; presses while it answers wait and run in order.
fn extend_selection(editor: &mut Editor, cx: &mut Context<Editor>) {
    if editor.core.queued_extends > 0 {
        editor.core.queued_extends += 1;
        return;
    }
    let main = editor.selections()[editor.main_index()];
    if reusable_chain(editor.core.chain.as_ref(), u64::from(editor.version()), main).is_some() {
        extend_once(editor, cx);
        return;
    }
    let Some(host) = cx.try_global::<Backend>().map(|backend| backend.host.clone()) else {
        extend_once(editor, cx);
        return;
    };
    editor.core.queued_extends = 1;
    let (version, doc, label, pos) = (editor.version(), editor.doc(), editor.window_label().to_owned(), main.start());
    let work = cx
        .global::<Backend>()
        .call(async move { ide_host::ops::doc::doc_selection_ranges(&host, &label, doc, pos).await });
    cx.spawn(async move |editor, cx| {
        // Without the backend's ranges the text's own still extend.
        let ranges = match work.await {
            Ok(Ok(ranges)) => ranges,
            Ok(Err(error)) => {
                tracing::debug!("no selection ranges for document {doc}: {}", error.message);
                Vec::new()
            }
            Err(error) => {
                tracing::debug!("no selection ranges for document {doc}: {error:#}");
                Vec::new()
            }
        };
        let _ = editor.update(cx, |editor: &mut Editor, cx| {
            let presses = std::mem::take(&mut editor.core.queued_extends);
            if editor.version() == version {
                let ranges = chain_ranges(editor, &ranges);
                editor.core.chain = Some(BackendChain { counter: u64::from(version), pos, ranges });
            }
            for _ in 0..presses {
                extend_once(editor, cx);
            }
        });
    })
    .detach();
}

/// One Extend Selection step with the ranges known now.
fn extend_once(editor: &mut Editor, cx: &mut Context<Editor>) {
    let current: Vec<(u32, u32)> = editor.selections().to_vec();
    let main = editor.main_index();
    let counter = u64::from(editor.version());
    let mut stacks = std::mem::take(&mut editor.core.stacks);
    stacks.resize_with(current.len(), SelectionStack::default);
    let mut moved = false;
    let this: &Editor = editor;
    let next: Vec<(u32, u32)> = current
        .iter()
        .enumerate()
        .map(|(index, &range)| {
            let at = simple(range);
            let mut candidates: Vec<SimpleRange> = if index == main {
                reusable_chain(this.core.chain.as_ref(), counter, range)
                    .unwrap_or_default()
                    .iter()
                    .map(|&(from, to)| SimpleRange { from: from.min(to), to: from.max(to) })
                    .collect()
            } else {
                Vec::new()
            };
            candidates.extend(text_candidates(this, at));
            match stacks[index].extend(at, &candidates) {
                Some(grown) => {
                    moved = true;
                    (grown.from, grown.to)
                }
                None => range,
            }
        })
        .collect();
    stacks.truncate(current.len());
    if moved {
        editor.set_selections(next, main, cx);
    }
    editor.core.stacks = stacks;
}

/// Shrink Selection (⌥↓): every range goes back to what it was extended from.
fn shrink_selection(editor: &mut Editor, cx: &mut Context<Editor>) {
    let current: Vec<(u32, u32)> = editor.selections().to_vec();
    let main = editor.main_index();
    let mut stacks = std::mem::take(&mut editor.core.stacks);
    stacks.resize_with(current.len(), SelectionStack::default);
    let mut moved = false;
    let next: Vec<(u32, u32)> = current
        .iter()
        .enumerate()
        .map(|(index, &range)| match stacks[index].shrink(simple(range)) {
            Some(smaller) => {
                moved = true;
                (smaller.from, smaller.to)
            }
            None => range,
        })
        .collect();
    if moved {
        editor.set_selections(next, main, cx);
    }
    editor.core.stacks = stacks;
}

/// The edit Auto-Indent Lines makes: every line touched by a selection gets the indentation the
/// language gives it, reading the lines above as already re-indented; a blank line loses its
/// indentation. `None` when every line is indented already.
pub(super) fn auto_indent(state: &EditState, indent: impl Fn(&IndentContext<'_>) -> Option<u32>) -> Option<Edit> {
    let mut lines: Vec<u32> = Vec::new();
    for range in &state.selections {
        let (first, last) = (state.line_at(range.start()).number, state.line_at(range.end()).number);
        lines.extend(first..=last);
    }
    lines.sort_unstable();
    lines.dedup();
    let mut current = state.clone();
    let mut changes = Vec::new();
    for number in lines {
        let line = current.line(number);
        let Some(columns) = indent(&IndentContext::new(&current, line.from)) else { continue };
        let columns = if line.text.trim().is_empty() { 0 } else { columns };
        let present = leading_space(&line.text);
        let wanted = current.indent_string(columns);
        if present == wanted {
            continue;
        }
        let edit = change(line.from, line.from + crate::editor_commands::utf16_len(present), &wanted);
        current =
            crate::editor_commands::apply(&current, &Edit::new(vec![edit.clone()], current.selections.clone(), 0));
        changes.push(edit);
    }
    if changes.is_empty() {
        return None;
    }
    // Each change was made after the previous ones; in the original text's coordinates they are
    // its own range moved back by what the changes before it added.
    let mut original = Vec::with_capacity(changes.len());
    let mut shift: i64 = 0;
    for edit in changes {
        let from = (i64::from(edit.from) - shift) as u32;
        let to = (i64::from(edit.to) - shift) as u32;
        shift += i64::from(crate::editor_commands::utf16_len(&edit.insert)) - i64::from(edit.to - edit.from);
        original.push(change(from, to, edit.insert));
    }
    let selections = state
        .selections
        .iter()
        .map(|&(anchor, head)| (map_after(anchor, &original), map_after(head, &original)))
        .collect();
    Some(Edit::new(original, selections, state.main))
}

fn auto_indent_lines(editor: &mut Editor, cx: &mut Context<Editor>) {
    if editor.is_read_only() {
        return;
    }
    editor.edit_with(EditKind::Other, cx, |editor| {
        auto_indent(&editor.edit_state(), |context| editor.facts().indent(context))
    });
}

/// Increase / Decrease Font Size of the editor by one px, within the limits.
fn step_font_zoom(editor: &mut Editor, delta: i32, cx: &mut Context<Editor>) {
    let zoom = editor.font_zoom() + delta;
    editor.set_font_zoom(zoom, cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::editor_commands::testing::{show, state};
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::input;
    use crate::platform::storage::UiStorage;
    use crate::theme;
    use crate::workbench::{self, Workbench};
    use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext};
    use std::collections::BTreeSet;
    use std::time::Duration;

    struct Fixture {
        editor: Entity<Editor>,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    /// A single-file window on `name` with `text`, the editor with the keyboard.
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

    fn run(cx: &mut VisualTestContext, id: &str) {
        cx.update(|window, cx| execute_action_or_throw(id, None, window, cx))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        cx.run_until_parked();
    }

    impl Fixture {
        /// Puts the carets and selections of `marked` on the document's text, which must be its text.
        fn select(&self, cx: &mut VisualTestContext, marked: &str) {
            let wanted = state(marked);
            self.editor.update(cx, |editor, cx| {
                assert_eq!(editor.text(), wanted.text(), "{marked}");
                editor.set_selections(wanted.selections.clone(), wanted.main, cx);
            });
            cx.run_until_parked();
        }

        fn shown(&self, cx: &mut VisualTestContext) -> String {
            self.editor.read_with(cx, |editor, _| show(&editor.edit_state()))
        }

        fn lines(&self, cx: &mut VisualTestContext) -> Vec<u32> {
            self.editor.read_with(cx, |editor, _| editor.shown_lines())
        }

        /// Waits for the backend's fold ranges of the current text.
        fn foldable(&self, cx: &mut VisualTestContext) {
            settle(cx, |cx| {
                self.editor.read_with(cx, |editor, _| editor.folds_are_current() && editor.core.folds.is_foldable(1))
            });
        }
    }

    const CODE: &str = "fn a() {\n    if x {\n        y();\n    }\n    z();\n}\n";

    /// Each case: the actions run in order from the marked selection, and the lines on screen after.
    type FoldCase = (&'static [&'static str], &'static str, &'static [u32]);

    const FOLD_CASES: &[FoldCase] = &[
        (&["code.fold"], "fn a() {\n    if x {\n        y|();\n    }\n    z();\n}\n", &[0, 1, 4, 5, 6]),
        (
            &["code.fold", "code.unfold"],
            "fn a() {\n    if x {\n        y|();\n    }\n    z();\n}\n",
            &[0, 1, 2, 3, 4, 5, 6],
        ),
        (&["code.foldToggle"], "fn a() {\n    if| x {\n        y();\n    }\n    z();\n}\n", &[0, 1, 4, 5, 6]),
        (
            &["code.foldToggle", "code.foldToggle"],
            "fn a() {\n    if| x {\n        y();\n    }\n    z();\n}\n",
            &[0, 1, 2, 3, 4, 5, 6],
        ),
        (&["code.foldAll"], "fn a() {\n    if x {\n        y();\n    }\n    z|();\n}\n", &[0, 6]),
        (
            &["code.foldAll", "code.unfoldAll"],
            "fn a() {\n    if x {\n        y();\n    }\n    z|();\n}\n",
            &[0, 1, 2, 3, 4, 5, 6],
        ),
        (&["code.collapseRecursively"], "fn| a() {\n    if x {\n        y();\n    }\n    z();\n}\n", &[0, 6]),
        // The outer fold opens and the inner one stays folded.
        (
            &["code.collapseRecursively", "code.unfold"],
            "fn| a() {\n    if x {\n        y();\n    }\n    z();\n}\n",
            &[0, 1, 4, 5, 6],
        ),
        (
            &["code.collapseRecursively", "code.unfold", "code.expandRecursively"],
            "fn| a() {\n    if x {\n        y();\n    }\n    z();\n}\n",
            &[0, 1, 2, 3, 4, 5, 6],
        ),
        (&["code.foldSelection"], "fn a() {\n    if x {\n        [y();\n    }]\n    z();\n}\n", &[0, 1, 2, 4, 5, 6]),
        (&["code.collapseBlock"], "fn a() {\n    if x {\n        y|();\n    }\n    z();\n}\n", &[0, 1, 4, 5, 6]),
    ];

    /// Collapse, Expand, Toggle Fold, Collapse / Expand All, Collapse / Expand Recursively, Fold
    /// Selection and Fold Code Block hide and show the lines of the regions they act on.
    #[gpui_kit::test]
    async fn the_folding_commands_hide_and_show_the_lines_of_their_regions(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", CODE).await;
        f.foldable(cx);
        for &(ids, marked, lines) in FOLD_CASES {
            run(cx, "code.unfoldAll");
            f.select(cx, marked);
            for id in ids {
                run(cx, id);
            }
            assert_eq!(f.lines(cx), lines, "{ids:?} on {marked:?}");
        }
        close(cx);
    }

    /// A caret a new fold would hide goes to the fold's start; ↓ and ↑ then skip the hidden lines,
    /// and → steps over the folded region as one character.
    #[gpui_kit::test]
    async fn the_caret_skips_the_lines_a_fold_hides(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", CODE).await;
        f.foldable(cx);
        f.select(cx, "fn a() {\n    if x {\n        y|();\n    }\n    z();\n}\n");
        run(cx, "code.fold");
        assert_eq!(f.shown(cx), "fn a() {\n    if x {|\n        y();\n    }\n    z();\n}\n");
        cx.simulate_keystrokes("down");
        assert_eq!(f.editor.read_with(cx, |editor, _| editor.caret_position().0), 4);
        cx.simulate_keystrokes("up");
        assert_eq!(f.editor.read_with(cx, |editor, _| editor.caret_position().0), 1);
        cx.simulate_keystrokes("end right");
        assert_eq!(f.shown(cx), "fn a() {\n    if x {\n        y();\n    |}\n    z();\n}\n");
        // Still folded: the caret is on the fold's end, not inside it.
        assert_eq!(f.lines(cx), [0, 1, 4, 5, 6]);
        // A caret put inside the fold opens it.
        f.select(cx, "fn a() {\n    if x {\n        y|();\n    }\n    z();\n}\n");
        assert_eq!(f.lines(cx), [0, 1, 2, 3, 4, 5, 6]);
        close(cx);
    }

    /// A fold follows an edit above it: typing a line at the top moves the folded lines down by one.
    #[gpui_kit::test]
    async fn a_fold_follows_the_edits_above_it(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, "main.rs", CODE).await;
        f.foldable(cx);
        f.select(cx, "fn a() {\n    if| x {\n        y();\n    }\n    z();\n}\n");
        run(cx, "code.fold");
        f.editor.update(cx, |editor, cx| {
            editor.select(0, 0, cx);
            assert!(editor.insert(None, "// top\n", cx));
        });
        cx.run_until_parked();
        assert_eq!(f.lines(cx), [0, 1, 2, 5, 6, 7]);
        close(cx);
    }

    /// Extend Selection grows the selection through the word, the line and the syntax ranges around
    /// it; Shrink Selection goes back the same way.
    #[gpui_kit::test]
    async fn extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret(cx: &mut TestAppContext) {
        let text = "fn a() {\n    let value = call(one, two);\n}\n";
        let (f, cx) = open(cx, "main.rs", text).await;
        f.select(cx, "fn a() {\n    let value = call(o|ne, two);\n}\n");
        let mut seen = Vec::new();
        for _ in 0..4 {
            let before = f.shown(cx);
            run(cx, "code.extendSelection");
            settle(cx, |cx| f.shown(cx) != before);
            seen.push(f.shown(cx));
        }
        assert_eq!(seen[0], "fn a() {\n    let value = call([one], two);\n}\n");
        // Every step selects strictly more than the one before.
        let sizes: Vec<u32> =
            seen.iter().map(|marked| state(marked).selections[0]).map(|(anchor, head)| anchor.abs_diff(head)).collect();
        assert!(sizes.windows(2).all(|pair| pair[0] < pair[1]), "{seen:#?}");
        for expected in seen.iter().rev().skip(1) {
            run(cx, "code.shrinkSelection");
            assert_eq!(&f.shown(cx), expected);
        }
        run(cx, "code.shrinkSelection");
        assert_eq!(f.shown(cx), "fn a() {\n    let value = call(o|ne, two);\n}\n");
        close(cx);
    }

    /// Auto-Indent Lines gives every selected line the language's indentation, as one undo step.
    #[gpui_kit::test]
    async fn auto_indent_lines_reindents_the_selected_lines(cx: &mut TestAppContext) {
        let text = "fn a() {\nlet b = 1;\n        let c = 2;\n}\n";
        let (f, cx) = open(cx, "main.rs", text).await;
        f.select(cx, "fn a() {\n[let b = 1;\n        let c = 2;]\n}\n");
        run(cx, "code.autoIndentLines");
        assert_eq!(f.editor.read_with(cx, |editor, _| editor.text()), "fn a() {\n    let b = 1;\n    let c = 2;\n}\n");
        run(cx, "edit.undo");
        assert_eq!(f.editor.read_with(cx, |editor, _| editor.text()), text);
        close(cx);
    }

    /// Scroll to Center puts the caret's line in the middle of the view.
    #[gpui_kit::test]
    async fn scroll_to_center_puts_the_caret_line_in_the_middle_of_the_view(cx: &mut TestAppContext) {
        let text: String = (0..200).map(|n| format!("line {n}\n")).collect();
        let (f, cx) = open(cx, "notes.txt", &text).await;
        let caret = f.editor.read_with(cx, |editor, _| editor.offset_of(100, 0));
        f.editor.update(cx, |editor, cx| editor.select(caret, caret, cx));
        cx.run_until_parked();
        run(cx, "editor.scrollToCenter");
        cx.run_until_parked();
        let (top, painted) =
            f.editor.read_with(cx, |editor, cx| (editor.top_line(cx), editor.painted_lines().expect("painted")));
        let shown = painted.end - painted.start;
        assert!(top < 100 && 100 - top >= shown / 2 - 2 && 100 - top <= shown / 2 + 1, "{top} of {shown}");
        close(cx);
    }

    /// Every id the registry runs here is run by a test of this module.
    #[test]
    fn every_command_is_run_through_the_registry_by_a_test() {
        let mut run: BTreeSet<&str> = FOLD_CASES.iter().flat_map(|case| case.0.iter().copied()).collect();
        run.extend(["code.extendSelection", "code.shrinkSelection", "code.autoIndentLines", "editor.scrollToCenter"]);
        // Increase / Decrease Font Size need a project (`when: projectOpen`): the workbench's
        // `reset_font_size_resets_every_editor` runs them.
        run.extend(["editor.increaseFontSize", "editor.decreaseFontSize"]);
        let registered: BTreeSet<&str> =
            FOLD_COMMANDS.iter().map(|(id, _)| *id).chain(COMMANDS.iter().map(|(id, _)| *id)).collect();
        assert_eq!(run, registered);
    }

    /// The edit of Auto-Indent Lines is in the coordinates of the text before it.
    #[test]
    fn auto_indent_answers_its_changes_in_the_original_text() {
        let before = state("[a\nb\nc]");
        let edit = auto_indent(&before, |_| Some(2)).expect("every line changes");
        let after = crate::editor_commands::apply(&before, &edit);
        assert_eq!(after.text(), "  a\n  b\n  c");
        assert!(auto_indent(&after, |_| Some(2)).is_none());
    }
}
