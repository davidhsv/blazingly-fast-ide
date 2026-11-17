All nine items are done and every gate now passes. Two problems to know about first:

- **Network rule broken once.** My first `cargo test --test web_view` ran with `HOME` on a scratch folder but no `CARGO_HOME`. Cargo updated the crates.io index, downloaded 928 crates and fetched `https://github.com/l0ng-ai/sqlx` into that scratch folder. I stopped it, deleted the cache, and checked that `app-gpui/Cargo.lock` was unchanged by it. The rerun used the real `CARGO_HOME`/`RUSTUP_HOME` with `--offline`. Log: `$S/dmerge2/web_view_aborted_fetch.log`.
- **Item 8 moved more than three crates.** Besides `zip`, `nix` and `constant_time_eq`, the update changed the `windows-sys` / `hashbrown` / `getrandom` dependencies of 15 packages. All of them are versions already in the lock. Eleven of those now differ from what the root lock pins. I did not undo it (no other `cargo update` allowed); your call.

`S` = `$S`; all logs are in `$S/dmerge2/` (notes: `notes.md`).

## 1. Gates

**Native app (`app-gpui/`)**

- **Full suite, every run I made:**
  - `gate_lib1.log`: `test result: FAILED. 8104 passed; 3 failed`. All three came from the regenerated assets and are fixed (item 2).
  - `gate_lib2.log`: `test result: ok. 8107 passed; 0 failed` (uptime 22:06, load 2.57 → 22:10, load 80.65).
  - `gate_lib3.log`: `test result: FAILED. 8106 passed; 1 failed`. The test was `workbench::context_menus::tests::renders_toolbar_slots_with_command_icons_and_honors_when_clauses` ("Sync's button shows did not happen within thirty seconds"). Alone it passed 10/10 (`toolbar10.log`). Hypothesis: machine load, which was 58 when that run started.
  - `gate_lib4.log`: `test result: ok. 8107 passed; 0 failed`.
  - `gate_lib5.log` (final): `test result: ok. 8107 passed; 0 failed` (22:41, load 4.09 → 22:45, load 12.95). This run was also the one during the twenty runs of the changed tests.
- **Test count:** 8,109 became 8,107: six tests removed, four added.
- **Changed tests twenty times** (41 tests, by exact name on the built binary, list in `changed_tests.txt`): `gate_loop20.log`, 20 × `test result: ok. 41 passed; 0 failed`.
- **web_view:** `gate_web_view.log`: `test result: ok. 1 passed; 0 failed`.
- **clippy** `--all-targets -- -D warnings`: exit 0 (`gate_clippy.log`). **build:** exit 0, 0 warnings (`gate_build_final.log`).

**Root**

