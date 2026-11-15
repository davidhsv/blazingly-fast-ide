# Editor paint and gesture wiring: integration notes

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. All paths below are relative to `app-gpui/src/`. Line numbers are hints at HEAD; apply by the quoted anchor text, which is unique in its file. Every item is independent: its hunks only insert next to anchors no other item rewrites (several items insert after the same anchor line; the anchor stays intact, so the order of application does not matter). Apply one item, build, run its tests, then the next.

Shared test-module locations (create the `mod` line only for the files of the items you apply):

- `editor/command_actions.rs` has an inline `mod tests` with no children yet. Its children live in `editor/command_actions/tests/<name>.rs`. Anchor for every `mod` line of that directory (line 283–284):

  ```rust
      use std::collections::BTreeSet;
      use std::time::Duration;
  ```

  insert the `mod <name>;` lines right after it.

- `editor/view.rs` `mod editing_tests` children live in `editor/view/editing_tests/`. Anchor (line 2815): `    mod proofs_editor_multi_caret;` — insert after it.
- `editor/paste_history_view.rs` `mod tests` (line 359) has no children; its children live in `editor/paste_history_view/tests/<name>.rs`.

---

## Item 5 — Column selection with the middle button, and a plain drag in Column Selection Mode

Webview source followed:

- `src/editor/core/cm/multiCaret.ts:46-51`: `blockMouseSelection` starts a block for `e.button === 0 && e.altKey`, and for `e.button === 1 && !e.altKey && usePlatform.getState().os !== "linux"` (after refusing the add-caret gesture, Shift, Meta and Ctrl).
- `src/editor/core/cm/columnMode.ts:163-169`: with Column Selection Mode on, a left press without Alt, Meta or Ctrl (Shift allowed) starts a block.

What exists today (quoted): `editor/view.rs:1736` only the ⌥ press starts `column_drag`; `editor/view.rs:1785` `if event.pressed_button != Some(MouseButton::Left) {` drops every drag that is not the left button; `editor/view.rs:2019` only `on_mouse_down(MouseButton::Left, …)` is registered.

New file: `editor/view/column_gestures.rs` (`impl Editor`: `press_selects_block`, `middle_mouse_down`, `middle_mouse_up`, `drags_with`; pure `left_press_selects_block`, `middle_press_selects_block` with unit tests).

Hunks, in order:

1. `editor/view.rs` line 54, after

   ```rust
   pub mod annotations;
   ```

   insert

   ```rust
   mod column_gestures;
   ```

2. `editor/view.rs` lines 1734–1736, replace

   ```rust
           // ⌥-drag selects a block of columns (the add-caret gesture, checked above, comes first).
           let modifiers = &event.modifiers;
           if event.click_count <= 1 && modifiers.alt && !modifiers.shift && !modifiers.platform && !modifiers.control {
   ```

   with

   ```rust
           // ⌥-drag, or a drag while Column Selection Mode is on, selects a block of columns (the
           // add-caret gesture, checked above, comes first).
           if self.press_selects_block(event) {
   ```

3. `editor/view.rs` line 1785 (in `fn mouse_move`), replace

   ```rust
           if event.pressed_button != Some(MouseButton::Left) {
   ```

   with

   ```rust
           if !self.drags_with(event.pressed_button) {
   ```

4. `editor/view.rs` line 2019 (in `impl Render for Editor`), replace

   ```rust
               .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
   ```

   with

   ```rust
               .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
               .on_mouse_down(MouseButton::Middle, cx.listener(Self::middle_mouse_down))
               .on_mouse_up(MouseButton::Middle, cx.listener(Self::middle_mouse_up))
   ```

Test files and `mod` lines:

- `editor/view/editing_tests/proofs23_middle_column.rs` → `    mod proofs23_middle_column;` after `    mod proofs_editor_multi_caret;` (view.rs:2815).
- `editor/command_actions/tests/proofs23_column_mode_drag.rs` → `    mod proofs23_column_mode_drag;` after the `use std::time::Duration;` anchor of `command_actions.rs`.

Parity rows:

- `unit:src/editor/core/cm/multiCaret.test.ts` → **done**, 6 of 6 cases. Case 6 "preserves middle-button column selection except on Linux" → `editor::view::editing_tests::proofs23_middle_column::preserves_middle_button_column_selection_except_on_linux` (asserts the block `[(1,3),(7,9),(14,16)]` off Linux and no change on Linux, and nothing with ⌥ held); also `…::a_middle_press_on_the_gutter_selects_nothing`. Cases 1–5 keep their existing tests (`proofs_editor_multi_caret::*`).
- `action:editor.toggleColumnSelection` → **done**: existing `editor::command_actions::tests::toggles_switch_column_selection_and_overwrite` (toggle, ⇧+arrows) plus `editor::command_actions::tests::proofs23_column_mode_drag::column_selection_mode_makes_a_plain_drag_select_a_block` (the action run through the registry, a plain left drag then selects `[(1,3),(7,9),(14,16)]`, and after toggling off the same drag selects `1..16`).

Behaviour notes: a double or triple click in Column Selection Mode still selects the word or line (the block starts only for `click_count <= 1`, as the ⌥ gesture already did). The existing test `matches_the_complete_stroke_and_only_the_left_button` presses the middle button with ⌥ held: `middle_press_selects_block` refuses ⌥, so it still sees one range.

Likely failures: on Linux CI the middle-drag test takes the `cfg!(target_os = "linux")` branch (expected). If the window does not route a middle `MouseDownEvent` to `on_mouse_down(MouseButton::Middle, …)` in the test platform, the drag test fails with `[(0,0)]` (hypothesis: check `gpui-pre-0.3.12/src/elements/div.rs` `on_mouse_down` filters on `event.button == button`).

---

## Item 1 — Right margin (hard wrap guide) and visual guides

Webview source followed:

- `src/editor/core/cm/prefs.ts:160-171` `marginGuides`: lines at `[o.hardWrapAt, ...visualGuides]`, deduplicated, `0 < c < 1000`, at `6px + c ch` (the 6px is `.cm-line` padding, `theme.ts:158`; the native text has no padding, so the guide is at `text_left + c × width of "0"`), 1px wide, colour `--border-right-margin`.
- `src/editor/core/cm/prefs.ts:537-538`: drawn only `if (prefs.appearance.hardWrapGuides && !o.distractionFree)`.
- `src/editor/core/cm/setup.ts:153-155`: the column is the language's `codeStyle.languages[id].hardWrapAt`, else `codeStyle.hardWrapAt`.

