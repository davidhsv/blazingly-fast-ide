# Driving the real window: the inspection script (debug builds only)

The debug binary (`cd app-gpui && cargo build`, `target/debug/openidea-gpui`) reads a script after its first window is shown and carries out its steps in order on the window's thread, through the same paths a user's input takes. The code is compiled only with `debug_assertions` (`app-gpui/src/inspect.rs`, called from `app-gpui/src/main.rs`); a release build has none of it.

## Variables

- `OPENIDEA_GPUI_INSPECT=<file>`: the script to run.
- `OPENIDEA_GPUI_INSPECT_LOG=<file>`: where marks and failures are appended (without it they go to the application's log at info level).
- `OPENIDEA_GPUI_HOME=<folder>`: always set it to a fresh folder of its own for each run (the layout, open tabs and recent projects persist there, and `toolwindow.<id>` toggles, so a reused home changes what a step shows), so the app never touches the real configuration. Trust a project in it beforehand by writing `<home>/data/project-trust.json` as `{"projects":["<canonical project path>"],"locations":[],"restricted":[]}`.

## Steps (one per line, the step and its argument separated by a TAB)

Blank lines and lines starting with `#` are skipped.

| Step     | Argument                            | What it does                                                                                                                                                                                                                                                             |
| -------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action` | `<id>` [TAB `<json argument>`]      | Runs the action through the registry in the active window, as a key or menu would (e.g. `action⇥file.settings⇥"keymap"`).                                                                                                                                                |
| `open`   | `<absolute path>`                   | Opens the file in the active window (a tab of its project).                                                                                                                                                                                                              |
| `keys`   | GPUI keystrokes separated by spaces | Dispatched to the active window through GPUI's keystroke dispatch (`cmd-shift-a`, `escape`, `enter`, `shift-f10`).                                                                                                                                                       |
| `type`   | text                                | Typed into the focused element, character by character (spaces kept). Fails when nothing focused takes text.                                                                                                                                                             |
| `click`  | `<x>` TAB `<y>`                     | The pointer moves to the point, then the left button is pressed and released there, through the window's own input dispatch (`Window::dispatch_event`). Coordinates are points from the top-left of the window's content (the title bar included), e.g. `click⇥120⇥160`. |
| `rclick` | `<x>` TAB `<y>`                     | The same with the right button: opens the context menu of what is under the point (a Project view row, the editor, a tab).                                                                                                                                               |
| `hover`  | `<x>` TAB `<y>`                     | The pointer moves to the point (hover states, tooltips after their delay).                                                                                                                                                                                               |
| `wait`   | milliseconds                        | Waits.                                                                                                                                                                                                                                                                   |
| `mark`   | name                                | Appends `mark⇥<name>⇥<unix ms>` to the log: take the screenshot when it appears.                                                                                                                                                                                         |
| `quit`   | —                                   | Quits the app.                                                                                                                                                                                                                                                           |

A pointer step goes through the same dispatch as a real mouse event, so the element under the point gets it (a menu or a popup above it gets it first). Points are window coordinates: in a screenshot of the window taken by window id at 1x, the pixel coordinates are the points; at 2x, halve them.

The screenshot of a `mark` is taken while the script goes on: put a `wait` (about a second) after a `mark` whose screen the next step changes (an `escape` after a menu), or the shot shows the next state.

The active window is the focused one, else the frontmost, else the last opened. Each step waits for the window to draw a frame before the next one; anything that loads in the background (a project, a Git read, a search) needs a `wait`. A step that fails is logged as `fail⇥<line>⇥<step, tabs as spaces>⇥<reason>` and the script goes on. A window that draws no frame within a second (covered or minimized: macOS stops its display link) gets `note⇥no frame was drawn within a second: is the window covered?` and the script goes on; keep the window uncovered. The log ends with `done⇥<unix ms>`.

## Launch (stop it by its PID)

```sh
S=<scratch folder>; P=$(cd "$S/project" && pwd -P)
OPENIDEA_GPUI_HOME="$S/home" OPENIDEA_GPUI_INSPECT="$S/tour.script" OPENIDEA_GPUI_INSPECT_LOG="$S/tour.log" \
  target/debug/openidea-gpui "$P" > "$S/tour.stdout" 2>&1 &
echo $! > "$S/tour.pid"
# poll $S/tour.log for each mark, screenshot, and at the end:
kill "$(cat "$S/tour.pid")"
```

## Example: a tour of the main screens

Ran in the real binary on 2026-11-14 (twice, the second time on a fresh home with the final binary) against a temp Git project (`Cargo.toml`, `README.md`, `src/main.rs` modified): every step ran, all 13 marks were logged, no `fail` line, no error in the app's log. Whether each screen looks as intended was not seen (no screenshots were taken in that run). The project path below is that run's; replace it. `⇥` stands for a TAB.

```
# A tour of the main screens: a mark after each one, for a screenshot.
wait⇥3000
open⇥/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp/project/src/main.rs
wait⇥1500
mark⇥editor
action⇥project.expandAll
wait⇥800
mark⇥project-view-expanded
action⇥toolwindow.git
wait⇥2000
mark⇥git
action⇥toolwindow.commit
wait⇥1500
mark⇥commit
action⇥toolwindow.terminal
wait⇥2500
mark⇥terminal
action⇥toolwindow.terminal
action⇥file.settings⇥"appearance"
wait⇥1500
mark⇥settings-appearance
action⇥file.settings⇥"keymap"
wait⇥1500
mark⇥settings-keymap
action⇥file.settings⇥"vcs.git"
wait⇥1500
mark⇥settings-vcs-git
keys⇥escape
wait⇥800
action⇥find.inPath
wait⇥1000
type⇥answer
wait⇥1500
mark⇥find-in-files
keys⇥escape
wait⇥500
action⇥navigate.searchEverywhere
wait⇥800
type⇥main
wait⇥1500
mark⇥search-everywhere
keys⇥escape
wait⇥500
action⇥run.chooseConfig⇥"select"
wait⇥1000
mark⇥run-widget-popup
keys⇥escape
wait⇥500
action⇥file.closeProject
wait⇥2000
mark⇥welcome
keys⇥shift-f10
wait⇥800
mark⇥welcome-context-menu
keys⇥escape
```

Notes: `toolwindow.<id>` toggles a tool window (the second `toolwindow.terminal` hides it again). `file.settings` takes a page id (`appearance`, `keymap`, `vcs.git`, `general`, `tools.terminal`, … as in `app-gpui/src/settings_model/builtin_pages/*.rs`). `run.chooseConfig` with `"select"` is what the run widget's chooser does. `file.closeProject` turns the window into the Welcome screen; `shift-f10` there asks for the selected recent project's context menu (as `welcome.rs` tests it; hypothesis that a row is selected right after closing the project, to confirm on the screenshot of the `welcome-context-menu` mark). The script file itself must hold real TAB characters.
