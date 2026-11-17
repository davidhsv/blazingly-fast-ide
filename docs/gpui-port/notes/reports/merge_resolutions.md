# Merge of v0.1.46 into gpui: conflict resolutions

Check used for every Rust file: `git diff 37c2a604 HEAD -- <file>` (what the fork and this branch changed) compared with `git diff v0.1.46 -- <file>` after the resolution (what the merged file keeps on top of upstream). Where the two match, every fork and branch change survived and nothing of upstream was dropped.

## crates/ide-core/src/settings.rs

- Fork: `ide: IdeSettings` field, `IdeSettings` / `IdeMode` (Light/Full in Rust, shown as Blaze/Deep in the UI), `auto_check_updates: false` by default. Upstream: `agent_security`, `scopes`, `file_colors`, `database` fields; `McpTransport` / `AgentMcpServer`, `or_default`, `REMOVED_THEMES`, agent display enums; `McpServerSettings` got a hand-written `Default` (derive dropped); `show_whats_new_after_update`, `last_run_version`.
- Kept: all of upstream plus the fork's field, types and `auto_check_updates: false`. `show_whats_new_after_update` keeps upstream's `true`. Verified: the delta of the merged file against v0.1.46 is exactly the fork's delta against 37c2a604.
- Unsure (correcting an earlier note of mine, which guessed it was local): the What's New page fetches release notes through `appReleaseNotes(version, updateSource())` and links upstream's GitHub releases (`src/features/whats-new/WhatsNew.tsx:17,38`), so after an update the fork shows upstream's notes, fetched from the network, although the fork turned automatic update checks off. Setting `show_whats_new_after_update` to `false` as for `auto_check_updates` would follow the fork's intent; I left upstream's `true`. Check: start a new version of the app and see whether What's New opens.

## crates/ide-agent/src/provider.rs

- Branch (acf8cb6cc): `CustomAgent` lost the derived `Debug` for a hand-written one that names environment variables without their values, plus the test `an_agent_file_debugs_its_environment_by_name_only`. Upstream: `CustomAgent` derives `Clone` too; new Qoder tests.
- Kept: upstream's `Clone`, the branch's hand-written `Debug` (no derived `Debug`), both sets of tests. Verified by delta comparison (only difference: the added `Clone`).

## crates/ide-host/src/lib.rs

- Branch (7fedad68f, 011d30941): `Host::trash` override (`TrashFn`, `set_trash`, `trash_override`) so tests never reach the system Trash; `Host::with_ssh_files`. Upstream: `plugin_host` became `RwLock<Weak<_>>` (no `Arc`), new `diagnostics` store field.
- Kept: upstream's `plugin_host` type and `diagnostics` field plus the branch's `trash` field and initialiser. Verified by delta comparison: identical.

## crates/ide-host/src/ops/fs.rs

