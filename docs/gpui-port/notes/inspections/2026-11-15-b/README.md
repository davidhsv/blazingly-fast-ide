# Broad look at the real window at 100 % (2026-11-15, working tree after `08f53875`: the interrupt handler and tabs in a single-file window)

Method as before (`../2026-11-14-c/run.sh`: fresh `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR` on temp folders, a temp Git project, capture by window id at each mark, stop by PID). Click coordinates were converted from shot pixels to window points this time. Four tours, 36 shots, no `panicked` line and no failed step; I looked at the ones listed and at the Settings, tool-window and menu shots in passing. Display awake and unlocked.

## Stop with the interrupt ignored (measured in the real binary)

`stopcheck.sh` starts the binary from a script with `trap '' INT` and `&`, runs `serve.sh` (echo, then `sleep 61.37`) from its context and presses Stop once: one `sleep` process while it runs, none after the Stop, none after quitting. The console then reads "Process finished with exit code 130 (interrupted by signal 2:SIGINT)" (shots 08, 09). This is the first time a running program's console was looked at: its tab, the command line in italics, the output, the toolbar, the Rerun and Stop buttons of the title bar's run widget, and the "1" badge on the Run stripe button.

## What was seen

| Shot | Screen                                                     | What it shows                                                                                                                  |
| ---- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| 01   | Settings → Appearance                                      | **Defect**: "Fallback font::" and "Size::" carry two colons; "UI font:" sits lower than its field.                             |
| 02   | Settings → Editor → General                                | Sections, checkboxes, radio buttons and fields inside the dialog.                                                              |
| 03   | Click on the title bar's project widget, shot 1.2 s later  | **Defect, still there**: the widget's tooltip (the project path) is drawn over the popup and hides its first rows.             |
| 04   | Click on the run widget's "more" button                    | Run Context Configuration, Debug Context Configuration, Edit Configurations…, Reload Run Configurations, with their shortcuts. |
| 05   | Click on the gear                                          | **Same defect**: the tooltip "IDE and Project Settings" covers the menu's first row.                                           |
| 06   | The binary launched with a file, then a second file opened | A single-file window with two tabs (`a.txt`, `b.md`), the title following the active tab, the Markdown split view.             |
| 07   | Commit and Git tool windows                                | The changes tree with status colours, the commit box, the log with its filters and branches pane.                              |

Also looked at and found in order: Keymap, Editor → Font, Color Scheme, Version Control → Git, Tools → Terminal, Plugins pages; diff, terminal, Problems, TODO, Structure, Database, Notifications, Debug; the editor's, a tab's and a Project view row's context menus; the branches popup; the run configuration chooser; Search Everywhere; Find in Files; Annotate in both themes; a single-file window's tab menu and the window after closing a tab; the Keymap Reference dialog opened from a single-file window.

## Findings

1. **A tooltip over the popup its target opened** is not fixed for the real flow. The fix of D101 holds for a press made before the tooltip's delay ends, and its test passes; but when the pointer rests on the target after the click, the tooltip appears over the popup about a second later (both title-bar targets tried show it). Hypothesis: opening the popup makes the target count as left and entered again, which starts the tooltip's delay anew. To reproduce: `click` on the project widget, `wait 1200`, look.
2. **Settings → Appearance**: two labels end in "::". The form label helper adds a colon to a label that already has one.
3. Not looked at in this tour: the Services tool window (it lists this machine's Docker containers), a debug session at a breakpoint, the merge editor, the New Pull Request tab (needs a signed-in `gh`).
