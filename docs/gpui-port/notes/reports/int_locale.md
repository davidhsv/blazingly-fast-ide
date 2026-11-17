# Report: the native UI now follows Settings › Language and Region › Language

S = `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`. All gates pass, and the real app switches to Chinese and back without a restart. Nothing is committed.

## 1. Failing tests first (measured)

I ran the tests before the wiring existed. At that point `ui_language::init` was an empty function, so the tests could compile. Log: `S/dloc/red.log`, which ends with `test result: FAILED. 0 passed; 4 failed; ... finished in 82.63s`. Each test failed for the defect itself:

- **`workbench::tests::ui_language::the_language_applied_in_settings_reaches_the_menus_a_tool_window_and_find_action`**: after Apply, the dialog still read `"Language: 简体中文 (Chinese, Simplified) ▾"`. Its label stayed English.
- **`workbench::tests::ui_language::a_home_whose_settings_choose_chinese_opens_its_first_window_in_chinese`**: the Project tool window title `项目` never showed (the wait ran out after about 41 s).
- **`workbench::tests::ui_language::an_unset_language_is_english_system_is_the_systems_and_an_unknown_one_is_english`**: for `"system"`, the left side was `"en"` and the right side `"zh"`.
- **`workbench::plugin_runtime::tests::proofs10_locale::a_plugin_reads_the_ui_language_the_settings_choose`**: failed with "the plugin host is told the chosen language did not happen within thirty seconds".

After the wiring, the same four pass. Log: `S/dloc/green1.log`, with `test result: ok. 4 passed; 0 failed`.

## 2. The wiring

**Where the language is applied:** a new module, `app-gpui/src/platform/ui_language.rs`. It follows the same pattern as `theme::appearance::init`:

- `init` registers a `Backend` listener for `SettingsChanged` with the key `""`, `"appearance"` or `"appearance.locale"`, then applies the language once.
- `apply` reads `settings::global(cx).appearance.locale` (in memory, no file read) and calls `I18n::set_locale(Some(value), system)`.
- Only when the language actually changed does it:
  - tell the plugin host with `set_ui_locale(tag)` if `Plugins` exists, after the catalog switched (as `src/features/app/locale.ts:17-21` does);
  - call `cx.refresh_windows()`.

**When it runs:**

- In `app-gpui/src/lib.rs:75-77`: after `Backend::init` and the theme, before `Plugins::init` and before any window. So the first window starts in the stored language, and `plugins.rs:178` already hands the host the right tag at start.
- In the tests: in `start_with_plugins` (`app-gpui/src/workbench/plugins_window.rs:975`), and explicitly in the new tests.

**`runtime_settings` was not the place.** Measured by grep: only its own tests call it. Its `language_tag` matches the setting's id exactly, so it disagrees with the webview's resolution for `"zh-CN"`, `"system"` and `"auto"`. I left it and its tests unchanged.

**What is redrawn or rebuilt:**

- **All windows** are redrawn. Views translate their text when they draw.
- **Native menu bar:** it already rebuilds its labels for each language (`workbench/menu.rs:427-431`), and its own listener counts every settings change. Measured in the real app: `文件, 编辑, 视图, 导航, 代码, 重构, 构建, 运行, Git, 工具, 窗口, 帮助`, then `File, Edit, View, …` after switching back.
- **Action titles:** stored in English and translated when read (`ActionMeta::localized`), so Find Action and the keymap page follow without any re-registration. The webview instead re-registers its translated actions (`locale.ts:33-46`).
- **Tool window titles — a defect I fixed.** The header drew `declared.title` untranslated. It now applies `tx` unless the title is verbatim, as `src/workbench/toolwindows/ToolWindows.tsx:68` and `:809` do (`app-gpui/src/workbench/mod.rs:4265-4288`). This adds a `tool-window-title:{id}:{title}` test selector.
- **Open Settings dialog:** it switches at Apply. The test asserts this and the shots show it.
- **Notifications already shown:** they translate their body when drawn (`notification_balloons.rs:150`), so they switch too.

**What stays in the old language until reopened:**