- Branch (7fedad68f): `delete` sends each path to `Host::trash_override()` when set (tests), else the system Trash; test `a_delete_goes_where_set_trash_says_instead_of_the_system_trash`. Upstream: `delete` keeps the project and, after deleting, tells the checkout backend (`files_deleted`); new `system_open_target` / `inside_any_root` with a test.
- Kept: upstream's flow (project kept, `files_deleted` hook, `Ok(())`) with the branch's trash override in place of the plain `file_ops::delete`; one `tests` module with both tests (imports merged: `use super::*` plus what the branch's test needs).

## crates/ide-host/src/plugins/api.rs

- Fork (cd059d4c6, 1d8796947, 7a95634e5): `plugin_settings` and the new `ide_mode` go through a shared `settings_at` helper; `file_written`, `IDE_MODE_KEY`, `has_task`. Upstream: `plugin_settings` falls back to the project covering a member root (`project_covering`) before the global settings.
- Kept: the fork's `settings_at` shape with upstream's member-root fallback inside it, so both plugin settings and `ide.mode` read the group's project for a member root. Delta comparison: only the moved comment differs.

## crates/ide-host/src/plugins/run_detectors.rs

- Fork (ac68c7c20): a detector's `debugger` / `debugArgs` (plugin API 1.4), carried to the run configuration by `detector_params`. Upstream (74e66a5da): `DEBUG_LANGUAGE` / `DEBUG_ATTACH` / `DEBUG_ATTACH_CONTAINS` param keys for plugin run configurations' debugger (API 1.9), a different path (dynamic configurations, not detectors).
- Kept: both, side by side. Delta comparison: identical.

## crates/ide-host/src/project.rs

- Fork (444106088, 8d0f588fb, 22f2bdbf1, 1d8796947): `lsp.set_servers_enabled(ide.mode.is_full())` at open, settings file applied when changed on disk or written by a plugin, `apply_ide_mode`. Upstream: content roots for language servers through `language_server_roots`, and `set_jdtls_import_roots` (Maven linked projects outside the root).
- Kept: the fork's `set_servers_enabled` line followed by upstream's two calls (upstream's `set_content_roots` replaces the old one both sides had). Delta comparison: identical.
- Unsure (Hypothesis, as before the merge): a `settings.toml` changed on disk re-reads settings and toggles servers but does not run `refresh_index_scope`, so a linked Maven project added that way reaches jdtls only on the next Settings apply or reopen. Check: add a linked project by editing `.openidea/settings.toml` in Deep mode and look for its classes in completion.

## crates/ide-host/tests/plugins.rs

- Fork: `TestWorkspace::ide_mode` field (three struct sites). Upstream: `server_messages` field at the same sites. Kept both. Delta comparison: identical.

## crates/ide-host/tests/remote_plugins.rs

- Branch (b8dfccb1b): the wrapped-run test waits for the file's last line rather than the file's existence (the script creates it before writing). Upstream: the same test now waits until the file's whole content equals the expected text.
- Kept: upstream's version; it fixes the same race (it never reads a half-written file as final). The branch's intent is met, its lines are dropped.

## crates/ide-host/tests/rpc.rs

- Fork (22f2bdbf1): test `project_settings_written_on_disk_apply_to_the_open_project`. Upstream: tests `the_desktop_routes_exactly_what_a_server_of_this_build_answers`, `renames_a_branch_with_the_clients_argument_names`, `handlers_read_the_argument_names_the_client_sends` at the same place; git interleaved the bodies.
- Kept: upstream's three tests whole, and the fork's test whole (its text taken from HEAD) placed before `handlers_read_the_argument_names_the_client_sends`. Delta comparison: identical apart from one blank line's position.

## crates/ide-host/tests/target_lsp.rs (deleted by upstream, changed by the fork)

- Upstream deleted it in 2b80c7531 "refactor(plugins): run only programs in plugin run targets (#2490)": language servers and debuggers run on the host again; the `languageServerTargets` setting, LSP/DAP path mapping and read-only container documents are gone. The test covered exactly that removed feature; its tests did not move anywhere (the feature is gone).
- Fork (8d0f588fb): the test sets `ide.mode = full` before opening the project, because servers do not start in Light (Blaze) mode. That change has nothing left to apply to here; the same opt-in is needed by any upstream test that starts a language server (checked in step 3).
- Result: the file should end deleted (manager: `git rm crates/ide-host/tests/target_lsp.rs`). It is already absent from the working tree: I moved it to `$S/dmerge/removed/target_lsp.rs` before step 3, because it calls APIs upstream removed (`ide_host::ops::doc::doc_open_virtual` no longer exists) and a test target that does not compile stops the whole `cargo test --workspace` run.

## crates/ide-lsp/src/lib.rs

- Fork (444106088, bff4a3721, fb180a058): a second launch gate `servers_enabled` (Blaze mode) next to Restricted Mode, `set_servers_enabled`, a shared `gate_changed`, `launch_block()` everywhere a start is decided; `References::approximate` (text-based usages), `false` for a server's answer. Upstream: `note_arduino_sources` after a references query; `open_compose_files`; `set_execution_allowed` also cancels pending `go list` queries of go.mod (`go_mod_queries.cancel_all()`).
- Kept: upstream's `note_arduino_sources` call with the fork's `approximate: false`; upstream's `open_compose_files`; the fork's gate functions, with upstream's `go_mod_queries.cancel_all()` moved into `gate_changed`'s "a gate closed" branch, so Restricted Mode cancels them as upstream does and Blaze mode does too (those queries only run for a document gopls serves, and Blaze stops gopls). Audit: every server-start check in the merged `ide-lsp` uses `launch_block()` except `go_mod.rs:116` (upstream's, checks Restricted Mode only; it runs only for a document with gopls attached, which Blaze mode never has).

## crates/ide-sdk/src/java.rs

- Branch (7fae37f02): `set_jdk_candidates` limits JDK discovery (a test process must not read the machine's JDKs); discovery ends in a shared `distinct_jdks`. Upstream: JDK homes added by hand (`ADDED`, `set_added_jdks`, Settings › SDKs › Add JDK from Disk…) join the scan.
- Kept: both; the hand-added homes join the result in both paths (normal discovery and the limited one), since they are named explicitly like the candidates. Doc of `set_jdk_candidates` names them.

## crates/ide-vfs/src/watch.rs

- Branch (c8d0b092f): FSEvents stream budget (`StreamBudget`, `SilentStream` test watcher, test `a_stream_past_the_budget_is_refused_until_one_is_given_back`). Upstream (e6e5e4b6c, #1916): removed the `ExhaustedWatcher` test double, no longer used.
- Kept: the branch's test double and test; `ExhaustedWatcher` removed as upstream did. Delta comparison: identical.

## crates/ide-workspace/tests/documents.rs

- Branch (cdf7e9648): test `a_rope_snapshot_is_the_text_at_its_version_and_survives_later_edits`. Upstream: three untitled-buffer tests at the same place. Kept all four. Delta comparison: identical.

## src-tauri/tauri.conf.json

- Fork (ff07083b3): `productName` and window title "Blazingly Fast IDE". Upstream: `version` 0.1.46. Kept the fork's name and upstream's version (as `Cargo.toml` and `package.json` already say after the merge). Delta against v0.1.46: exactly the fork's two lines.

## .oxfmtrc.json

- Branch (0b7fe10b9): ignores `app-gpui/assets/gen` and `docs/gpui-port/INVENTORY.md`. Upstream: ignores `evals/*/golden`. Kept all three.

## plugins/README.md

- Fork (ac68c7c20, cd059d4c6, bff4a3721): `debugger?` / `debugArgs?` of `runDetectors`, "Several per line open a menu" in the `gutter-actions` row, `ide.mode` section, JVM debug section. Upstream: the `formatting` row now names Settings › Editor › Code Style › Formatter › Plugin Formatters. Kept upstream's table with the fork's `gutter-actions` row. Delta against v0.1.46: exactly the fork's lines.

## src/features/build-tools/index.ts

- Fork (3e5a5160a, d1d0aca12): no automatic build import in Blaze mode (`if (ideMode() === "light") return;` before the first-sync loop, re-apply on a mode change); no Maven tool window when every Maven project is unlinked (`model.maven.projects.length`) and a running Maven import is cancelled then. Upstream: an sbt tool window registered from the model (`Registered` type with `"sbt"`), the Maven first sync per workspace member.
- Kept: upstream's `Registered` type and sbt registration, placed before the fork's Blaze-mode return (registering a tool window is not an import, so the sbt window shows in Blaze mode as Maven's and Gradle's do); the fork's Maven condition, cancel and return. sbt has no automatic import upstream (only `sbt.reload`), so Blaze mode needs nothing more for it. Delta against v0.1.46: exactly the fork's lines.

## src/features/code-insight/hierarchy.ts

- Fork (fb180a058): the tree's note comes from `resultNote(result, name)` (adds the "approximate: found by text" note of the text-based call hierarchy to upstream's incomplete note). Upstream: `request` takes the document's path (Python type hierarchy without a server, `pythonHierarchy` imports), look-ahead loading (`prefetchLevels`, `loadChildren(node, ahead)`), `loadRootDetail` for type mode.
- Kept: upstream's code with `resultNote` in place of `incompleteNote` at both call sites. Delta against v0.1.46: only the fork's note change.

## src/features/code-insight/usages.ts

- Fork (fb180a058): `resultNote(refs, word)` in place of `incompleteNote` (text-based usages say they are approximate). Upstream: imports `showPendingHint`. Kept upstream's import plus the fork's `resultNote` import (the fork's two call sites had auto-merged). Delta against v0.1.46: exactly the fork's lines.

## src/features/plugins/languageVisuals.ts

- Fork (ac68c7c20): one gutter marker per line holding all of that line's plugin actions (`LineAction[]`); one action runs on click, several open a menu (Run, Debug…); `eq` compared the actions as JSON. Upstream (4d2263260, d171a946f, e59893652): gutter actions live in the shared icon area and keep their button mounted while edits move their line; `eq` compares plugin, icon, tooltip, command and argument but not the line.
- Kept: the fork's per-line marker and menu on upstream's icon area and line mapping; `eq` now compares the fork's action list element by element with upstream's fields (line left out), so a multi-action button also stays mounted when its line moves. JSON equality would have included the line and remounted the button on every move, defeating upstream's fix.

## src/features/plugins/languageVisuals.test.ts

- Fork: test "puts the actions of one line behind one icon whose click opens a menu". Upstream: tests "keeps Run buttons mounted while edits and delayed refreshes move their lines" and "retains buttons on refresh failures and removes them when the provider confirms removal"; its tests mount `iconAreaExtension` since the buttons render in the shared icon area.
- Kept: all three. The fork's test now mounts `iconAreaExtension` too (without it no button renders after upstream's change) and gained a final step: the same two actions answered on the line an edit moved them to keep the same button element (proves the merged `eq`).

## src/features/run/javaDebug.ts

- Fork (ac68c7c20): a plugin configuration whose run detector declared `debugArgs` with a JVM debugger gets those arguments (`pluginDebugArgs`), otherwise `jdwpLaunch` returns null for it. Upstream: `jar-application` configurations get the JDWP agent before their arguments.
- Kept: both branches (plugin first, then jar-application). Upstream's API 1.9 plugin configurations (`debugLanguage` / `debugAttach` params) carry no `debugArgs`, so `jdwpLaunch` returns null for them and Debug falls through to `debug.startConfig`, where the backend reads those params (`ide-host/src/plugins/api.rs`), as upstream intends.

## src/features/settings/model/behaviorSchema.ts

- Fork (ff07083b3): `autoCheckUpdates: false` default. Upstream: `showWhatsNewAfterUpdate: true`, `lastRunVersion: ""`. Kept the fork's default and upstream's two fields (same choice as in `ide-core/src/settings.rs`).

## src/platform/locales/text/en.json, zh.json

- Fork (fefe3aa65, bff4a3721, fb180a058, 444106088 and others): 19 keys of the IDE mode (Blaze/Deep), its toggle and the text-match notes. Upstream: about a thousand new keys appended after 0.1.40's last key.
- Kept: upstream's keys in upstream's order, then the fork's 19 keys appended at the end, as the catalogs grow by appending (no script sorts them; `textCatalog.test.ts` checks for duplicates and that both catalogs hold every key). No key overlapped. Both files parse as JSON with no duplicate keys (checked with a pairs hook). Delta against v0.1.46: exactly the fork's 19 keys.

## src/bindings.ts (generated)

- Not hand-merged: regenerated by `cargo test -p openidea --test export_bindings` (2 passed) after the Rust side was resolved. No markers. Delta against v0.1.46: exactly the fork's generated additions (`approximate` on two result types, `IdeMode`, `IdeSettings`, the `ide` settings field).

# Step 3: Rust workspace test failures and what was done

- `ide-host` lib `ops::debug::kotlin_tests::a_missing_kotlin_lsp_gives_one_notice_over_two_debugs` (upstream test): caused by the merge. In Light (Blaze) mode the LSP manager lists no servers, so kotlin-lsp reads as "not the Kotlin language server of this project" instead of "not installed". Fixed in the test fixture by the fork's opt-in (`ide.mode = full`, as 8d0f588fb did for the other server tests). Unsure: in Blaze mode a Kotlin Debug shows the "not configured" notice text, which names the wrong reason (kotlin-lsp is fine, the mode keeps it off). Check: Blaze mode, Gradle Kotlin project, Debug a main function: read the notice.
- `ide-host --test plugin_server_commands` `a_command_the_server_leaves_to_the_client_runs_the_plugin_that_claims_it` (upstream test, API 1.9 serverCommands): caused by the merge, the mock server never starts in Light mode. Fixed with the same opt-in.
- `ide-host --test plugins` `a_changed_ide_mode_reaches_plugins_as_a_settings_change` (fork) and `api14_settings_and_input_events_use_other_and_capture_the_editor` (upstream): caused by the merge. Upstream 7d9efa199 (#3642) makes the first `changed_settings` call of a (window, plugin) hand over every current key instead of an empty diff; the fork adds `ide.mode` to every plugin's current keys, so the handover is now `["enabled","ide.mode"]`. Kept both rules as they are (upstream's "every current key", the fork's "`ide.mode` is a key every plugin reads") and changed both tests' expectations to that: the fork's test now sees two events (the handover with `ide.mode`, then `["ide.mode"]` alone for the change), upstream's sees `["enabled","ide.mode"]` first. Unsure: whether the owner prefers the handover to carry only the plugin's own keys (then `changed_settings` would leave `ide.mode` out of the first-contact branch and upstream's test would stay as written). Check: install a plugin subscribed to `settings-changed` and look at its first event.
- Not caused by the merge (code and test identical to v0.1.46: `git diff v0.1.46 -- <paths>` empty):
  - `openidea` lib `menu::tests::every_menu_item_has_a_catalog_label`: `src-tauri/src/menu.rs` at v0.1.46 has the items `navigate.relatedSymbol` and `refactor.extractInterface`, and `src/platform/locales/en.json` at v0.1.46 has no `menu.` label for them (measured by `git show v0.1.46:...` grep: 2 ids in menu.rs, 0 labels in en.json).
  - `openidea` lib `logging::tests::panic_record_names_thread_location_and_message`: its panic hook is process-wide and recorded the concurrent panic of the menu test above (the log shows the menu test's message in its record). Measured: it passes alone (`cargo test -p openidea --lib logging::tests::panic_record`: 1 passed, `step3_logging_alone.log`).
  - `ide-vcs --test repo` `checkout_of_a_branch_held_by_another_worktree_reports_it_and_changes_nothing` (upstream db5a30645, #3484): expects `git checkout -B topic main` to be refused while `topic` is checked out in another worktree; this machine's git 2.39.5 allows it (measured in a scratch repo: "Switched to and reset branch 'topic'", exit 0). Hypothesis: newer git (2.43+) refuses it and the test passes there.
