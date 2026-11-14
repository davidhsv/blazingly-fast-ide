# Integrating the editor's remaining language and core actions

Three new files, none compiled yet:

- `app-gpui/src/workbench/editor_extras.rs`. It holds the `impl Workbench` blocks, `register_actions`, the handlers, and 15 end-to-end tests.
- `app-gpui/src/editor/extras.rs`. It adds `impl Editor` methods: `expand_to_level`, `fold_marker` / `FoldMarker`, `toggle_fold_at_line`, `context_header_lines`, `custom_regions`, `go_to_line_text`.
- `app-gpui/src/editor_lsp/reformat_dialog.rs`. It holds `ReformatDialog`, `ReformatOptions`, `LastReformatOptions` and `ReformatDialogEvent`, with 2 view tests and 1 model test.

All hunks below are anchored on text quoted from the files as they are now. Sections 1 to 9 are required; without them the new files do not compile, or `-D warnings` fails on dead code. Run `cargo fmt` after pasting.

## 1. `mod` lines

`app-gpui/src/editor/mod.rs`, after `pub mod core_actions;`:

```rust
pub mod extras;
```

`app-gpui/src/editor_lsp/mod.rs`, after `pub mod positions;`:

```rust
pub mod reformat_dialog;
```

`app-gpui/src/workbench/mod.rs`, after `mod editor_intelligence;`:

```rust
mod editor_extras;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The field

In `pub struct Workbench`, replace

```rust
    language: editor_language::EditorLanguage,
```

with

```rust
    language: editor_language::EditorLanguage,
    /// The status bar's transient message (Next Occurrence's "2 of 5").
    extras: editor_extras::EditorExtras,
```

### 2b. Its initial value

In `Workbench::new`, replace `            language: Default::default(),` with:

```rust
            language: Default::default(),
            extras: Default::default(),
```

The anchor `language: Default::default(),` occurs once. If it does not, put the line after `vcs_dialog: None,`.

### 2c. The actions

In `pub fn register_actions(cx: &mut App)`, replace `    editor_language::register_actions(registry);` with:

```rust
    editor_language::register_actions(registry);
    editor_extras::register_actions(registry);
```

### 2d. `pasteHistoryListFocus` (defect 5)

In `fn publish_context`, replace

```rust
        set_context(window, cx, "pasteHistoryOpen", self.paste_history_dialog().is_some());
```

with

```rust
        set_context(window, cx, "pasteHistoryOpen", self.paste_history_dialog().is_some());
        // The dialog's list and filter are one keyboard target: while it has the keyboard, the keys
        // bound to Paste and Delete in the keymap run them.
        let paste_list_focused =
            self.paste_history_dialog().is_some_and(|dialog| dialog.focus_handle(cx).contains_focused(window, cx));
        set_context(window, cx, "pasteHistoryListFocus", paste_list_focused);
