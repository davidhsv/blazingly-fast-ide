I finished Parts 0 to 4 and Part 5 a and b; I stopped after 5b, so 5c, 5d and 5e are not started. The `app-gpui` suite is at 6,110 passed, 0 ignored (it was 6,102 passed, 1 ignored), and clippy `-D warnings` is clean. Over five whole-suite runs one run failed one test (details under "Suite runs"), and one earlier suite run hung.

## Part 0: the intermittent completion test

**Cause (measured).** I printed the requests on failure; 1 of 30 runs of the test alone failed. The fake server had already received `initialize`, `initialized` and `didOpen` before the first completion. It then got three `textDocument/completion` requests:

- column 2, triggerKind 1
- column 3, triggerKind 1
- column 4, triggerKind 2 with `.`

There was no column-1 request in that run.

**The product was wrong, not the test.** The native editor asked the server on every keystroke, cancelling the previous request, so once the server was attached the superseded requests still reached it. The webview waits for typing to pause: CodeMirror's `activateOnTypingDelay` is 100 ms and restarts on each keystroke (`node_modules/@codemirror/autocomplete/dist/index.js:379, 1180`). Typing `obj.` quickly therefore asks once, at column 4 with `.`.

- **Hypothesis, not measured:** passing runs see one request because the earlier ones are cancelled before the server attaches. To check: read `CompletionSource::ask` and `ide_host::ops::lsp::lsp_completion`, and log when requests are sent relative to `initialize`.

**Fix** in `app-gpui/src/editor/intelligence.rs`:

- Typing now starts completion only after a 100 ms pause (`ACTIVATE_ON_TYPING_DELAY_MS`), held in a new `activation` field; each keystroke restarts the wait.
- The pending start is dropped by explicit completion, by aborting or closing the lookup, by moving the caret, and by typing something that leaves no word.

**Test added:** `a_word_typed_in_one_go_asks_the_attached_server_once_at_its_end`. It waits until the server has the document, types `obj.`, and checks that nothing is asked while typing goes on, that exactly one request goes out at column 4 with trigger `.`, and that moving the caret during the pause cancels the start.

**Result:** 40 runs of `editor::intelligence::tests` with 0 failures.

**Open difference, not changed:** while a request is already in flight, the next keystroke still re-asks at once and cancels the first. The webview keeps the running request and asks again only after it answers. `an_answer_overtaken_by_a_newer_request_is_dropped` encodes the native behaviour.

## Part 1: secrets in `Debug` output

- **`crates/ide-db/src/source.rs`:** `DataSource` now has a hand-written `Debug`. It prints every field as before, but the URL goes through a private `redacted_url`, which replaces the userinfo password and also any `password=` query value with `<redacted>`. Test: `debug_hides_the_password_of_the_url`.
- **`crates/ide-agent/src/model.rs`:** `AgentProvider` gets a hand-written `Debug`. A new internal `EnvNames` helper prints only the variable names. Test: `debug_names_the_environment_without_its_values`; it also shows that serde still writes the values.
- **`crates/ide-agent/src/provider.rs`:** `CustomAgent` gets the same treatment. Test: `an_agent_file_debugs_its_environment_by_name_only`.
- The public API and serde are unchanged.

**Do any log lines change?** By grep, no. No `tracing` or log call formats these types; the matches were test assertions and panics. `AgentsStore`'s `Debug` already used its own `ProvidersDebug`. My grep keyed on variable names; I did not grep `#[instrument]` or the `Debug` of containers that hold these types (`ops/db.rs:191`, `commands/db.rs:40`, `Command`, `DbOp`, `SourceDialogEvent`).

**The settings page's environment field:** the webview shows the values in clear, so I left the native page as is. In `src/features/agents/BuiltinAgents.tsx`, `formatEnv` writes `NAME=value; …` (lines 96 and 233–236) into a plain `<Input>` (lines 186–192). `format_env` in `app-gpui/src/agents/builtin_agents.rs:50` does the same.

## Part 2: the Tauri app (run from the repository root)

