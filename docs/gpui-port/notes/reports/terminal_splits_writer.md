I've written all four items as new files plus exact integration hunks, but nothing has been compiled or run (no cargo, as agreed). The six new files pass `rustfmt --edition 2024 --check`. The integration notes are at `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/terminal_splits_integration.md`. They hold the hunks in apply order, the `mod` lines, the expected state of each parity row, the webview lines each behaviour follows, what is left out, and the tests likely to fail.

**1. Split groups.** The panel now draws a header for every group: the title "Terminal" in the first group, the group's own tabs, and the window's Options and Hide buttons in the top-right group. An unsplit window is the one-group case, which also gives the "tabs in the header" look. The workbench stops drawing its own header for the Terminal (hunk W3, which keeps the `more:terminal` and `hide:terminal` selectors).

- **Tabs:** clicking a tab selects it within its group, and pressing inside a group's terminal makes its tab the active one. The "+" button opens a tab in that group, and the chevron button opens the More menu.
- **Tab menu:** Close Tab, Close Other Tabs, Split Right / Split and Move Right, Split Down / Split and Move Down, and Unsplit while split, each shown under the same conditions as the webview.
- **Divider:** a 1px line with a 7px grab area that can be dragged; each group keeps at least 60px, as in the webview. Sizes are not persisted, same as the webview.

**2. Closing.** The tab's ✕, Close Tab, Close Other Tabs, ⌘W in a terminal and the `terminal.closeSession` action all go through one new path. It asks "Process 'X' Is Running" with Terminate / Cancel when a command runs, and Close Other Tabs stops at the first Cancel. Hunk T2 makes the action use this path and removes the old functions.

**3. Drops.** A file drop on a terminal types the quoted paths at its prompt, without Enter, using the existing `terminal::drop` model. Only the terminal under the pointer gets them.

**4. Paste.** ⌘V now carries out the full `terminal::paste` plan: text, quoted paths of copied files, or a copied picture stored on the backend with its quoted path pasted afterwards. The stored path is pasted only if the terminal still runs the same session. Failures become a notification.

**Tests:** 12 new tests. The two e2e flows and the busy-command question are driven through the real window: right-click menus, ✕, ⌘W, ⌘T, a real divider drag and a real file drop. Busy commands use the existing `set -m` + `sleep 30` and `/bin/sleep` setups; all files and the plain shell stay in temp folders.

**Not built:**

- Rename Session… (needs a rename popup that doesn't exist natively).
- Dragging tabs between groups.
- The shell list in the More menu (the native menu has no header rows and the app can't list shells yet).
- The pane's right-click menu and the find bar.
- Hiding the header on middle click and maximizing on double click.
- Header buttons that only appear on hover.

Remote windows are treated as local everywhere; this is a hypothesis (I found no remote window wired into the native app).

**Spec cases that collapse:** natively the clipboard is read synchronously, so the webview's "target changed while reading the clipboard" cases reduce to the one async step, the upload. That step is tested with a stale session. The "terminal closed during upload" case is handled in code but no test drives it.

**Hypotheses to check first:**

1. The divider's grab area is drawn on top of both neighbouring terminals, so a press there starts a drag rather than a text selection.
2. The drop handler on the terminal fires for a simulated file drop in tests.
3. `cmd-w` and `cmd-v` reach the terminal on Linux and Windows too.
4. ⌘T opens a new tab while the terminal has focus.

**Overlap with another agent's draft:** the undeclared `app-gpui/src/workbench/tests/proofs18_e2e_toolwindows_pending.rs` has two tests with the same e2e names. They expect a `TabCommand` type and a `terminal-header:1` selector, neither of which exists. My tests supersede them, so those two should be dropped.

**Most likely first-run failures:** the divider drag test (hypothesis 1), the drop test (hypothesis 2), and the stored-picture test, which reads the stored path back off the screen.

New files, all under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/`:

- `terminal/panel/groups.rs`
- `terminal/view/clipboard.rs`
- `workbench/terminal_groups.rs`
- `terminal/panel/tests/proofs23_groups.rs`
- `terminal/view/tests/proofs23_paste.rs`
- `workbench/tests/proofs23_e2e_terminal_groups.rs`
