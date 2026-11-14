# Inspection of the real window, 2026-11-14

Commit `37c53373`, dev binary (`cd app-gpui && cargo build`, `target/debug/openidea-gpui`), macOS, display awake and unlocked. Launched four times with `OPENIDEA_GPUI_HOME` on a temp folder and a temp Git project (three files, one modified); no plugin, no saved password, no Restart; the owner's running app was not touched. Each window was captured with `screencapture -l <window id>` (`shot.sh`, `windows.swift` in this folder) and the images were looked at. No key or mouse input was sent: there is no way yet to drive the app without asking for the Accessibility permission, so only what a launch shows could be seen.

## What was seen

- `01-welcome-trust-dialog.png`: the Welcome screen with "Trust and Open Project?" for the undecided project. Text legible, buttons aligned, the primary button blue.
- `02-project-window.png`: the project window after trusting (the trust file written by hand into the temp home): title bar with the project widget, the branch, the run widget (`cargo run inspect`), Run, Debug, Blaze mode, search and settings; tool window stripes left and right; the Project view with the tree; an empty editor area.
- `03-single-file-window.png`: a file opened on its own: line numbers, syntax colours, the current line tint, the caret, and the status items (position, line ending, encoding, indent, lock) at the bottom right.
- `04-welcome.png`: the Welcome screen listing the recent project with its branch and path.
- Memory while idle a few seconds after start (one sample each, `ps`): 128 to 154 MB resident, 0.0 % CPU.

## Defects and differences found

1. Fonts. The log says `failed to load font "Inter Variable"` at weights 400 and 600: the webview bundles Inter Variable and JetBrains Mono Variable (`@fontsource-variable/inter`, `@fontsource-variable/jetbrains-mono`, `--font-ui` and `--font-mono` in `src/styles/tokens.css`); the native app bundles neither, so the UI text is drawn in the system font and the code in the system's monospace font. The visual design is not the webview's until the same fonts are bundled.
2. The project avatar is blue in the title bar and orange on the Welcome screen for the same project: the title bar uses the accent colour instead of the project's colour.
3. The trust dialog shortens the path in the middle and leaves a double gap before the last segment ("… /project"). To compare with the webview's dialog.
4. A `.openidea` folder appears in the project and is listed in the Project view right after the first opening (it holds `.gitignore` and `changelists.json`). To check against the webview: whether it creates the folder on open and whether the tree shows it.
5. The project window shows no status bar while nothing is open, and the empty editor area is blank. To compare with the webview (its empty state may show hints).
6. In the single-file window the first status item ("1:1") is drawn with a background as if hovered or pressed. Hypothesis: the pointer happened to be there; to confirm with the pointer elsewhere.

## Not seen yet

Everything that needs input: an editor inside a project window, the Git and Commit windows, Settings, the terminal, menus and context menus, dialogs other than trust, the debugger, plugins. A debug-only way to run a script of actions at start is the next step, so the following inspections can cover them.
