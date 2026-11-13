The notes file is `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/plugins_all_integration.md` (2,107 lines, about 114 KB). Nothing was compiled or run, so everything about how the code will behave comes from reading it.

The drafted files were reverted once while I worked: at 12:14:14 another process restored all of them to their committed content. They were re-applied, and byte-identical copies (checked with `cmp`) are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/plg/files/app-gpui/src/`. The notes tell the integrator to copy from there if `git status` shows them unmodified.

**Hunks per stage** (every anchor counted once in its file today, recounted after the compiling agent's latest edits)

| Stage | Edit sites | As drafted | Changed                      | New                            |
| ----- | ---------- | ---------- | ---------------------------- | ------------------------------ |
| 1     | 56         | 52         | 1 made exact (a doc comment) | 3 in `plugins.rs` for point b  |
| 2     | 12         | 10         | 1 rewritten                  | 1                              |
| 3     | 44         | 17         | 1 re-anchored, 5 rewritten   | 21 in the debugger for point c |

- **Stage 1:** the draft's three tests for the `mod tests` region of `workbench/mod.rs` moved into `plugins_window.rs`, so no stage adds anything to that region.
- **Stage 2:** the plugin question layer moved after the plugins dialog layer so it draws over Settings. The new hunk adds `program`/`programs_in` to the shared test harness (in stage 1 they would have been dead code).
- **Stage 3:** `if still_open {` now occurs twice in the file-opening task, so that hunk anchors on the `watch_debug_editor` line. The run-request hunk now passes `window`. `menu.rs` no longer edits its import list, which would have gone past 120 columns. `project_files.rs` gets a wrapper instead of a signature change, which would have broken the New popup and the undeclared `project_view_actions.rs` draft.

**Edits to drafted files**

- `welcome_pages.rs`: one compile fix (`Some(view.theme.as_str())`) and a comment wrapped to 120 columns.
- `secret_field.rs`: `painted()` added, `render` draws exactly that, and stronger tests.
- `plugins_window.rs`: a shared test harness for all three stages, tests on the existing helpers with fixture copies, 3 new tests, and `plugins.updateAll` no longer registered (no test can run it).
- `plugin_runtime.rs`: an early return when the window follows no plugin host; the old harness and keychain swap removed; the branch test uses `init_repo`/`git_in`.
- `prompt_dialog.rs`: one new test for point f.
- `plugin_contributions.rs`: the real `debug.pluginStart` path and the new debug test.
- `settings_page.rs`: two lines wrapped.

**Points a–f**

- **a.** The inner text field is never rendered, Copy/Cut are swallowed, and the secret's `Debug` shows only the placeholder. Tests check that a clipboard sentinel survives Copy and Cut and that the painted text is bullets (webview `secrets.tsx:55`).
- **b.** The keychain is chosen in `app-gpui/src/plugins.rs`, not in the crates; the crates are unchanged. It now exists only outside test builds, so tests keep the host's file store under the temp home from the moment the host is built. There is no swap and no window. One test proves it.
- **c.** A debug request runs `debug.pluginStart` (hidden, `when: projectOpen`, as in `debugger/actions.ts:403`). The operation travels with the session, and the plugin hears the exit code. The test enters through the plugin's run request with `debug: true`, on the fake adapter from `debug.rs`'s tests. Partial: the Java debug path is not built.
- **d.** The webview has no link action: nothing under `src/` calls `pluginLink`. What it offers for a local folder is `plugins.installFromDisk`, now tested with a copy of the `hello` fixture.
- **e.** Every fixture is copied into the temp home first. No test uses the network, `$HOME` or the Bazel plugin folder; the `MODULE.bazel` strings are file names written into temp projects.
- **f.** None of the registered actions has a key in `keymaps.json`. The only keys taken are claimed by focused views, and a test shows they do nothing without focus.

**Anchors likely to move** (in `workbench/mod.rs` unless noted)

- `welcome: None,` followed by `};`
- `input::window_blurred(id, cx);`
- `self.note_active_tab(cx);`
- the `agents.openTask:` arm
- `watch_debug_editor`
- `.child(notification_balloons::layer(&self.balloons, cx))` and `.children(self.settings_layer(cx))`
- in `debug.rs`: `DebugDriverEvent::FocusAppWindow => window.activate_window(),`, the `mod tests` header and `FAKE_ADAPTER`

**Not verified**

- The API conformance of the drafted files comes from three parallel read-throughs, which I spot-checked.
- Every test outcome; the file ends with assumptions to check in order, compile risks, likely first failures and the action table.
- The undeclared `context_menus.rs` draft still swaps the secret store and assumes the old `with_menu_context` signature; I left that unresolved.