| Command                                                              | Result                                                                                                                                                                                                                                                                                                                     |
| -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo test -p ide-language -p ide-workspace -p ide-db -p ide-agent` | ide_agent `178 passed; 0 failed; 5 ignored` · ide_db `75 passed; 1 ignored` · ide_db integration: clickhouse `0 passed; 1 ignored`, mysql 3, postgres 2, sqlite 13, sqlserver 1 passed · ide_language `107 passed` · ide_workspace `127 passed` · documents 24, file_operations 4, workspace_edits 22 passed · doc-tests 0 |
| `cargo check -p openidea`                                            | `Finished dev profile … in 1m 17s`, 0 warnings. The frontend bundle was not needed.                                                                                                                                                                                                                                        |
| `cargo clippy -p ide-db -p ide-agent --all-targets -- -D warnings`   | `Finished … in 16.25s`, no warnings                                                                                                                                                                                                                                                                                        |

There were no failures to trace back to the base commit.

## Part 3: the ignored plugin test

**Safety check before running:**

- The home is `AppDirs::under(tempdir)`; the plugins folder is `backend.dirs.data/plugins`, and the host's `.cache/components` sits inside it.
- Checksums of the folder copy were the same before and after the run, so the test does not write into the folder it is given.
- **The login keychain:** `Plugins::init` installs the login keychain as the plugin secret store (`plugins.rs:105–113`).
  - `api14` declares a secret, so the new default fixture is `hello`, which declares none.
  - The environment-variable mode can still reach the keychain if `OPENIDEA_GPUI_TEST_COMMAND` names a command that uses secrets (for example `api14`'s `secret`). That mode is left to the person running it.

**Manual run:** a copy of `crates/ide-plugin/tests/fixtures/api14` in the scratchpad with command `global-status` passed:

- `api14 0.1.0: Inactive; UI contributions by point: {"commands": 30, "secrets": 1, "settings": 1}`
- `api14 global-status: Ok(None); the plugin is now Some(Active)`
- `test result: ok. 1 passed`

**Replacement:** the ignored test is now `a_plugin_folder_copied_into_a_temporary_home_loads_and_runs_a_command`, not ignored.

- It copies `fixtures/hello` into a temp folder (skipping `target` and `.git`) and runs `word-count`.
- Output: `hello word-count: Err("no file is open in the editor"); the plugin is now Some(Active)`.
- `OPENIDEA_GPUI_TEST_PLUGIN` and `OPENIDEA_GPUI_TEST_COMMAND` still override, and the named folder is copied too.

## Part 4: open and close flows

**Wired:**

- `mod app_shell` is declared and the close hook calls `app_shell::window_should_close`; `confirm_close` and the `closing` field are gone.
- New workbench fields `close_requests`, `app_dialog` and `leave_decided`; the dialog layer is drawn.
- `PromptDialog::message(Message)` plus a `Message` struct in `vcs/prompt_dialogs.rs`, and `CloseRequests::deciding()`.
- `windows.rs`: opening projects and loose files hands over to `app_shell`; `file.openProjectPath` also accepts `{path, newWindow}`; `file.closeProject` goes through the leave guard.
- `menu.rs`: `app.quit` calls `app_shell::quit`.
- `Content::Viewer` is treated as LightEdit alongside `Content::Editor`; the draft did not compile without this.

**Errors in the draft, fixed:**

- **Autosave skipped the question.** The draft saved first when autosave was on; I removed that path. `closeGuard.ts:101–114` asks whenever anything is unsaved and never reads autosave.
- **Autosave overrode "Don't Save" in two places, now fixed.**
  - The window-blur autosave ran while the question was on screen. It now skips while a close is being decided, as the webview does (`controller.ts:352, 428`).
  - `leave()` still autosaved after the decision. The guarded close and open no longer save there, as in the webview's "the guard decided already…" (`controller.ts:313`). The `leave_decided` flag is cleared after each call so it cannot linger.

**What each flow asks:**

| Flow                                                  | Native                                                                                                                                                                                                                                                                                                             | Webview                                        |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------- |
| Closing a window                                      | Order matches `decideClose`: exit confirmation (last window, no project, `general.confirmExit` on by default, with "Don't ask again") → "Process 'x' Is Running" (`run.confirmTerminate`, with "Don't ask again") → Unsaved Changes (Save All / Don't Save / Cancel) → stop and wait for the processes, then close | same order                                     |
| Closing the last project or LightEdit window          | Leaving is decided without the exit question, then the window shows the Welcome screen                                                                                                                                                                                                                             | `closeGuard.ts:187–194`                        |
| File › Close Project, or opening another project here | The leave decision, with no exit question                                                                                                                                                                                                                                                                          | `confirmLeave`                                 |
| Quit                                                  | Exit confirmed once, then each window's close is decided in turn; a window that stays calls the quit off                                                                                                                                                                                                           | `quitApp`                                      |
| Open Project                                          | Asked with "Don't ask again" even when another window has the project; This Window then brings that window to the front                                                                                                                                                                                            | `windows.ts:161–171`, `projectWindow.ts:26–48` |

**Not covered natively:**

- **Live debug sessions** are neither named nor stopped when a window closes. The webview does both (`src/features/run/index.ts:401–420`), so this is a real gap.
- There is no "Terminating…" panel with a Kill button. The native close stops again after 10 s instead; that this second stop kills the process is a hypothesis.
- No document windows, Attach, remote projects or WSL.
- `unsaved_count` ignores dirty view tabs. `ViewTabs::dirty_tabs` exists, but no view sets `dirty` today (a grep for `dirty: Some` finds nothing), so nothing is missed yet.

**Quit question from the coordinator:** confirmed by mechanism, not by running the old handler. My quit test asserts that a window's own update cannot reach it again (`update` returns `Err`), which is what the old loop would have counted as "agrees to close". The new `quit` runs its loop in a spawned task, outside the window's update.

**Tests added (all in `workbench/mod.rs` tests):**

- `closing_asks_about_unsaved_changes_whatever_autosave_says`: nothing unsaved goes to Welcome without a question; Cancel keeps the project and text; Don't Save leaves the file as on disk with autosave on; Save All writes it. This is drafted test (b), redone to the spec.
- `closing_the_last_window_confirms_the_exit_and_dont_ask_again_turns_it_off`: drafted test (f).
- `closing_with_a_running_process_asks_to_terminate_it_before_the_unsaved_question`: Cancel at either question keeps the `sleep` process running. This is drafted test (a).
- `quit_from_a_window_asks_about_its_unsaved_changes_and_cancel_keeps_it`: the coordinator's case.
- `opening_another_project_here_asks_about_the_unsaved_changes_of_this_one`: also covers `file.closeProject` and `{path, newWindow:false}`. This is drafted test (c).

**Tests changed:**

- `closing_a_window_asks_about_unsaved_changes` was replaced by the first test above. Its last window now returns to Welcome instead of closing (`closeGuard.ts:187–194`).
- `projects_open_in_this_window_a_new_one_or_the_one_that_has_them` now answers by clicking the dialog. It asks first when another window has the project (`windows.ts:161`), and "Don't ask again" stores `general.openProjectIn = "same"`. This covers drafted test (d).

**Not tested:** drafted test (e), LightEdit turning into a project, and the routing of loose files to other windows in `app_shell::open_files`.

**False statements in the notes:**

- "with autosave on, closing saves without asking".
- "the native debugger is not compiled".
- The drafted `PromptDialog::message` with seven positional arguments was replaced by a `Message` struct.
- Every line-number anchor was stale; apart from the missing `Viewer` arm, the draft compiled.

## Part 5

**a. Context keys, each read on and off.**

- New test `context_keys_follow_the_project_the_editor_the_tool_windows_and_the_repository` covers `projectOpening`, `projectOpen`, `editorFocus`, `editorTextFocus`, `editorAreaFocus`, `editorPath`, `activeToolWindow`, `terminalFocus` and `vcsAvailable` (a plain folder, then a git repository).
- Assertions added to existing tests:

| Key                                                               | Test it was added to                                                     |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `imageViewerFocus`                                                | the PNG test                                                             |
| `dbTableEditorFocus`, `dbTableEditorChanges`, `databaseViewFocus` | `edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one` |
| `diffEditorFocus`, `vcsAvailable` on                              | the Show Diff test                                                       |
| `mergeEditorFocus` off, then on                                   | the merge editor test                                                    |

- `agentSessionFocus` was already read both ways.
- Natively `editorAreaFocus` is always equal to `editorFocus`; I did not check `src/workbench/focus.ts` to see whether the webview means the same.

**Bug found and fixed:** Focus Editor (Escape in a tool window) left the keyboard nowhere when the active tab is an image or hex view. `Workbench::with_layout` now focuses the active tab's view, matching the webview's `focusEditor`. It is proven by the PNG test: Escape gives `imageViewerFocus` back.

**b. Run configuration types.** New test `edit_configurations_adds_and_saves_a_configuration_of_every_built_in_type` covers all 13 types: shell, program, npm, gradle, maven, godot, pytest, django-test, go, go-test, application, spring-boot, compound.

- For each, it adds the type through the dialog, types its required fields, saves with OK, and checks the stored kind and the command line the type builds.
- Go Test is stored as kind `go` with `goKind=test`, as the webview does (`types.ts:507`).
- The compound gets the shell configuration as its member.
- The "listed" check reads the store's configurations, not the visible chooser rows.
- It is one loop over a named list of the types, with each assertion naming its type, rather than one block per type.

**c, d, e:** not started.

## Suite runs

- **Five final runs:** 6,110 passed in four. Run 3 (50 s) failed `a_rebase_whose_last_conflict_is_resolved_from_the_dialog_continues_by_itself` with "the rebase goes on did not happen within thirty seconds". It passed 8 of 8 times alone.
- **Earlier failures:**
  - `abort_merge_once_its_conflicts_are_resolved_leaves_the_repository_clean` failed once (a 30 s wait timed out, in a 51 s run); it passed 3 of 3 alone.
  - One suite run after Part 1 hung for more than 10 minutes and I killed it. The output was filtered, so the hanging test is unknown; the rerun passed in 17.86 s.
  - Two runs took about 35 s instead of about 17 s.
- The cause of all of these is unmeasured; the slow runs and timeouts look like machine load, but that is a hypothesis.
- **Final state:** suite `6110 passed; 0 failed; 0 ignored` in 17.79 s; clippy `--all-targets -D warnings` clean. rustfmt was run on my files; `workbench/mod.rs` was formatted after `--check` showed only my hunks.

## Out of scope

- The Open Project body text still mentions attaching projects, but natively the Attach button is removed.
- Merge-editor settings menu: `mergeEditorFocus` stays on while it is open; I did not check whether the webview does the same.
