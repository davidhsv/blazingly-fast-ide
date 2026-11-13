The reconciled notes file is written, and every anchor in it counts exactly once today (`anchors.py`: `NOT ONE: 0`). Nothing was compiled or run.

**Notes file:** `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/app_actions_reconciled.md` (52,644 bytes).

**Hunks:** 20 edit sites in 7 files.

- **As drafted:** 12.
- **Rewritten:** 4.
  - The `mod.rs` test hunk (3i) has a wider anchor.
  - The `chrome.rs` hunk is wrapped to 120 columns.
  - The trust question moves from `RequestWindow` to `OpenProject`, after "Open Project" and the unsaved-changes question, as the webview asks it. Don't Open is a cancelled opening.
  - The `menu.rs` hunk now sits on plugin stage 3.5's line, without the import edit. Stage 3.5 is already in the worktree.
- **New:** 4.
  - 3j: the Welcome-screen test now meets the question and trusts its folder.
  - 5c: `open_result` keeps waiting while a window's question is open, so files opened with a new window are not dropped.
  - 6.1 and 6.2: in `windows.rs`, a window opened for a project asks in that window. This covers New Window, Open in New Window and the command-line folder, which the draft missed.

**Edits to drafted files:** only `workbench/app_actions.rs` (now 1853 lines); the five `app_views/*.rs` are unchanged. Copies are under `$S/app/files/app-gpui/src/`; copy from there if `git status` shows `app_actions.rs` unmodified.

- **Restart:** a test build panics instead of restarting.
- **Trust prompt:** it now opens the project in the same update as the answer, and a new helper `open_project_asking` does the asking for windows opened for a project.
- **Plugins:** losing trust now unloads the project's plugins, as the Tauri adapters do.
- **Trust tests:** they now open undecided temp folders through `file.openProjectPath` and click each answer, plus a new-window case. The New Project test now answers the trust question.
- **New tests:** Invalidate Caches with sentinel files (exactly seven cache folders under the app's home are removed), a refused import, and ⇧F12 doing nothing without a project.

**How the webview asks:** at the start of every opening, in the window the project opens in, whenever the folder is undecided. It does not look at what the project contains. The decision is stored in `<data>/project-trust.json`, which in tests is under the temp home. The webview calls the mode "Restricted Mode", not Safe Mode.

**Seam for existing tests:** `Workbench::open_project` stays ungated, like the backend's `open_project` command. So no shared helper gets a hunk: all of them call it directly. Only two test bodies go through the open flow (`projects_open_in_this_window_…` and `a_window_with_nothing_open_welcomes_…`). They trust their folders first with `host.trust.decide`, the store write that Trust Project starts with.

**What an untrusted project can still do natively that the webview forbids:** database access (test connection, schemas, table details). Nothing in `database/` or `ops::db` checks trust; the integrator needs to decide on a gate. Plugins, language servers, run configurations, the debugger, agents and Git hooks are restricted by the backend itself or by native checks.

**Could not verify:**

- Whether the macOS relaunch keeps `OPENIDEA_GPUI_HOME`. GPUI relaunches through `open`, so it may come back on the user's real directories; the notes give the `ps -E` check.
- The keystroke and popup assumptions in the tests.
- Building the trust question while the window is being constructed.

**Oversized imports:** neither the webview nor the backend has a size limit for a settings file, so none was added. Malformed files are refused with the backend's message.

**Anchors the compiling agent may touch:** in `mod.rs`, `register_actions`, the layer list under the title bar, the file-opening task in `open_project`, and the two tests. Also `menu.rs` `fn state`. Re-run `anchors.py` right before pasting.
