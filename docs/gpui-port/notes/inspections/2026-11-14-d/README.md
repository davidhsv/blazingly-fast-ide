# Fourth inspection of the real window (2026-11-14, working tree after `8a86fce1`: fonts, shadows, status colours, click steps)

Same method as the third (`../2026-11-14-c/run.sh`: fresh `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR` on temp folders, temp Git project, capture by window id at each mark, stop by PID), now with the script's `click`, `rclick` and `hover` steps. Display awake and unlocked. No `panicked` line. Shots 09 and 10 are from the integration's own run on the same binary.

## What was seen

| Shot | Screen                                                  | What it shows                                                                                                                                                                                                  |
| ---- | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 01   | Right click in the editor                               | The editor's context menu: Show Context Actions, Paste, Copy / Paste Special, Column Selection Mode, Go To, Folding, Rename, Generate, Send to Agent, Open In, Local History, Git, Compare With. Shadow drawn. |
| 02   | Right click on a tab                                    | Close … Configure Editor Tabs; the rows that need another tab are greyed.                                                                                                                                      |
| 03   | Click on the title bar's project widget                 | **Defect**: the widget's tooltip (the project path) is drawn over the open popup and hides its first rows; only "Close Project" shows.                                                                         |
| 04   | Click on the branch widget                              | The branches popup: search field, Update Project, Commit, Push, New Branch, Checkout Tag or Revision, Local → main (current).                                                                                  |
| 05   | Click on the run configuration chooser                  | Current File, the Cargo configurations with their run and debug buttons, Edit Configurations.                                                                                                                  |
| 06   | After a click on Debug and on Search Everywhere         | Debug tool window with a finished session, the "CodeLLDB is not installed" balloon with its two links and a shadow, Search Everywhere with the recent file, the button's tooltip.                              |
| 07   | Find Action in the dark theme, "theme" typed            | Three actions listed with their groups.                                                                                                                                                                        |
| 08   | Pointer resting on the caret-position status item, dark | Tooltip "Go to Line:Column ⌘L" with a shadow.                                                                                                                                                                  |
| 09   | Right click on a Project view row                       | The row's context menu (New, Cut, Copy, …, Git, Compare With).                                                                                                                                                 |
| 10   | Settings → Editor → Font                                | Inter in the interface, shadow on the dialog; the checkbox label is still cut by the dialog's edge.                                                                                                            |

Fonts: the interface is drawn in Inter and code in JetBrains Mono (`--self-check=fonts` exits 0 on this binary). The Project view colours a modified or unversioned file and the tab title of a modified file, as the Commit tree does.

The terminal started with the temp `HOME`: a plain `zsh` prompt, and its history file is the temp folder's (`.zsh_history` there). That shot is not committed (it shows the machine's user and host name).

## Findings

1. **Tooltip over an open popup** (shot 03): the project widget's tooltip covers the popup it opened. A tooltip must go away when its target is pressed and stay away while its popup is open. Check every `WithTooltip` target that opens a popup.
2. **Settings rows overflow** (shot 10): unchanged. Measured cause: the native rows use a fixed 220 px label column (`settings_view/pages/mod.rs`), the webview's grid is `max-content minmax(0, 1fr)` per row and its hint spans both columns (`src/features/settings/components/Form.module.css:30-33`).
3. **"Commit an…"**: the second commit button now ends in an ellipsis at the default width of 240. Whether the webview reads "Commit and Push…" in full at that width is not measured (the integration's arithmetic says it also truncates; the webview has to be looked at).
4. **Terminal**: the prompt starts at the panel's left edge with no inset. To compare with the webview's terminal padding.
5. **Still to compare with the webview**: Find Action with an empty query, the diff without syntax colours, the Database filter field, the terminal's font fallback, `toolwindow.focusEditor` unavailable from the Project view, the hints of a single-file window.
6. A mistake of mine in this run: three title-bar clicks used screenshot pixels as window points (the window is 1536 points wide, the shots 1500 pixels), so they hit Debug and Search Everywhere instead of the menu buttons. The Debug click built the temp project with the temp home's Cargo folder; nothing outside the temp folders was touched. The gear menu, the run "more" menu and the Project view's options menu were not seen.
7. Not seen yet: a running program's console, a debug session at a breakpoint, the merge editor, a plugin's views, the main menu's contents.