- Hypothesis: an open popup that builds its rows when opened keeps them until its query changes. This is not measured; reopening Find Action shows the new language. Check: open Find Action, switch the language in another window, and read `popup.lines()`.
- Not built: plugin text catalogs (`plugin_models::plugin_text::PluginCatalogs`) have no caller. Measured by grep. So a plugin's own `locales/<tag>.json` is never loaded, and therefore not reloaded on a switch. The webview reloads it at `src/features/plugins/bridge.ts:79`.

**Scope difference:** the language is one per process, read from the global settings, like the theme (D25). The webview reads each window's own effective settings, project layer included (`locale.ts:24`, `src/platform/project.ts:67`). A project file's own `appearance.locale` therefore does not give its window another language natively.

## 3. Unset and unknown values (follows the webview)

| Stored value                   | Result                                | Webview source                                                                                                        |
| ------------------------------ | ------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Unset                          | English, whatever the system language | backend default `"en"` (`crates/ide-core/src/settings.rs:858`, `serde(default)`); `locale.ts` passes the raw value on |
| `""`, `system`, `auto`         | the system language                   | `src/platform/i18n.ts:72-77`                                                                                          |
| A regional tag such as `zh-CN` | its language                          | `src/platform/i18n.ts:72-77`                                                                                          |
| An unknown tag (`xx`, `de`)    | English                               | `src/platform/i18n.ts:72-77`                                                                                          |

The brief asked for "system Chinese with the setting unset". The test covers that case, and per the webview it asserts English.

**The seam:** the global `SystemLanguage`. Without it, `system` means English, and only `lib::init` sets it (from `SystemLanguage::from_env()`). So no test ever reads the machine's language; the tests set `SystemLanguage("zh-CN")` themselves.

**Production source of the system language (a measured gap):** it is `LC_ALL`, then `LC_MESSAGES`, then `LANG`. Reading macOS's preferred languages would need either `unsafe` (objc2 `msg_send!`) or a new direct dependency (`objc2-foundation` or `sys-locale`), both forbidden. Hypothesis: an app started from Finder has none of these variables, so `system` falls back to English there. Check: `launchctl getenv LANG`. The Language page offers only the seven languages, so `system` can only come from editing the settings file by hand.

## 4. Pending plugin-locale test: done

- `app-gpui/src/workbench/plugin_runtime/tests/proofs10_locale.rs` is written with exactly the pending file's content (`cmp` printed "same") and declared in `plugin_runtime.rs:1608`. It passes.
- **For the manager to delete:** `app-gpui/src/workbench/plugin_runtime/tests/proofs10_locale_pending.rs`.

## 5. Gates (measured)

**Translation and Language-page tests by filter (45 filters):**

- Before: `S/dloc/locale_before.log`, `test result: ok. 97 passed; 0 failed`.
- After: `S/dloc/locale_after.log`, `test result: ok. 98 passed; 0 failed`.
- The same 97 pass; the extra one is my new test, which the filter also matches. No expectation changed. The `proofs36`, `proofs38` and `proofs41` drafts the grep found are not declared, so they did not run.

**Whole suite:**

- Run 1, alone: `S/dloc/suite1.log`, `test result: ok. 8273 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 220.76s`. Uptime load went from 3.12 before to 93.43 after.
- Run 2: `S/dloc/suite2.log`, `test result: ok. 8273 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 222.74s`.

**Twenty runs of the four new and declared tests**, during suite 2 (between 11:39:55 and 11:40:50; suite 2 ran 11:39–11:43): 20 of 20 passed, each `4 passed; 0 failed`. Summary in `S/dloc/loop20.txt`, logs in `S/dloc/loop_1.log` … `S/dloc/loop_20.log`. No test failed at any time.

**Other gates:**