**Conflict with the assignment text:** the assignment says the margin is painted "also in Distraction Free Mode". The source says the opposite (`prefs.ts:537` `&& !o.distractionFree`), and the spec asserts it (`distractionFree.test.ts:44-48`: "hides line numbers, gutter, margin guides…", `not.toContain("background-image")`). The draft follows the source: no guides in Distraction Free Mode.

New file: `editor/view/right_margin.rs` (`pub(crate) fn margin_columns`, `Editor::right_margin_offsets`, `Editor::painted_right_margins` (public accessor, not test-only, cheap), `paint_right_margins`; unit test).

Hunks, in order:

1. `editor/view.rs` line 54, after `pub mod annotations;` insert

   ```rust
   pub(crate) mod right_margin;
   ```

   (`pub(crate)` because `editor/language_facts.rs` calls `margin_columns`.)

2. `editor/language_facts.rs` line 40 (struct `EditorFacts`), after

   ```rust
       /// Distraction Free Mode's text width, in characters.
       pub distraction_free_width: u32,
   ```

   insert

   ```rust
       /// The columns of the right margin and the visual guides, in order; empty when Appearance › Show
       /// hard wrap and visual guides is off.
       pub right_margins: Vec<u32>,
   ```

3. `editor/language_facts.rs` line 135 (in `from_settings`), after

   ```rust
               distraction_free_width: editor.appearance.distraction_free_width,
   ```

   insert

   ```rust
               right_margins: if editor.appearance.hard_wrap_guides {
                   let hard_wrap = style.and_then(|style| style.hard_wrap_at).filter(|at| *at > 0);
                   super::view::right_margin::margin_columns(
                       hard_wrap.unwrap_or(editor.code_style.hard_wrap_at),
                       &editor.code_style.visual_guides,
                   )
               } else {
                   Vec::new()
               },
   ```

   (`style` is the `let style = editor.code_style.languages.get(&language.id);` of line 92, still in scope.)

4. `editor/view.rs` line 185 (struct `Layout`), after

   ```rust
       line_height: Pixels,
       /// The first painted row (a row is a line no fold hides).
   ```

   replace with

   ```rust
       line_height: Pixels,
       /// Where the right margin and the visual guides are drawn, right of `text_left`.
       right_margins: Vec<Pixels>,
       /// The first painted row (a row is a line no fold hides).
   ```

5. `editor/view.rs` line 2267 (in `prepaint`), after

   ```rust
           let text_hitbox = Some(window.insert_hitbox(text_area, HitboxBehavior::Normal));
   ```

   insert

   ```rust
           let right_margins = editor.right_margin_offsets(&code_font, font_size, window, cx);
   ```

6. `editor/view.rs` line 2276 (the `Layout` literal of `prepaint`), replace

   ```rust
                   line_height,
                   first_line: first_row,
   ```

   with

   ```rust
                   line_height,
                   right_margins,
                   first_line: first_row,
   ```

7. `editor/view.rs` line 2509 (the `Layout` literal of `std::mem::replace` in `paint`), replace

   ```rust
                   line_height,
                   first_line: 0,
   ```

   with

   ```rust
                   line_height,
                   right_margins: Vec::new(),
                   first_line: 0,
   ```

8. `editor/view.rs` line 2341 (in `paint`), after

   ```rust
               window.paint_quad(fill(bounds, theme::color(cx, "bg-editor")));
   ```

   insert

   ```rust
               right_margin::paint_right_margins(&layout.right_margins, bounds, text_left, window, cx);
   ```

Paint cost: one shaped `"0"` per frame, and only while there are guides and the mode is off; one quad per guide. Nothing read from disk; the columns come from `EditorFacts`, read on `SettingsChanged`.

Test file and `mod` line: `editor/command_actions/tests/proofs23_right_margin.rs` → `    mod proofs23_right_margin;`.

Parity rows:

