I wrote 20 declared tests and 3 pending ones in 7 new `proofs14_*.rs` files. Nothing was compiled or run, and I edited no existing file. The full case-by-case notes, with `file:line` evidence, are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs14_github_plugins.md`. They include the 5 `mod` lines to add, each with its anchor line.

**Files** (all under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/`):

| File                                                         | Lines | Tests                            |
| ------------------------------------------------------------ | ----- | -------------------------------- |
| `workbench/github/tests/proofs14_github_window.rs`           | 119   | 4                                |
| `workbench/github/tests/proofs14_github_window_pending.rs`   | 48    | 2 pending, not declared          |
| `github/list_panel/tests/proofs14_ui.rs`                     | 107   | 3                                |
| `workbench/plugin_runtime/tests/proofs14_plugins.rs`         | ~120  | 2, plus two `pub(super)` helpers |
| `workbench/plugin_runtime/tests/proofs14_plugins_pending.rs` | 35    | 1 pending, not declared          |
| `plugin_models/plugins_page/tests/proofs14_page.rs`          | 126   | 7 (the folder is new)            |
| `terminal/view/tests/proofs14_ime.rs`                        | 116   | 4 (the folder is new)            |

The declared tests need no new accessors and no hunks.

**What the rows would become:**

- **`pluginIcon.test.tsx`:** done, 7 of 7 through the Plugins page. The tests check what the app's icon asset source serves under each row's icon name.
- **`toolwindow.test.tsx`:** partial, 6 of 8. New: tab de-duplication with close-back-to-previous, list selection as fallback target (checked on the clipboard), and Change Repository showing the window while not connected.
- **`ui.test.tsx`:** partial, 5 of 10 plus 1 partial. New: relative dates in list rows, plain dates with pretty formatting off, and the chooser listing the chosen value first and choosing with Enter.
- **`plugins.test.tsx`:** stays partial. About 44 of 61 cases are already proven by existing tests the row doesn't name. I listed them in the notes, with a few marked unverified. New: command-failure reporting that skips plugins that trapped, the plugins search filter, and the update announcement with its two buttons.
- **`imeInput.test.ts`:** 9 of 20 through the terminal's input handler, using the plain test shell. The other 11 test xterm's DOM events and have no native counterpart.
- **Not written, because the view does not exist:** `reviewMode` (7 of its 20 cases are pure model functions), `reviewDiff`, `RemoteBranchDialog`, `GitHubCloneSource`, `settingsPages`, `webViews` and `embedded`. For each, the notes say which view is missing, what it would show, and which native model already holds its logic.

**Behaviour that differs from the webview, with pending tests:**

- **Missing actions:** 7 GitHub actions are not registered (the list is in the notes). One of them, `github.createPullRequest`, means the New Pull Request tab cannot be opened.
- **Update All does nothing:** the update notification's Update All button runs `plugins.updateAll`, which nothing registers. The fix is to register it and have it call `PluginsPage::update_all`, which already reports "Updated N plugins".
- **GitHub settings page:** `vcs.github` is listed in Settings but has no page view, so it shows "This page is not available yet".
- **No view at all:** avatar images, label chips, the multiple-choice reviewer chooser, Markdown link handling, the timeline editor, plugin web panels, plugin file templates and file icons, and a terminal inside Run consoles.

**Most likely to fail on first run:**

- **Date format setting:** the test assumes the key is `general.dateFormat`.
- **Trap test:** it waits about 0.6 s of real time after the trap, because nothing visible shows when the command's answer arrives.
- **Update announcement:** it assumes `refresh_market` on a local index reaches the window through the app's plugin host.
- **IME keys test:** it would catch a real bug if GPUI delivers a printable key to both `key_down` and the input handler, so the text arrives twice.
- **Icon tests:** they assume `gpui_kit::assets::Assets` is usable from the library's tests, as it is from `main.rs`.

**Not reached:** `PreviewEditor.test.tsx`, `ServicesTree.debug.test.tsx` and `selectableText.test.ts`.

The review step timed out, so no second pass was made over these files.
