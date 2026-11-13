I've written the reconciled notes. Nothing was compiled or run, so the row lists and test outcomes are derived from reading the registry code, not from a run.

**Notes file:** `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/context_menus_reconciled.md` (768 lines, about 55 KB).

**Hunks:** 34 edit sites in 7 files. 20 apply as drafted, 1 is made exact (the `find/view.rs` import), 10 are rewritten and 3 are new. The script `.../scratchpad/ctx/anchors.py` counts every anchor of today's code exactly once; I ran it last at HEAD `479b4c42`, with the stage 1 and 2 edits in the tree.

- **Anchor fixes:** 1c, 1e.3 and 1f; and `code_insight.rs` and `commit_box.rs`, which each held a duplicate anchor.
- **Project view (§3):** goes through stage 3.6's `with_contributed_menu_context` wrapper. That leaves `plugin_project_view_items` and its `window_context` import unused, so the new hunks 3.3/3.4 delete them from `plugin_contributions.rs`.
- **Title bar:** adds the webview's `!menu_expanded` condition.
- **Tab test 1h:** gets its literal row list.
- **`show_menu` split (1i):** the signature is unchanged, so no caller has to change. A grep found the toggles whose state reads the workbench only outside menus.

**Edits to `context_menus.rs`** (copy at `.../scratchpad/ctx/context_menus.rs`, 1781 lines, checked identical with `cmp`):

- `Effects::NONE` is private, so it is now `Effects::default()`.
- The menu group closure now has a typed binding.
- The Cut/Copy/Paste hints are the webview's literal ⌘X/⌘C/⌘V.
- The gutter's two Configure rows are now its own rows, so they no longer show ⌘, beside them.
- `slot_buttons` no longer hides or greys entries.
- The tests use the shared helpers (`start`, `wait_long`, `init_repo`, `start_with_plugins`, `plugin_copy`, `link`), with no secret-store swap and no private helpers.
- The tests assert literal row lists, and there are 2 new tests (one is the ContributedToolbar spec case).

**Rows that changed because actions now exist:** Git › now shows in the editor and tab menus (Commit File…, Add, Show Diff, Rollback…, Push…, Pull…, Fetch, Branches…). The tab menu also gains Add Bookmark.

Some rows the draft assumed would show do not, because their actions live in undeclared drafts:

- **`core_actions.rs`:** the Folding menu.
- **`editor_language.rs`:** Rename…, Generate…, Go To › Test.
- **`editor_core.rs`:** Paste from History….
- **`project_view_actions.rs`:** Open In.

Local History, the Compare rows, Annotate, Evaluate, Emmet, Open in Browser and the New-group rows have no registered action at all.

**Not verified:**

- That nothing registers an action through a constant or table my grep missed.
- Whether `show_menu_later`'s deferred menu is open by the time the tests look.
- That the hello fixture runs in the plugin tests.
- The `workspaceContains` context key flipping when the file appears.

**Deviations I left in place, listed in the notes:**

- A right click on a breakpoint line opens the gutter menu; there is no native breakpoint popover.
- A Markdown preview right click opens the editor menu.
- Rows that are the webview's own rows but action rows here show the action's shortcut and get greyed out by its `when`.

**Anchors in the compiling agent's area:** in `workbench/mod.rs`, the tab strip, the editor area body, the tool window header, the Find panel mounting, `show_menu` and `a_tabs_menu_closes_pins_and_splits`. Also `find/view.rs`, `code_insight.rs`, `project_files.rs`, and the stage 3 draft `plugin_contributions.rs`.

Another process was editing `editor_core.rs`, `editor_language.rs` and `core_actions.rs` at the end of the session. Once those are declared, the literal `ROOT`/`GO_TO` lists in the tests must be updated.
