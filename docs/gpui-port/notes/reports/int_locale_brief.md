# Integrator brief: the native UI never follows the Language setting

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`. Your own scratch subfolder: `$S/dloc/`.

Format only files you edited, taken from `git diff --name-only` plus the files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## The defect (measured by the manager with grep at HEAD; confirm it by a failing test first)

Settings › Appearance has a Language choice (`appearance.locale`; page spec in `app-gpui/src/settings_model/builtin_pages/behavior.rs`), and the native app has the catalogs and the lookup (`app-gpui/src/platform/i18n.rs`: `Localization`, `I18n::set_locale`), with many tests that switch to Chinese and assert translated texts. But outside tests nothing ever calls `set_locale`, `runtime_settings` (`app-gpui/src/settings_model/runtime.rs`, which computes `language_tag`) is called by nothing but its own tests, and the plugin host is told the language once at start from the default (`app-gpui/src/plugins.rs:178`). So the running app shows one language whatever the setting says, and plugin code that asks for the UI language gets that one too. An earlier integrator found it through a test that never ran (`app-gpui/src/workbench/plugin_runtime/tests/proofs10_locale_pending.rs`: read it; it is yours to make pass and declare).

The webview: `src/features/app/locale.ts` (the UI locale follows `appearance.locale`; after its own catalog switched it tells the plugin host: `commands.pluginSetUiLocale`; localised contributions are registered again), `src/platform/i18n.ts` and `src/platform/locales/` (what an unset or unknown value means: the system's language? a fallback? read `setLocale` and the list of locales), the Language page of Settings, and every place that must show the new language without a restart (menus and the native menu bar, action titles in Find Action and the keymap page, tool window titles, status items, open dialogs, notifications already shown: read what the webview re-registers and what it leaves until the next open).

## What to do

1. A failing test first, in a project window on the real backend: with the setting written to Chinese through the Settings dialog's Apply, a text the window draws (take three of different kinds: a main menu title, a tool window's title, an action's title in Find Action) is the Chinese catalog's; set back to English they are English again; no restart. And at start: a home whose stored settings say Chinese opens its first window in Chinese.
2. The wiring: one place that applies the language when the app starts and whenever the settings change (the same place other runtime settings are applied, or where they should be: read how appearance, keymap and idle-save settings reach the windows today, and whether `runtime_settings` was meant to be that place), switches the catalog, redraws every window, rebuilds what holds titles (the native menu bar, registered actions' titles if they are stored translated: read), and tells the plugin host the new tag after the catalog switched, as the webview does. No file is read and nothing blocks on the window's thread beyond what the lookup already does.
3. An unset or unknown value: do what the webview does. If that is the system's language, the system language is read through a seam so a test never depends on the machine's (read how `I18n::set_locale(input, system)` takes it and what the app passes today), and a test covers system Chinese with the setting unset, and an unknown stored tag.
4. Declare `proofs10_locale_pending.rs` (renamed without `_pending`) once it passes; the manager deletes the older undeclared `proofs10_locale.rs`.
5. Run every test that asserts a translated text or the Language page by filter before and after (grep the tests for `set_locale` and for the Language page's key); none changes its expectation.

## Hard rules

- Tests use a temp home; the system's language never leaks into a test's expectation; no network, no program of the machine, nothing under the user's home.
- No assertion is weakened. No new dependencies, no `unsafe`, no stubs, no `#[ignore]`.
- The real binary: look at it once, through `$S/insp8/run.sh` only, with a temp `HOME`, `OPENIDEA_GPUI_HOME` and project under a neutral path `/private/tmp/openidea-inspect-<random>/` (removed afterwards): Settings › Appearance › Language set to Chinese and applied: a shot of the window in Chinese (menus, a tool window, the Settings dialog), and back to English. Never the Agents tool window, the Agents pages of Settings, the Services window's Services tab or a debug session.

## Rows

`ipc:plugin_set_ui_locale`, the settings row of the Appearance page (find its key in `docs/gpui-port/parity.json`), `unit:src/features/app/locale.test.ts` if it exists (every case with its native test), and any row whose proof says a text is translated: say whether its test already switched the language through the setting or only through `set_locale`. Do not write to `parity.json`.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): HEAD, and the other agents' undeclared drafts then in the tree.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: the failing tests first with their logs; the wiring (where, what is redrawn or rebuilt, what stays in the old language until reopened and whether the webview does the same); the unset and unknown values; the rows; the shots' absolute paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