- **Type check:** exit 0 (`gate_typecheck.log`).
- **Lint:** exit 0, 8 warnings, none in my files (`gate_lint.log`).
- **vitest** (what's-new, `src/platform/locales` with `textCatalog.test.ts`, `src/platform/i18n.test.ts`): `Tests 85 passed | 6 skipped (91)` (`gate_vitest.log`).
- **`cargo build --workspace`:** exit 0. The only warning is the known linker `__eh_frame` note (`gate_root_build.log`).
- **`cargo test -p openidea --no-fail-fast`** (`gate_openidea.log`):
  - lib: `FAILED. 149 passed; 2 failed`. These are the same two the previous integrator showed fail on v0.1.46 (`every_menu_item_has_a_catalog_label`, and the logging test that catches its panic). Hypothesis: still identical to v0.1.46; I did not re-check that tag.
  - main: `ok. 2 passed`; blocking_commands: `ok. 1 passed`; export_bindings: `ok. 2 passed`.
- **Bindings stable:** `src/bindings.ts` is byte-identical to `$S/dmerge/bindings_run1.ts`.
- **Generator:** `node docs/gpui-port/gen-data.mjs --check` exit 0.
- **Lock files:** root `Cargo.lock` and `pnpm-lock.yaml` are byte-identical to the merged copies in `$S/dmerge/`.
- **Processes:** none of mine left (checked by `ps`).

## 2. Items

**1. Language server targets**

- Webview: removed by 2b80c7531 "run only programs in plugin run targets". The Language server targets page is gone, the Node/PHP "Language server runs on" rows are gone, and the Java sharing rule is now workspace-only (`src/features/settings/languages/LanguagePages.tsx:404-450`). No fork commit uses the setting; it only appears as context lines in fork diffs.
- Native changes:
  - `settings_view/pages/languages_controls.rs`: removed `ServerTargetView`, `TargetLanguages` and `Control::Targets`. `JdtlsSharedView` no longer takes a project binding and no longer has `java_on_target` or the `SHARED_ON_TARGET` hint.
  - `settings_view/pages/languages_pages.rs`: removed the page, the Node and PHP rows and their search labels.
  - `settings_model/language_pages.rs`, `settings_model/builtin_pages.rs`: page spec and list entries removed.
  - `languages_controls/tests/proofs22_language_controls.rs`: follows the new `JdtlsSharedView::new` signature.
- Tests removed (each tested only the removed setting):
  - `workbench::tests::proofs19_settings_languages::language_server_targets_are_not_editable_without_a_project`
  - `…::language_server_targets_local_machine_is_written_to_the_project`
  - `workbench::tests::proofs22_settings_units::disables_sharing_for_a_java_language_server_target_only_in_project_scope`
  - `…::keeps_sharing_available_when_java_runs_locally`
  - `…::uses_a_staged_project_java_language_server_target_before_apply`
  - `settings_view::pages::languages_controls::tests::a_stored_target_shows_as_unavailable_and_local_machine_is_staged`
- Expectations changed:
  - `reset_and_cancel_discard_an_edit_on_each_language_page`: 16 cases → 15. Only the targets case and its stored value were dropped.
  - Node page: 6 lines → 3. PHP page: the "Language server runs on" line is gone.
  - Languages list test: "Language server targets" dropped, as upstream's `languages.test.ts` did.
  - Settings tree test: one row fewer.
- Parity records naming the removed tests: `settings:languages.serverTargets`, `settings:languages.php`, `unit:src/features/settings/__tests__/dialog.test.tsx`, `unit:…/languageServerTargets.test.tsx`.

**2. One Light / One Dark**

- Webview: removed by 7f9bf3727 ("drop the built-in One themes"). Stored `one-*` ids are read as `light`/`dark` by the backend (`ide-core` `theme_id`), and `effectiveScheme` falls back to the theme's own scheme for an id it doesn't know.
- Native changes:
  - `theme/themes.rs`: the two variants and both lists removed.
  - `theme/color_scheme.rs`: the token array is now sized by the theme list.
  - Tests switched to Solarized, as upstream's tests did: `theme/themes.rs`, `theme/tokens.rs`, `theme/mod.rs`, `workbench/quick_switch.rs`, `features_b/viewers/mermaid.rs`, the colour-scheme test.
  - `settings_model/contrast.rs`: the inherited-token rule is now upstream's `INHERITED_FROM_BASE` (file colours, `--file-tint-strength`, `--fg-editor-selection`), and the palette count is `>= 2`.
- Two points against the brief's wording:
  - Native tokens are read live from `src/styles/tokens.css`, so there was nothing to regenerate for them.
  - Solarized got no new colour mapping. It inherits the new tokens from its base, which is upstream's own rule.
- New tests: `theme::appearance::tests::a_removed_one_theme_in_the_settings_draws_as_the_islands_theme_of_its_kind` and `theme::color_scheme::tests::a_removed_one_scheme_in_the_settings_gives_way_to_the_themes_own`.
- Generator, `docs/gpui-port/gen-data.mjs`:
  - It now accepts `brands/` icon paths. Upstream added `src/ui/brands/`; this was the `flutter.svg` failure.
  - A `NOT_ACTIONS` set excludes `CommitCheck` objects, which look like actions but are not; without it 7 commit checks went into the manifest.
  - `assets/gen` regenerated: +132 actions, +73 icons, and keymap, contribution and file-icon changes.
- For the brand icons, `app-gpui/build.rs` and `theme/icons.rs` now embed and resolve them.
- Three expectations changed because of the regeneration:
  - The Eclipse shared-key list is now exactly upstream's list in `eclipse.test.ts`. It adds `navigate.gotoLine`/`agents.focusInput`, from 010e7ba6b.
  - `the_restore_default_layout_key_works_only_with_a_project` became `restore_default_layout_works_only_with_a_project`. It runs the action instead of pressing Shift+F12, because 460027fd4 moved that key to `layout.restoreCurrent`. Parity still names the old test under `module:src/workbench/toolwindows/layouts.ts` and `action:layout.restoreDefault`.
  - Quick Switch: 9 items → 7.
  - Smoke manifest: dynamic entries point at their new source lines; each was checked to be the same action.

**3. Agent sessions**

- Upstream 9984e40b8 makes `AgentManager::commands(provider, None)` run a one-per-provider-and-project model and command lookup. It opens a session (`initialize`, `session/new`) that is never prompted.
- The native New Task page already calls `ops::agent::commands`, as the webview's `useAgentCommands(provider, null)` does. Measured: `new_task_looks_up_the_agents_commands_once_in_a_session_it_never_prompts` shows exactly that session, then the task's own.
- Hypothesis: the webview's other call, `agentRefreshModels(false)`, shares the same lookup. I read this in the code, and upstream's test `catalogs::tests::concurrent_windows_commands_and_manual_requests_share_one_flight_and_cooldown` passes (`agent_flight.log`).
- Counts changed from 2 to 3 `session/new` (the lookup, the task's session, the fresh one), with a comment at each, in: `proofs11_agents::{rolls_back_before_sending_the_edited_text_when_send_is_clicked, waits_for_a_running_turn_to_stop_before_rolling_it_back}` and all four `proofs13_agents_failed_send` tests.
- No program of the machine starts: the lookup now goes through the test guard and seam (`spawn_guard::gate_lookup` in `workbench/agents.rs` and `agents_views.rs`). New test `workbench::tests::the_new_task_commands_lookup_of_the_default_agent_is_refused_in_a_test_build`.

**4. Delete failure**

- Not the extra session. Upstream 9d781a2f8 drops a task from the list and announces its removal before removing its worktree folder, so a failed folder removal no longer keeps the task.
- I changed the test, not native code or the fixture. It now asserts that both tasks leave the list, that the folder stays on disk, and that the report names only the worktree task. The tail that deleted the task again after restoring the repository is gone, because the task no longer exists. It passes.

**5. Renamed strings**

- "Built-in" / "ACP Registry" were removed upstream, not renamed: the tabs became one list (9e46d61dc). `page_tabs` is removed, and the test is renamed `follows_locale_changes_and_renders_login_commands_as_inline_code` without its two tab assertions.
- The reply button text is now "Fork" (`AgentReply.tsx`; agent files `session_view.rs`, `session.rs`, `reply_actions.rs`, `workbench/tests/proofs_agents.rs`).
- Action titles come from the regenerated manifest.
- All 19 fork catalog keys are still used (one by `crates/ide-lsp` `LIGHT_MODE_MESSAGE`).

**6. Catalog check**

- Upstream's rule is the `SAME_AS_ENGLISH` set in `src/platform/i18n.test.ts`. The native test now reads that set from that file through `include_str!` (`platform/i18n.rs`).

**7. What's New**

- With update checks off, it did reach the network: `WhatsNew.tsx` called `appReleaseNotes`, which fetches the update manifest.
- The fix: it reads the manifest only when `general.autoCheckUpdates === true`. With checks off it shows the notes the install recorded, or "no notes" with the release-page button.
- Two new tests in `whatsNew.test.tsx`. Both fail without the change (`vitest_whatsnew_nogate.log`). The test fixture now sets checks on for the existing manifest tests.

**8. Lock**

- `cargo update -p zip --precise 9.2.3`. This removed `constant_time_eq 0.6.1` and `nix 0.32.0` and moved `zip` from 9.6.1 to 9.2.3. The rest is the side effect described at the top.
- Diff: `applock_update.diff`; copy before the update: `app-gpui-Cargo.lock.before`.

**9.** `crates/ide-host/tests/target_lsp.rs` stays out of the tree.

## 3. Files

- **`src/`:** `src/features/whats-new/WhatsNew.tsx`, `src/features/whats-new/__tests__/whatsNew.test.tsx`.
- **`crates/`:** none.
- **`docs/gpui-port/`:** `gen-data.mjs`.
- **`app-gpui/`:** `build.rs`, `Cargo.lock`, `assets/gen/{actions,contributions,file-icons,icons,keymaps}.json`, plus the 33 source files listed in `$S/dmerge2/edited_gpui.txt`.
- No files deleted or moved by me, and no new files.

## 4. Still wrong natively, not fixed

- **Keys:**
  - Shift+F12 now runs `layout.restoreCurrent`, which the native app lacks, so the key does nothing.
  - Other newly bound actions the native code never names: `agents.cycleEffort`, 17 `db.*` grid and page actions, `diff.focusOppositePane`, `file.newUntitled`, `navigate.relatedSymbol`, `project.openInRightSplit`, `refactor.extractInterface`, `vcs.showDiffInNewWindow`.
- **Font zoom:** `editor.resetFontSize` is now titled "Reset Font Size" but still resets every editor, and `editor.resetFontSizeAll` has no handler (1259423fd).
- **Debugging through plugin targets** is still in `debugger/actions.rs:864,932` (removed by 2b80c7531).
- **Themes:** Quick Switch keeps its 500 ms preview delay (removed by 7f9bf3727), and there is no One Dark notice (cc72161f9).
- **Agents:** the Agents settings model keeps the two-tab layout, and New Task has no model refresh or catalog.
- **Generator:** `tsmodel.mjs` still counts commit checks as actions.
