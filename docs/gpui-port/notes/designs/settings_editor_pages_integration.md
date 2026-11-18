# Settings › Editor pages: integration of the drafts

Nothing here was compiled or run. "Read" means read at HEAD with `git show HEAD:<path>`; every claim not marked hypothesis is a read or a grep at HEAD.

## Files written (new, undeclared)

- `app-gpui/src/settings_view/pages/editor_natural_languages_pending.rs` (103 lines): `NaturalLanguagesView` (page view of `editor.naturalLanguages`), 2 view tests.
- `app-gpui/src/settings_view/pages/editor_spelling_pending.rs` (237 lines): `SpellingView` (page view of `editor.naturalLanguages.spelling`), three `ListEditor`s, 6 view tests.
- `app-gpui/src/settings_view/pages/editor_font_preview_pending.rs` (284 lines): `FontPreviewView`, `PreviewStyle`, `SAMPLE`, 3 view tests. Needs the `TextArea` hunk below.
- `app-gpui/src/settings_view/pages/editor_code_style_tabs_pending.rs` (396 lines): `CodeStyleTabsView`, `CodeStyleSlots`, `CodeStyleTab`, 5 view tests.
- `app-gpui/src/workbench/tests/proofs46_settings_editor_pages_pending.rs` (551 lines): 13 window tests on the real backend (declarable once the views land).
- `app-gpui/src/workbench/tests/proofs46_spelling_typo_effect_pending.rs` (51 lines): 1 window test of the Typo effect; stays undeclared until the Typo inspection runs in editors (blocker below).

## Hunks for existing files

### `app-gpui/src/settings_view/pages/mod.rs`

After `pub mod editor_font;` (line 29) add, keeping the alphabetical order of the list:

```rust
pub mod editor_code_style_tabs_pending;
pub mod editor_font_preview_pending;
pub mod editor_natural_languages_pending;
pub mod editor_spelling_pending;
```

(or the same modules without `_pending`, renamed together with the `use` lines of the proofs46 file).

### `app-gpui/src/ui/text_area.rs` (needed by the Font preview)

The area draws with a fixed font: `metrics(cx)` (:195-198, `fs-ui`) and `text_font(code, cx)` (:200-207). Add a font an owner can give:

1. After the `Painted` struct (ends :76) add:

```rust
/// The font, size and row height an owner gives the text in place of the theme's (the Font page's
/// preview draws with the font being edited).
#[derive(Debug, Clone, PartialEq)]
pub struct AreaFont {
    pub font: Font,
    pub size: Pixels,
    pub line_height: Pixels,
}
```

2. In `struct TextArea`, after `code_font: bool,` (:105) add `own_font: Option<AreaFont>,`; in `TextArea::new` after `code_font: false,` (:377) add `own_font: None,`.

3. After `pub fn set_code_font` (:382-385) add:

```rust
    /// Draws the text in `font` (`None`: the theme's font again).
    pub fn set_font(&mut self, font: Option<AreaFont>, cx: &mut Context<Self>) {
        self.own_font = font;
        cx.notify();
    }

    /// The font an owner gave the text.
    pub fn font(&self) -> Option<&AreaFont> {
        self.own_font.as_ref()
    }

    /// What the text is drawn with: the owner's font, else the theme's code or interface font.
    fn face(&self, cx: &App) -> AreaFont {
        match &self.own_font {
            Some(own) => own.clone(),
            None => {
                let (size, line_height) = metrics(cx);
                AreaFont { font: text_font(self.code_font, cx), size, line_height }
            }
        }
    }
```

4. `fn lay_out(text: &str, wrap: Option<Pixels>, color: Hsla, code: bool, window: &Window, cx: &App) -> Laid` (:311): replace the `code: bool` parameter by `face: &AreaFont`, and its first two lines `let (font_size, line_height) = metrics(cx);` / `let font = text_font(code, cx);` by `let (font_size, line_height, font) = (face.size, face.line_height, face.font.clone());`.

