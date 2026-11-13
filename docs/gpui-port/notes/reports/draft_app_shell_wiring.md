I wrote both files; nothing was compiled or run. `app_shell.rs` only builds once the integrator adds a new `PromptDialog::message` constructor to `vcs/prompt_dialogs.rs` (exact code is in the drafts file). No file that is already compiled was edited.

## 1. Files written; what `app_shell.rs` holds

- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/app_shell.rs` (new, not yet declared). It is a child module of `workbench`, so it reads `Workbench`'s private fields and calls its private methods.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/app_shell_integration.md`

**Opening a project** (`open_project_with_policy`, `open_project_explicit`) runs `OpenFlow` and carries out every `OpenStep`:

- **Ask:** the "Open Project" question appears in a dialog with "Don't ask again".
- **WriteOpenPolicy:** the answer is written to the global `general.openProjectIn`.
- **RequestWindow:** with no backend router natively, it is answered from the open windows. The window that has the project comes to the front (Focused), a new window opens it (Opened), or this window does (Current).
- **ConfirmLeave:** the same close decision as leaving the project.
- **ResolveWsl** answers None, and **ConnectRemote** is answered as a failure (no remote windows natively).
- **OpenProject:** opens the project, then waits for the result by observing the workbench, without timers.
- **Trust, Attach, ReadCurrentProject, RevealProject** are carried out with `host.trust`, `attach_workspace` and `ProjectTree::reveal_path`.
- **OpenFiles** opens tabs at their positions; **Failed** raises the "Cannot open {name}" notice.

**Opening loose files** (`open_files`) runs `LooseFiles`:

- In a window with a project, they open as tabs.
- Otherwise each file goes to the window whose project contains it, or that already edits it.
- The rest: the first takes this window when it shows nothing, and each other file gets its own window.

**Closing a window, leaving a project, quitting:**

- `window_should_close` replaces `confirm_close`. `request_close` uses `CloseRequests`: a second request while one is decided is refused, and the last window with a project returns to Welcome.
- `request_leave` / `request_close_project` decide leaving with no exit question; `quit` confirms the exit once, then decides each window's close in turn and calls `cx.quit()`.
- A decision that asks nothing settles at once, so the window closes on the request itself. Otherwise it continues in a task and the request is always released at the end.
- The order is: exit confirmation (with "Don't ask again", turning off `general.confirmExit`), then "Process 'x' Is Running" naming the live run sessions (with "Don't ask again", turning off `run.confirmTerminate`), then Save All / Don't Save / Cancel, then Stop and wait, a second Stop after 10 s (which kills), then close.
- With autosave on, documents are saved first and only those that failed are asked about. Following your (b) means departing from the spec here: `closeGuard.ts` asks whenever anything is unsaved, whatever autosave says.

**Dialogs:** questions with a checkbox show as `PromptDialog::message` in a layer of their own, above the Git dialog's, with Cancel added. The unsaved-changes question stays on `window.prompt`, so existing `simulate_prompt_answer("Save All" / "Don't Save" / "Cancel")` calls keep working.

## 2. Drafts file, per section

1. **`prompt_dialogs.rs`:** the `PromptDialog::message` constructor, needed because `AskButton.label` is `&'static str` and the models' labels are translated `String`s.
2. **`mod.rs`:** the `mod app_shell;` line, two new fields replacing `closing`, the close hook calling `app_shell::window_should_close`, deleting `confirm_close`, and drawing the dialog layer.
3. **`windows.rs`:** imports trimmed; `open_project_with_policy` and `open_files` hand over to `app_shell`; `file.openProjectPath` also accepts `{path, newWindow}`; `file.closeProject` goes through the leave guard.
4. **`menu.rs`:** `app.quit` calls `app_shell::quit`.
5. **Tests:**
   - The table covers about 120 existing tests. **Two must change:** `closing_a_window_asks_about_unsaved_changes` (it is replaced) and `projects_open_in_this_window_a_new_one_or_the_one_that_has_them` (four exact edits). Every direct caller of `open_project` / `close_project` passes unchanged, because those stay as unguarded building blocks.
   - New tests, as complete Rust:
     - (a) `asks_before_terminating_a_running_process`
     - (b) `saves_all_on_save`
     - (c) `keeps_the_ordinary_unsaved_change_guard_for_current_window_opens`
     - (d) `dont_ask_again_on_the_open_project_question_stores_the_answer`
     - (e) `turns_a_window_without_a_project_into_light_edit_when_no_light_edit_window_exists`
     - (f) `dont_ask_again_turns_the_setting_off`
   - Shared helpers `app_dialog_title`, `answer_app_dialog` and `shows_app_dialog`.
6. **What is not wired**, with the reason for each (the list in section 3 below).
7. **Assumptions and compile risks** (the most likely ones are in section 4 below).

## 3. Not wired, and why

- **Attach:** the backend can do it (`ide_host::ops::project::attach_workspace`), but the button is not offered. The Project view only shows the project's own folder, and there is no native trust dialog. The steps are carried out anyway, so removing the `ATTACH` filter turns it on.
- **LightEdit tabs:** a native LightEdit window holds a single editor, so there is one window per loose file.
- **Terminate scope:** only live run sessions are named and stopped, matching the webview, which only registers run/debug.
  - The native debugger is not compiled.
  - A running build could be stopped and seen to end (`BuildSlot.running`, `stop_build`); the draft says how to add it.
  - I did not read how the terminal exposes whether a session is alive or how to stop it.
  - There is no "Terminating…" panel with a Kill button.
- **Remote, WSL, prepared custom documents, document windows:** none exist natively.
- **Not wired at all:** `LaunchQueue`, the CLI launcher, `updates.rs`, `workspace_dialog.rs`, `project_actions.rs`, `shell.rs`, `invalidate_caches.rs`, `RemoveFromWorkspace`, `tabs_to_close`, `light_edit_should_close`, `split_dropped`, `light_edit_file_as_project`. Nothing natively drives them, and they are outside opening and closing.

## 4. Most likely to be wrong, or not to compile

- **The new constructor:** `PromptDialog::message` has to be added first.
- **gpui calls I could not check together:**
  - `Window::spawn` and `AsyncWindowContext::update` returning `Result`, while `AsyncApp::update` returns the value directly.
  - `AnyWindowHandle::update` with `&mut AsyncApp` inside `quit`.
  - `futures::future::select` over a pinned receiver and a gpui `Task`.
  - Let-chains in `stopped`, `project_opened` and `quit`.
- **Ownership spots:**
  - Values moved into `window.spawn` inside the `loop` of `decide_close`.
  - `sender` moved in one arm and taken in the other in `project_opened`.
  - `state.clone()` in `request_close`.
- **Timing:** a session's end is only seen after the run driver's `FINISH_DELAY_MS` timer, so the tests must keep advancing the clock (the `long` loop does).
- **Tests:**
  - (a) depends on `/bin/sleep` ending on the first Stop.
  - (e) compares paths that are already canonical.
  - (f) relies on `confirm_exit` being true by default.
  - The multi-window tests answer by emitting the dialog's event rather than clicking.