```

The webview sets the key in `src/editor/core/view/PasteHistoryDialog.tsx:181` (`onFocusCapture` … `closest('[role="listbox"]')`) and clears it on blur and unmount (`:122`). The native dialog has a single focus, the filter field, which claims Enter, Delete and Backspace itself (`paste_history_view.rs` `claim_key`). So the key is true while the dialog holds the keyboard. That makes a keymap binding other than the claimed keys work, which is what the `keyWhen` exists for. Test: `the_paste_history_keys_are_enabled_while_its_dialog_has_the_keyboard`.

## 3. `app-gpui/src/editor/core_actions.rs`: one fold-applying function

Replace the whole function

```rust
/// Applies what a folding command answers: the folds change, then the selection moves where the
/// command says. A caret a new fold hides goes to the fold's start, where the placeholder is.
fn run_fold(editor: &mut Editor, command: FoldCommand, cx: &mut Context<Editor>) {
    let state = editor.edit_state();
    let Some(edit) = command(&state, editor) else { return };
    let changed = editor.core.folds.apply(&state, &edit.effects);
```

(only these first lines change; the rest of the body stays) with

```rust
fn run_fold(editor: &mut Editor, command: FoldCommand, cx: &mut Context<Editor>) {
    let state = editor.edit_state();
    let Some(edit) = command(&state, editor) else { return };
    apply_fold_edit(editor, &state, edit, cx);
}

/// Applies what a folding command answers for the text of `state`: the folds change, then the
/// selection moves where the command says. A caret a new fold hides goes to the fold's start, where
/// the placeholder is.
pub(super) fn apply_fold_edit(editor: &mut Editor, state: &EditState, edit: FoldEdit, cx: &mut Context<Editor>) {
    let changed = editor.core.folds.apply(state, &edit.effects);
```

The remaining body of the old `run_fold` (from `let (ranges, main) = match edit.selection {` to its closing `}`) becomes the body of `apply_fold_edit` unchanged. `state.selections.clone()`, `state.main` and `ranges != state.selections` work on `&EditState`. `FoldEdit` is already imported (`use crate::editor_commands::folding::{self, FoldEdit};`), and so is `EditState`.

## 4. `app-gpui/src/editor/folds.rs`: the backend's ranges for Context Info

After

```rust
    /// Whether line `line` offers a region to fold.
    pub fn is_foldable(&self, line: u32) -> bool {
        self.foldable.get(line as usize).is_some_and(Option::is_some)
    }
```

add

```rust
    /// The backend's fold ranges as offsets, sorted by start.
    pub fn backend(&self) -> &[BackendFold] {
        &self.backend
    }
```

## 5. `app-gpui/src/editor/view.rs`: fold chevrons in the gutter (defect 6, 3 sites)

### 5a. Paint the open chevron too

In `prepare`, replace

```rust
            markers.push(folded.map(|_| {
                let chevron = "›";
                window.text_system().shape_line(chevron.into(), font_size, &[run(chevron.len(), marker_color)], None)
            }));
```

with

```rust
            // A folded header shows the closed chevron, a line that starts a fold the open one.
            let chevron = if folded.is_some() { Some("›") } else { folds.is_foldable(line).then_some("⌄") };
            markers.push(chevron.map(|chevron| {
                window.text_system().shape_line(chevron.into(), font_size, &[run(chevron.len(), marker_color)], None)
            }));
```

`folds` is the binding that `folds.folded_on(state, line)` uses two statements above. If that binding is named differently, use the same name.

### 5b. A click on the chevron toggles the fold

In `fn mouse_down`, replace

```rust
        if let Some(line) = self.gutter_line_at(event.position) {
            (self.drag_anchor, self.adding) = (None, None);
            cx.emit(EditorEvent::GutterPressed { line, shift: event.modifiers.shift, alt: event.modifiers.alt });
            return;
        }
```

with

```rust
        if let Some(line) = self.gutter_line_at(event.position) {
            (self.drag_anchor, self.adding) = (None, None);
            // The chevrons' column, right of the line numbers, opens and closes the line's fold.
            if self.in_fold_column(event.position) && self.toggle_fold_at_line(line, cx) {
                return;
            }
            cx.emit(EditorEvent::GutterPressed { line, shift: event.modifiers.shift, alt: event.modifiers.alt });
            return;
        }
```

### 5c. The column

Directly before `   /// The 0-based line whose gutter (left of the text) is under`position`.` add:

```rust
    /// Whether `position` is on the column of the fold chevrons: the last 10 px of the gutter, where
    /// the chevron is painted. The line numbers end 12 px left of the text, outside it.
    fn in_fold_column(&self, position: Point<Pixels>) -> bool {
        self.layout.as_ref().is_some_and(|layout| {
            let x = position.x - layout.bounds.left();
            x >= layout.text_left - px(10.0) && x < layout.text_left
        })
    }
```

A press on the chevron column of a line with a fold no longer emits `GutterPressed`, so it no longer sets a breakpoint (`workbench/debug.rs` `GutterPressed`). Breakpoints are still set from the line numbers. Test: `a_click_on_the_fold_chevron_folds_and_unfolds_its_region`.

## 6. `app-gpui/src/workbench/chrome.rs`: draw the usages text and the status message (defect 2)

### 6a. Feed the two items

Replace

```rust
const FED_STATUS_ITEMS: [&str; 6] = [
    status_items::TASKS,
```

with

```rust
const FED_STATUS_ITEMS: [&str; 8] = [
    status_items::USAGES,
    status_items::INFO,
    status_items::TASKS,
```

and its doc line `/// shown editor's caret, line separator, encoding, indentation and read-only attribute.` with `/// shown editor's caret, line separator, encoding, indentation and read-only attribute, the highlighted usages' count and the transient status message.`

### 6b. Their texts

In `fn status_texts`, replace

```rust
        std::iter::once((status_items::TASKS, task_item(self.tasks.tasks(), i18n)))
            .chain(editor_items)
```

with

```rust
        let usages = self.usages_status().map(|text| StatusText::new(text).tooltip(text));
        std::iter::once((status_items::TASKS, task_item(self.tasks.tasks(), i18n)))
            .chain([(status_items::USAGES, usages), (status_items::INFO, info_item(self.status_info()))])
            .chain(editor_items)
```

and in the `use super::status_items::{` import add `info_item`, giving `self, Indent, caret_item, encoding_item, indent_item, info_item, line_ending_item, read_only_item, task_item,`.

Webview: `src/editor/lsp/index.ts:86` registers `editor-lsp.usagesStatus` (left, priority 10). `src/workbench/contributions.ts:1558` `showStatusInfo` (8 s) is drawn by the `workbench.info` item. Both specs already exist in `status_items.rs` `BUILT_INS`, and `info_item` existed but nothing called it. Test: `the_status_bar_shows_the_highlighted_usages_count`.

## 7. `app-gpui/src/workbench/editor_language.rs`: visibility for Reformat File…

- `struct FormatRun {` becomes `pub(super) struct FormatRun {`, and its three fields `    explicit: bool,`, `    selection_only: bool,` and `    tool: Option<ExternalFormatter>,` each become `pub(super)`. Anchor on the struct: the other `explicit: bool,` in the file is a function parameter of `format_failed`.
- `const REFORMAT: FormatRun =` becomes `pub(super) const REFORMAT: FormatRun =`.
- `type This = WeakEntity<Workbench>;` becomes `pub(super) type This = WeakEntity<Workbench>;`.
- `async fn format_document(` becomes `pub(super) async fn format_document(`.
- `async fn apply_silently(` becomes `pub(super) async fn apply_silently(`.

### 7b. Create New Test… only with the file templates (defect 4)

In `async fn goto_test`, replace

```rust
            GotoTestStep::Choose { title, items } => workbench.choose_test(editor, &title, items, window, cx),
```

with

```rust
            GotoTestStep::Choose { title, mut items } => {
                // Create New Test… opens the New File dialog of the file templates: without that
                // action the row is not offered.
                if !cx.global::<ActionRegistry>().contains("templates.newFromTemplate") {
                    items.retain(|item| !matches!(item.run, TestRun::CreateTest(_)));
                }
                if items.is_empty() {
                    hint(editor, "No tests found", None, Tone::Info, None, cx);
                } else {
                    workbench.choose_test(editor, &title, items, window, cx);
                }
            }
```

`TestRun` and `ActionRegistry` are already imported.

Webview: `src/editor/navigation/gotoTest.ts:117` always offers the row. Its `createTest` (`src/editor/navigation/index.ts:140`) runs `templates.newFromTemplate`, which (`src/features/templates/index.ts:233`) opens the New File dialog from the test template with `{ directory, name, extension, kindId: "test" }`. The native templates feature has no New File dialog registered, so the row would do nothing.

The Rust `CreateRustTestModule` row is kept, because it needs no template. "No tests found" is a new string, used only when a non-Rust file has no test at all; that case showed the lone Create row before. **Assumption**: add the string to the text catalog, or pick the wording you prefer. Test: `go_to_test_offers_create_new_test_only_with_the_file_templates`, which passes either way the templates go.

## 8. `app-gpui/src/find/view.rs`: Rerun Search

After `pub fn search(&mut self, cx: &mut Context<Self>) { … }` add:

```rust
    /// Rerun Search: the search last made runs again with the same query and options; without one,
    /// what is typed is searched.
    pub fn rerun(&mut self, cx: &mut Context<Self>) {
        match self.searched.clone() {
            Some(query) => self.run(query, cx),
            None => self.search(cx),
        }
    }
```

## 9. `app-gpui/src/vcs/prompt_dialogs.rs`: shared dialog parts

- `pub(super) fn checkbox(cx: &App, label: String, on: bool) -> Div {` becomes `pub(crate) fn checkbox(`…
- `pub(super) fn button(cx: &App, label: String, primary: bool, danger: bool, enabled: bool) -> Div {` becomes `pub(crate) fn button(`…

## 10. `app-gpui/src/workbench/editor_core.rs`: nothing to change

Reload All from Disk (defect 3) already runs `project.refresh` when it is registered (`for id in ["project.refresh", "vcs.refresh"]` guarded by `contains`). `editor_extras` now registers `project.refresh` (webview `src/features/project-tree/actions.ts:515`, `run: reloadAll`). Its handler is `ProjectTree::reload_all`, which existed with no caller.

**Check first**: grep the live `project_view_actions.rs`, which another agent is editing, for `"project.refresh"`. If it registers the id too, delete mine. Registering an id again replaces it silently. Test: `reload_all_from_disk_reads_the_project_tree_again`.

## Fixture changes to the fake server

None. Hover, formatting, range formatting and document highlight are all switched on through `answers["capabilities"]`, as the existing tests do.

## Context-menu literal rows

None change. The main menu (`menu_spec.rs`) already lists `navigate.customRegion`, `code.expressionTypeInfo`, `editor.contextInfo`, `code.unwrap` and the ten `code.expand*ToLevel*`. They appear once registered.

## Assumptions (check these first)

1. **Keystroke names.** `cmd-shift-* 1` reaches the `Meta+Shift+* 1` chord: the dispatcher's `key_name("*")` is `*`. `cmd-shift-delete` is `Meta+Shift+Delete`. `cmd-alt-.` is `Meta+Alt+.`. If the chord test fails, run the action instead and report the key mapping.
2. **`cmd-r` picks `find.rerun`.** `Meta+R` is shared with `debug.rerun`, `usages.rerun` and `run.rerun`. With the Find window active and no run or debug session, only `find.rerun`'s `when` should hold. The test also assumes that `toolwindow.find` makes `activeToolWindow == 'find'`.
3. **TODO navigator.** `TodoView::select_occurrence(forward, focus, cx)`'s `focus` means "focus the editor" (`TodoViewEvent::Open { focus }`). It returns no index, so a TODO step shows no "N of M". The Usages and Problems steps do (`step_occurrence`), but no test covers the "N of M" message: the TODO test proves the stepping only.
4. **Light-mode folds for Rust.** `doc_folding` answers `Region` folds for the blocks in `src/main.rs`; `the_editor_actions_do_nothing_outside_the_editor` in `editor_core.rs` relies on the same. Context Info reads only `FoldKind::Region` folds, as `src/editor/core/cm/stickyScroll.ts:20` does. Without backend folds the webview falls back to the Lezer tree, which is not built natively.
5. **Find bar focus.** Unwrap's negative key test opens the Find bar (`find.inFile`) and assumes `editorTextFocus` is false while its field has the keyboard. The Project view is not used for that, because ⇧⌘⌦ there is the tree's own Delete.
6. **Rust unwrap.** The tree-sitter chain at `foo();` contains `block(consequence)` → `if_expression`, and `expression_statement` is not in `unwrap.rs` `STATEMENTS`. That gives one option and the result `fn f() {\n    foo();\n}\n` with the caret at (1, 4).
7. **`publish_context` after Escape.** It runs again after the Paste from History dialog closes; `close_vcs_dialog` notifies, and the render publishes. The pasteHistory test asserts the key goes false.
8. **Reload All test.** If the file watcher is on in tests, `b.txt` shows without the reload. The test would then pass without proving the reload.

## Existing tests these hunks touch (read, not run)

- `workbench/debug.rs` `clicking_the_gutter_toggles_the_lines_breakpoint` presses at `text x - 12 px`. The fold column is the last 10 px of the gutter (§5c), so that press stays outside it and still sets the breakpoint. Do not widen the column past 11 px.
- `workbench/mod.rs` status bar tests (`texts.windows(3) … status_items`, `texts.contains("CRLF")`): the two new fed items have no text unless usages are highlighted or a message is shown, so they draw nothing there.
- `workbench/editor_core.rs` `paste_from_history_pastes_the_entry_chosen_in_the_dialog`: Backspace with text in the filter must not remove an entry. With §2d the `keyWhen` holds while the dialog is focused, but `platform/dispatcher.rs` passes plain keys typed in a `KeyTarget::TextField` (no command modifier) to the field before any binding is tried (`if !press.mods.command() && press.target == KeyTarget::TextField …  return Outcome::Pass`). So Backspace in the filter still edits it; the webview has the same exception (`PasteHistoryDialog.tsx` `!inSearch || !query || e.key !== "Backspace"`). This assumes the filter's key context is `TextField`; check `key_target` in `platform/input.rs`.
- `workbench/editor_language.rs` Go to Test tests: `opens_the_only_test_of_a_class` has one test, so it never reaches the chooser and does not change. No existing test asserts the "Create New Test…" row.
- `workbench/tests/smoke_registered_actions.rs`: the ten fold ids match its `code.expandToLevel` / `code.expandAllToLevel` `Digits` rules; every other id is in the manifest.
- Main menu: `menu_spec.rs` already lists the newly registered ids. A menu test that lists rows literally and expects them hidden while unregistered would now see them; grep for `"Custom Folding…"`, `"Context Info"`, `"Unwrap/Remove…"` and `"Expression Type Info"` in the tests before the first run. None was found in `workbench/mod.rs` at the time of reading.

## Conflicts with other drafts (read)

- `code_insight_integration.md` §5 lists `navigate.nextOccurrence` / `prevOccurrence` under "Not wired", so there is no double registration. My handler also steps the Problems window (`ProblemsView::step_occurrence`), through the `problems_view()` accessor that `workbench/code_insight.rs` already has.
- `project_services_terminal_integration.md` leaves `project.refresh` unregistered ("the file watcher re-reads the same folders, so no test can tell"). I register it here for Reload All. Its test has the same limit (assumption 7).

## Compile risks

- `editor_extras.rs` `syntax_chain`: `ide_host::ops::syntax::syntax_support(host, label, language)?.tree` takes `language: LanguageId` by value. `NodeAt` comes from `ide_syntax`, a dependency of app-gpui.
- `cx.spawn_in(window, async move |this: This, cx| …)`: the type annotation on the closure argument. Drop it if inference complains.
- `reformat_dialog.rs` `render`: `let i18n = &cx.default_global::<Localization>().0;` borrows `cx` until the last `tx` call, before `cx.listener` is used. If the borrow checker objects, collect the strings into a block that ends first, as `show_custom_regions` does.
- `go_to_occurrence` keeps no borrow of `self.tool_windows()` past building `candidates` (owned `String`s).
- `FormatRun { selection_only, ..REFORMAT }` needs all three fields `pub(super)` (section 7).
- Section 3: if `state` in the moved body is used by value somewhere, add `&`/`*` as needed.

## Tests most likely to fail first

1. `expand_to_level_folds_the_regions_nested_deeper_than_the_level`: the `*` chord (assumption 1), and the exact `shown_lines`, which depend on where Rust's folds start.
2. `rerun_search_finds_what_was_written_since_the_search`: the `cmd-r` arbitration (assumption 2).
3. `a_click_on_the_fold_chevron_folds_and_unfolds_its_region`: the click point is computed from `painted_bounds`, `w-gutter` and `line_height`. A zoom or a horizontal offset of the gutter would miss the column.
4. `next_occurrence_steps_through_the_results_of_the_tool_window`: whether the TODO view keeps the keyboard after an occurrence opens with `focus: false`, so that `cmd-alt-down` is dispatched in the tool window.
5. `unwrap_replaces_the_statement_around_the_caret_by_its_body` (assumption 5).

## What the webview does that is not built here, and why

- **Recent Locations** (`navigate.recentLocations`): not registered. The webview's popup (`src/features/search/recent/RecentPopups.tsx`, the "locations" kind: snippets, Show changed only, Delete) needs a new view. It also needs the workbench to record `RecentLocation`s on caret moves and edits (`everywhere/locations.rs` has the merge and record model). Today `RecentFilesEvent::ShowRecentLocations` only closes the popup.
- **Inline completion ghost text** (`editor.inlineCompletion.accept/acceptWord/dismiss/next/previous/trigger`, `src/editor/lsp/inlineCompletion.ts:305–325`): not registered. It is design only, below. The model (`editor_lsp/inline_completion.rs`, `InlineSession`, `Ghost`, `accept_inline`) is written; the paint and the per-keystroke wiring are not.
- **Soft wrap** (`editor.toggleSoftWrap`, `src/editor/core/actions.ts:736`): design only, below.
- **Semantic highlighting / rainbow brackets paint**: the toggles are registered and switch `editor.semanticHighlighting` / `editor.rainbowBrackets`, exactly the webview's `toggleSchemeSwitch` (`actions.ts:619`, `settingsSet("global", …)`). The editor paint does not read either setting yet (`grep semantic_highlighting app-gpui/src` finds only the settings page). Design below.
- **Expression Type Info popup**: the webview opens a doc popup with the type as a highlighted code block, anchored at the hover range (`typeInfo.ts:67`). This build shows the same text as the editor hint at the hover range's start. `QuickDocView` exists but no workbench code shows it yet; once Quick Documentation gets a popup layer, use a `DocEntry` with the fenced type.
- **Pasted imports** (`code.applyPastedImports`, `importsOnPaste.ts:389`): not registered. Its flow is not small. It watches pastes, waits for the document to sync and for a fresh diagnostics publication, asks quick fixes per unresolved name, then shows the Add Imports dialog. `PasteSession` models it all, but nothing in the workbench drives it. The action is only the dialog's button.
- **Add or Remove Caret** (`editor.addOrRemoveCaret`): registered with an empty handler, as in the webview (`actions.ts:809`, `run: () => {}`, hidden). Its only binding is `Alt+Shift+Button1`, a mouse gesture `editor/view.rs` already handles by reading the action's bindings. There is no keyboard form to build.
- **Other unregistered ids of `src/editor/lsp/actions.ts`**: `refactor.changeSignature`, `refactor.extractConstant`, `refactor.extractField`, `refactor.extractMethod`, `refactor.extractParameter`, `refactor.extractVariable`, `refactor.inline`, `refactor.move`, `refactor.pullMembersUp`, `refactor.pushMembersDown`, `refactor.safeDelete`, `refactor.thisMenu`. These are the refactoring dialogs (`src/editor/refactor/**`). Their models exist in `editor_refactor/`, but they are a feature of their own and are not drafted here.
- **`navigate.navigationBar`** (`src/editor/core/actions.ts:1004`): needs the navigation bar view. Not drafted.
- **Find window occurrence navigator**: the webview's Find tool window steps through matches with Next Occurrence. The native `FindPanel` has no step API, so only Usages and TODO step.
- **Fold persistence**: the webview does not save folds. `src/editor/core/model/session.ts` `FileViewState` keeps the caret and scroll only, so there is nothing to build. Fold by default (`src/editor/core/model/document.ts:573`: file header, imports, doc comments on the first fold answer) is not built natively, and the `editor.folding.*` settings exist without effect. It would go in `core_actions::refresh_folds` on the first `set_backend` of a document.

## Design (no code): soft wrap

Soft wrap changes every line↔row conversion. The row map is `editor/folds.rs` (`row_of_line`, `line_of_row`, `row_count`, `shown_lines`) behind `core_actions.rs` (`Editor::row_of_line` / `line_of_row` / `row_count`). Today a row is an unhidden line.

1. Add a `WrapMap` to `CoreState`, built from the text, the wrap width (the text area's width in columns, measured with the shaped `M` advance of the editor font) and the folds. It holds, per shown line, the UTF-16 offsets where a visual row starts, wrapping at the last whitespace before the width, else at the width.
2. Replace the four row functions by `WrapMap` lookups: `row_of_offset`, `offset_of_row_start`, `row_count`, and `rows_of_line`. Every caller of `row_of_line` / `line_of_row` that works by line must switch to offsets: `view.rs` `prepare` (the painted rows), `gutter_line_at`, `offset_at`, `position_of`, `scroll_to_line`, `top_line`, `reveal_caret`, `scroll_by`, the triple click in `mouse_down`, vertical caret motion (`editor_commands/vertical_motion.rs`), Page Up/Down, and `mark` painting, where a range spans rows.
3. Rebuild the map on text change (mapped, like folds), fold change, width change (the layout's bounds in `prepaint`) and font zoom.
4. `prepare` shapes one `ShapedLine` per visual row. Its gutter shows the number on the first row only.
5. The toggle: the `soft_wrap` field on the editor (webview `model.softWrapOn()`), seeded from `settings.editor.soft_wrap`. `toggleSoftWrap` is `handle_toggle` with `toggled` reading the active editor. `softWrapSurfaceFocus` (consoles) is a second target, for the Run / Terminal views.
6. Tests: a long line wraps into N rows at a width; ↓ from the first row of a wrapped line goes to its second row; a click on the second row places the caret in the line's tail; a fold inside a wrapped document hides whole lines; toggling it off restores one row per line.

## Design (no code): inline completion ghost text, semantic highlighting, rainbow brackets

- **Ghost text** is one new file plus anchored hunks in `view.rs` at 3 sites. Wire it in `editor/intelligence.rs`: on an edit, if `settings.editor.inline_completion.enabled`, start a timer. The delay is the webview's (`src/editor/lsp/inlineCompletion.ts`): `pauseMs` when `editor.inlineCompletion.onlyOnPause` is on (line 225), else `TYPING_DELAY = 75` ms (line 49); it is skipped while the completion lookup is active (line 223); after "no server supports inline completion" a per-document back-off of `BACKOFF_MS = 30_000` (line 51) applies; losing focus dismisses the ghost (line 210). When the timer fires, ask `lsp_inline_completion(automatic = true)` through `InlineSession::ask`, and cancel the previous request with `cancel_request`. Put the answer in `Ghost`.
  - Paint site 1: in `prepare`, the caret line's `ShapedLine` gets the ghost's first line appended as a muted run.
  - Paint site 2: the ghost's following lines are drawn as extra rows below. That is a row-map change too, so keep v1 to the first line and say so.
  - Paint site 3: `InlineCompletionVisible` is published as the context key `inlineCompletionVisible` for the six actions.
  - `accept` / `acceptWord` call `accept_inline`, then `apply_edit`; `next` / `previous` call `Ghost::cycle`; `dismiss` clears. Tab must yield to the completion lookup and to snippets, so check the `keyWhen` order.
  - Tests use the fake server with `"textDocument/inlineCompletion"` and `inlineCompletionProvider`. Assert that typing asks once after the delay, not on every keystroke; that Tab accepts; that `⌘→` accepts a word; that Escape dismisses; and that the keys do nothing without a ghost.
- **Semantic highlighting**: `editor_model/semantic_tokens.rs` exists. The paint needs `HighlightSpan`s from the server's semantic tokens, with a colour derived from the name for locals and parameters, merged over the syntax spans in `prepare` when the setting is on. That is one hunk at the span merge.
- **Rainbow brackets**: a pass over the `literal_tree` / bracket pairs that recolours `()[]{}` by depth with the `rainbow-*` tokens, as `recolor` is used for marks. That is one hunk in `prepare` after the syntax runs, read from `settings::effective(..).editor.rainbow_brackets`.

## Delays

Nothing in this draft asks a server per keystroke. Expression Type Info, Reformat File… and Unwrap ask once per invocation. The completion delay (100 ms) is untouched.

## Action → end-to-end test → IPC → expected `parity.json`

| Action id                                                                                                   | Test (`workbench/editor_extras.rs` unless noted)                                                                                                               | IPC (adapter → backend fn)                                                                                                                     | Status                                                   |
| ----------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `code.expandToLevel1..5`, `code.expandAllToLevel1..5`                                                       | `expand_to_level_folds_the_regions_nested_deeper_than_the_level`, `the_editor_actions_need_an_editor`                                                          | `doc_folding` (`src-tauri/src/commands/doc.rs` → `ide_host::ops::doc::doc_folding`)                                                            | done                                                     |
| `editor.contextInfo`                                                                                        | `context_info_shows_the_header_lines_of_the_code_around_the_caret`                                                                                             | `doc_folding`                                                                                                                                  | partial (no syntax-tree fallback)                        |
| `navigate.customRegion`                                                                                     | `custom_folding_lists_the_regions_and_moves_the_caret_to_the_chosen_one`                                                                                       | none (the text)                                                                                                                                | done                                                     |
| `code.unwrap`                                                                                               | `unwrap_replaces_the_statement_around_the_caret_by_its_body`                                                                                                   | `syntax_support`, `syntax_node_at` (`commands/syntax.rs` → `ops::syntax::syntax_support` / `syntax_node_at`)                                   | done                                                     |
| `code.expressionTypeInfo`                                                                                   | `expression_type_info_shows_the_type_of_the_expression_at_the_caret`                                                                                           | `lsp_hover` (`commands/lsp.rs` → `ops::lsp::lsp_hover`)                                                                                        | partial (hint, not a doc popup)                          |
| `code.reformatDialog`                                                                                       | `reformat_file_reformats_the_scope_chosen_in_the_dialog`; view: `runs_with_the_scope_and_options_chosen`, `without_a_selection_only_the_whole_file_is_offered` | `lsp_format` (`commands/lsp.rs` → `ops::lsp::lsp_format`); with Optimize imports `lsp_code_actions` + the action's edit (via `apply_silently`) | done                                                     |
| `find.rerun`                                                                                                | `rerun_search_finds_what_was_written_since_the_search`                                                                                                         | `search_start` (`commands/search.rs` → `ops::search`)                                                                                          | done                                                     |
| `navigate.nextOccurrence`, `navigate.prevOccurrence`                                                        | `next_occurrence_steps_through_the_results_of_the_tool_window`                                                                                                 | none (TODO: `search_start` for the scan)                                                                                                       | partial (no Find-window navigator; no "N of M" for TODO) |
| `editor.toggleSemanticHighlighting`, `editor.toggleRainbowBrackets`                                         | `the_color_scheme_switches_flip_their_settings`                                                                                                                | `settings_set` (`commands/app.rs` → `ide_host` global settings)                                                                                | partial (paint ignores them)                             |
| `editor.addOrRemoveCaret`                                                                                   | `the_editor_actions_need_an_editor`                                                                                                                            | none                                                                                                                                           | done                                                     |
| `project.refresh` (defect 3)                                                                                | `reload_all_from_disk_reads_the_project_tree_again`                                                                                                            | `fs_read_dir` (`commands/fs.rs`)                                                                                                               | done                                                     |
| defect 1 (Next Highlighted Usage fallback)                                                                  | `next_occurrence_steps_through_the_results_of_the_tool_window` (⌃⌥↓ step)                                                                                      | none                                                                                                                                           | done                                                     |
| defect 2 (usages text drawn)                                                                                | `the_status_bar_shows_the_highlighted_usages_count`                                                                                                            | `lsp_document_highlights`                                                                                                                      | done                                                     |
| defect 4 (Create New Test…)                                                                                 | `go_to_test_offers_create_new_test_only_with_the_file_templates`                                                                                               | `nav::find_files`                                                                                                                              | done (row hidden)                                        |
| defect 5 (`pasteHistoryListFocus`)                                                                          | `the_paste_history_keys_are_enabled_while_its_dialog_has_the_keyboard`                                                                                         | none                                                                                                                                           | done                                                     |
| defect 6 (fold chevrons)                                                                                    | `a_click_on_the_fold_chevron_folds_and_unfolds_its_region`                                                                                                     | `doc_folding`                                                                                                                                  | done (folds are not persisted, as in the webview)        |
| `editor.inlineCompletion.*`, `editor.toggleSoftWrap`, `navigate.recentLocations`, `code.applyPastedImports` | not registered                                                                                                                                                 | —                                                                                                                                              | missing                                                  |
