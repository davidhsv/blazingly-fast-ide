# Second inspection of the real window, 2026-11-14 (with keyboard input)

Commit `37c53373`, the same dev binary as the first inspection, isolated home, temp Git project with one modified file, no plugin, nothing saved, no Restart. This time the application was driven: keys were sent with `osascript` / System Events to the process chosen by its PID (`drive.applescript`: it brings that process to the front, checks that it is the frontmost one and only then sends the key), and each state was captured by window id (`snap.sh`). The owner's own application was never addressed. Nothing was typed into the terminal.

## What was seen

- `01-editor-in-project.png`: `src/main.rs` opened from the Project view with the arrow keys and Enter: a tab with its icon and close button, the gutter, syntax colours, the status bar with Blaze mode, position, line ending, encoding, indent and the lock.
- `02-settings-appearance.png`: Settings (⌘,) on Appearance: the tree of pages, the search field, selects, colour swatches, checkboxes, Cancel / Apply / OK.
- `03-git-log.png`: the Git tool window (⌘9): filters, the branches pane with its toolbar, the commit row with its refs and time, the details placeholder.
- `04-commit-and-terminal.png`: the Commit tool window (⌘0) with the changes tree (the modified file in blue, the unversioned one in red), Amend, the message field and the buttons; the Terminal (⌥F12) with a shell prompt.
- `05-find-action.png`, `06-find-in-files.png`: Find Action (⇧⌘A) and Find in Files (⇧⌘F).
- Memory after visiting these screens: 202 MB resident (one `ps` sample).

## Defects and differences found

1. Fonts: as in the first inspection, system fonts instead of the webview's Inter and JetBrains Mono.
2. Dialogs and popups (Settings, Find Action, Find in Files) are drawn with a thin border and no drop shadow or dimming, so they barely separate from the editor behind them. To compare with the webview's popup and dialog shadows (`src/styles/tokens.css`).
3. In the Commit window the second button reads "Commit and Pu": its label is cut at the default width of the tool window instead of fitting or ending in an ellipsis.
4. The Project view does not colour `main.rs` although it is modified; the Commit tree does. To compare with the webview's file colours in the tree and on the tab.
5. The editor shows no change marker in the gutter for the added line and no breadcrumbs: both are known as not built.
6. Find Action opens with an empty list. To compare with the webview (whether it lists actions before anything is typed).
7. The terminal's prompt shows a boxed question mark where the owner's prompt has an icon glyph: the terminal font has no such glyph. To compare with the webview's terminal font chain.
8. Clicks sent by coordinates through the accessibility API do not reach the window, and GPUI logs that the focused element has no accessibility node: the native window exposes almost no accessible elements, where the webview's page has real buttons, inputs and labels. This is a gap in itself (screen readers, UI automation), not only an obstacle to inspections.
9. `.openidea/.gitignore`, created in the project when it is opened, is listed as an unversioned file. To compare with the webview.

## A caution about the terminal

The terminal of the native application starts the owner's own login shell with the real home directory, also when the application's own directories are isolated with `OPENIDEA_GPUI_HOME`; the prompt in the capture is the owner's. That is what a terminal is for, but it means a command typed there during an inspection would be written to the owner's shell history. Nothing was typed. Inspections must leave the terminal's input alone or set the shell to a plain one in the isolated home's settings first.

## Not seen yet

The debugger, the Run window with a running program, Services, Database, the Pull Requests window, Plugins, context menus (they need a right click, which could not be sent), the merge editor, the diff, Local History, dark themes, the menu bar's menus, more Settings pages.
