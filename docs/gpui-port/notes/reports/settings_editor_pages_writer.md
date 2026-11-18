Everything I was asked for is drafted, items 1 to 5: four page views, a window-test file, and the integration notes. Nothing was compiled or run, and the hypotheses below are unconfirmed. If every hunk lands, five rows reach parity. Spelling does not, because nothing in the app runs the Typo inspection, and Code Style does not, for want of effect tests. Font reaches parity only if the extra hunk to `ui/text_area.rs` lands as well.

## Files (all new and undeclared; no existing file was edited)

| File                                                                     | Lines | What it holds                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------ | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/settings_view/pages/editor_natural_languages_pending.rs`   | 103   | `NaturalLanguagesView::new(binding, cx)`, the page view of `editor.naturalLanguages`, with `lines()` and `matched()`. It shows Languages › "English (USA, UK)", its hint and "Used by Proofreading › Typo", and stores nothing. 2 view tests. |
| `app-gpui/src/settings_view/pages/editor_spelling_pending.rs`            | 237   | `SpellingView::new(model, has_project, cx)`: three `ListEditor`s (details below). It passes the lists' `ControlEvent`s on to the dialog. Accessors `project()`, `application()`, `dictionaries()`, `lines()`. 6 view tests.                   |
| `app-gpui/src/settings_view/pages/editor_font_preview_pending.rs`        | 284   | `FontPreviewView::new(binding, cx)` with `SAMPLE` and `PreviewStyle` (details below). Accessors `style()`, `font()`, `area()`, `text()`. 3 view tests.                                                                                        |
| `app-gpui/src/settings_view/pages/editor_code_style_tabs_pending.rs`     | 396   | `CodeStyleTabsView::new(binding, CodeStyleSlots { scheme, guides, do_not_format, formatters }, cx)`, the Scheme row over General / Formatter tabs (details below). Accessors `tab()`, `show_tab()`, `lines()`. 5 view tests.                  |
| `app-gpui/src/workbench/tests/proofs46_settings_editor_pages_pending.rs` | 551   | 13 tests through File › Settings… on the real backend with a temp home and project. Declarable once the views land.                                                                                                                           |
| `app-gpui/src/workbench/tests/proofs46_spelling_typo_effect_pending.rs`  | 51    | The Typo effect test. It stays undeclared until something runs the Typo inspection in editors.                                                                                                                                                |
| `$S/drafts/settings_editor_pages_integration.md`                         | —     | Integration notes: every hunk with its anchor, every called signature with `file:line`, the webview behaviours, the hypotheses, the tests likely to fail first and the per-row proof text.                                                    |

**Spelling lists:**

- **Project dictionary** writes `spelling.projectWords` in `Scope::Project`. It is shown only when a project is open and the dialog is not "Settings for New Projects".
- **Application dictionary** writes `spelling.ideWords` and **Dictionary files** writes `spelling.customDictionaries`, both in `Scope::Global`.
- A row is added with + (then typing and Enter), edited with Enter, F2 or a double-click, and removed with − or Delete. The existing `ListEditor` already trims, keeps each entry once and drops an entry edited to nothing, which matches `spelling.tsx:30`.

**Font Preview:**

- It draws the sample with the staged family (or the fallback for a family not installed), size, line height, ligatures and main weight, through the editor's own `code_font_of`. It follows edits before Apply, and Reset brings the stored font back.
- Typed text, Enter included, stays in the preview and is never staged; Escape is the dialog's Cancel.

**Code Style tabs:**

- General holds the line separator (with its hint), Hard wrap at, Visual guides, Indents Detection and EditorConfig.
- Formatter holds Do not format, the markers, and Plugin Formatters. Off, On and the regular-expressions option are indented and disabled while the markers checkbox is off, as in the webview.
- The page opens on General again each time it is shown.

**proofs46 tests:**

- **Natural Languages:** 1 test (the page's lines).
- **Spelling:** 2 tests. One adds to all three lists, then checks the words in `.openidea/settings.toml` and the global file, then a delete written by OK. The other covers Reset and Cancel.
- **Font:** 1 test. The preview follows the edits before Apply, can be typed into, stores no text, and Reset and Cancel restore the stored font.
- **Code Style:** 3 tests. The tabs with Apply, Reset and Cancel; Hard wrap at 100 plus guide 80 becoming an open editor's right margins; the slot views surviving under the tabs.
- **Other File Types:** 1 test. The Scheme row, then Tab size 2 and Use tab character deciding what Tab inserts in a plain-text file.
- **File Encodings:** 3 tests. The note line; a GBK project file decoded after choosing GBK; New File writing the BOM after choosing "with BOM".
- **Code Folding:** 2 tests. All four options changed on the page decide the fold column and the folds of a newly opened file, File header unticked included; Reset and Cancel.

## Hunks the integrator must apply

The notes give the exact anchors; in short:

- **`pages/mod.rs`:** four `pub mod` lines for the new views.
- **`ui/text_area.rs`:** the area hard-codes its font, so it cannot follow the Font page as it stands. The hunk adds `AreaFont`, `set_font()` and `font()`, and changes `lay_out` to take that font instead of `code: bool` at its call sites.
- **`builtin_pages/editor.rs`:** a `custom("Preview")` section at the end of `font()`, and the `.editorconfig` note as the hint of `editor.encodings.utf8Bom`.
- **`custom_controls.rs`, `install`:**
  - put the Preview view in the Font page's new slot;
  - mount the Code Style tab view as the page view, keeping the existing slot views and passing them in;
  - put a second `SchemeView` in the `editor.codeStyle.other` "Scheme" slot;
  - mount the Natural Languages and Spelling page views.
- **Two existing tests** must first click `settings-code-style-tab-formatter`, because Do not format and Plugin Formatters are now on the Formatter tab: `custom_controls::tests::code_style_visual_guides_and_do_not_format_apply` and `proofs19_settings_languages::code_style_offers_a_plugin_formatter_per_covered_language_and_stores_the_pick`.
- **`workbench/mod.rs`:** add `mod proofs46_settings_editor_pages_pending;` after `mod proofs44_editor_tabs;`.

## Rows

| Row                                         | Parity if the draft lands              | Remaining clause                                                                                                                                                                                     |
| ------------------------------------------- | -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:editor.naturalLanguages`          | yes                                    | none                                                                                                                                                                                                 |
| `settings:editor.naturalLanguages.spelling` | no                                     | Missing: no editor runs the Typo inspection, so a word accepted on the page is never shown to stop being a typo. `SpellChecker` in `app-gpui/src/editor_lsp/spelling.rs` has no caller.              |
| `settings:editor.font`                      | yes, only with the `text_area.rs` hunk | none                                                                                                                                                                                                 |
| `settings:editor.codeStyle`                 | no                                     | Missing: no test shows the effect, from a change on the page, of the line separator on a new file, of indents detection, of EditorConfig support, or of Do not format and the markers on a reformat. |
| `settings:editor.codeStyle.other`           | yes                                    | none                                                                                                                                                                                                 |
| `settings:editor.fileEncodings`             | yes                                    | none, if a help line under the last select is accepted for the webview's note item. Global Encoding and the properties encoding still have no effect test.                                           |
| `settings:editor.general.codeFolding`       | yes                                    | none                                                                                                                                                                                                 |

