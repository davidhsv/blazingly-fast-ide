## Report back

**Summary.** 16 of the 17 language pages are now drafted as native Settings pages; `projectStructure` is not built. Nothing has been compiled or run, so every test listed below is unrun. Five pages should be done after integration; the other eleven open with real controls but each has something missing.

### 1. Files written (new files only)

- `app-gpui/src/settings_view/pages/languages_sdk.rs` (972 lines)
  - `ToolchainCatalog`: a global list of detected installations per kind. It is emptied each time Settings opens and filled the first time a page that needs it is shown.
  - `LanguageAsk` / `LanguageEvent`: what the pages ask of the backend or the dialog.
  - `answer(ask, window_label, cx)`: makes the backend calls.
  - `TestToolchains`: a test-only global. In test builds the toolchain answers come from it, so no tool is probed, looked up on the PATH or run.
  - `SdkFieldView::new(binding, SdkSpec, cx)`: the SDK combo. Exposes `choices`, `current`, `python`, `lines`, `choose`.
  - `PathFieldView::new(binding, PathSpec, cx)`: a trimmed path field with Browse…. Exposes `field`, `lines`.
  - 3 view tests.
- `app-gpui/src/settings_view/pages/languages_controls.rs` (1203 lines)
  - `LanguagePageView::new(page, binding, sections, detect, cx)`: the page view. It passes on its controls' dialog events and backend questions, and asks for detection on first render. Exposes `lines`.
  - `FormatterLanguagesView`, `PreviewFontSizeView`, `JdtlsSharedView`, `ServerTargetView` (`Picker` or `Fixed`), `PluginFormattersView`, `PhpInterpreterView`, `ProjectOnlyView`, each with `lines()`.
  - 5 view tests.
- `app-gpui/src/settings_view/pages/languages_pages.rs` (864 lines): the 16 page builders, `search_labels`, `page_view`, and `install(dialog, model, scope, label, has_project, cx)`. 3 plain tests.
- `app-gpui/src/workbench/tests/proofs19_settings_languages.rs` (617 lines): 19 end-to-end tests. Each opens Settings through `file.settings` with the page id, checks every control's line, changes one with the mouse or keyboard, applies, and checks the stored value. Project-scoped pages also check `.openidea/settings.toml`.
  - Reset: Java, Build Tags, Rust and Markdown.
  - Cancel: Go Modules, Rust and Markdown.
  - Browse…: GOPATH, Ruff's configuration directory and TypeScript, using `simulate_path_prompt_response`.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/settings_languages_integration.md`

**Design choice.** Each page is mounted with `set_page_view` plus `set_page_labels`, as Keymap is, and `language_pages.rs` keeps its specs without sections. Putting the controls into spec sections would break `every_option_writes_a_setting_the_backend_takes`, because the `languages.go.modules.env.*` keys have no default. It would also break the smoke test for pages made only of custom controls. The smoke test already accepts `<page view>`.

**Integration hunks** (all are insertions after quoted anchor lines; exact text is in the notes):

- `pages/mod.rs`: three `pub mod` lines after `pub mod general;`.
- `dialog.rs`: a `page_view(page)` getter after `custom_control`.
- `custom_controls.rs`: a 7-line block in `install` after `slots.put(pages::code_style::PAGE, "Scheme", &scheme, cx);`.
- `workbench/mod.rs`: `mod proofs19_settings_languages;` after `mod proofs18_e2e_search;`.

### 2. Actions

No actions were registered; the tests run the existing `file.settings` action. Backend calls the pages stand for:

- `sdk_detect`, `sdk_locate_tools` (php), `sdk_php_xdebug` and `sdk_python_effective`, through `ide_host::ops::sdk`.
- `languages`, through `host.languages.all()`.
- `plugin_formatters`, through `ide_host::ops::lsp::plugin_formatters()`.
- Browse… goes through the existing `ControlEvent::Browse`, which opens the system file chooser.

### 3. Spec coverage

- The existing `languages.test.ts` cases stay with `language_pages::tests`.
- The new view tests cover the Java "defaults shared JDT LS to off" case (workspace rule) and the formatter languages matching the backend's defaults.

### 4. Not built, and what to check first

- **Not built:**
  - `projectStructure` (2,023-line page): nothing is mounted.
  - Python's Add Interpreter… and the packages table (they run pip and interpreters).
  - Download JDK….
  - Plugin run targets: no native list reaches the dialog, so Run in target and "Language server runs on" offer only Default/Local and a stored target.
  - Browse… has no title or start folder, the same gap that marks `settings:general` partial.
- **Smaller divergences:**
  - Option details are folded into the label (e.g. "pnpm — 9.1.0 /path").
  - The webview's spinner is shown as the text "Detecting…".
  - The catalog is emptied for each Settings opened rather than kept per window. If two windows have Settings open at once, the first window's pages show nothing found until Detect again.
  - The registry keyword "jdk for importer" is missing from `language_pages.rs`; I added it to the page's search labels instead.
- **Seam:** `TestToolchains` is new. The tests prove that Detect again asks the backend again, not that detection itself works. The production callers are `#[cfg(not(test))]` and never compiled by a test build, so also run `cargo build -p openidea-gpui` (or clippy without `--tests`).
- **Assumptions to check first:**
  - `update_global` notifies `observe_global` observers.
  - The `LanguageConfig` field names and `plugin_formatters` being reachable from `app-gpui`.
  - `IpcPath` derefs to `Path`.
  - Emitting events from `render` is allowed.
  - The registry has JS/TS languages with servers, and PHP ("PHP") with servers.
- **Most likely to fail first:**
  - The exact-line assertions in `proofs19` (translated labels, trailing spaces such as "Go Version: ").
  - Node's row count of 6, which depends on the registry.
  - The Go detection count, which assumes exactly one render before the check.
  - Clicks on rows lower on long pages (Java, Rust, Node, PHP) may land outside the scrolled view. Scroll first, as `custom_controls::tests::scroll_page` does.
- **Gaps against the assignment:**
  - `pluginFormatters` is tested only in a window without a project.
  - The oxfmt test checks only the first four labels.
  - Go Modules has Cancel but no Reset; Reset on a project-only page is covered by Build Tags.

### Expected state per page

- `languages.go` → partial (Browse… has no title or start folder)
- `languages.go.buildTags` → done
- `languages.go.modules` → done
- `languages.java` → partial (Browse…, no Download JDK…)
- `languages.markdown` → done
- `languages.node` → partial (no plugin run targets, option details in labels, Browse…)
- `languages.oxfmt` → done
- `languages.php` → partial (no plugin run targets, Browse…)
- `languages.pluginFormatters` → partial (the per-language select is not exercised by any test)
- `languages.prettier` → done
- `languages.ruby` → partial (Browse…)
- `languages.ruff` → partial (Browse…)
- `languages.rust` → partial (Browse…)
- `languages.serverTargets` → partial (no plugin targets)
- `languages.typescript` → partial (Browse…)
- `python.interpreter` → partial (no Add Interpreter…, no packages table)
- `projectStructure` → not built