- `cargo test --test web_view`: `S/dloc/web_view.log`, `test result: ok. 1 passed; 0 failed`.
- Clippy (`--all-targets -- -D warnings`): `S/dloc/clippy2.log`, exit 0, 0 warnings or errors.
- `cargo build`: `S/dloc/build2.log`, exit 0, 0 warnings.
- No process of mine is left (checked with `pgrep`; the app's PID 2855 is gone).

## 6. Rows (proposed; I did not write to `parity.json`)

- **`ipc:plugin_set_ui_locale`** → done, by `workbench::plugin_runtime::tests::proofs10_locale::a_plugin_reads_the_ui_language_the_settings_choose`. The key is absent from `parity.json`; `INVENTORY.md:2663` lists it unticked.
- **`settings:appearance`:** the Language option is not on that page. It lives on `general.language`, and I found no row for that page's key. The page is proven by `the_language_applied_in_settings_reaches_the_menus_a_tool_window_and_find_action`, which goes through the dialog's select and Apply. `settings:appearance`'s own gaps are unchanged.
- **`wit:wit/host.wit#ui`:** remove "locale … no compiled test reads it, and the native app cannot change its language while running" from its missing list. Same proof test.
- **`unit:src/features/app/locale.test.ts`:** it does not exist.
- **Rows whose proof says a text is translated** (`unit:src/platform/i18n.test.ts`, `unit:src/platform/locales/actionTitles.test.ts`, `module:src/platform/actions.ts`, `unit:...keymap.test.ts`, `unit:...loginHint.test.ts`, `unit:...builtinNames.test.ts` and others): their tests all switch with `set_locale` directly (or with a hand-built `I18n`). None goes through the setting. The three new workbench tests are the only ones that do.

## 7. Shots (all in `S/dloc/shots/`)

- **`lang1-02-settings-en-1.png`**: the Settings dialog on Language and Region, in English, with Apply disabled.
- **`lang2-01-dropdown-1.png`**: the Language list showing all seven languages.
- **`langen-01-staged-zh-1.png`**: Chinese chosen but not yet applied. The window is still English and Apply is enabled, so the choice waits for Apply.
- **`langzh2-02-applied-zh-1.png`**: after Apply, everything is Chinese: the dialog (设置, 外观与行为, 语言和地区, 取消/应用/确定), the Project tool window title 项目, the toolbar (版本控制, 当前文件, 极速模式) and the empty-editor hints.
- **`langzh2-04-findaction-zh-1.png`**: Find Action with "Toggle Light" typed shows 切换浅色/深色主题 · 视图, with tabs 全部/类/文件/符号/操作/文本.
- **`langzh-03-main-zh-1.png`**: the main window in Chinese after OK.
- **`langen-03-applied-en-1.png`** and **`langen-04-main-en-again-1.png`**: English again, in the dialog and in the main window.
- **Native menu bar names:** `langzh.menus` (Chinese) and `langen.menus` (English).
- `langzh-01-main-en-1.png` is the English window before switching.

**How the app was run:** through `S/insp8/run.sh` reached via a symlink from `/private/tmp/openidea-inspect-Xl4aP9/`, so the home, `HOME` and the project all sat under that neutral folder. The folder has been removed. I did not open the Agents window or pages, the Services tab, or a debug session.

## 8. False or stale in the briefs

- HEAD is `e41306476`, not `bf33621dd`.
- `S/insp8/run.sh` puts the home under the scratch folder, which contradicts the neutral-path rule. I ran it through a symlink in a neutral folder instead.
- The Language option is on "Language and Region" (`general.language`), not on the Appearance page.
- `proofs10_plugins.md:199` says the start-up tag is missing; `plugins.rs:178` already sets it.

## Files

**Edited:**

- `app-gpui/src/lib.rs`
- `app-gpui/src/platform/mod.rs`
- `app-gpui/src/workbench/mod.rs`: header translation, test selector, `start_in`, `mod ui_language;`
- `app-gpui/src/workbench/plugin_runtime.rs`
- `app-gpui/src/workbench/plugins_window.rs`

**New:**

- `app-gpui/src/platform/ui_language.rs`
- `app-gpui/src/workbench/tests/ui_language.rs`
- `app-gpui/src/workbench/plugin_runtime/tests/proofs10_locale.rs`

**For the manager to delete:** `app-gpui/src/workbench/plugin_runtime/tests/proofs10_locale_pending.rs`.
