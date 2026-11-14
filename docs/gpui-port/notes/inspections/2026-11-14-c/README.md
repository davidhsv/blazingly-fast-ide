# Third inspection of the real window (2026-11-14, working tree after `f22311fb`)

The debug binary (`app-gpui/target/debug/openidea-gpui`, built 13:53 from the working tree that became the next commit) was launched five times by `run.sh`, each time on a fresh `OPENIDEA_GPUI_HOME`, with `HOME` and `ZDOTDIR` on an empty temp folder, on a temp Git project (one modified file, one unversioned file). It was driven by the inspection script (`OPENIDEA_GPUI_INSPECT`, steps in `*.script`), its window was captured by window id at every mark, and it was stopped by PID; the focus went back to the app that had it. Display awake and unlocked (measured with `display.swift`). No `panicked` line in any run's output.

## What was seen

| Shot | Screen                                | What it shows                                                                                                                                                                                                  |
| ---- | ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 01   | Trust dialog (untrusted folder)       | Path split head/tail with no gap before the tail. The dots are three full stops, not "…". No shadow.                                                                                                           |
| 02   | Trusted project, no file open         | The empty-editor hints with their shortcuts, the status bar with "Blaze mode", the title bar avatar in the project's colour.                                                                                   |
| 03   | Same, dark theme (`view.toggleTheme`) | All surfaces follow the theme.                                                                                                                                                                                 |
| 04   | `vcs.showDiff` on the modified file   | Side-by-side diff, word highlight on the changed word, toolbar. The code is drawn in one colour (no syntax colours). Drawn as an overlay with a thin border, no shadow.                                        |
| 05   | `vcs.localHistory`                    | A tab "Local History: main.rs" with the empty state (fresh home).                                                                                                                                              |
| 06   | Debug tool window                     | "Not debugging" empty state.                                                                                                                                                                                   |
| 07   | TODO, Pull Requests, Database         | TODO lists the one comment; Pull Requests says "Not logged in to GitHub" (the temp home has no `gh` login); Database empty state. The Database toolbar's "Filter objects" field touches the pane's right edge. |
| 08   | Structure, Notifications              | Empty states ("No structure to show" because the active tab is Local History).                                                                                                                                 |
| 09   | Editor, dark                          | Syntax colours, gutter, current-line band.                                                                                                                                                                     |
| 10   | Git tool window, dark                 | Filters, branches pane, two commits with graph dots.                                                                                                                                                           |
| 11   | Settings → Editor → Font, dark        | The "Show only monospaced fonts" label is cut by the dialog's right edge. No shadow, the window behind is not dimmed.                                                                                          |
| 12   | Settings → Plugins, dark              | The empty state of the details pane is cut by the dialog's right edge ("No p…", "…to instal").                                                                                                                 |
| 13   | Run tool window                       | The empty state with its three hints and the link to run configurations.                                                                                                                                       |
| 14   | Problems tool window                  | Tabs File and Project Errors, the severity filters, "No file is open" (the active tab is Local History).                                                                                                       |

Not committed: the shots that show the Services tool window, because it lists the Docker containers of this machine (the app reads the local Docker daemon, as the webview app does).

Other measurements of these runs:

- Menu bar of the process (System Events, read only): `Apple, openidea-gpui, File, Edit, View, Navigate, Code, Refactor, Build, Run, Git, Tools, Window, Help`. The app menu is named after the binary.
- RSS at the end of the tool-window run 178 MB, of the dark run 208 MB, of the empty window 161 MB (`ps -o rss`, one sample each, debug build: not a measurement of the final comparison).
- The "hovered" status item of the first inspection: the real pointer rested at 1336,1055 points in the window, on the caret-position item. It is the pointer's hover, not a defect.

## Findings

1. **Fonts**: still the system's. The bundled variable `.woff2` faces cannot be used (`--self-check=fonts`: semibold resolves to the regular face). Static faces exist on this machine in IntelliJ IDEA's runtime (`Inter` 3.019 Regular/SemiBold/Italic, `JetBrains Mono` 2.304 in every weight; both name tables say SIL Open Font License 1.1). The webview bundles Inter 4.001 and JetBrains Mono 2.211. Inter Medium and Bold are not there.
2. **No shadow** on the trust dialog, the diff overlay and Settings (`--shadow-dialog`, `--shadow-popup` in `src/styles/tokens.css`). Whether the webview dims the window behind a dialog: to compare (`src/ui/Dialog.module.css` `.backdrop`).
3. **Settings → Editor → Font**: the checkbox label overflows the dialog.
4. **Settings → Plugins**: the details pane's empty state overflows the dialog.
5. **Diff**: no syntax colours. To compare with the webview's diff viewer before calling it a defect.
6. **Database toolbar**: the filter field touches the pane edge. To compare.
7. **Trust dialog path**: "..." where the webview's CSS draws "…". To compare.
8. **Context menus still unseen.** `shift-f10` opens none in the Project view or the editor (to compare: the webview may not either on macOS). A right click posted to the process (`CGEventPostToPid`, `click.swift`) reaches no GPUI view, like the System Events click. The inspection script needs `click`/`rclick` steps that dispatch a mouse event at window coordinates.
9. `toolwindow.focusEditor` answered "Action unavailable" with the Project view focused and an editor open. To compare with the action's condition in the webview.
10. Not seen yet: a running program in Run, a debug session, the merge editor, a plugin's views, Search Everywhere and Find in Files in dark, the menus' contents.