- `unit:src/editor/core/cm/distractionFree.test.ts` → **done**, 3 of 3 cases:
  - "defaults the text width to 80 columns" → existing `workbench::editor_extras::tests::proofs13_editor::defaults_the_text_width_to_80_columns_and_centres_at_the_configured_width`.
  - "shows line numbers and the right margin normally" → `editor::command_actions::tests::proofs23_right_margin::shows_line_numbers_and_the_right_margin_normally` (one guide at the x of column 120 of the painted line, then 40 and 120 with a visual guide, the language's 100, none with the setting off). Line numbers normally: existing `distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text`.
  - "hides line numbers, gutter, margin guides and sticky lines and centres at the configured width" → existing `workbench::editor_extras::tests::distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text` plus `…::proofs23_right_margin::hides_line_numbers_gutter_margin_guides_and_sticky_lines_and_centres_at_the_configured_width` (`editor.toggleZen` through the registry: no guides; toggled back: the guide returns).

Likely failures: the settings keys `editor.codeStyle.visualGuides`, `editor.codeStyle.languages`, `editor.appearance.hardWrapGuides` are the camelCase serde names of `CodeStyleSettings::visual_guides`, `languages`, `EditorAppearanceSettings::hard_wrap_guides` (`crates/ide-core/src/editor_settings.rs:77,418,424`); hypothesis: `set_settings` accepts a dotted path into nested objects as it does for `editor.smartKeys.camelHumps`. If `editor.toggleZen` is not registered by the command-actions fixture (it calls `workbench::register_actions`), the second test panics in `run`.

---

## Item 4 — String-literal clipboard

Webview source followed: `src/editor/core/cm/stringClipboard.ts:125-160` — a transaction filter on `input.paste` only (so Paste as Plain Text, `input.pastePlain`, typing and drops are untouched), escaping each change for the literal of the range it replaces, run before the paste reindentation (`:128` "escape before paste reindentation"); and a `clipboardOutputFilter` unescaping a copy or cut whose every range is inside a plain literal. Installed when `smartKeys.escapeOnPaste` is on (`prefs.ts:552`).

What exists today: `editor_commands::string_clipboard::{escape_paste, unescape_copied}` (model, 15 tests) and `Editor::literal_tree` (`editor/view.rs:759`), with no caller. `Editor::copy` (`view.rs:1187-1192`) writes `clipboard::copy` as is; `Editor::paste_text` (`view.rs:1214-1226`) pastes `clipboard::paste` then reindents.

New file: `editor/view/string_literals.rs` (`Editor::escaped_paste`, `Editor::unescaped_copy`).

Hunks, in order:

1. `editor/view.rs` line 54, after `pub mod annotations;` insert `mod string_literals;`.

2. `editor/language_facts.rs` line 31–32 (struct), after

   ```rust
       /// Smart Keys › Enter › Insert documentation comment stub.
       pub insert_doc_stub: bool,
   ```

   insert

   ```rust
       /// Smart Keys › Escape text on paste: string literals escape what is pasted into them and
       /// unescape what is copied from them.
       pub escape_on_paste: bool,
   ```

3. `editor/language_facts.rs` line 124 (in `from_settings`), after

   ```rust
               insert_doc_stub: keys.insert_doc_stub,
   ```

   insert

   ```rust
               escape_on_paste: keys.escape_on_paste,
   ```

   (`SmartKeysSettings::escape_on_paste`, `crates/ide-core/src/editor_settings.rs:264`, default `true` at `:282`.)

4. `editor/view.rs` lines 1188–1189 (in `pub fn copy`), replace

   ```rust
           let copied = crate::editor_commands::clipboard::copy(&self.edit_state());
           cx.write_to_clipboard(ClipboardItem::new_string(copied.text.clone()));
   ```

   with

   ```rust
           let state = self.edit_state();
           let mut copied = crate::editor_commands::clipboard::copy(&state);
           copied.text = self.unescaped_copy(&state, &copied.text);
           cx.write_to_clipboard(ClipboardItem::new_string(copied.text.clone()));
   ```

   (Cut goes through `copy`, so it unescapes too. The history records the unescaped text, as the webview records `copiedText`, `clipboardHistory.ts`.)

5. `editor/view.rs` lines 1219–1222 (in `pub(super) fn paste_text`), replace

   ```rust
               let pasted = clipboard::paste(&state, text, linewise)?;
               if !reformat {
                   return Some(pasted);
               }
   ```

   with

   ```rust
               let pasted = clipboard::paste(&state, text, linewise)?;
               if !reformat {
                   return Some(pasted);
               }
               // An ordinary paste into a string literal is escaped for it, before it is reindented.
               let pasted = editor.escaped_paste(&state, &pasted);
   ```

   (`reformat == false` is Paste as Plain Text, `command_actions.rs:256`, which stays raw.)

Test file and `mod` line: `editor/command_actions/tests/proofs23_string_clipboard.rs` → `    mod proofs23_string_clipboard;`.

Parity row `unit:src/editor/core/cm/stringClipboard.test.ts` → **done**, 16 of 16: the 15 model cases keep `editor_commands::string_clipboard::tests::*`; the window tests (each opens a real file of the language in the workbench, waits for its highlights, and drives ⌘V / ⌘C / ⌘X / ⌘Z or the registry):

| spec case                                                                                                                                                              | native test (`editor::command_actions::tests::proofs23_string_clipboard::…`)                                                                     |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| %s escapes ordinary pastes into a string (9 languages)                                                                                                                 | `escapes_ordinary_pastes_into_a_string`                                                                                                          |
| %s unescapes copied string contents (9 languages)                                                                                                                      | `unescapes_copied_string_contents`                                                                                                               |
| %s round-trips pasted control characters and Unicode                                                                                                                   | `round_trips_pasted_control_characters_and_unicode` (JavaScript)                                                                                 |
| %s escapes only the single-quoted string's delimiter                                                                                                                   | `escapes_only_the_single_quoted_strings_delimiter`                                                                                               |
| leaves %s unchanged (pastePlain, type, drop)                                                                                                                           | `leaves_paste_as_plain_text_and_typing_unchanged` (`code.pasteAsPlainText` through the registry, and typing; the native editor has no text drop) |
| disables both escaping and unescaping with the setting                                                                                                                 | `disables_both_escaping_and_unescaping_with_the_setting`                                                                                         |
| escapes Kotlin dollars and unescapes them on copy                                                                                                                      | `escapes_kotlin_dollars_and_unescapes_them_on_copy`                                                                                              |
| requires every caret to be inside a plain string                                                                                                                       | `requires_every_caret_to_be_inside_a_plain_string`                                                                                               |
| escapes per-caret inserts with each literal's delimiter…                                                                                                               | `escapes_per_caret_inserts_with_each_literals_delimiter_and_maps_all_carets`                                                                     |
| copies multiple string selections as decoded fragments                                                                                                                 | `copies_multiple_string_selections_as_decoded_fragments`                                                                                         |
| replaces a substring and preserves the caret, undo and redo                                                                                                            | `replaces_a_substring_and_preserves_the_caret_undo_and_redo`                                                                                     |
| handles native paste, copy, cut and rich copy consistently                                                                                                             | `handles_native_paste_copy_cut_and_rich_copy_consistently` (no rich copy natively)                                                               |
| leaves unsupported literals/code unchanged; preserves copied source outside one plain string; preserves edits that split an existing escape; decodes supported escapes | model tests only (they depend on the tree's node shapes, which the model tests build by hand)                                                    |

Difference kept (not fixed here): Paste from History pastes through `workbench/editor_core.rs:390-400` `paste_into` → `Editor::apply_edit`, which skips `paste_text`, so it is not escaped; the webview's history paste goes through `pasteEditorText` → `dispatchPaste` with `input.paste` (`clipboard.ts:75-79`) and is escaped. Fix (one line, separate change): build the edit through `paste_text`, or apply `escaped_paste` in `paste_into`.

Likely failures:

- The native literal tree comes from the highlights (`literal_tree`, `view.rs:759-780`): a string node exists only where the grammar's query highlights the literal as `String`/`StringSpecial`/`Escape` including its quotes. Hypothesis: true for the nine grammars; check with `cargo test -p ide-language` on `highlights` of `"x"` per language. Where a grammar highlights the quotes as punctuation, `plain_string_at` finds no quote and nothing is escaped (the loops name the file in the assertion).
- `round_trips_…`: the test platform's clipboard must keep `\0`.
- Difference recorded: `literal_tree()` is empty while the highlights are of an older version (`view.rs:763`), so a paste made within the highlight delay after an edit (40 ms plus the backend's answer, `view.rs:108`) is not escaped and a copy then is not unescaped; the webview's tree is always current. The tests wait for the highlights before every paste for that reason.
- `open_file` reads the fixture's private `_folders.1` (allowed from a child module of `tests`).

---

## Item 2 — Doc-comment tags

**Read first: in the default (light) theme the tags look like the comment.** `--syn-doc-comment-tag` equals `--syn-doc-comment` there (`#8c8c8c`, `src/styles/tokens.css:530-531`); the webview tells them apart by the underline of `--syn-doc-comment-tag-effect: underline #999999` (`:532`), which the native editor does not draw (it draws no `--syn-*-effect` for any token). Only `@param`'s value (`--syn-doc-comment-tag-value: #3d3d3d`) differs in the light theme; the dark themes colour every tag kind. The tests assert under the dark theme for that reason. Drawing the effects is a follow-up for all tokens (a `TextRun.underline`/`strikethrough` from the resolved `-effect` value).

Webview source followed: `src/editor/core/cm/docCommentTags.ts` (decorations over the visible ranges, from the comment nodes of the syntax tree; rebuilt on a highlighter or language change); colours `highlight.ts:73-75`: `doc-comment-tag`, `doc-comment-tag-value`, `doc-markup`, each `italic: "normal", bold: "var(--fw-code, 400)"`.

What exists today: the model `editor_commands::doc_comment_tags::{doc_comment_tag_ranges, visible_doc_comment_tags, has_doc_comment_tags}` (9 tests); nothing calls it. `Editor.language` is set once in `with_text` (`view.rs:395`) and never changes: the listener (`view.rs:353-371`) ignores `IdeEvent::DocLanguage` (`crates/ide-core/src/event.rs:74`, emitted by `Documents::redetect_languages`, `crates/ide-workspace/src/documents.rs:807`).

Design: per frame, only the highlight spans that touch the painted rows are read (`partition_point`, as the run loop does), comments of one kind joined into nodes of a small tree (`DocComment` for `SyntaxHighlightKind::DocComment`, `Comment` otherwise — the model's Kotlin branch needs the `DocComment` name), and `visible_doc_comment_tags` scans the painted rows only (consecutive rows merged; a fold's hidden lines are never scanned). Tags are drawn over the comment's run with the code font (upright, main weight) in their token's colour, before the search marks recolour (so a match still wins). Highlights of an older text are clamped to the current length.

New file: `editor/view/doc_tags.rs` (`Editor::painted_doc_tags`, `Editor::change_language`, `colour_doc_tags`; 2 unit tests).

Hunks, in order:

1. `editor/view.rs` line 54, after `pub mod annotations;` insert `mod doc_tags;`.

2. `editor/view.rs` line 363 (in `with_text`, the listener's `match`), before

   ```rust
               IdeEvent::SettingsChanged { .. } => {
                   let _ = weak.update(cx, |editor, cx| {
                       editor.read_facts();
   ```

   insert

   ```rust
               IdeEvent::DocLanguage { doc: changed, language, .. } if *changed == doc => {
                   let language = language.clone();
                   let _ = weak.update(cx, |editor, cx| editor.change_language(language, cx));
               }
   ```

3. `editor/view.rs` line 2153 (in `prepaint`), after

   ```rust
           let last_offset = buffer::line_range(&editor.rope, last_line.saturating_sub(1)).end;
   ```

   insert

   ```rust
           let doc_tags = editor.painted_doc_tags(&painted);
   ```

4. `editor/view.rs` line 2208 (in `prepaint`'s row loop), before

   ```rust
               for mark in marks.iter().filter(|mark| mark.kind.recolors_text()) {
   ```

   insert

   ```rust
               if !doc_tags.is_empty() {
                   runs = doc_tags::colour_doc_tags(runs, &text, range.start, &doc_tags, &code_font, cx);
               }
   ```

Paint cost: bounded by the spans touching the painted rows and the painted rows' text; `edit_state()` clones the rope (shared chunks) once per frame only when the language has tags and a comment is on screen.

Test file and `mod` line: `editor/command_actions/tests/proofs23_doc_comment_tags.rs` → `    mod proofs23_doc_comment_tags;`.

Parity row `unit:src/editor/core/cm/docCommentTags.test.ts` → **partial, 12 of 13** (expected). The 9 model cases keep `editor_commands::doc_comment_tags::tests::*`; window tests (`editor::command_actions::tests::proofs23_doc_comment_tags::…`):

- "styles the tags with the highlight style's classes, inside the comment's own" → `styles_the_tags_with_the_highlight_styles_classes_inside_the_comments_own` (a Java file: `@param`, its name `id` and `<b>` painted in `syn-doc-comment-tag`, `-tag-value`, `syn-doc-markup`; the other words in `syn-doc-comment`; dark theme, because in the light theme `--syn-doc-comment-tag` equals `--syn-doc-comment`, `src/styles/tokens.css:530-531`).
- "rebuilds decorations when the highlighter is reconfigured" → `rebuilds_decorations_when_the_highlighter_is_reconfigured` (light → dark changes the painted tag colour; deleting the second `*` turns the tag into `syn-comment` once highlighted again).
- "removes decorations when switching to an unsupported language" → `removes_decorations_when_switching_to_an_unsupported_language` (`files.fileTypeOverrides` makes the file plaintext; the `DocLanguage` arm switches `Editor::language`; `@param` is then `fg-editor`).
- "gives other languages nothing" also gets `gives_other_languages_nothing` (Rust).
- "keeps tags deep inside a long %s doc comment" also gets `keeps_tags_deep_inside_a_long_doc_comment` (400-line JavaDoc, scrolled to line 300, far below the `/**`: the painted `@param` and its name have the tag colours). Hypothesis it rests on: the grammar highlights a whole doc comment as one `DocComment` span (or adjacent ones), so the span reaching the painted rows starts at `/**`; check with `Syntax::highlights` on a multi-line `/** … */` in Java.
- Missing: "keeps tag marks inside backend syntax and semantic comment marks" — the native editor draws the tags over the backend's highlight (covered by the first test), but it has no semantic-token mark layer over comments to nest inside (hypothesis: `MarkKind::recolors_text` in `editor/marks.rs` is only search; check that file). Also not drawn: the `-effect` of the light theme (`--syn-doc-comment-tag-effect: underline #999999`, `tokens.css:532`): the native editor paints no `--syn-*-effect` for any token yet.

Likely failures:

- `files.fileTypeOverrides` keyed by the absolute path: hypothesis that `FileTypes::from_settings(&settings.files, Some(&root))` (`crates/ide-host/src/project.rs:1119-1125`) honours `file_type_overrides` with an absolute key; the doc comment on `FileSettings::file_type_overrides` (`crates/ide-core/src/settings.rs:645`) says "project-relative and `/`-separated, or absolute". If redetection does not run, the test times out in its first `settle`.
- The light-theme assertion `light == syn-doc-comment-tag` in the second test assumes `ActiveTheme::default()` is the block of `tokens.css:530-533`; hypothesis: check with `theme::color(cx, "syn-doc-comment-tag")` under `ThemeAttributes::light()`.

---

## Item 7 — Paste from History preview with the target's grammar and editor font

Webview source followed: `src/editor/core/view/PasteHistoryDialog.tsx:219-243` `PastePreview`: an `EditorView` over the active row's text with `syntaxHighlighting(openideaHighlightStyle)`, the source editor's `language` extension and its font compartment (`compartments.font`), read-only, with line numbers and special-character highlighting.

What exists today: `editor/paste_history_view.rs:297-308` draws `self.preview().lines().take(8)` as plain `div`s in `fg-editor`; the dialog knows nothing of the target editor beyond `read_only` (`PasteHistoryDialog::new(entries, read_only, cx)`, `:81`).

Design: `PasteHistoryDialog::new` keeps its signature (its tests and the workbench call it). The workbench hands the dialog a `PreviewLook` (language, `Editor::text_style` font/size/line height, `Host::syntax()`) right after creating it. The preview text is highlighted with `Syntax::highlights(language, text)` on the background executor when the active row changes (asked from `render`, deduplicated by text; a stale answer is dropped), the same call `workbench/diff_window.rs:169-183` uses for diff sides. Colours come from `syntax_token`, as the editor's.

New file: `editor/paste_history_view/preview.rs` (`pub struct PreviewLook` with `PreviewLook::of(&Editor, &App)`; on the dialog: `set_preview_look`, `preview_style`, `preview_colours` (public accessors), `preview_element`).

Hunks, in order:

1. `editor/paste_history_view.rs` line 21, before

   ```rust
   /// How long a row's label may be, in UTF-16 code units.
   const LABEL_WIDTH: u32 = 80;
   ```

   insert

   ```rust
   mod preview;

   pub use preview::PreviewLook;

   ```

2. `editor/paste_history_view.rs` lines 71–73 (struct), replace

   ```rust
       read_only: bool,
       key_claim: u64,
   ```

   with

   ```rust
       read_only: bool,
       /// How the target editor draws code; the preview is plain text until it is known.
       preview_look: Option<PreviewLook>,
       /// The highlights of the preview's text.
       preview_spans: preview::PreviewHighlights,
       key_claim: u64,
   ```

3. `editor/paste_history_view.rs` lines 103–104 (the `Self { … }` of `new`), replace

   ```rust
               read_only,
               key_claim,
   ```

   with

   ```rust
               read_only,
               preview_look: None,
               preview_spans: Default::default(),
               key_claim,
   ```

4. `editor/paste_history_view.rs` lines 297–308 (in `render`), replace

   ```rust
           let preview_lines: Vec<SharedString> =
               self.preview().lines().take(8).map(|text| SharedString::from(text.to_owned())).collect();
   ```

   with

   ```rust
           let preview_text = self.preview_element(cx);
   ```

   and replace

   ```rust
               .overflow_hidden()
               .children(preview_lines.into_iter().map(|text| div().child(text)));
   ```

   with

   ```rust
               .overflow_hidden()
               .child(preview_text);
   ```

5. `workbench/editor_core.rs` line 365 (in `show_paste_history`), after

   ```rust
           let dialog = cx.new(|cx| PasteHistoryDialog::new(history.entries().to_vec(), read_only, cx));
   ```

   insert

   ```rust
           let look = crate::editor::paste_history_view::PreviewLook::of(editor.read(cx), cx);
           dialog.update(cx, |dialog, cx| dialog.set_preview_look(look, cx));
   ```

Test file and `mod` line: `editor/paste_history_view/tests/proofs23_paste_preview.rs`; in `editor/paste_history_view.rs` line 360–363, after

```rust
mod tests {
    use super::*;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
```

insert `    mod proofs23_paste_preview;`.

Parity rows:

- `unit:src/editor/core/view/PasteHistoryDialog.test.tsx` → stays **partial, 6 of 12**: adds case "shows newest first and a read-only preview with the target grammar and editor font" → `editor::paste_history_view::tests::proofs23_paste_preview::shows_newest_first_and_a_read_only_preview_with_the_target_grammar_and_editor_font` (rows newest first; the preview of a JavaScript look draws `const` in `syn-keyword`, `// detail` in `syn-comment`, at 17 px on 22 px lines in the given font; ↓ moves the preview to `older`). Also `…::the_preview_is_plain_text_until_the_target_editors_look_is_known`. The other missing cases of that row are not in this slice.
- `point.registerOverlay:editor.pasteHistory` → **done** for its recorded gap ("the preview is plain text without the target's grammar and editor font"): `workbench::editor_core::tests::proofs23_paste_preview_look::the_paste_history_preview_draws_with_the_target_editors_font_and_grammar` runs `edit.copy` and `code.pasteFromHistory` through the registry in a project window, then asserts the dialog's `preview_style()` is the editor's `text_style` and the copied `fn main() {}` is previewed with `fn` in `syn-keyword`. Test file `workbench/editor_core/tests/proofs23_paste_preview_look.rs`; `mod` line `    mod proofs23_paste_preview_look;` after `    mod proofs7_read_only;` (`workbench/editor_core.rs:416`).
- `action:code.pasteFromHistory`, `module:src/editor/core/extras.ts`: their "Missing" points at the overlay row; update their text once that row is done.

Not built: the preview's line numbers and `highlightSpecialChars` (the webview's `lineNumbers()`, `highlightSpecialChars()`); plugin-contributed syntaxes (`SyntaxCache::highlights` falls back to `external::highlights`, `crates/ide-language/src/syntax/cache.rs:222-235`, which is not public; `Syntax::highlights` is the grammar's query only, as the diff viewer's).

Likely failures: `cx.run_until_parked()` must run the background highlight task in the test dispatcher (it does for `background_executor().spawn` in gpui tests; the loop sleeps between tries anyway). `ide_language::Syntax::new()` loads the language registry from disk (`syntax/mod.rs:89-92`), once per test.

---

## Item 3 — Smooth caret movement

Webview source followed: `src/editor/core/cm/prefs.ts:470-520` `caretGlide`: only a move of the selection alone glides; a doc change, a geometry change, a different number of ranges, or a move where either caret is off screen puts the caret in place at once; `caretExtension` marks the editor `cm-openidea-caretMotion-{snappy|gliding}` when `appearance.smoothCaretMovement`. Curves `src/editor/core/cm/theme.ts:58-59`: Snappy `110ms cubic-bezier(0.16, 1, 0.3, 1)`, Gliding `180ms cubic-bezier(0.45, 0, 0.25, 1)`.

What exists today: `vcs_more::viewer_look::viewer_caret` (`vcs_more/viewer_look.rs:42-54`) turns the settings into `ViewerCaret { motion: Option<CaretMotion>, … }`; its only caller is `vcs_diff/view.rs:119`, with default settings. The editor paints the caret at its place (`editor/view.rs:2487-2492`) and never blinks (no `blink` anywhere in `editor/view.rs`).

Design: `set_selections` (the only path of a selection-only move; edits go through `follow` → `put_selections`, `view.rs:901`) arms a glide with the old caret's painted position, the text version and the scroll before the caret is revealed; `glide_frame` (prepaint) draws the main caret `eased(t)` of the way along, and drops the glide after an edit (version) or a scroll (`scroll_top`). Frames come from a 16 ms executor timer loop that ends when the move has arrived, so the test clock drives it. Only the main caret glides (the webview animates every caret element; with several carets the others jump).

New file: `editor/view/caret_glide.rs` (`motion_curve`, `eased`, `Glide`, `GlideFrame`; `Editor::arm_glide`, `caret_glides` (public), `glide_frame`, `painted_caret_glide` (public); 2 unit tests).

Hunks, in order:

1. `editor/view.rs` line 54, after `pub mod annotations;` insert `mod caret_glide;`.

2. `editor/language_facts.rs` line 38 (struct), after

   ```rust
       /// Code Folding › Show code folding outline: the gutter's column of fold chevrons.
       pub fold_outline: bool,
   ```

   insert

   ```rust
       /// Appearance › Smooth caret movement: the curve a moved caret glides along; none: it jumps.
       pub caret_motion: Option<crate::vcs_more::viewer_look::CaretMotion>,
   ```

3. `editor/language_facts.rs` line 134, after

   ```rust
               fold_outline: editor.folding.show_outline,
   ```

   insert

   ```rust
               caret_motion: crate::vcs_more::viewer_look::viewer_caret(editor).motion,
   ```

4. `editor/view.rs` line 331 (struct `Editor`), after

   ```rust
       annotations: Option<annotations::Annotations>,
   ```

   insert

   ```rust
       /// The main caret's glide to its new place, while Smooth caret movement draws one.
       glide: Option<caret_glide::Glide>,
   ```

5. `editor/view.rs` line 431 (the `Self { … }` of `with_text`), after

   ```rust
               annotations: None,
   ```

   insert

   ```rust
               glide: None,
   ```

6. `editor/view.rs` lines 803–804 (in `set_selections`), after

   ```rust
           let before = (self.selections.clone(), self.main);
           self.put_selections(ranges, main);
   ```

   insert

   ```rust
           self.arm_glide(&before, cx);
   ```

7. `editor/view.rs` line 190 (struct `Layout`), after

   ```rust
       /// The document line of each painted row.
       line_of: Vec<u32>,
   ```

   insert

   ```rust
       /// The main caret's glide this frame draws.
       caret_glide: Option<caret_glide::GlideFrame>,
   ```

8. `editor/view.rs` line 2267 (in `prepaint`), after `        let text_hitbox = Some(window.insert_hitbox(text_area, HitboxBehavior::Normal));` insert

   ```rust
           let caret_glide = editor.glide_frame(cx);
   ```

9. `editor/view.rs` line 2279 (prepaint's `Layout` literal), after `                line_of: painted,` insert `                caret_glide,`.

10. `editor/view.rs` line 2512 (paint's `mem::replace` literal), after `                line_of: Vec::new(),` insert `                caret_glide: None,`.

11. `editor/view.rs` lines 2489–2490 (in `paint`), replace

    ```rust
                            let caret_bounds =
                                Bounds::new(point(text_origin.x + x_of(head), top), size(px(2.0), line_height));
    ```

    with

    ```rust
                            let place = point(text_origin.x + x_of(head), top);
                            let place = layout.caret_glide.map_or(place, |glide| glide.place(head, place));
                            let caret_bounds = Bounds::new(place, size(px(2.0), line_height));
    ```

Paint cost: one interpolation of one point per frame while a glide runs.

Checked: undo and redo (`replay`, `view.rs:1079-1098`), backend patches (`remote_patch`, `:964-980`) and every edit (`send` → `follow`) set the selection through `follow` → `put_selections` (`:901`), never `set_selections`, so none of them arms a glide; only selection-only moves do (including `edit_with` with no changes, `:1013-1016`).

Test file and `mod` line: `editor/command_actions/tests/proofs23_smooth_caret.rs` → `    mod proofs23_smooth_caret;`.

Parity row `unit:src/editor/core/cm/ideaParity.test.ts` → stays **partial, 26 of 27**:

- "glides selection-only moves and puts the caret in place at once when typing" → `editor::command_actions::tests::proofs23_smooth_caret::glides_selection_only_moves_and_puts_the_caret_in_place_at_once_when_typing` (a selection move glides; typing `x` ends it; a second caret ends it).
- "marks the editor for smooth blinking and the chosen movement curve" → only its movement half: `…::a_move_follows_the_chosen_movement_curve` (Gliding still on its way at 150 ms and arrived by 200 ms, the frame at 90 ms draws the caret part way to offset 1; Snappy arrived by 120 ms; off: no glide). **Missing: smooth blinking**, because the native editor's caret does not blink at all (no blink timer in `editor/view.rs`; Appearance › Caret blinking and its period are not followed either). That is a feature of its own (a blink timer that `viewer_caret(..).blink_ms` and `smooth_blink` drive), not wiring.

Likely failures: the frame assertion at 90 ms (`painted_caret_glide`) relies on `run_until_parked` drawing the window after the loop's `cx.notify()` (hypothesis: the existing `painted_*` tests read the layout the same way after `update`). `settle` in the fixture advances the clock by 50 ms per try; the tests use their own `wait` after arming a glide so a glide is not finished by a `settle`.

---

## Item 6 — Colour-scheme overrides and font weights

Webview source followed: `src/editor/core/colorSchemeRuntime.ts:13-29` (`installColorScheme`: the `schemeCss` stylesheet of the chosen scheme and its edits, and `--fw-code` / `--fw-code-bold` on the root from `fontWeightVars`, re-applied on every settings change and theme attribute change); `src/platform/editorSettings.ts:238-249` (`fontWeightOf`: 100–900, rounded to 50; defaults 400 / 700); `src/editor/core/cm/highlight.ts:17-33,73-80` (every token's `color`, `fontStyle: var(--syn-<t>-italic, default)`, `fontWeight: var(--syn-<t>-bold, default)`; comment `italic: var(--syn-comment-style)`, doc comment and constant italic).

What exists today: `theme::color_scheme::{scheme_style, SchemeSource, custom_schemes, …}` (model, 11 tests), called by nobody but its tests (`grep -rn scheme_style app-gpui/src`); `theme::appearance::apply` (`theme/appearance.rs:103-158`) cascades the theme and then runs the `after_apply` followers (`:154-157`); the editor draws every token with `theme::color(cx, syntax_token(kind))` and `code_font` (`editor/view.rs:2184`), so no `--syn-*-bold`/`-italic`, and no per-language override.

Design:

- `theme/scheme_runtime.rs`: an `after_apply` follower sets `scheme_style(…).root` and `font_weight_vars(editor.font_weight, editor.bold_font_weight)` on the freshly cascaded `ActiveTheme` (`Theme::set`, which is the inline-style override layer), and stores `scheme_style(…).languages` as the global `SchemeLanguageTokens` keyed by the backend language id (`SCHEME_LANGUAGES`). A `SettingsChanged` of `editor.colorScheme | colorOverrides | customSchemes | fontWeight | boldFontWeight` (or all settings) calls `appearance::apply`, so the theme is cascaded afresh and no stale override survives. One `LazyLock<SchemeSource>` from the bundled `TOKENS_CSS`; nothing is read from disk at paint.
- `editor/view/token_style.rs`: per frame, a table of the 25 highlight kinds → (colour, font): the language's override, else the theme's resolved `--syn-<t>`, `--syn-<t>-bold` (a number, else the code font's weight), `--syn-<t>-italic` (`italic`/`oblique`/`normal`, else the defaults above). The row loop takes the colour and font from the table.

New files: `theme/scheme_runtime.rs` (`init`, `apply`, `font_weight_of`, `font_weight_vars`, `SchemeLanguageTokens`; 3 unit tests), `editor/view/token_style.rs` (`TokenStyles`, test accessor `Editor::painted_font_at`).

Hunks, in order:

1. `theme/mod.rs` line 9, after `pub mod installed_fonts;` insert `pub mod scheme_runtime;`.
2. `theme/color_scheme.rs` line 256, replace `const SCHEME_LANGUAGES: [(&str, &str); 4] =` with `pub(crate) const SCHEME_LANGUAGES: [(&str, &str); 4] =` (rest of the line unchanged).
3. `lib.rs` line 71, after `    theme::appearance::init(cx);` insert `    theme::scheme_runtime::init(cx);`.
4. `editor/view.rs` line 54, after `pub mod annotations;` insert `mod token_style;`.
5. `editor/view.rs` line 2121 (in `prepaint`), after

   ```rust
           let code_font = editor.code_font(cx);
   ```

   insert

   ```rust
           let token_styles = token_style::TokenStyles::new(&code_font, editor.language.as_str(), cx);
   ```

6. `editor/view.rs` line 2184, replace

   ```rust
                       runs.push(run(to - from, theme::color(cx, syntax_token(s.kind))));
   ```

   with

   ```rust
                       let (colour, font) = token_styles.get(s.kind);
                       runs.push(TextRun { font: font.clone(), ..run(to - from, colour) });
   ```

7. `editor/view.rs` line 199–200 (struct `Layout`), after

   ```rust
       painted_chevrons: Vec<(u32, super::extras::FoldMarker)>,
   ```

   insert

   ```rust
       /// The weights and slants (UTF-8 lengths) each painted row was shaped with; for tests only.
       #[cfg(test)]
       painted_fonts: Vec<Vec<(usize, FontWeight, FontStyle)>>,
   ```

8. `editor/view.rs` line 2168, after `        let mut painted_chevrons = Vec::new();` insert

   ```rust
           #[cfg(test)]
           let mut painted_fonts = Vec::with_capacity(painted.len());
   ```

9. `editor/view.rs` line 2216, after `            painted_colors.push(runs.iter().map(|run| (run.len, run.color)).collect());` insert

   ```rust
               #[cfg(test)]
               painted_fonts.push(runs.iter().map(|run| (run.len, run.font.weight, run.font.style)).collect());
   ```

10. `editor/view.rs` line 2285 (prepaint's `Layout` literal), replace

    ```rust
                    painted_chevrons,
                },
                numbers,
    ```

    with

    ```rust
                    painted_chevrons,
                    #[cfg(test)]
                    painted_fonts,
                },
                numbers,
    ```

11. `editor/view.rs` line 2518 (paint's `mem::replace` literal), replace

    ```rust
                    painted_chevrons: Vec::new(),
                },
            );
    ```

    with

    ```rust
                    painted_chevrons: Vec::new(),
                    #[cfg(test)]
                    painted_fonts: Vec::new(),
                },
            );
    ```

Paint cost: 25 entries per frame (two `format!`s and three theme lookups each), independent of the document.

Test file and `mod` line: `editor/command_actions/tests/proofs23_color_scheme.rs` → `    mod proofs23_color_scheme;`. The tests call `theme::appearance::init` and `theme::scheme_runtime::init` themselves, as `lib.rs` does at start (the command-actions fixture does not).

Parity rows:

- `unit:src/platform/colorScheme.test.ts` → **partial, 13 of 14** expected:
  - "sets the configured weights on the root and follows settings changes" → `editor::command_actions::tests::proofs23_color_scheme::sets_the_configured_weights_on_the_root_and_follows_settings_changes` (root `--fw-code`/`--fw-code-bold` 400/700, 300/800 after the writes, back to 400/700).
  - "validates Typography Settings weights" → `theme::scheme_runtime::tests::validates_typography_settings_weights` (349 → 350, 1200 → 900, defaults 400/700). The TS case's non-number input (`"bold"`) cannot reach the native code: `EditorSettings::font_weight` is a `u16` (`crates/ide-core/src/settings.rs:565`), so serde rejects it at the settings edge.
  - Missing: "coerces editor settings to their defaults' shape": the case feeds wrong-typed values (`tabLimit: "x"`) and expects each field's default. `#[serde(default)]` on `EditorSettings` (`settings.rs:538`) fills missing fields only; a wrong-typed field fails the whole deserialization (hypothesis: check how `ide_core::settings` reads a settings file with `"tabLimit": "x"`). It is a settings-loading behaviour of `ide-core`, not of the editor.
  - The 11 model cases keep `theme::color_scheme::tests::*`.
- `module:src/platform/colorScheme.ts` → **partial** (missing: token backgrounds `--syn-*-bg` and effects `--syn-*-effect` in the editor, and the theme-follow observer, all below): `…::the_editor_draws_tokens_with_the_schemes_edits_and_the_typography_weights` (a Keyword edit recolours `fn` to `#ff0000` and, bold, draws it at Typography Settings' bold weight 800; `main` stays at 400; the light scheme's comments are italic; clearing the edits restores `syn-keyword`), `…::offers_a_language_scheme_page_and_scopes_its_overrides_to_that_languages_editors` (Java › Keyword `#00ff00` recolours `class` in a Java editor; the root keyword colour is untouched), `…::applies_an_explicit_scheme_to_the_editor` (scheme `dark` under the light theme: `fn` in `#cf8e6d`).

Not built (with why):

- The webview's `followTheme` (`colorSchemeRuntime.ts:33-50`): when the IDE theme turns light or dark, a chosen scheme of the other kind is written back to `theme-default` unless it was chosen in the same Apply. It writes settings from a theme-attribute observer with a "same task" window; the pure rule is ported (`scheme_after_theme_change`, tested), the observer is not. It belongs with the theme switcher (`theme/appearance.rs`), not the editor.
- `--syn-*-bg` (token backgrounds) and `--syn-*-effect` (underline/wave/strikeout per token): the scheme sets them on the root, but the editor's runs carry no background or decoration per token yet.
- Per-language overrides of root (non-`--syn-`) tokens: `SchemeLanguageTokens` is read for the syntax tokens only.
- When `appearance::apply` finds no theme to apply (`applied == None`, `appearance.rs:127`) the follower still runs over the old theme, so an override removed from the settings lingers until the next cascade (edge case; hypothesis: `apply_theme` returns `None` only before any theme is known).

Likely failures: `theme::appearance::init` in a test registers a second listener on each call (harmless). The exact-colour asserts compare `Hsla` from `Rgba` built two ways (`hex()` in the test vs `parse_color`); if they differ by rounding, compare `Rgba` from `Hsla::to_rgb()` or use the theme lookup of the written hex. The attribute ids `keyword` and `java.keyword` come from `theme/color_scheme.rs:189` and `:272-280`.

---

## Item 8 — Markdown paste of copied file paths and images

**Skipped as wiring: the native models are missing.** What the webview does and what does not exist natively:

- `src/editor/core/cm/clipboard.ts:14-19` `clipboardFileFormatter`: in a Markdown document (`isMarkdown(path)`, `.md`/`.mdx` any case) of a local window, copied file paths paste as `[](<relative path>)`, one per line; `relativeImagePath` (`src/editor/core/cm/markdownImages.ts`) makes the path relative to the document's folder with `/` separators (Windows drive paths included) and percent-encodes it like `encodeURIComponent` plus `!'()*`. No native function does this (`grep -rn "relative_image_path\|markdown_link\|fn is_markdown" app-gpui/src` finds only the GitHub and agents markdown renderers, `github/ui_model.rs:258`, `agents/markdown_images.rs`, which resolve links the other way).
- `src/platform/clipboard.ts` `pasteClipboardFiles` / `readClipboardText`: the file list comes from the backend command `clipboard_file_list` when the clipboard holds files and no text (and text wins when present; a backend error pastes nothing). GPUI's clipboard does carry `ClipboardEntry::ExternalPaths` (`gpui-pre-0.3.12/src/platform.rs:3143-3150`), so the native route would read those instead of the IPC command; the formatter is what is missing.
- `markdownImages.ts`: a pasted PNG or screenshot is written into the configured folder (`general.markdownImageFolder`, name pattern `image-{timestamp}` or the copied file's name) through `fs_create_file_bytes` and inserted as `![](assets/…)`. Natively missing: the name-pattern model, the image write through the backend, and reading `ClipboardEntry::Image` in the editor (only `agents/new_task_view.rs:482` reads images).

What is wired and proven: the read-only refusal. `editor/view.rs:1018-1020` (`edit_with` returns `false` when `self.read_only`) already refuses every paste. Test: `editor/command_actions/tests/proofs23_clipboard_read_only.rs` → `    mod proofs23_clipboard_read_only;` — `editor::command_actions::tests::proofs23_clipboard_read_only::does_not_insert_into_a_read_only_editor` (⌘V and `Editor::paste` leave a read-only editor's text; writable again, the same paste goes in). No hunk.

"does not fall back while a successful native paste is still pending" is webview plumbing, not a behaviour: it exists because the webview first tries `document.execCommand("paste")` (which may finish asynchronously and carry image blobs) before reading the clipboard text itself (`clipboard.ts:81-95`). The native editor reads the platform clipboard synchronously (`Editor::paste`, `view.rs:1206-1211`), so there is no pending native paste to wait for.

Parity row `unit:src/editor/core/cm/clipboard.test.ts` → stays **partial, 19 of 25** (18 model cases plus the read-only case); the other 6 of the 7 cases the row records as missing stay open: the Markdown file-path and file-list cases, the PNG/screenshot case (models missing, above), and the pending-native-paste case (not applicable natively, above; the integrator may record it as such).

---

## Summary of `mod` lines (only for the items applied)

- `editor/view.rs` after `pub mod annotations;`: `mod column_gestures;` (5), `pub(crate) mod right_margin;` (1), `mod string_literals;` (4), `mod doc_tags;` (2), `mod caret_glide;` (3), `mod token_style;` (6).
- `editor/view.rs` `mod editing_tests`, after `    mod proofs_editor_multi_caret;`: `    mod proofs23_middle_column;` (5).
- `editor/command_actions.rs` `mod tests`, after `    use std::time::Duration;` (line 284): `    mod proofs23_column_mode_drag;` (5), `    mod proofs23_right_margin;` (1), `    mod proofs23_string_clipboard;` (4), `    mod proofs23_doc_comment_tags;` (2), `    mod proofs23_smooth_caret;` (3), `    mod proofs23_color_scheme;` (6), `    mod proofs23_clipboard_read_only;` (8).
- `editor/paste_history_view.rs`: `mod preview;` + `pub use preview::PreviewLook;` (7, hunk 1), and in `mod tests` `    mod proofs23_paste_preview;` (7).
- `workbench/editor_core.rs` `mod tests`, after `    mod proofs7_read_only;`: `    mod proofs23_paste_preview_look;` (7).
- `theme/mod.rs` after `pub mod installed_fonts;`: `pub mod scheme_runtime;` (6).

## Cross-item notes

- Items 1 and 6 compose: the scheme's `rightMargin` attribute sets `--border-right-margin` on the root (`color_scheme.rs:168`), which `paint_right_margins` reads, so the guide takes the scheme's colour once item 6 is in.
- Items 2 and 6 compose: doc tags are restyled with `code_font` (upright, main weight) as `highlight.ts:73-75` asks, over whatever item 6 gives the doc comment.
- GPUI APIs used without a prior use in the crate: `BackgroundExecutor::now` (`gpui-pre-0.3.12/src/executor.rs:192`, the test clock in tests), `on_mouse_up(MouseButton::Middle, …)` (same API as the left button's).
