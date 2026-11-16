# Live templates in the editor: integration notes

New files (untracked, not declared yet):

- `app-gpui/src/templates/expand.rs` (382 lines): `TemplateStores` (the three stores read from the UI storage text, re-parsed only when a key's text changed), `EditorSnapshot` (an editor as `templates::editor` reads it), `literal_ancestors` (the comment or string at the caret from the editor's highlights, by binary search), `TemplateSession` (the snippet session: fields, completing flags, own-edit counter), `tab_press`, `escape_press`, `TemplateEdit`, `insert_live_template` (the rows of Insert Live Template's lookup), `lookup_keeps_rows`.
- `app-gpui/src/templates/chooser_view.rs` (465 lines): `TemplateChooser` (Choose Template, Surround With, Surround with Live Template, Expressions), `ChooserEvent { Picked { key, filter }, Cancelled }`, `filter_chooser_items`, `navigate`; accessors `title(cx)`, `filter()`, `lines()`, `selected_key()`.
- `app-gpui/src/templates/proofs33_expand.rs` (207), `app-gpui/src/templates/proofs33_chooser_view.rs` (205): their tests.
- `app-gpui/src/workbench/editor_templates.rs` (472 lines): `EditorTemplates` (window field), `register_actions`, `install_editor_templates` (key claim on the editor's focus + editor event subscription), the three actions, the chooser in the popup slot, the backend call of Surround With, `accept_lookup_template`, `template_session`, `template_chooser`, `template_chooser_closed`.
- `app-gpui/src/workbench/tests/proofs33_editor_templates.rs` (392), `app-gpui/src/workbench/tests/proofs33_surround.rs` (199): end-to-end tests.

Run `cargo fmt` after applying: a few code lines are over 120 columns (rustfmt breaks them); the JSON string literals in the tests stay long.

## (a) Hunks, in order, one group per part

Every anchor is quoted exactly from the file as it was read. The editor's key handling (`editor/view.rs` `key_down`, `platform/input.rs`) needs no hunk: the keys are claimed with `input::claim_keys` on the editor's focus handle.

### Part 1: Tab expansion and the snippet session (apply first; Parts 2 and 3 need it)

1. `app-gpui/src/templates/mod.rs`. After `pub mod emmet_model;` add `pub mod expand;`; before `pub mod editor;` add `pub mod chooser_view;`; at the end of the file, after `pub mod storage;`, add:

```rust
#[cfg(test)]
mod proofs33_chooser_view;
#[cfg(test)]
mod proofs33_expand;
```

2. `app-gpui/src/editor/marks.rs`, enum `MarkLayer`. After the anchor

```rust
    /// The usages Highlight Usages in File marks, until Escape.
    Usages,
```

add

```rust
    /// The fields of the live template being filled in.
    Snippet,
```

3. `app-gpui/src/editor/marks.rs`, enum `MarkKind`. After the anchor

```rust
    /// The line a code preview is on: its row is tinted in the current line's colour, focused or not.
    CurrentLine,
```

add

```rust
    /// A field of the live template being filled in: a faint background behind its text.
    SnippetField,
```

4. `app-gpui/src/editor/view.rs`, in the paint closure `let mark_color = |cx: &App, kind: MarkKind| match kind {` (the only exhaustive match on `MarkKind`). After the anchor

```rust
            MarkKind::CurrentLine => theme::color(cx, "bg-current-line"),
```

add

```rust
            MarkKind::SnippetField => theme::color(cx, "bg-editor-selection-inactive"),
```

It must stay inside that `match`; it does not depend on `key_down`.

5. `app-gpui/src/workbench/mod.rs`, module list. After `mod editor_structure;` add `mod editor_templates;`.

6. `app-gpui/src/workbench/mod.rs`, struct `Workbench`. After the anchor

```rust
    /// Where the caret has been in the window's editors: Back / Forward and the edit locations.
    editor_core: editor_core::EditorCore,
```

add

```rust
    /// Live and postfix templates in the window's editors: the template each one fills in, the chooser.
    editor_templates: editor_templates::EditorTemplates,
```

7. `app-gpui/src/workbench/mod.rs`, in `Workbench::new`'s struct literal. After `            editor_core: Default::default(),` add `            editor_templates: Default::default(),`.

8. `app-gpui/src/workbench/mod.rs`, `pub fn register_actions(cx: &mut App)`. After `    editor_core::register_actions(registry);` add `    editor_templates::register_actions(registry);`. (This registers the three actions.) `editor_templates.rs` calls `EditorIntelligence::show_exclusive`, builds `ContributedApply::Deferred` and defines `accept_lookup_template`, which hunk 16 calls: Parts 1 and 2 land together, hunks 1 to 23. Part 2's hunks change nothing until Insert Live Template runs.

9. `app-gpui/src/workbench/mod.rs`, `pub fn close_popup`. After the anchor `        self.usages_popup_closed(&popup.view, cx);` add `        self.template_chooser_closed(&popup.view);`.

10. `app-gpui/src/workbench/editor_intelligence.rs`, `install_intelligence`. After the anchor `        self.install_editor_core(editor, window, cx);` add `        self.install_editor_templates(editor, window, cx);`. It must come after `EditorIntelligence::install`: key claims run in the order they were made, so the completion lookup and the intelligence's Escape see a key first, as the webview's lookup takes Tab and Enter while it is open.

11. `app-gpui/src/workbench/mod.rs`, `mod tests`. After `    mod proofs27_gutter;` add:

```rust
    mod proofs33_editor_templates;
    mod proofs33_surround;
```

### Part 2: Insert Live Template in the completion lookup (the seam in the intelligence)

12. `app-gpui/src/editor_lsp/completion.rs`, enum `ContributedApply`. After the anchor

```rust
    /// An editor snippet template.
    Template(String),
```

add

```rust
    /// Put in by the feature that listed the row, which is told the row's id
    /// ([`crate::editor::intelligence::IntelligenceEvent::AcceptDeferred`]).
    Deferred(String),
```

13. `app-gpui/src/editor_lsp/completion.rs`, `pub fn apply_contributed` (it carries `#[expect(clippy::single_range_in_vec_init, …)]`). Before the anchor `        ContributedApply::Template(template) => {` add:

```rust
        // The feature that listed the row puts it in itself: nothing changes here.
        ContributedApply::Deferred(_) => AppliedCompletion {
            changes: Vec::new(),
            selection: vec![to..to],
            snippet: None,
            follow_ups: Vec::new(),
        },
```

14. `app-gpui/src/editor/intelligence.rs`, the `use crate::editor_lsp::completion::{` list. Replace

```rust
    AUTO_INSERT_DELAY_MS, Answered, ApplyMode, ApplyOptions, Ask, CompletionRequest, CompletionSource, FollowUp,
```

with

```rust
    AUTO_INSERT_DELAY_MS, Answered, ApplyMode, ApplyOptions, Ask, CompletionRequest, CompletionSource,
    ContributedApply, FollowUp,
```

15. `app-gpui/src/editor/intelligence.rs`, enum `IntelligenceEvent`. After the anchor

```rust
    /// Show the intentions list as the window's popup.
    Intentions(Entity<ListPopup>),
```

add

```rust
    /// A row another feature listed alone ([`EditorIntelligence::show_exclusive`]) was accepted:
    /// that feature puts in the row `id` in place of the text from `from` to the caret.
    AcceptDeferred { from: u32, id: String },
```

16. `app-gpui/src/workbench/editor_intelligence.rs`, `fn intelligence_event` (the only match on `IntelligenceEvent`). After the anchor `            IntelligenceEvent::Intentions(list) => self.show_intentions(intelligence, list, window, cx),` add:

```rust
            IntelligenceEvent::AcceptDeferred { from, id } => self.accept_lookup_template(*from, id, cx),
```

17. `app-gpui/src/editor/intelligence.rs`, struct `EditorIntelligence`. After the anchor

```rust
    intentions: Option<OpenIntentions>,
```

add

```rust
    /// The lookup lists the rows of another feature alone (Insert Live Template): typing filters them
    /// and no server is asked.
    exclusive: bool,
```

and in `fn new`'s struct literal, after `            intentions: None,` add `            exclusive: false,`.

18. `app-gpui/src/editor/intelligence.rs`, after the anchor

```rust
    pub fn lookup(&self) -> &Entity<CompletionLookup> {
        &self.lookup
    }
```

add

```rust
    /// Opens the lookup on `items`, rows another feature lists alone, for the word that starts at
    /// `from` and reads `typed` so far. Typing filters them without asking a server; accepting a
    /// [`ContributedApply::Deferred`] row is reported as [`IntelligenceEvent::AcceptDeferred`].
    pub fn show_exclusive(&mut self, from: u32, items: Vec<LookupItem>, typed: &str, cx: &mut Context<Self>) {
        self.close_lookup(cx);
        self.lookup_from = from;
        self.lookup_incomplete = false;
        let settings = self.settings(cx);
        // Typing filters the rows or closes the lookup: no character commits one, and a single row
        // waits for Enter.
        let options = LookupOptions {
            insert_on_special_keys: false,
            show_documentation: settings.show_documentation,
            sort_alphabetically: settings.sort_alphabetically,
            auto_insert_single: false,
        };
        let event = self.lookup.update(cx, |lookup, cx| {
            lookup.set_options(options, cx);
            lookup.set_items(items, typed, cx)
        });
        self.exclusive = self.lookup.read(cx).is_open();
        self.lookup_event_result(event, cx);
    }

    /// Whether the open lookup lists another feature's rows alone.
    pub fn is_exclusive(&self) -> bool {
        self.exclusive
    }

    /// The word before the caret changed while the lookup lists another feature's rows: they are
    /// filtered on it while an abbreviation may hold it, and the lookup closes otherwise.
    fn follow_exclusive(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let typed = {
            let editor = editor.read(cx);
            let caret = editor.caret();
            (caret >= self.lookup_from).then(|| buffer::slice(editor.rope(), self.lookup_from, caret))
        };
        match typed.filter(|typed| crate::templates::expand::lookup_keeps_rows(typed)) {
            Some(typed) => {
                let event = self.lookup.update(cx, |lookup, cx| lookup.set_query(&typed, cx));
                self.lookup_event_result(event, cx);
                if !self.lookup.read(cx).is_open() {
                    self.exclusive = false;
                }
            }
            None => self.close_lookup(cx),
        }
    }
```

19. `app-gpui/src/editor/intelligence.rs`, `fn follow_word`. After the anchor `    fn follow_word(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {` add:

```rust
        if self.exclusive {
            self.follow_exclusive(editor, cx);
            return;
        }
```

20. `app-gpui/src/editor/intelligence.rs`, `pub fn complete`. After the anchor

```rust
    pub fn complete(&mut self, explicit: bool, cx: &mut Context<Self>) {
        self.activation = None;
```

add `        self.exclusive = false;`.

21. `app-gpui/src/editor/intelligence.rs`, `fn close_lookup`. After the anchor `    fn close_lookup(&mut self, cx: &mut Context<Self>) {` add `        self.exclusive = false;`.

22. `app-gpui/src/editor/intelligence.rs`, `fn lookup_event`. Replace `            CompletionEvent::Closed => self.abort_completion(cx),` with

```rust
            CompletionEvent::Closed => {
                self.exclusive = false;
                self.abort_completion(cx);
            }
```

23. `app-gpui/src/editor/intelligence.rs`, `fn accept`. After the anchor `        let from = self.lookup_from.min(caret);` add:

```rust
        self.exclusive = false;
        if let LookupItem::Contributed(contributed) = item
            && let ContributedApply::Deferred(id) = &contributed.apply
        {
            cx.emit(IntelligenceEvent::AcceptDeferred { from, id: id.clone() });
            return;
        }
```

### Part 3: Surround With

No hunk beyond Part 1's: `code.surroundWith` and `code.surroundWithLiveTemplate` are registered by hunk 8 and use the popup slot (hunk 9).

### Part 4: postfix templates

No hunk: Tab on `expr.key` goes through `tab_press` (Part 1). Postfix and live templates in the ordinary completion list are not built (see (c)).

### Menus

`app-gpui/src/workbench/menu_spec.rs` already lists `code.insertLiveTemplate`, `code.surroundWith` and `code.surroundWithLiveTemplate` (lines with `act("code.insertLiveTemplate", "Insert Live Template…")`): nothing to add.

## (b) Parity rows

| Key                                                    | Expected                  | Proving tests (full names)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Missing                                                                                                                                                                                                                                                                                                      |
| ------------------------------------------------------ | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:code.insertLiveTemplate`                       | done                      | `workbench::tests::proofs33_editor_templates::opens_a_lookup_restricted_to_live_templates_and_ends_with_it` (registry and ⌘J, the lookup's rows, Enter, Escape, text and caret)                                                                                                                                                                                                                                                                                                                                                                                                                                                               | —                                                                                                                                                                                                                                                                                                            |
| `action:code.surroundWith`                             | done                      | `workbench::tests::proofs33_surround::wraps_the_selected_lines` (registry), `…::lists_the_languages_surrounders_with_numeric_mnemonics_and_a_mnemonic_key_picks` (⌥⌘T), `…::a_caret_on_a_multi_line_statement_surrounds_the_whole_statement`, `…::offers_expression_surrounders_for_a_selection_inside_a_line`                                                                                                                                                                                                                                                                                                                                | IPC `syntax_support` / `syntax_structural_ranges` are the commands it stands for                                                                                                                                                                                                                             |
| `action:code.surroundWithLiveTemplate`                 | done                      | `workbench::tests::proofs33_surround::surround_with_live_template_lists_the_selection_templates_and_configure_opens_the_settings` (⌥⌘J and registry, Escape, Configure Live Templates… opens Settings on `editor.liveTemplates`)                                                                                                                                                                                                                                                                                                                                                                                                              | —                                                                                                                                                                                                                                                                                                            |
| `point.registerEditorContribution:templates`           | partial                   | `workbench::tests::proofs33_editor_templates::expands_a_live_template_abbreviation`, `…::enter_moves_to_the_next_field_like_tab`, `…::esc_first_drops_the_fields_selection_then_leaves_the_template`, `…::does_not_expand_code_templates_inside_comments`, `…::expands_todo_with_the_languages_comment_start`, `…::tab_expands_the_live_templates_the_settings_stored`, `…::an_edited_built_in_expands_as_edited_and_a_disabled_template_does_not`, `…::a_clipboard_variable_takes_the_clipboard_text`, `…::expands_postfix_templates_replacing_the_expression`, `…::several_templates_of_one_abbreviation_open_choose_template_at_the_caret` | the completion source (live templates for a word, postfix after a dot, `showInCompletion`), completion opened on a `complete()` field (wired, not tested: no server in tests), the Expressions chooser from Tab (needs a syntax tree at the dot), Emmet's turn of Tab (other writer), the empty-field marker |
| `point.registerOverlay:templates.chooser`              | done                      | the chooser tests above (Choose Template, Surround With, Surround with Live Template) and `templates::proofs33_chooser_view::typing_filters_the_templates_and_enter_picks_the_selected_one`, `…::the_initial_filter_applies_and_escape_cancels`, `…::a_mnemonic_picks_its_item_and_section_rows_are_skipped`, `…::a_digit_picks_the_surrounder_it_numbers`, `…::an_empty_chooser_shows_its_empty_text`, `…::the_filter_puts_prefix_matches_first_then_loose_and_search_matches`, `…::the_keys_move_over_items_and_skip_section_rows`                                                                                                          | the Expressions chooser is built and reachable only from a postfix pick with several candidates, which Tab cannot produce yet                                                                                                                                                                                |
| `context:EDITOR_WHEN`                                  | not a context key         | `workbench::tests::proofs33_surround::the_template_actions_are_unavailable_in_a_read_only_editor` proves the condition the constant stands for                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | the inventory regex takes `EDITOR_WHEN` from the template literal `when` of `emmet.updateTag` (`index.ts:229`); the row should be dropped from the inventory, or kept as the condition's proof                                                                                                               |
| `unit:src/features/templates/__tests__/editor.test.ts` | done (already, as models) | 14 of 14 as models (`templates::editor::tests`). The app now reaches 11: Tab expansion 4 of 6 end to end (`expands_a_live_template_abbreviation` + falls through, `does_not_expand_code_templates_inside_comments`, `expands_todo_with_the_languages_comment_start`, `expands_postfix_templates_replacing_the_expression`), surround with 3 of 3, template fields 2 of 2, ⌘J 1 of 1                                                                                                                                                                                                                                                           | not reached: `wraps_compound_expressions_for_not_and_offers_a_chooser` (no tree at the dot), completion source 0 of 2                                                                                                                                                                                        |
| `unit:…/surround.test.ts`                              | done (models)             | 2 of 2 (`templates::editor::tests::takes_the_whole_lines_of_a_multi_line_statement`, `…::leaves_single_line_broken_and_line_sharing_statements_to_the_line_logic`); the app reaches both through `a_caret_on_a_multi_line_statement_surrounds_the_whole_statement` and the one-line Java test                                                                                                                                                                                                                                                                                                                                                 | —                                                                                                                                                                                                                                                                                                            |
| `unit:…/surrounders.test.ts`                           | done (models)             | 3 of 3; the app reaches all three through the Surround With tests (numbering, a template's mnemonic `C`, statement / expression filtering)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | —                                                                                                                                                                                                                                                                                                            |
| `unit:…/live.test.ts`                                  | done (models)             | 18 of 18; the app now reaches expansion, contexts, scopes, merges of user edits and deletions (`tab_expands_the_live_templates_the_settings_stored`), `$SELECTION$` indentation (Surround With), comment functions (`todo`), `clipboard()`                                                                                                                                                                                                                                                                                                                                                                                                    | `suggests variable names` only through postfix `.for`                                                                                                                                                                                                                                                        |
| `unit:…/postfix.test.ts`                               | done (models)             | 9 of 9; the app reaches the Rust method chain and the enabled / disabled settings                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | wider candidates from the tree are not reached                                                                                                                                                                                                                                                               |
| `unit:…/snippets.test.ts`                              | done (models)             | 2 of 2                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | plugin snippets are not fed to the native store (`LiveTemplates::set_contributed_snippets` has no caller)                                                                                                                                                                                                    |

## (c) Behaviours of the webview, built or left out

- The snippet session the brief says the native editor has for completion items does not exist: `editor_lsp::completion::apply_lsp_completion` and `apply_contributed` compute `AppliedCompletion::snippet` (an `ActiveSnippet`), but `EditorIntelligence::accept` drops it and nothing else consumes `ActiveSnippet`. Server snippet completions select their first field and Tab never walks them. The session here is new (`templates::expand::TemplateSession`, kept by the workbench per editor). Handing a server snippet to it is one optional hunk in `accept` (emit the `applied.snippet` after the edit); such a session starts with no own edit to skip, since the editor's report of the completion edit comes before that event.

- `index.ts:28` EDITOR_WHEN on the three actions: built (the manifest's `when`), proven read-only off.
- `index.ts:176` editor contribution `templates`: built as `install_editor_templates`.
- `index.ts:177` overlay `templates.chooser`: built as `TemplateChooser` in the popup slot.
- `index.ts:188-224` the three actions, `surroundTarget` awaited and dropped when the active editor changed (`index.ts:209`, `:222`): built.
- `editor.ts:77-80` user name from the project root: built (model).
- `editor.ts:82-94` the clipboard read only when a variable calls `clipboard()`: built, synchronous (`App::read_from_clipboard`); tested.
- `editor.ts:105-136` `expandInto`: built; `editor.ts:131` (document changed while the clipboard was read) cannot happen, the read is synchronous.
- `editor.ts:135` completion opened at once for a `complete()` first field, `editor.ts:158-166` on moving to such a field: built (`complete_in`, deferred past the expansion's own edit report), not tested (no server in tests).
- `editor.ts:172-186` Escape: built, but applied only while a template is filled in; the webview's keymap applies "drop the selection" whenever any snippet was ever used in the editor. With the lookup open and a selection, the webview closes both with one Escape; here the lookup's claim takes the first Escape.
- `editor.ts:194-198` Tab / Shift+Tab / Enter / Escape keys of a session: built.
- `editor.ts:203-215` `wordBefore`: model.
- `editor.ts:225-229` `templatesAt` with `TEMPLATE_CONTEXT_KEY` providers: built without providers (no native feature reports finer contexts such as `sql-statement`).
- `editor.ts:232-241`, `:278-293` exact matches, Choose Template when several: built, tested.
- `editor.ts:243-253` `caretAnchor`: built (`chooser_place`: under the caret's line, else the editor's corner + 40).
- `editor.ts:296-347` the ⌘J lookup restricted to templates, ending with the lookup: built through the seam (hunks 12-23), tested.
- `editor.ts:354-392` `multiLineStatement`, `surroundTarget`: built; the backend asked with `syntax_support` then `syntax_structural_ranges` off the main thread; a changed document gives no target, an error gives no target (as the webview's `catch`).
- `editor.ts:402-472` `surroundWith`: built (model + chooser), Configure Live Templates… runs `file.settings` with `editor.liveTemplates`.
- `editor.ts:477-499` `applyPostfix` and the Expressions chooser: built; reachable only with several candidates, which the text scan alone never gives (left out: a synchronous syntax tree at the dot).
- `editor.ts:501-521` postfix on Tab honouring `enabled`, `expandWithTab`, `disabled`, comments and strings: built, tested.
- `editor.ts:526-555` the completion source (postfix rows after a dot with `showInCompletion`, live template rows for a word with `showInCompletion`): left out. Native completion rows come only from the server; adding them needs a contributed source in `EditorIntelligence::completion_answered` and in its fallback path when no server runs, which the seam above prepares (`ContributedApply::Deferred`) but does not fill.
- `editor.ts:561` Emmet's load and `editor.ts:584` its Tab turn: left to the Emmet writer (`TabPress::Editor` stands where `expandEmmetAtCaret` goes).
- Snippet fields drawn (`cm-snippetField`): built as `MarkKind::SnippetField`; the empty-field marker (`cm-snippetFieldPosition`) is not drawn, `Editor::set_marks` drops empty marks.
- `sync.ts` reload on storage events: built differently, every key press compares the stored text with the last one read.
- Undo: the expansion is one `apply_edit_from` (its own history group). The webview's `snippet()` dispatch has no `userEvent`, so CodeMirror joins it with the typing just before when it comes within 500 ms; there one ⌘Z can take back the abbreviation and the expansion together, here ⌘Z always gives the abbreviation back first.

## (d) Hypotheses (not settled by compiling)

- Hypothesis: GPUI delivers the editor's `Edited` event for the expansion after `expand_template` stored the session (effects flushed at the end of the outer update), so the own-edit counter skips exactly that report. Settle: `enter_moves_to_the_next_field_like_tab` passes; if the fields shift, the report came before.
- Hypothesis: `cx.defer` in `complete_in` runs after that report, so the intelligence's `close_lookup` on an `Other` edit does not close the completion it opens. Settle: a template with `complete()` in a project with a language server.
- Hypothesis: `simulate_keystrokes("f o")` gives the chooser's claim a `key_char` (or the key name is used, as written).
- Hypothesis: the TypeScript, Python, Rust and Java grammars are bundled and highlight comments; `does_not_expand_code_templates_inside_comments` waits for highlights. Settle: run it.
- Hypothesis: `syntax_structural_ranges` at column 0 of `    if (x) {` returns the `if` statement as `statement`. Settle: `a_caret_on_a_multi_line_statement_surrounds_the_whole_statement`.
- Hypothesis: the effective indent units are TypeScript 2, Python, Rust and Java 4 (`languages/*.toml`), with no Code Style override in a fresh temp home; the expected texts use them.
- Hypothesis: Tab after a word no template has inserts spaces at the caret (`smart_keys::insert_tab`), so `logzzz ` starts the line.
- Hypothesis: a file made read-only on disk opens with `DocumentInfo::read_only` (documents.rs computes `!writable`) and publishes `editorReadOnly`.
- Hypothesis: the default keymap in tests is IntelliJ (macOS on a Mac): `cmd-j`, `cmd-alt-t`, `cmd-alt-j`; on Linux `ctrl-…`.
- Hypothesis: the token `bg-editor-selection-inactive` is a fitting field background (CodeMirror draws `#ffffff22` on dark, `#00000022` on light, no token exists).
- Hypothesis: `fs-code`, `fg-emphasis`, `accent-soft`, `h-row`, `fg-search-match` parse as theme tokens (they are in `tokens.css`; `accent-soft` and `h-row` are already used natively).
- Hypothesis: the Rust postfix `.for` stops on `item` with `item` selected, and Surround's `synchronized` leaves `this` selected (first field with a default).
- Hypothesis: `ChooserItem` mnemonics for Rust's expression surrounders are `1` and `2`, and `!(expr)` on `a + b` gives `!(a + b)`.
- Hypothesis: with `insert_on_special_keys` off, the template lookup commits on no typed character until its rows are navigated: `commits_lookup` still commits on `.`, `,`, `;`, `=`, space, `:` and `(` once the lookup is `focused()` (opened explicitly or navigated with the arrows), where the webview's lookup only filters or closes. Settle: type `.` after ↓ in the ⌘J lookup.

## (e) Tests most likely to fail first

- `a_caret_on_a_multi_line_statement_surrounds_the_whole_statement`: depends on the backend's statement at column 0 and on Java's indent unit.
- `wraps_the_selected_lines`, `expands_postfix_templates_replacing_the_expression`: the exact indentation of the inserted lines (base indent + indent unit per leading tab).
- `does_not_expand_code_templates_inside_comments`: needs the highlights of the TypeScript comment; without them the caret reads as code.
- `opens_a_lookup_restricted_to_live_templates_and_ends_with_it`: relies on the seam (hunks 12-23), on the lookup's row text `label — type` and on the lookup not being replaced by an auto-popup completion.
- `the_template_actions_are_unavailable_in_a_read_only_editor`: whether a 0444 file opens read-only.
- `esc_first_drops_the_fields_selection_then_leaves_the_template`: the second half depends on typing at the document's end after the first loop.

## (f) What a look at the real window must check

- Type `fori` and Tab in a `.ts` file: the three `i` are selected and drawn with the field background; typing renames all three; Enter goes to the array field; Tab to the body; the backgrounds go.
- Two templates sharing an abbreviation: Choose Template opens just under the caret's line, 260 to 520 px wide, the title in the small subtle font over a hairline, rows 22 px with the abbreviation in the code font and the description right-aligned, the selection in the tree's selection colour, typed letters shown in the header's accent badge and highlighted in the labels.
- ⌥⌘T on a selection: `1. if`, `2. if / else`… with the digit underlined, a "Live templates" section row with a line after it, the templates' mnemonic letter underlined inside the abbreviation, a separator and Configure Live Templates….
- ⌘J: the completion lookup (not the chooser) lists templates with their descriptions in the type column.
- A popup near the window's bottom edge stays inside the window.
