# Cargo-lane backlog (one integrator at a time). Reports are in ./reports/, shared writer rules in ./view_rules.md

Order of value (my judgment): things a user cannot do at all today come first.

1. [DONE 4bee3301] Bookmarks / Structure / TODO / Build tool windows — four_tool_windows.md
2. [DONE fb68300f] Project view file operations: compile project_view/{new_popup,file_dialogs,copy_path_popup}.rs; workbench owns ProjectViewState + FileOperations + Flow; actions project.newFile/newFolder/rename/move/copyElement/delete, editor.copyPathReference — project_view_dialogs.md
3. [DONE c94b2160, also log_branches] Commit: compile ui/{text_area,tooltip}.rs + vcs/{commit_box,log_details,log_filters}.rs; commit box under the changes tree (vcs_commit, amend, commit and push), log details + filters in the Git window — text_area_tooltip.md, git_log_commit_views.md
4. [DONE f1e62e6b] Editor code intelligence: compile editor_lsp/{completion_view,hover_view,signature_view,intentions_view,targets_view}.rs; editor overlay layer; completion on typing + code.completion, hover, parameter info, code.showIntentions, navigate.declaration/implementation — editor_lsp_popups.md, editor_lsp_1.md, editor_lsp_2.md (real ids: code.completion, code.quickDocumentation, navigate.declaration)
5. [RUNNING, with drafted wiring] Settings dialog: compile settings_model/builtin_pages* + settings_view/keymap.rs; page_spec additions (14 listed); file.settings, settings.keymap, Apply through platform::settings::set — settings_view.md, settings_pages_keymap.md
6. [RUNNING, with drafted wiring] Code insight: compile code_insight/; Problems + Hierarchy tool windows, Show Usages popup, find.usages, server status item — code_insight.md
7. Agents: compile agents/*_view.rs; store owner, tool window, session view (needs view tabs or docked only) — agents_views.md, agents_1.md, agents_2.md
8. Foundations: view tabs in the editor area (diff tab, agent session, table editor, previews); tooltips applied across views; TextArea adoption (find bar multi-line, composer); MenuItem checked/header; TextField set_placeholder/select_range/read-only; Cmd-key shortcuts reaching text fields before the keymap
9. Database tool window wiring (sqlite e2e) + a host for dbResults — database_views.md, database.md
10. GitHub PR tool window wiring (needs gh; only unavailable state testable offline) — github_views.md
11. Debug: driver for the debug adapter + tool window — debugger_views.md
12. App shell: replace open/close code with app_models (leave guard, Attach, close guard) — app_models.md
13. Remaining Docker operations in Services; Plugins page; remote-dev, deployment, viewers (markdown preview, image, hex) views

Not yet written at all: Plugins page views, Find Usages tool window view, log branches pane, Breakpoints dialog, Docker dashboards, merge editor view, Welcome Customize page, most title-bar items, status bar items beyond three.

Standing facts: inventory.mjs runs cargo (never run it while an integrator works); commit by file with PATH=$S/bin; push loop to fork gpui; `cargo clean -p openidea-gpui` when target grows (was 150 GB).

14. Conflicts: compile vcs/{conflict_banner,conflicts_dialog,merge_view}.rs; register vcs.resolveConflicts + 30 merge.* actions; conflict notifications — merge_conflicts_views.md (note: merge_view uses ui::tooltip WithTooltip, so compile after ui/tooltip.rs)
15. code_insight/usages_view.rs (with slice 6), debugger/breakpoints_view.rs (with slice 11), vcs/log_branches.rs (with slice 3) — usages_breakpoints_logbranches.md
16. Plugins page + Welcome Customize: compile plugin_models/plugins_page.rs, workbench/welcome_pages.rs (with slice 5, Settings). MUST add TextField::masked before wiring: the secrets field shows the secret in clear. consent::sort_by_risk should put high risk first — plugins_page_welcome_pages.md
17. Status bar and title bar as hosts: compile workbench/{status_bar,status_items,title_bar}.rs; replace the hand-placed rows (selectors unchanged); Customize Toolbar dialog; actions view.toggleStatusBarWidget, toolbar.customize — status_title_bars.md
18. Viewers: compile features_b/viewers/{markdown_view,preview_host,image_view,hex_view}.rs; needs view tabs in the editor area (slice 8); open_file must ask file_kinds first — viewers_views.md
    Slice 9 (Database) also has: database/{store,source_dialog,console_view,table_editor_view}.rs — database_store_console.md (PasswordField there paints bullets; reuse it for the plugin secrets field of slice 16)

## From the advisor (2nd review)

- AT THE NEXT CARGO-FREE GAP: build and launch the real binary on a temp project for ~20 s with an isolated HOME (read which dirs Backend::init uses first; never touch the owner's app data or single-instance lock), grep stderr for `panicked`; run self_check if it has an entry point. Nothing has been launched across ~25 wiring slices; blocks any "works in the app" wording and is a precondition for measurement.
- Regenerate INVENTORY.md (needs cargo) after the gate fix; dry count was 369/3077, 225 model only, 388 partial at 0571ba87.
- Commit checks: show Analyze Code / Check TODO rows disabled with a reason (they never run) in the next VCS slice.
- Measurement fairness list (fix before measuring idle CPU): branches and tags listed on every VCS refresh; TODO feed scans while hidden; publish_context locks + clones the repo list every time (vcsAvailable).
- Wiring drafts by no-cargo writers (trial): drafts/settings_integration.md + workbench/settings_window.rs; drafts/code_insight_integration.md + workbench/code_insight.rs. Compare integrator duration with earlier slices (45-80 min).
- Launch no new view writers until the queue is short.

## State at d5294865 (2026-11-13)

- Real inventory: 396/3077 at parity, 208 model only, 397 partial; suite 5,673; smoke launch of the binary done (D77): alive 25 s, no panic.
- Wiring drafts being written (no cargo): drafts/conflicts_integration.md + workbench/conflicts.rs; drafts/database_integration.md + workbench/database.rs; drafts/chrome_integration.md + workbench/chrome.rs (status/title hosts + tooltip list).
- Next cargo jobs after Settings+code-insight: conflicts, chrome (status/title hosts), database, then agents, view tabs (foundation), viewers, debug driver, app shell.

## State at 6a2b5a7a (2026-11-13, later)

- Inventory 417/3077 at parity, 201 model only, 412 partial; suite 5,833.
- DONE with drafts: Settings + code insight (4219e330). Drafted wiring cut the integrator's time to ~30 + ~50 min. NOTES ARE HYPOTHESES (one hid a secrets-in-clear defect).
- RUNNING (cargo): conflicts + chrome (status/title hosts, tooltips) + the flaky branch test.
- Drafts ready, in order to integrate next: view tabs + viewers (drafts/view_tabs_integration.md), Database (database_integration.md), Agents (agents_integration.md; fake ACP agent in python), Debug (debug_integration.md; fake DAP adapter in python).
- Drafts being written: app shell open/close (app_shell_integration.md), Plugins page full + SecretField + Welcome pages (plugins_welcome_integration.md), GitHub window (github_integration.md).
- Known flake: workbench::tests::the_branch_widget_names_the_branch_and_the_popup_checks_out_another (~1/15 full runs).
- Still not drafted: editor follow-ups (word completion without a server, bulb, next/prev error, snippet session, quick-fix in hover), commit checks runner / disabled rows, log pane toolbar actions + Go to Hash, Docker operations, remote-dev and deployment views and wiring, HTTP client / notebook / vim engine / graphql views, title/status items without data, Find tool window extras, terminal extras.

- Plugins draft (plugins_welcome_integration.md) REVERSES the secrets-hidden fix on purpose once SecretField is in: the integrator must verify the field really paints bullets only (test) before accepting section 2; keep the old behaviour if any doubt.
- GitHub draft uses env var OPENIDEA_GH via set_var in tests (process-wide, unsafe in Rust 2024): integrator decides; real gh must never run in tests.

- App shell draft: MY BRIEF WAS WRONG on (b): closeGuard.ts asks whenever anything is unsaved, whatever autosave says. The draft follows my brief (autosave on = save first, ask only for failures). The integrator must follow the spec (src/features/app/closeGuard.ts + closeGuard.test.ts), not the brief, and fix the draft and test (b).

## State at 086d664f (2026-11-13, after compaction)

- Inventory 435/3077 at parity, 193 model only, 414 partial; suite 5,922. Target cleaned (19 GB).
- RUNNING (cargo, Opus): view tabs + viewers, then Database.
- RUNNING (no cargo, Opus writers, brief = ./wiring_brief.md): workbench/vcs_operations.rs (pull/merge/rebase/stash/rollback/remotes/worktrees/reset/tag/sequencer; drafts/vcs_operations_integration.md), workbench/log_actions.rs (vcs.log.*; drafts/log_actions_integration.md), workbench/shelf.rs (shelf, changelists, patches, commit checks; drafts/shelf_integration.md). The three may each have written a New Tag / Reset / patch dialog: merge duplicates at integration.
- Queue after that: Agents, Debug, app shell, Plugins/Welcome, GitHub, then the three VCS drafts.
- Biggest blank action groups (blank counts): vcs 103, code 36, debug 36, db 34, project 26, agents 23, editor 22, navigate 20, run 20, file 14, docker 13, refactor 13, terminal 11, deployment 11, maven 11, window 10, dotnet 10.
- Draft vcs_operations DONE (cca2968a): 29 actions; not registered: vcs.compareWithBranch. Check first: AsyncFnOnce in start(), GIT_EDITOR for --continue, BranchInfo.operation strings.
- Draft shelf DONE: 21 actions + commit checks runner. CAUTIONS for the integrator: (1) detached-HEAD/CRLF test "depends on the user's git config": must be made independent; (2) deliberate deviation: CRLF check reads the stored file because the source's can never fire (claim unverified: check fsReadText normalisation; if true, 1:1 means the check never fires: decide and record); (3) every commit now runs checks, Analyze waits up to 2 s: existing commit tests slow down; (4) init_repo would commit .openidea/.gitignore: changes shared helper; (5) first on_drag/on_drop in the crate.
- Draft log_actions DONE: 38 actions; not registered: vcs.log.createPatch (use the shelf draft's CreatePatchDialog at integration), vcs.log.openExternal. DEVIATIONS that keep rows partial unless fixed at integration: comparisons open the diff window instead of the "Changes between" list; Go to ref only finds loaded commits; Undo Commit skips the changelist chooser; menu treats no commit as pushed (history-rewriting rows never disabled: must fix, the pushed set is knowable from upstream); log view has no multi-selection (squashCommits/compareVersions depend on it), no collapsed-graph drawing, no toolbar. run_log_operation duplicates branches::run_git_operation; Reset/Tag dialogs duplicate the git_forms ones: merge. Integrate order: vcs_operations first, then shelf, then log_actions.

## State at view tabs + Database (2026-11-13)

- Inventory 490/3077, 179 model only, 420 partial; suite 6,019. D80 written. `inventory.mjs --listed $S/listed` regenerates without cargo (refresh $S/listed/lib.txt after each integration: `cargo test --lib -- --list > lib.txt`).
- RUNNING (cargo): Agents + Debug. Then: app shell + Plugins/Welcome, GitHub, vcs_operations, shelf, log_actions.
- Manifest gap: actions.json lacks when/keyWhen set through helpers → every slice must confine its shortcuts (D80).
- Audit of action conditions (reports/audit_action_conditions.md): dangerous set 0. Latent when ported with registry.handle: db.new, db.jumpToConsole, db.quickDoc, db.modifyTable, db.ddlToClipboard, db.ddlToConsole (need projectOpen/databaseViewFocus), code.generate (keyWhen 21 languages), 9 more db tree actions and 11 deployment actions lose `when`; GitHub actions absent from the manifest (run added in .map). Generator: docs/gpui-port/gen-data.mjs (+ tsmodel.mjs); proposed fix in the report (helper-spread walk after gen-data.mjs:247, throw on "computed", shorthand lookup at :124). APPLY AT A CARGO GAP (regenerates assets the build embeds).

## Advisor, 3rd review (2026-11-13): do at the NEXT CARGO GAP, before the next integrator

1. Tauri side: from the repo root run the tests of crates/ide-language and crates/ide-workspace (and ide-db after the fix) + `cargo check -p <src-tauri package>`; record commands + result in DECISIONS and OWED.md. (Heavy root build: one at a time.)
2. Plugin host: run the ignored plugin test with OPENIDEA_GPUI_TEST_PLUGIN set (see reports/plugin_host_assessment.md for what to point it at).
3. ide-db: redact URL userinfo in DataSource's Debug (I own crates/); test.
4. Apply $S/gen/gen.patch (generator), regenerate, --check, suite.
5. D80 says the probe change "removed the race" but 1 failure in 30 was seen after: run `cargo test --lib -- editor::intelligence` (the module with typing_completes_from_the_server… / an_answer_overtaken…) ~30 times; if it fails, list as known flake and reword D80.
6. Regenerate $S/listed/lib.txt and the inventory BEFORE launching the next integrator (--listed reads the working tree's mod lines).
   Queue order now: app shell → VCS (reconciled notes drafts/vcs_all_integration.md, 3 stages) → Plugins (+ plugin host slice per assessment) → GitHub last.
   Sweep output: $S/sweep/proven.json (merge after spot-checking 10 entries), needs_test.md (hand to integrators as smoke work).

- Plugin host assessment DONE (reports/plugin_host_assessment.md). Slices: 0 link+grant (drafted plugins_window.rs + a link-folder action), 1 runtime bridge (status item, background task, notification buttons, file opens, workspaceContains keys, window events to plugins), 2 prompts + log tab, 3 run requests + gutter + menus, 4 the plugin's settings page, 5 views/web views (Bazel does not need). Prebuilt fixture plugins are checked in (jobs, api14, files-events, path-events, execution, language-features, settings-pages, plugins/official/string-tools). Writer for slices 1-2 launched (workbench/plugin_runtime.rs, drafts/plugin_runtime_integration.md); 3-4 after it.
- Generator patch READY: $S/gen/gen.patch (also docs/gpui-port/notes/generator/). Baseline byte-identical. Changes: actions.json 39 changed + 18 added (GitHub), contributions.json 3. Native impact: only the ten db tree actions (hand guard becomes redundant; category "Database"). After applying: run `cargo test --lib -- platform::actions` (every manifest condition compiles), suite. ALSO: inventory.mjs:246 uses tsmodel.mjs actions() which requires `run` → the inventory omits 18 GitHub actions; fix so the inventory lists them (total rows will grow).

## State at 4824dcaf (2026-11-13)

- Inventory 533/3077, 179 model only, 432 partial (regenerated from a clean `git archive HEAD` export in $S/clean with the working tree's parity.json; do the same whenever an integrator has uncommitted mod lines).
- Sweep merged: 53 done + 6 partial. DOWNGRADED (no proof): context terminalFocus, vcsAvailable, diffEditorFocus (never read by a test); projectOpen, editorFocus, editorTextFocus, mergeEditorFocus, imageViewerFocus (asserted on, never off); action run/stop (button, not registry); ipc db_connect removed (app never issues Connect), db_connected partial. STANDARD from now: a boolean context key is done only when a test reads it on AND off. → next integrator: one "context keys follow focus" smoke test restores ~8 rows + the sweep's 7 B rows. TO WRITE in the next DECISIONS entry (D81).
- VCS drafts reconciled: docs/gpui-port/notes/integration/vcs_all_integration.md (3 stages, 20/45/26 hunks). Unused, do not declare: vcs/log_reset_dialog.rs, vcs/log_tag_dialog.rs (delete at integration). Caveats in reports/reconcile_vcs.md §5 (Rollback Lines in editor not built; vcsLogFocus never published; the reconciler did not read 4 view files in full).
- needs_test.md (sweep list B, 309 rows) = smoke work to hand to integrators: best first: 10 built-in run config types through Edit Configurations (10 rows), Settings page sweep (8 done + 11 partial), backend-event test (6), Services Run All/Restart All/Stop All (4), Find popup commands as actions (4), call/type hierarchy on the fake server (4).
- Draft plugin_runtime DONE: workbench/plugin_runtime.rs (11 e2e), plugin_views/{mod,prompt_dialog,log_tab}.rs; notes integration/plugin_runtime_integration.md. CHECK at integration: (1) Plugins::init installs the macOS keychain store; tests swap in FileSecretStore after init: prove no test can touch the user's keychain (ideally init with the file store when a temp home is used); (2) fixtures copied to temp home before linking; (3) cancel of a background task has no UI (view.backgroundTasks unregistered) → register it or the Tasks popup; (4) not built: file-saved / input-mode-changed / on-language / on-file activation events (ide-extension not a dependency: add it; I own Cargo), Find in Logs. Next plugin draft (slices 3-4): run requests into Run window, gutter actions, plugin menus (Tools slot), plugin settings page + file-saved events.

## State after Agents + Debug (2026-11-13)

- Inventory 600/3077, 168 model only, 455 partial; suite 6,102. D81 written. Generator patch applied (dda15e87).
- Flake MEASURED: editor::intelligence::tests::typing_completes_from_the_server… 2/30 (Part 0 of the running integrator).
- RUNNING (cargo): Part 0 flake, Part 1 crates Debug redactions, Part 2 Tauri-side checks, Part 3 plugin ignored test, Part 4 app shell, Part 5 smoke tests (context keys, run config types, events, settings pages, services/find/hierarchy).
- RUNNING (no cargo): plugin_contributions draft (runs, gutter, menus, settings page, events, Background Tasks popup).
- Next smoke round: debug context keys on AND off (debugSessionPaused/Running/Active/CanRerun), agentSessionFocus off, debug.startConfig/evaluate/forceStepInto/resetFrame/forceRunToCursor, agents addAgent/regenerateReply/forkReply.
- Queue: VCS stage 1-3 (vcs_all_integration.md) → plugins (plugins_window + plugin_runtime + plugin_contributions) → GitHub (+ inventory.mjs action list fix for the 18 GitHub actions).
- Draft plugin_contributions DONE: workbench/{plugin_contributions,background_tasks_view}.rs, plugin_views/settings_page.rs; notes integration/plugin_contributions_integration.md (apply after plugin_runtime). FOR THE INTEGRATOR: (1) the debugger is in the build now: serve a plugin's debug request through it (register debug.pluginStart as the webview does) instead of the drafted "can't be debugged" notification, which the webview does not show; (2) gutter icons are a window-level layer for the focused editor only (the editor's one overlay slot is taken): check clicks do not move the caret or toggle a breakpoint; (3) Cargo dep ide-extension path.
- MISSING NATIVE HOSTS found by this draft → next drafts: the EDITOR CONTEXT MENU (none exists natively; the Bazel submenu and registerMenuGroup 0/25 depend on it), editor tab / Problems / Search Results context menus, the editor.title / mainToolbar / commit.message slots, Run Anything (unregistered), Vim (input-mode-changed).
