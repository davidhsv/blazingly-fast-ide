I drafted 13 tests in 5 new files. Nothing was compiled or run, and I didn't touch any existing file. Six of the tests are written to fail until two small fixes land; the notes give both as exact hunks. Version control, the debugger, agents, the search popups, file/project, build, terminal and most editor commands got no tests from me.

**Files** (all under `app-gpui/src/workbench/`, 372 lines in all):

- `tests/proofs5_navigation.rs` (86 lines, 2 tests)
- `tests/proofs5_database.rs` (44 lines, 1 test)
- `tests/proofs5_run.rs` (48 lines, 1 test)
- `tests/proofs5_layout.rs` (55 lines, 2 tests)
- `editor_language/tests/proofs5_editor.rs` (139 lines, 7 tests)

The integrator adds `mod proofs5_database; mod proofs5_layout; mod proofs5_navigation; mod proofs5_run;` next to `    mod proofs_vcs;` in `workbench/mod.rs`, and `mod proofs5_editor;` after `    use std::path::Path;` in the `mod tests` of `editor_language.rs`. All helpers and accessors used already exist.

Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs5.md`

**Of the 198 ids:**

- **With a test: 16.**
  - 5 would be done at once: `bookmarks.toggleFileBookmark`, `db.refresh`, `run.services.openBrowser`, `run.services.hideConfig`, `toolwindow.maximize`.
  - 6 more would be done after the fixes: `view.backgroundTasks`, `editor.copyPath`, `editor.copyPathWithLineNumber`, `code.addCaretNextOccurrence`, `code.cyclicExpandWord`, `code.cyclicExpandWordBackward`.
  - 5 stay partial: `navigate.toggleBookmarkWithMnemonic`, `code.cleanupFile`, `code.quickTypeDefinition`, `editor.showHint`, `code.pasteFromHistory`.
  - One extra row outside the list, `action:open-browser`, also becomes done.
- **(B) with a fix: 8**, all among the tested ids.
  - **Hint fix:** natively the "editor hint" is a notification, or for ⌃G and ⌥/ it is only logged; the webview shows it at the caret. The hunk is in `editor_core.rs` (`show_hint`) and `editor/command_actions.rs`.
  - **Background Tasks fix:** the native action toggles the popup, but in the webview the action only opens it and the status-bar click toggles. The hunk is in `plugin_contributions.rs` and `status_items.rs`.
  - Four existing tests assert the old notification behaviour and must change with the hint fix; the notes name them.
- **(B) read this session, no contained fix: 4.**
  - `plugins.log.wrap`: soft wrap is hard-coded on; the webview starts it from the soft-wrap settings.
  - `editor.pasteHistory.paste` and `editor.pasteHistory.remove`: the `pasteHistoryListFocus` key is never set.
  - `vcs.showLog`: a branch or path argument opens the main log, not a tab of its own.
- **(C): 0 confirmed.** I didn't read any handler that proved a view is missing.
- **Skipped: 62.**
  - 26 are covered by the proofs3/proofs4 drafts, or left out there for Docker, system Trash, full screen or Java.
  - `merge.annotate` is in the merge drafts being compiled.
  - The 20 numbered bookmark ids and 15 `toolwindow.*` ids are covered by dynamic rows that are already done.
- **Not reached: 116.** I classified these only from the gap text their row records; I didn't read their handlers or webview source, so each class is a guess to check. The notes split them into likely (B), likely (C) and a few where a test looks possible: `code.extendSelection`/`code.shrinkSelection`, `file.reloadAllFromDisk`, the two `plugins.*` ids, and the second half of `debug.pluginStart`.

**Most likely to fail first**, besides the six expected failures:

- **Mnemonic chooser:** if the chooser doesn't take the keyboard when it opens, the typed `5` goes into the file instead.
- **Key names:** ⌘⇧' (`cmd-shift-'`) or ⌥F3 (`alt-f3`) may not parse as GPUI keys.
- **Code-action count:** the cleanup test expects exactly one code-action request; anything else asking for code actions on open would break it.