The notes hold the full proof text for each row.

## Hypotheses (each has the test or command that settles it in the notes)

1. Apply writes a project-scoped change staged from a page that has no scope switch (Spelling's project list).
2. The settings files are written as TOML, for example `hardWrapAt = 100`.
3. The native defaults of the formatter markers are `@formatter:off` and `@formatter:on`.
4. Cmd-A selects all in `TextArea`.
5. Plain text uses the Other File Types indentation, and Tab at column 0 inserts one indent unit.
6. The native editor decodes a project file through the path that applies `editor.encodings.project`.
7. `editorconfig::create_file` leaves a new file empty when there is no `.editorconfig`, so the BOM gets written.
8. `bring_into_view` reaches the drop-down choice of the last select on File Encodings.
9. The Typo inspection is on for plain text by default.

**Likely to fail first:** the Font preview test (needs both hunks, and the preview sits at the bottom of the page), then the BOM test, the Tab-inserts test, the Code Style tabs test (a select's list inside a page view), and the folding test (`start` sets no IDE mode, while the folding fixture sets Light).

## Where the native code or the triage turned out different

- **Spelling is unreached** (measured by text): nothing outside `editor_lsp/spelling.rs` uses `SpellChecker`, `SpellConfig`, `typo_diagnostics`, `SPELLING_PRODUCER` or `ops::spell`. A backend-only speller test is not possible either: `spell_check` takes an `ide_spell::SpellScope` and `ide-spell` is not a dependency of `app-gpui`.
- **Font Preview is more than "a view":** the triage sizes it as a view, but `TextArea`'s font is fixed and the Font spec has no footer, so it needs the `text_area.rs` hunk and a spec slot. The new "Preview" slot label also becomes something the Settings search matches, which the webview's footer is not.
- **Code Style tabs cost more:** making them tabs needs a page view, and a page view stops the dialog drawing the spec's standard controls. I rebuilt them as forms with the same selectors, and two existing tests need the tab click.
- **No `#[ignore]` convention:** no test in the crate uses `#[ignore]`; pending tests here are simply undeclared files, so the blocked spelling test is in a file of its own.
- **Strings:** `en.json` is sparse. Every string the new views add exists in `zh.json`, except "EditorConfig", which is a proper name shown as written.
