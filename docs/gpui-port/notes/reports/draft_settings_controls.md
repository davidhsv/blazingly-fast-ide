I've drafted own controls for 12 of the 13 pages; MCP Server's Endpoint is left out. None of it has been compiled or run: there was no compiler, so every pass/fail below is what I expect, not what I saw. No existing file was edited.

**Files written** (about 5,200 lines, all under `app-gpui/src/settings_view/`)

- `custom_controls.rs` (737): `install(dialog, Install { window_label, has_project }, window, cx) -> Vec<Subscription>`, the backend answers, and 13 end-to-end tests.
- `pages/mod.rs` (1324): the shared pieces (how a control reads and stages its value, its events, and one form view that draws checkboxes, numbers, radio rows, drop-downs, text fields, two-column grids and page links), plus test helpers.
- Shared views: `list_editor.rs` (549, string lists and name/value tables, Tab between cells) and `font_picker.rs` (451). The installed-font list comes from a `FontCatalog` global, which tests fill by hand.
- One file per page, each with its own view tests: `appearance.rs` (396), `general.rs` (81), `notifications.rs` (186), `console.rs` (63), `data_views.rs` (45), `deployment_options.rs` (42), `terminal.rs` (317), `vcs_commit.rs` (83), `vcs_confirmation.rs` (62), `vcs_git.rs` (389), `editor_font.rs` (292), `code_style.rs` (184).
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/settings_controls_integration.md`.

**Edits needed in compiled files**

1. In `settings_view/mod.rs`, add `pub mod custom_controls;` and `pub mod pages;`.
2. In `workbench/settings_window.rs`, after the line `subscriptions.extend(self.mount_plugin_settings_pages(&dialog, has_project, window, cx));` add:
   ```rust
   let install = crate::settings_view::custom_controls::Install { window_label: &self.label, has_project };
   subscriptions.extend(crate::settings_view::custom_controls::install(&dialog, install, window, cx));
   ```

No accessor is needed: the end-to-end tests have their own copies of the test helpers and drive everything by selectors.

**Per page (expected status)**

| Page                                                                                                                                  | Status  | What is missing                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------- | ----------------------------------------------------------------------------------------- |
| `general`, `notifications`, `build.console`, `build.debugger.dataViews`, `build.deployment.options`, `vcs.commit`, `vcs.confirmation` | done    | —                                                                                         |
| `vcs.git`                                                                                                                             | done    | Passwords' Forget is tested only at view level                                            |
| `tools.terminal`                                                                                                                      | done    | the fallback font's hint line                                                             |
| `appearance`                                                                                                                          | partial | Theme's Sync with OS / preferred / plugin themes (no slot); file icon themes from plugins |
| `editor.font`                                                                                                                         | partial | the Preview footer (no slot)                                                              |
| `editor.codeStyle`                                                                                                                    | partial | General / Formatter tabs                                                                  |
| `tools.mcpServer`                                                                                                                     | partial | Endpoint not built                                                                        |

- **Tests:** each page has one end-to-end test that opens it through `file.settings`, operates its own controls, applies, and checks the stored setting. One more test runs Reset and Cancel on an own control of each of the 12 pages. Every page test also fails if any row still says "This page is not available yet".
- **Visible effects checked:** the accent colour is checked in the active theme, and a "No popup" group's notification is listed but gets no balloon.
- **`parity.json`:** `editor.font` and `editor.codeStyle` have no `settings:` key yet.

**Left out**

- **MCP Endpoint:** the app has no MCP server of its own; the server and its status live only in the Tauri side. If it is built later, two things need a decision: `McpServerStatus` derives `Debug` with the token in it, and "Copy HTTP Config" puts the token on the clipboard.
- **Editor font effect:** the editor never reads `editor.fontFamily`; it takes its font from the theme. The test therefore checks the stored setting only.
- **Forget end to end:** forgetting a credential goes through the keychain. Covering it needs ide-ssh's `testing` feature as a dev-dependency.

**Safety constraints**

- No test opens a socket.
- Git Test runs the real `git --version` through `vcs_git_version`, on a trusted temp project.
- Nothing reads the user's home or keychain.
- Tests enumerate no system fonts.

**Most likely first failures**

- The git Test round trip.
- Clicks on controls below the page's visible height (about 590 px).
- Whether `TextField` keeps focus when a click lands inside the dialog's containers.
- Whether a control can read the dialog's scope while it draws.

**Where the existing standard controls differ from the webview**

- Theme is a plain select: no Sync with OS, preferred themes, plugin themes or hover preview.
- Code Style flattens the General / Formatter tabs into one page of sections.
- A stored legacy `"attach"` for Open project in shows no radio selected; the webview loads it as Ask.
- Hypothesis, not checked: typing a whole number into an `f32` setting may leave the page marked modified with nothing really changed. The notes file says how to check.

Production risks:

- The first Settings open lists every installed font on the main thread, which may pause with many fonts.
- The detected git path and the default shell are read on the main thread when the dialog opens.

Run `cargo fmt` after adding the files; several lines are over 120 columns.
