# Integrator brief: 27 Settings pages drafted without a compiler (two drafts, one integration)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dset/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time. Work one page at a time (declare, compile, run that page's tests by filter, fix, next). Stop early only if blocked, and say what blocks it.

## The two drafts

1. Languages: notes `docs/gpui-port/notes/integration/settings_languages_integration.md`, report `docs/gpui-port/notes/reports/draft_settings_languages.md`. Files: `app-gpui/src/settings_view/pages/languages_{sdk,controls,pages}.rs`, `app-gpui/src/workbench/tests/proofs19_settings_languages.rs` (19 tests). 16 pages: `languages.go`, `.go.buildTags`, `.go.modules`, `.java`, `.markdown`, `.node`, `.oxfmt`, `.php`, `.pluginFormatters`, `.prettier`, `.ruby`, `.ruff`, `.rust`, `.serverTargets`, `.typescript`, `python.interpreter`.
2. Misc: notes `docs/gpui-port/notes/integration/settings_misc_integration.md`, report `docs/gpui-port/notes/reports/draft_settings_misc.md`. Files: `app-gpui/src/settings_view/pages/{tools_record_table,general_date_formats,general_http_proxy,general_updates,editor_files,tools_actions_on_save,tools_external_tools,vcs_mappings,vcs_issue_navigation,build_sdk_field,build_cmake,build_gradle,build_maven}.rs`, `app-gpui/src/workbench/tests/proofs19_settings_misc.rs` (12 tests). 11 pages: `general.dateFormats`, `general.httpProxy`, `general.updates`, `editor.files`, `build.cmake`, `build.gradle`, `build.maven`, `tools.actionsOnSave`, `tools.externalTools`, `vcs.mappings`, `vcs.issueNavigation`.

The notes are hypotheses; where a note and the webview source (`src/features/settings/pages/*.tsx`, labels in `src/platform/locales/text/en.json`) disagree, the webview source wins. A page's controls, labels, keys, defaults, ranges and enablement rules are the webview's, exactly.

## Decisions already taken, which override the drafts

- The two writers each wrote a toolchain drop-down with its own test seam (`languages_sdk.rs`: `SdkFieldView`, `TestToolchains`; `build_sdk_field.rs`: `SdkFieldView`, `SdkDetection`). Keep ONE field and ONE seam for all pages (read both, keep the one closer to the webview's shared component, move the other's users onto it, delete the duplicate file's copy). In the test build no tool is searched for, looked up on the PATH or run, and a test proves that by asking for detection with no detector supplied.
- Both drafts add a `page_view` getter to `settings_view/dialog.rs`; a previous integration may have added it already. One getter.
- Both drafts need the smoke test (`workbench/tests/smoke_settings_pages.rs`) to accept pages made of their own controls: change the smoke test once, without weakening what it asserts for the pages it already covers.
- A secret is never part of a `Debug` output. The misc writer reports that the Settings dialog model's `Debug` already prints the saved proxy password inside its base settings, and its hunk hides only the staged change. Fix both: a test formats the model, the staged changes and the proxy view with `{:?}` after a password was typed and stored, and asserts the password text is absent. The password field draws bullets only and its text is never copied.
- HTTP Proxy's Check connection: find the IPC adapter of the webview's command in `src-tauri/src/commands/` and call the same backend function. If that needs a path dependency on a crate of this repository that `app-gpui` does not name yet, add it only if `app-gpui/Cargo.lock` gains no package; otherwise leave the button out, say so, and the row stays partial. Tests never make a request: they go through the seam.
- Updates: read when the webview shows the page and what `app.checkForUpdates` does there. Do what the webview does as far as the backend allows without a network request in tests; no stub action in production code. What cannot be done is reported and the row stays partial.
- Issue Navigation checks patterns with the `regex` crate already in the graph; the webview uses JavaScript regular expressions. Keep it, and report the patterns that differ (look-around, backreferences) so the row's note can say so.
- Not to build here: `projectStructure`, Download JDK…, Python's Add Interpreter… and packages table, plugin run targets.

## Tests

No test may start a language server, an SDK tool, a JVM, node, python, Maven, Gradle, CMake, a login shell, make a network request, or read or write anything under the user's home. An external tool test may only launch a script the test wrote into its temp folder. Read each fixture before running it.

## Real window

After the gates pass, take one screenshot of each of these pages in your own binary's window (inspection script: `$S/insp/USAGE.md`, launcher to copy: `$S/d111/insp/run.sh`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, temp project): Languages → Java, Node, Python Interpreter; Build → Maven, CMake; HTTP Proxy; File Types; External Tools; Issue Navigation. Look at each: nothing cut, nothing over another control, labels end in one colon, the page fits or scrolls inside the dialog. Do not press Detect, Download, Check connection or anything that would run a tool or make a request in the real binary.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings (production callers of the toolchain seam are compiled only there). Never pipe a test run into `head`. Start no test run with `&` inside a script.

## Report

Besides the format of `$S/int_rules.md`: one table with a row per page: `settings:<page id>`, state (done or partial), the tests proving it by full name, what is missing for partial. A page is done only when every control of the webview's page is drawn, a change is stored after Apply, and Cancel and Reset discard it, each proven by a named test. Also every other `parity.json` key these tests prove (ipc rows such as `sdk_detect`, `languages`, `plugin_formatters`: only when the state shown can only come from that backend function and the native path calls the adapter's function).

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{agents_views,diff_tab}.rs`, `app-gpui/src/agents/{config_pickers_view,new_task_view}.rs`, `app-gpui/src/agents/composer_view/pickers.rs`, `app-gpui/src/agents/new_task_view/`, `app-gpui/src/vcs_diff/{wrap,tab_view}.rs`, `app-gpui/src/vcs_diff/view/moved.rs`, `app-gpui/src/platform/error_log_sink.rs`, any build-tools draft that is still undeclared, and every undeclared `proofs*` test file that this brief does not hand you.