5. Call sites: `lay_out(&self.text, wrap, theme::color(cx, "fg-strong"), self.code_font, window, cx)` (:626) → `..., &self.face(cx), window, cx)`; in `request_layout` (:1045, :1047) and `prepaint` (:1067, :1069) replace `area.code_font` by `&face` with `let face = area.face(cx);` right after `let area = self.area.read(cx);` (:1044 and :1065); `metrics(cx).1` in `request_layout` (:1049) → `face.line_height`; in `page_rows` (:658) `metrics(cx).1` → `self.face(cx).line_height`. The test helpers at :1224 and :1520 keep `metrics(cx)` (no own font there).

### `app-gpui/src/settings_model/builtin_pages/editor.rs`

- In `fn font()` (:508), after the section whose anchor is `vec![custom("Main font weight", &["Bold font weight"]), custom("Fallback font", &[])],` (:522) and its closing `)`, add `.section(None, vec![custom("Preview", &[])])`. Hypothesis: "Preview" then becomes a search label of the page (the webview's footer is not searchable, `font.tsx:211`, `kit.tsx` `sectionLabels` reads only `sections`); acceptable or use a label the search skips.
- In `fn file_encodings()` (:654), after `.detail("editor.encodings.project", hint("Encoding of new files; decodes project files that are not valid UTF-8 and have no BOM."),)` (:685-688) add:

```rust
        .detail(
            "editor.encodings.utf8Bom",
            hint("A charset in .editorconfig takes precedence. Change one file's encoding from the status bar."),
        )
```

The webview draws it as a `note` item after the select (`codeStyle.tsx:384-388`); natively it is the select's help line.

### `app-gpui/src/settings_view/custom_controls.rs`, function `install`

1. Font preview: after `slots.put(pages::editor_font::PAGE, "Fallback font", &fallback, cx);` (:159) add:

```rust
    let preview = cx.new(|cx| pages::editor_font_preview_pending::FontPreviewView::new(editor_font.clone(), cx));
    slots.put(pages::editor_font::PAGE, "Preview", &preview, cx);
```

(`editor_font` is still alive there: it is cloned for every use at :155-166; the last use at :166 is a `clone()` inside a closure, check that it is not moved; if it is, clone it before.)

2. Code Style tabs: replace from `slots.fill(pages::code_style::PAGE, "Plugin Formatters", formatters.into(), cx);` (:184) to `slots.put(pages::code_style::PAGE, "Scheme", &scheme, cx);` (:186) by:

```rust
    slots.fill(pages::code_style::PAGE, "Plugin Formatters", formatters.clone().into(), cx);
    let scheme = cx.new(|cx| pages::code_style::SchemeView::new(code_style.clone(), can_project, cx));
    slots.put(pages::code_style::PAGE, "Scheme", &scheme, cx);
    let slot = |label: &str, cx: &App| dialog.read(cx).custom_control(pages::code_style::PAGE, label).cloned();
    let guides = slot("Visual guides", cx).and_then(|view| view.downcast::<FormView>().ok());
    let do_not_format = slot("Do not format", cx).and_then(|view| view.downcast::<ListEditor>().ok());
    if let (Some(guides), Some(do_not_format)) = (guides, do_not_format) {
        let slots_of = pages::editor_code_style_tabs_pending::CodeStyleSlots { scheme, guides, do_not_format, formatters };
        let tabs = cx.new(|cx| pages::editor_code_style_tabs_pending::CodeStyleTabsView::new(code_style, slots_of, cx));
        slots.page_view(pages::code_style::PAGE, &tabs, cx);
    }

    // Editor › Code Style › Other File Types: the same Scheme row.
    let other = binding("editor.codeStyle.other");
    let other_scheme = cx.new(|cx| pages::code_style::SchemeView::new(other, can_project, cx));
    slots.put("editor.codeStyle.other", "Scheme", &other_scheme, cx);
```

(the `if let` never fails: both slots were filled by `slots.forms` / `slots.lists` three lines above; an `expect` would also do, but a panic in `install` would take the dialog down.) The line with `CodeStyleSlots { … }` is over 120 columns: rustfmt splits it.

3. Natural Languages and Spelling: before `slots.subscriptions` (:272) add:

```rust
    // Editor › Natural Languages and its Spelling page.
    let languages = binding(pages::editor_natural_languages_pending::PAGE);
    let view = cx.new(|cx| pages::editor_natural_languages_pending::NaturalLanguagesView::new(languages, cx));
    slots.page_view(pages::editor_natural_languages_pending::PAGE, &view, cx);
    let view = cx.new(|cx| pages::editor_spelling_pending::SpellingView::new(model.clone(), install.has_project, cx));
    slots.page_view(pages::editor_spelling_pending::PAGE, &view, cx);
```

(`model` is the dialog's model, bound at :48; `install` is still borrowed there: `install.has_project` is a `bool` field read at :174 and :259.)

### Existing tests that the tabs change

- `custom_controls.rs` `code_style_visual_guides_and_do_not_format_apply` (:865): before `click(cx, "settings-list-editor.codeStyle.doNotFormat-add");` (:870) add `click(cx, "settings-code-style-tab-formatter");` (Do not format is on the Formatter tab now).
- `workbench/tests/proofs19_settings_languages.rs` `code_style_offers_a_plugin_formatter_per_covered_language_and_stores_the_pick`: before `bring_into_view(cx, "settings-formatter-2");` (:598) add `click_on(cx, "settings-code-style-tab-formatter");`. Its `custom_control("editor.codeStyle", "Plugin Formatters")` lookup still finds the view (the slot stays filled).
- `on_every_page_reset_and_cancel_discard_an_edit_of_an_own_control` (Code Style row types into `settings-text-editor.codeStyle.visualGuides`, on General, the tab shown first): unchanged.

### `app-gpui/src/workbench/mod.rs`, the `mod tests` list

After `    mod proofs44_editor_tabs;` (:4685) add `    mod proofs46_settings_editor_pages_pending;`. Do not declare `proofs46_spelling_typo_effect_pending` (blocker below).

## Existing functions called (signatures at HEAD)

- `Binding::new(model: Entity<SettingsDialog>, page: &'static str, scope: ScopeOf) -> Self` (`settings_view/pages/mod.rs:106`); `Binding::fixed(model: Entity<SettingsDialog>, page: &'static str, scope: Scope) -> Self` (:111); `value(&self, key: &str, cx: &App) -> Option<Value>` (:121); `bool` (:127); `number(&self, key: &str, fallback: f64, cx: &App) -> f64` (:132); `text(&self, key: &str, cx: &App) -> String` (:137); `query(&self, cx: &App) -> String` (:158); `matches(&self, label: &str, cx: &App) -> bool` (:163).
- `row(cx: &App, matched: bool) -> Div` (:214); `row_label(cx: &App, label: &str, query: &str) -> Div` (:227); `hint_line(cx: &App, text: &str, indent: f32) -> Div` (:334); `muted(cx: &App, text: &str) -> Div` (:344); `fixed_choices(list: &[(&'static str, &'static str)]) -> ChoicesFn` (:504); `toggle(key, label) -> ToggleSpec` (:520); `field(key, label, kind: FieldKind) -> FieldSpec` (:554); `number(key, label, min: f64, max: f64, step: f64) -> FieldSpec` (:575); `FormView::new(binding: Binding, parts: Vec<Part>, cx: &mut Context<Self>) -> Self` (:734); `FormView::lines(&self, cx: &App) -> Vec<String>` (:896); `Part::Select { key, label, choices, hint: Option<SharedString>, beside, indent }` (:617-624).
- `ListEditor::new(binding, key: &'static str, label: &'static str, shape: ListShape, empty: &'static str, hint: Option<&'static str>, cx) -> Self` (`list_editor.rs:73`); `from_spec(binding: Binding, spec: ListSpec, cx) -> Self` (:123); `rows(&self, cx: &App) -> Vec<Vec<String>>` (:128); `lines(&self, cx: &App) -> Vec<String>` (:149); `start_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>)` (:188).
- `controls::section(cx: &App, title: Option<&str>, query: &str, rows: Vec<AnyElement>) -> Div` (`settings_view/controls.rs:457`); `controls::tx(cx: &App, text: &str) -> String` (:438).
- `SchemeView::new(binding: Binding, can_project: bool, cx: &mut Context<Self>) -> Self` (`code_style.rs:56`); `SchemeView::lines(&self, cx: &App) -> Vec<String>` (:87); `code_style::forms()` (:19), `code_style::lists()` (:34).
- `PluginFormattersView::new(binding: Binding, cx: &mut Context<Self>) -> Self` (`languages_controls.rs:496`), `lines` (:569).
- `code_font_of(families: &[&str], ligatures: bool, weight: u16, cx: &App) -> Font` (`editor/view.rs:68`); `editor_font::font_weight_of(raw: Option<f64>, default: u16) -> u16` (`settings_view/pages/editor_font.rs:39`); `theme::zoomed(cx: &App, pixels: f32) -> Pixels` (`theme/mod.rs:53`); `theme::length(cx: &App, name: &str) -> Pixels` (:71).
- `TextArea::new(cx)` (`ui/text_area.rs:354`), `text(&self) -> &str` (:398), `set_text(&mut self, text: &str, cx)` (:404), `set_rows(&mut self, min: usize, max: usize, cx)` (:438); `TextAreaEvent::Cancelled` (:40).
- `SettingsDialog::select_page(&mut self, target: &str)` (`settings_model/staging.rs:359`, sets `page_id` at :361), `stage(&mut self, page: &str, scope: Scope, key: &str, value: Value)` (:458), `reset_page(&mut self, page: &str)` (:526).
- `SettingsDialogView::set_page_view` (`dialog.rs:226`), `custom_control(&self, page: &str, label: &str) -> Option<&AnyView>` (:238), `page_view(&self, page: &str) -> Option<&AnyView>` (:243).
- `EditorIntelligence::of(editor: &Entity<Editor>, cx: &App) -> Option<Entity<Self>>` (`editor/intelligence.rs:141`), `problems(&self) -> &[LintDiagnostic]` (:1025).
- Editor: `shown_lines` (`editor/core_actions.rs:85`), `folds_are_current` (:110), `facts(&self) -> &EditorFacts` (`editor/view.rs:545`), `fold_column_width(&self, cx: &App) -> Pixels` (:839), `text` (:553), `select(&mut self, anchor: u32, head: u32, cx)` (:951), `painted_right_margins(&self) -> Vec<Pixels>` (`editor/view/right_margin.rs:48`); `EditorFacts::{tab_size, indent_unit, right_margins}` (`editor/language_facts.rs:20-48`).
- Workbench test helpers used through `use super::*`: `start`, `trusted_project`, `click_on`, `run_action`, `wait_long`, `settle`, `setting_of`, `settings_dialog_of`, `global_settings_file`, `select_in_tree`, `new_popup_of`, `enter_new_name`, `close_backend_project` (`workbench/mod.rs` :4733, :6841, :8596, :8618, :8336, :4799, :12217, :12223, :12231, :11284, :11303, :11311, :11323) and `proofs19_settings_misc::bring_into_view` (:142).

## Behaviours and their webview lines

- Natural Languages (`src/features/settings/pages/editor/spelling.tsx:15-28`): one section Languages; row "English (USA, UK)" with hint "Bundled Hunspell dictionaries…" and "Used by Proofreading › Typo" beside; no setting.
- Spelling (`spelling.tsx:32-82`): Project dictionary `spelling.projectWords`, project scope, only `hasProject && !newProjects` (:41); Application dictionary `spelling.ideWords`, global (:36); Dictionary files `spelling.customDictionaries`, global (:37); each list trimmed and de-duplicated, empties dropped (`words`, :30), which `ListEditor::end_edit` / `stage_rows` / `commit` already do (`list_editor.rs:220-247`). Placeholders "word" / "~/dictionaries/team.dic", empty texts "No words" / "No custom dictionaries", hints as at :42, :53, :68.
- Font preview (`font.tsx:144-187`, mounted as the page's `footer` at :211): title "Preview", a textarea starting with `SAMPLE` (:144-154), drawn with family + fallback, size, line height, ligatures, main and bold weights as the page shows them (staged values, before Apply); typed text is local state (`useState`, :163), never stored.
- Code Style (`codeStyleScheme.tsx:118-150`): Scheme row, then General / Formatter tabs, General first, local state (so General again whenever the page is shown again). General (`codeStyle.tsx:62-97`): Line separator with hint "Used for new files.", Hard wrap at 1-999 columns, Visual guides, Indents Detection, EditorConfig with hint. Formatter (`codeStyleScheme.tsx:91-112`, :146-147): Do not format, the markers checkbox whose children Off / On / regular expressions are indented and disabled while it is off (`kit.tsx:1-6`, `CheckItem`), Plugin Formatters.
- Other File Types (`codeStyle.tsx:114-130`): the Scheme row in the page's first section.
- File Encodings note (`codeStyle.tsx:384-388`).

## Hypotheses not settled by reading, and what settles them

1. Apply writes a `Scope::Project` change staged from a page whose spec has no `project_scope()` (Spelling's project list) to `.openidea/settings.toml`. Settled by `proofs46::spelling_stores_the_project_and_application_words_and_the_dictionary_files_on_apply` (`settle(... project_file(&root).contains("teh"))`).
2. The settings files write `spelling.*` and `editor.codeStyle.hardWrapAt` as `hardWrapAt = 100` inside `[editor.codeStyle]` (TOML, as `scrollPastEnd = true` at `workbench/mod.rs:12267`). Settled by the same tests' `contains` asserts; if the writer quotes keys differently only those asserts change.
3. `default_settings()` holds `editor.codeStyle.formatterOffTag` = "@formatter:off" and `formatterOnTag` = "@formatter:on" (webview `editorSettings.ts:204-205`). Settled by `editor_code_style_tabs_pending::tests::the_formatter_tab_shows_do_not_format_and_the_markers`; `git grep -n "formatter_off_tag" HEAD -- crates/ide-core/src/settings.rs` shows the Rust default.
4. Cmd-A in `TextArea` selects all (`text_area.rs:728` calls `select_all` in the key handler; the modifier is read, not run). Settled by `the_previews_text_can_be_typed_into_and_is_not_stored`.
5. A plain-text file's indentation is the Other File Types values (`language_facts.rs:99` falls back to them for a language without its own; whether `plaintext` has its own is unread) and Tab at column 0 inserts one indent unit. Settled by `other_file_types_shows_the_scheme_and_its_indentation_is_what_tab_inserts`; read `languages/plaintext.toml` for an indent entry.
6. A GBK file without BOM in the project is decoded with `editor.encodings.project` after Apply: `ops::project` resets the file defaults on every settings write (`crates/ide-host/src/ops/project.rs:199-211`) and `fallback_for` uses `project_fallback` inside the root (`crates/ide-workspace/src/file_defaults.rs:58-68`); whether the native editor opens through the path that applies it is unread. Settled by `a_project_encoding_chosen_on_the_page_decodes_a_project_file`.
7. New File writes the BOM: `ops::fs::create_file` writes `new_file_bytes()` into an empty new file (`crates/ide-host/src/ops/fs.rs:102-116`), and `project_files.rs:756` calls it (read). Whether `editorconfig::create_file` leaves the file empty without an `.editorconfig` is unread: `git show HEAD:crates/ide-workspace/src/editorconfig.rs | grep -n "pub fn create_file" -A15`.
8. `bring_into_view` reaches the drop-down choice of the last select of File Encodings: the list opens under the select and may sit under the dialog's buttons. Settled by `create_utf8_files_with_bom_chosen_on_the_page_writes_the_bom_to_a_new_file`.
9. Plain text is checked by the Typo inspection by default (`PROSE` includes `plaintext`, `crates/ide-host/src/ops/spell.rs:13`; the profile default of `builtin:spelling` is unread). Only matters once the pending effect test is declared.

## Measured by text: the Typo inspection is not reached

`git grep -n "SpellChecker\|SpellConfig\|typo_diagnostics\|SPELLING_PRODUCER\|ops::spell" HEAD -- app-gpui/src` hits only `app-gpui/src/editor_lsp/spelling.rs` (and `editor_lsp/mod.rs:43 pub mod spelling;`). Nothing in the app creates a `SpellChecker`, calls `ide_host::ops::spell::spell_check` or publishes `SPELLING_PRODUCER` diagnostics, so no editor shows a typo whatever the dictionaries hold. The backend reads the dictionaries at each check (`crates/ide-host/src/ops/spell.rs:30-34`, `project.settings.effective().spelling`), so once an editor runs the checker the page's lists take effect. A backend-only test of the speller is not possible from the app crate: `spell_check` takes an `ide_spell::SpellScope` and `ide-spell` is not a dependency of `app-gpui` (`app-gpui/Cargo.toml`). Hence `proofs46_spelling_typo_effect_pending.rs` stays undeclared.

## Tests most likely to fail first

1. `font_preview_follows_the_edits_before_apply_and_its_text_is_not_stored`: needs the `TextArea` hunk and the spec hunk; `type_into(cx, "settings-font-editor", …)` then Enter follows the existing font test, but `bring_into_view` of `settings-font-preview` at the page's end may need more scroll than the 40 steps.
2. `create_utf8_files_with_bom_chosen_on_the_page_writes_the_bom_to_a_new_file` (hypotheses 7, 8).
3. `other_file_types_shows_the_scheme_and_its_indentation_is_what_tab_inserts` (hypothesis 5; also the line `Scheme: Default (IDE) ▾ [Copy to Project]` assumes `can_project`, true with a trusted project open).
4. `code_style_tabs_switch_and_their_edits_apply_reset_and_cancel`: the line separator drop-down is a `FormView` select inside a page view; the dialog's own "press elsewhere closes the list" does not reach a `FormView` list, which closes on its own press handler (`pages/mod.rs` `FormView::render`).
5. `code_folding_options_changed_on_the_page_decide_the_fold_column_and_the_folds_a_file_opens_with`: the shown lines assume the backend folds as in `proofs15_folding.rs:87-107` in the default mode `start` gives (that test sets Light mode explicitly; `start` sets none).
6. `editor_spelling_pending::tests::words_are_staged_trimmed_and_once_in_their_own_scopes`: the staged order is first-made order (`staging.rs:376`).

## Rows if everything lands

| Row                                       | Parity                             | Proof text                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Missing                                                                                                                                                                                                                                                                                                                                                                                      |
| ----------------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| settings:editor.naturalLanguages          | yes                                | `workbench::tests::proofs46_settings_editor_pages_pending::natural_languages_shows_the_bundled_english_dictionaries_used_by_typo`, `settings_view::pages::editor_natural_languages_pending::tests::shows_the_bundled_english_dictionaries_used_by_typo`; the page opened through File › Settings… shows Languages with English (USA, UK), its bundled dictionaries and "Used by Proofreading › Typo", and stores nothing                                                                                                                                                                                                                                                                  | none                                                                                                                                                                                                                                                                                                                                                                                         |
| settings:editor.naturalLanguages.spelling | no                                 | `workbench::tests::proofs46_settings_editor_pages_pending::spelling_stores_the_project_and_application_words_and_the_dictionary_files_on_apply`, `workbench::tests::proofs46_settings_editor_pages_pending::spelling_reset_and_cancel_discard_an_added_word`, `settings_view::pages::editor_spelling_pending::tests::*`; the project dictionary (with a project, not for new projects), the application dictionary and the dictionary files are edited with the mouse and keys, trimmed and kept once, written to the project's and the global settings files on Apply and OK; Reset and Cancel discard an edit                                                                           | Missing: a word accepted on the page is not shown to stop being a typo because no editor runs the Typo inspection (`app-gpui/src/editor_lsp/spelling.rs` `SpellChecker` has no caller; webview `src/features/settings/pages/editor/spelling.tsx:1-6`); the test is `proofs46_spelling_typo_effect_pending::a_word_accepted_on_the_spelling_page_is_no_longer_reported_as_a_typo`, undeclared |
| settings:editor.font                      | yes, only with the `TextArea` hunk | the five existing proofs of the triage plus `workbench::tests::proofs46_settings_editor_pages_pending::font_preview_follows_the_edits_before_apply_and_its_text_is_not_stored` and `settings_view::pages::editor_font_preview_pending::tests::*`; …; the Preview under the controls draws its sample with the family (or the fallback for a family not installed), size, line height, ligatures and main weight being edited before Apply, takes typed text and Enter, and stores none of it; Reset and Cancel bring the stored font back                                                                                                                                                 | none (the bold weight changes nothing drawn in the sample, as in the webview's plain textarea)                                                                                                                                                                                                                                                                                               |
| settings:editor.codeStyle                 | no                                 | existing proofs plus `workbench::tests::proofs46_settings_editor_pages_pending::code_style_tabs_switch_and_their_edits_apply_reset_and_cancel`, `workbench::tests::proofs46_settings_editor_pages_pending::code_style_hard_wrap_and_guides_set_on_the_page_are_the_editors_right_margins`, `workbench::tests::proofs46_settings_editor_pages_pending::code_style_keeps_its_slot_views_under_the_tabs`, `settings_view::pages::editor_code_style_tabs_pending::tests::*`; the Scheme row over General and Formatter tabs (General first, again each time the page is shown), each tab's controls written on Apply; hard wrap and guides set on the page are an open editor's right margins | Missing: no test shows the effect of the line separator on a new file, of indents detection, of EditorConfig support changed on the page, or of Do not format and the markers on a reformat (`src/features/settings/pages/editor/codeStyle.tsx:1-9`)                                                                                                                                         |
| settings:editor.codeStyle.other           | yes                                | `workbench::tests::proofs46_settings_editor_pages_pending::other_file_types_shows_the_scheme_and_its_indentation_is_what_tab_inserts`; the Scheme row is drawn; Tab size 2 and Use tab character applied on the page are what Tab inserts in a plain-text file                                                                                                                                                                                                                                                                                                                                                                                                                            | none                                                                                                                                                                                                                                                                                                                                                                                         |
| settings:editor.fileEncodings             | yes                                | `workbench::tests::proofs22_settings_units::stages_properties_conversion_and_applies_the_checkbox`, `workbench::tests::proofs46_settings_editor_pages_pending::file_encodings_says_editorconfig_takes_precedence`, `workbench::tests::proofs46_settings_editor_pages_pending::a_project_encoding_chosen_on_the_page_decodes_a_project_file`, `workbench::tests::proofs46_settings_editor_pages_pending::create_utf8_files_with_bom_chosen_on_the_page_writes_the_bom_to_a_new_file`; every control is drawn with the note on .editorconfig and the status bar; a project encoding chosen on the page decodes a project file that is not UTF-8; with BOM chosen, New File writes the BOM   | none, if a note drawn as the last select's help line is accepted for the webview's note item (`codeStyle.tsx:384-388`); Global Encoding and the properties encoding have no effect test (say so if the row needs one per control)                                                                                                                                                            |
| settings:editor.general.codeFolding       | yes                                | the two existing `proofs15_folding` proofs plus `workbench::tests::proofs46_settings_editor_pages_pending::code_folding_options_changed_on_the_page_decide_the_fold_column_and_the_folds_a_file_opens_with`, `workbench::tests::proofs46_settings_editor_pages_pending::code_folding_reset_and_cancel_discard_an_edit`; the four options changed through the Settings window decide the fold column and the folds a newly opened file has, File header unticked included; Reset and Cancel discard an edit                                                                                                                                                                                | none                                                                                                                                                                                                                                                                                                                                                                                         |
