# Empty backend-call rows, batch 2 (110 commands), judged at each webview call site

Read at HEAD `67c5e41f1` with `git grep`/`git show` only; nothing compiled or run. "Native" means non-test code under `app-gpui/src`. A grep hit inside a doc comment only is not counted as built. For every command the Tauri wrapper in `src-tauri/src/commands/*.rs` was read to find the backend function it delegates to, and `app-gpui/src` was grepped for both the command's name and that function.

## docker_registry_login

- Site `src/features/services/store.ts:221` (`testRegistryLogin`): in the Docker registry dialog of Services the user presses Test Connection; the dialog shows docker's login answer or its error.
- NOT BUILT. Backend `ide_docker::login`. The store model has `DockerRequest::RegistryLogin` (`app-gpui/src/services/docker.rs:803`), but `carry_out_docker` (`app-gpui/src/workbench/services_window.rs:689-738`) does not carry it out (falls to `other => "not carried out in this window"`), and `DockerRequest::RegistryDialog` is not carried out either. Grepped `ide_docker::login`, `registry_password(`, `docker_registry_login`.
- Rules: `ide_docker::login` runs the `docker` CLI (`crates/ide-docker/src/lib.rs:88` `Command::new(binary()?)`): a real test would need Docker or a stand-in script on `PATH`.
- Verdict: not built.

## docker_remove

- Site `src/features/services/store.ts:353`: in Services › Docker the user deletes a container/image/volume/network (after the confirm); the node leaves the tree, or "Could not remove …" is notified.
- NOT BUILT. Backend `ide_docker::remove`. `DockerRequest::Remove` exists in the model (`app-gpui/src/services/docker.rs:789`) but `carry_out_docker` does not carry it out (nor `ConfirmDelete`). Grepped `ide_docker::remove`, `docker::remove`.
- Rules: runs the `docker` CLI.
- Verdict: not built.

## docker_run_env

- Site `src/features/services/runTypes.ts:105` (`withConnection`): starting a Dockerfile / Docker Compose / Docker Image run configuration; the launched command line gets the connection's daemon selection and a `PATH` that finds `docker`.
- NOT BUILT. Backend `ide_host::ops::docker::run_env`. `app-gpui/src/services/run_types.rs:453` `with_connection_env` takes the answer as an argument, but nothing in non-test code calls `run_env` or `with_connection_env` (only its unit test at `run_types.rs:731`). Grepped `docker::run_env`, `run_env(`, `with_connection_env`.
- Verdict: not built.

## docker_save_settings

- Site `src/features/services/store.ts:144` (`saveSettings`): adding/editing/removing a Docker connection or registry in Services (dialogs and Remove confirms); the tree shows the changed connection list, saved to the app data dir.
- Handler BUILT, no UI reaches it. `app-gpui/src/workbench/services_window.rs:702-704` carries out `DockerRequest::SaveSettings` through `ide_docker::save_settings` (same function as the webview). But the only producers are `DockerStore::save_connection/remove_connection/save_registry/remove_registry` (`app-gpui/src/services/docker.rs:1079-1092`), and no non-test code calls them: the connection and registry dialogs and the remove confirms are listed as not carried out (`services_window.rs:735`). Grepped `.save_connection(`, `.remove_connection(`, `.save_registry(`, `.remove_registry(`: only `services/docker.rs:2325,2334` (tests).
- Tests: `workbench::services_actions::tests::proofs11_docker::*` call `save_connection` directly and answer `settings_saved` themselves, never letting the window carry out the save (`app-gpui/src/workbench/services_actions/tests/proofs11_docker.rs`, `add_connection`); they do not cover the write.
- Rules: `ide_docker::save_settings` writes a JSON file in the data dir (the test harness's temp dir); a window test is allowed once a dialog exists.
- Verdict: not built (the write is wired, the user path to it is not).

## docker_set_registry_password

- Site `src/features/services/store.ts:200`: saving a registry with a password in the Docker registry dialog; the password is stored for later logins/pulls.
- NOT BUILT. `DockerRequest::SetRegistryPassword` is produced by `settings_saved` (`app-gpui/src/services/docker.rs:1121`) but `carry_out_docker` does not carry it out, and the registry dialog is not built. Grepped `set_registry_password`.
- Rules: read: `ide_docker::set_registry_password` (`crates/ide-docker/src/connection.rs:209`) writes a passwords file in the app data dir, not the keychain; a window test is allowed once the registry dialog exists.
- Verdict: not built.

## docker_snapshot

- Site `src/features/services/store.ts:240`: opening Services › Docker or refreshing a connection; the tree shows the daemon's containers, images, volumes, networks (or "not available").
- BUILT, NO TEST. `app-gpui/src/workbench/services_window.rs:706-708` carries out `DockerRequest::Snapshot` with `ide_docker::snapshot(&endpoint)`, the same function as the webview. Reached from `services.show` and Refresh (the store issues the read).
- Tests: `workbench::services_actions::tests::proofs11_docker::refresh_docker_reads_the_connection_again_and_the_tree_shows_what_it_found` and the two filter tests take the `Snapshot` request out of the store and answer it themselves (file header: "no Docker program runs"), so no assertion depends on `ide_docker::snapshot`.
- Rules: `ide_docker::snapshot` runs the `docker` CLI (`crates/ide-docker/src/lib.rs:88`); only a stand-in `docker` script on `PATH` would let a test drive it, and whether such a script counts as "a program of the machine" is not settled by the rules. As written, cannot be tested under the rules.
- Verdict: cannot be tested under the rules (built).

## end_process

- Site `src/features/perf/MemoryStatus.tsx:86`: in the Memory status popup the user presses End Process on a child process row; the process tree is killed and the row disappears (or an error is shown).
- NOT BUILT. Backend `memory::end_process_tree` (src-tauri). `app-gpui/src/features_b/perf.rs` has the memory summary model only. Grepped `end_process`, `end_process_tree`.
- Rules: kills a real process of the machine; a test would need to spawn its own child, which the rules forbid only for programs of the machine, so a stand-in is doubtful.
- Verdict: not built.

## env_reload

- Site `src/features/remote-dev/environment.ts:45` (`reloadEnvironment`): action Reload Environment (remote/shell environment); a notification lists the variables that changed and offers Restart.
- NOT BUILT. Backend `ide_host::ops::env::reload`. `app-gpui/src/features_b/remote_dev/environment.rs:38` `reload_notification` formats an answer, and `remote.reloadEnvironment` is listed in `app-gpui/src/workbench/menu_spec.rs:204`, but no handler is registered and nothing calls `ops::env::reload`. Grepped `env::reload`, `ACTION_RELOAD_ENVIRONMENT`, `reload_notification` (only its unit tests call it).
- Rules: hypothesis, not read: `env::reload` re-reads the login shell's environment (runs the user's shell), which a test may not do (check `git grep -n "pub async fn reload" HEAD -- crates/ide-host/src/ops/env.rs`).
- Verdict: not built.

## file_dialog_needs_gtk

- Site `src/platform/dialog.ts:23`: probed once at load on Linux, so that file pickers fall back to the GTK dialog when no portal and no zenity exist; the user sees a working picker.
- DIFFERENT FUNCTION. The native app opens pickers through GPUI `cx.prompt_for_paths` (e.g. `app-gpui/src/workbench/app_actions.rs:789`), with no probe. Hypothesis, not read in GPUI: GPUI's Linux picker uses the XDG portal only, so a desktop without a portal gets no picker where the webview had the GTK fallback. Grepped `needs_gtk`, `portal_reachable`, `zenity`.
- Verdict: not built (webview-only platform plumbing; the loss, if any, is Linux without a portal).

## find_dirs

- Site `src/features/agents/Composer.tsx:415`: typing `@` and a query in the agent composer; the popup lists matching folders among files.
- Site `src/features/search/everywhere/deps.ts:17`: Search Everywhere's Files tab also lists matching directories.
- Composer: NOT BUILT. `app-gpui/src/agents/composer.rs:191` `merge_matches(files, dirs, …)` exists but only its unit test calls it; nothing calls `ops::nav::find_dirs`. Grepped `find_dirs`, `merge_matches`.
- Search Everywhere: NOT BUILT. `app-gpui/src/everywhere/view.rs:349-353` answers `SeFetch::Files` with `ops::nav::find_files` only; there is no directory fetch.
- Verdict: not built.

## find_symbols_named

- Site `src/features/agents/symbolLinks.ts:19`: in an agent reply, code spans naming a symbol become links to its declaration.
- NOT BUILT. Grepped `find_symbols_named`, `symbols_named`.
- Verdict: not built.

## fs_compare_directories

- Site `src/features/vcs/DirectoryCompare.tsx:95`: Compare Directories (project view context menu on two folders); a diff tab lists the differing files, with a progress task.
- NOT BUILT. `app-gpui/src/vcs_diff/compare.rs:345,354` (`compare_directories_start`, `compare_directories`) are pure models called only by their unit tests; `diff.compareDirectories` appears in `app-gpui/src/project_view/context_menu.rs:496` as a delegated item, but no handler is registered and nothing calls `ops::fs::compare_directories`. Grepped `compare_directories`, `COMPARE_DIRECTORIES`.
- Verdict: not built.

## fs_copy_merge

- Sites `src/features/vcs/DirectoryCompare.tsx:164,176`: in the directory compare tab, Copy to Left/Right (and Overwrite after a confirm) copies the selected entry; the row turns equal.
- NOT BUILT. Grepped `copy_merge`.
- Verdict: not built.

## fs_open_with_system

- Site `src/platform/fileManager.ts:87` (`openWithSystemApp`): Open in Associated Application from the project view; the OS opens the file.
- NOT BUILT. Grepped `system_open_target`, `open_with_system`, `open_path`.
- Rules: launches a program of the machine; cannot be tested under the rules.
- Verdict: not built.

## fs_resolve_reference

- Site `src/editor/lsp/links.ts:137`: Cmd-click a file reference in an editor (a path in a string, `#fragment`); the referenced file opens.
- Site `src/features/http-client/fileNavigation.ts:36`: Cmd-click `< ./file` or `>> response` in a `.http` file; that file opens.
- Both NOT BUILT. `app-gpui/src/editor_lsp/links.rs:57` resolves server document links only and is a model; nothing calls `ops::nav::fs_resolve_reference`. Grepped `resolve_reference`.
- Verdict: not built.

## gh_account_repos

- Site `src/features/github/clone/GitHubCloneSource.tsx:41`: Get from Version Control › GitHub; the list of the signed-in account's repositories (or sign-in/retry state).
- NOT BUILT. `app-gpui/src/github/clone.rs` is a model (`CloneSource`) built only by its unit tests (`clone.rs:155,172`); nothing calls `ide_github::account_repositories`. Grepped `account_repositories`, `CloneSource`, `github::clone`.
- Rules: `ide_github::account_repositories` runs `gh`; a test may only use an in-process stand-in, which this call does not take.
- Verdict: not built.

## gh_pr_comment, gh_pr_list, gh_pr_review, gh_pr_review_comments, gh_pr_view

- No webview call site: `git grep -n -e "commands\.ghPr…\b" -e "\"gh_pr_…\""` over `src` (bindings excluded) finds nothing for any of the five.
- The native window does the same work through `gh_api` GraphQL requests (`ide_github::GhApiRequest`, e.g. `app-gpui/src/github/list_panel.rs:245`), never `ide_github::{add_comment, list, submit_review, review_comments, view}`.
- Verdict for each: nothing to cover (no webview site); candidates for "not ported, unused by both" rather than any of the five verdicts.

## gradle_dependencies

- Site `src/features/build-tools/DependencyAnalyzer.tsx:102`: the Dependency Analyzer of a Gradle project, choosing a configuration; the resolved dependency tree is shown.
- NOT BUILT. Grepped `gradle_dependencies`, `gradle_deps`.
- Rules: runs Gradle (a real toolchain); cannot be tested under the rules.
- Verdict: not built.

## gradle_published_catalog

- Site `src/features/build-tools/versionCatalog.ts:140`: resolving a version catalog published as an artifact (`from("group:artifact:ver")`) so `libs.*` accessors complete and navigate.
- NOT BUILT. Grepped `published_catalog`.
- Verdict: not built.

## gtk_pick_paths

- Site `src/platform/dialog.ts:49`: on Linux without a portal, every file/folder picker opens the GTK dialog.
- DIFFERENT FUNCTION: GPUI `prompt_for_paths` (see file_dialog_needs_gtk). Grepped `gtk_pick`, `pick_paths`.
- Verdict: not built (webview-only Linux fallback).

## http_curl

- Site `src/features/http-client/store.ts:178` (`copyAsCurl`): Convert to cURL and Copy on a request in a `.http` file; the clipboard gets the curl command and the status bar says so.
- NOT BUILT. Grepped `ide_http::curl`, `curl` (no hit in non-test `app-gpui/src`).
- Verdict: not built.

## http_environment_file

- Site `src/features/http-client/store.ts:198` (`openEnvironmentFile`): Add Environment to Public / Private File; the env file opens in the editor, created if missing.
- NOT BUILT. `app-gpui/src/workbench/http_client.rs:329` reads environments (`ide_http::environments`) but never calls `ide_http::env::environment_file`. Grepped `environment_file`.
- Verdict: not built.

## http_from_curl

- Site `src/features/http-client/store.ts:192` (`requestFromCurl`): pasting a `curl …` command into a `.http` file turns it into request text.
- NOT BUILT. Grepped `from_curl`.
- Verdict: not built.

## http_variable_definitions

- Site `src/features/http-client/variableNavigation.ts:48`: Cmd-click `{{var}}` in a `.http` file; navigates to its definitions in the env files.
- NOT BUILT. Grepped `variable_definitions`.
- Verdict: not built.

## index_status

- Site `src/features/run/navigate.ts:45` (`findByName`): clicking a file link in run output while connected to an older remote server without `find_files_named`; the inventory size bounds a fuzzy search.
- DIFFERENT FUNCTION. `app-gpui/src/run/driver.rs:1429` always resolves by `ops::nav::find_files_named`. Locally the user loses nothing; against an older remote server the native app has no fallback (hypothesis: native remote windows are not checked here).
- Verdict: not built (the fallback the call exists for).

## inspect_code

- Site `src/features/analysis/inspect.ts:249`: Code › Inspect Code with a scope; the Problems/Inspection results fill as files are analysed.
- NOT BUILT. `app-gpui/src/features_b/analysis/inspect.rs:16` imports the event types for a model only; nothing calls `ops::analysis::inspect_code`. Grepped `inspect_code`, `ops::analysis::inspect`.
- Verdict: not built.

## jdk_download

- Site `src/platform/jdk.ts:65`: Download JDK dialog › Download; progress shows and the new SDK is added.
- NOT BUILT. `java.downloadJdk` is offered as a fix by `app-gpui/src/code_insight/server_status.rs:133` and `app-gpui/src/debugger/actions.rs:320` only when registered (`action_registered`, `app-gpui/src/workbench/code_insight.rs:444`), and no handler for it is registered. Grepped `jdk_download`, `java.downloadJdk`, `ops::tools::`.
- Rules: downloads from the network; cannot be tested under the rules beyond a loopback stand-in, which `ops::tools::jdk_download` does not take (hypothesis, not read).
- Verdict: not built.

## jdk_download_location

- Site `src/features/settings/languages/DownloadJdkDialog.tsx:42`: opening the Download JDK dialog fills the Location field.
- NOT BUILT (no dialog). Grepped `jdk_download_location`.
- Verdict: not built.

## jdk_download_releases

- Site `src/features/settings/languages/DownloadJdkDialog.tsx:51`: choosing a vendor in the Download JDK dialog lists its versions.
- NOT BUILT. Grepped `jdk_download_releases`.
- Rules: network.
- Verdict: not built.

## jdk_download_vendors

- Site `src/features/settings/languages/DownloadJdkDialog.tsx:41` (`commands\n.jdkDownloadVendors()`, split over two lines so the brief's grep misses it): opening the Download JDK dialog fills the vendor list.
- NOT BUILT. Grepped `jdk_download_vendors`.
- Verdict: not built.

## jdtls_required_java

- Site `src/features/settings/languages/components.tsx:161`: Settings › Languages › Java shows which Java version the installed jdtls needs.
- NOT BUILT. Grepped `jdtls_required_java`, `required_java`.
- Verdict: not built.

## jvm_file_presentations

- Site `src/ui/filePresentation.ts:41`: project tree and tabs show JVM files with class/interface/enum icons read from the file.
- NOT BUILT. Grepped `jvm_file_presentations`, `jvm_icons`.
- Verdict: not built.

## light_edit_open_untitled

- Site `src/platform/project.ts:90`: File › New File on the welcome screen switches the window to LightEdit with an untitled editor.
- NOT BUILT. `app-gpui/src/app_models/close_guard.rs:385` and `project_actions.rs:141` know a `light_edit` flag, but nothing calls `open_light_empty`. Grepped `open_light_empty`, `light_edit`, `LightEdit`.
- Verdict: not built.

## log_frontend

- Sites `src/platform/errorLog.ts:25,44`: webview errors are written to the backend's log file.
- DIFFERENT FUNCTION: native code logs through `tracing` directly; there is no webview to forward. Nothing for the user to lose.
- Verdict: not built (no counterpart needed).

## lsp_can_download_sources

- Site `src/editor/core/librarySources.ts:137`: a decompiled library class editor shows a Download Sources / Choose Sources banner when the build tool can fetch sources.
- NOT BUILT. Grepped `lsp_can_download_sources`.
- Verdict: not built.

## lsp_change_signature

- Site `src/editor/refactor/signature.ts:199`: Refactor › Change Signature dialog › Refactor; the method and its callers change.
- NOT BUILT. Grepped `lsp_change_signature`.
- Verdict: not built.

## lsp_code_lens

- Site `src/editor/lsp/serverCodeLens.ts:64`: server code lenses (e.g. "N references", "Run test") above declarations.
- NOT BUILT. `app-gpui/src/editor_lsp/code_lens.rs` is a model; nothing calls `ops::lsp::lsp_code_lens`. Grepped `lsp_code_lens`.
- Verdict: not built.

## lsp_document_links

- Site `src/editor/lsp/links.ts:41`: server document links become Cmd-clickable.
- NOT BUILT. `app-gpui/src/editor_lsp/links.rs:57` takes links as input; nothing fetches them. Grepped `lsp_document_links`.
- Verdict: not built.

## lsp_execute_code_action

- Site `src/editor/lsp/interactiveCommand.ts:81`: an interactive code action (one that asks for a value, e.g. a new file name) is run; the created file opens.
- NOT BUILT. The native app applies actions through `lsp_apply_code_action` (`app-gpui/src/editor/intelligence.rs:1454`), which has no form flow. Grepped `lsp_execute_code_action`.
- Verdict: not built.

## lsp_extract_interface

- Site `src/editor/refactor/extractInterface.ts:141`: Refactor › Extract Interface dialog › Refactor; the interface file is created.
- NOT BUILT. Grepped `lsp_extract_interface`.
- Verdict: not built.

## lsp_extract_interface_info

- Site `src/editor/refactor/extractInterface.ts:100`: opening the Extract Interface dialog lists the members.
- NOT BUILT. Grepped `lsp_extract_interface_info`.
- Verdict: not built.

## lsp_goto_implementation

- Site `src/editor/lsp/navigation.ts:281`: Navigate › Implementation(s) (Cmd-Alt-B / Cmd-Alt-click); the chooser opens with the implementations found first and fills as more stream in, or the only one opens.
- DIFFERENT FUNCTION. `navigate.implementation` is registered at `app-gpui/src/editor/intelligence.rs:1531` and calls `ops::lsp::lsp_goto(.., GotoKind::Implementation)` (`intelligence.rs:829`). `lsp_goto` asks the server and merges plugin implementations like `lsp_goto_implementation` does, but returns one answer: the user loses the streamed partial results (the chooser opens only when the server is done) and the request cannot be cancelled by id.
- Test: `editor::intelligence::tests::cmd_click_goes_to_the_declaration_or_chooses_between_several` runs `navigate.implementation` after the stand-in server answers `textDocument/implementation: []` and asserts at `app-gpui/src/editor/intelligence.rs:2198-2199` `this.hint() == Some("No implementations found")`. That covers only the empty answer; no window test asserts a found implementation opening (the `editor_lsp::navigation::tests::*implementation*` tests drive the flow model, not a window).
- Rules: a stand-in server is allowed.
- Verdict: built, no test (for navigating to a found implementation; the not-found hint is covered).

## lsp_hierarchy_members

- Site `src/editor/refactor/hierarchy.ts:121`: opening Pull Members Up / Push Members Down lists the class's members.
- NOT BUILT. Grepped `lsp_hierarchy_members`.
- Verdict: not built.

## lsp_hierarchy_refactor

- Site `src/editor/refactor/hierarchy.ts:179`: Pull Members Up / Push Members Down › Refactor; the members move.
- NOT BUILT. Grepped `lsp_hierarchy_refactor`.
- Verdict: not built.

## lsp_implementation_locations

- Site `src/editor/navigation/implementationGutter.ts:528`: clicking an implementation gutter icon whose targets are library stubs resolves the real locations before the chooser opens.
- NOT BUILT. Grepped `lsp_implementation_locations`.
- Verdict: not built.

## lsp_implementations

- Site `src/editor/navigation/implementationGutter.ts:746`: the "is implemented / overridden" gutter icons of a file.
- NOT BUILT. Grepped `lsp_implementations`.
- Verdict: not built.

## lsp_inlay_hints

- Site `src/editor/lsp/inlay.ts:219`: parameter-name and type inlay hints in the visible range.
- NOT BUILT. Grepped `lsp_inlay_hints` (hits on "inlay" are the debugger's Run to Cursor inlay).
- Verdict: not built.

## lsp_inline_completion

- Site `src/editor/lsp/inlineCompletion.ts:169`: grey inline (ghost-text) suggestions while typing; Tab accepts.
- NOT BUILT. `app-gpui/src/editor_lsp/inline_completion.rs` is a model (`to_suggestions`); nothing calls `ops::lsp::lsp_inline_completion`. Grepped `lsp_inline_completion`, `InlineCompletion`.
- Verdict: not built.

## lsp_java_move

- Site `src/editor/refactor/javaMove.ts:77`: Refactor › Move (Java) › Refactor; the class/member moves.
- NOT BUILT. Grepped `lsp_java_move`.
- Verdict: not built.

## lsp_java_move_destinations

- Site `src/editor/refactor/javaMove.ts:100`: the Move dialog lists destination packages/classes.
- NOT BUILT. Grepped `lsp_java_move_destinations`.
- Verdict: not built.

## lsp_java_source_types

- Site `src/editor/refactor/javaMove.ts:115`: the Move dialog's class chooser lists the project's source types.
- NOT BUILT. Grepped `lsp_java_source_types`.
- Verdict: not built.

## lsp_library_sources

- Site `src/editor/core/librarySources.ts:42`: Download Sources / Choose Sources on a decompiled class; the editor switches to the source.
- NOT BUILT. Grepped `lsp_library_sources`.
- Rules: downloading sources runs the build tool / network.
- Verdict: not built.

## lsp_prepare_type_hierarchy

- Site `src/features/code-insight/hierarchy.ts:336`: the Type Hierarchy tool window shows the root type's detail (Java: its package).
- NOT BUILT. Native Type Hierarchy uses `ops::lsp::lsp_type_hierarchy` (`app-gpui/src/workbench/code_insight.rs:314`) for the nodes; no root detail is asked. Grepped `lsp_prepare_type_hierarchy`, `prepare`.
- Verdict: not built.

## lsp_resolve_class_file

- Site `src/editor/core/controller.ts:1154`: opening a `.class` file (from the tree or a link) opens its decompiled/source view.
- NOT BUILT. Grepped `lsp_resolve_class_file`, `class_file` (only breakpoint-reason helpers).
- Verdict: not built.

## lsp_resolve_command

- Sites `src/editor/lsp/interactiveCommand.ts:50,67`: an interactive code action asks its questions (input fields) before running.
- NOT BUILT. Grepped `lsp_resolve_command`.
- Verdict: not built.

## lsp_restart_named

- Site `src/features/tools/backend.ts:44`: Settings › Tools, after installing or updating a language server, the servers of that name restart in every open project.
- NOT BUILT. The native app restarts one server from the status item (`ops::lsp::lsp_restart`, `app-gpui/src/workbench/code_insight.rs:1304`), a different user action; there is no Tools page calling `lsp_restart_named`. Grepped `lsp_restart_named`, `restart_servers`, `ops::tools::`.
- Verdict: not built.

## lsp_semantic_tokens

- Site `src/editor/lsp/semantic/index.ts:309`: semantic highlighting from the server.
- NOT BUILT. `app-gpui/src/editor_lsp/semantic.rs:147` maps given tokens (model); nothing fetches them. Grepped `lsp_semantic_tokens`.
- Verdict: not built.

## lsp_signature_info

- Site `src/editor/refactor/signature.ts:120`: opening Change Signature fills the dialog with the method's signature.
- NOT BUILT. Grepped `lsp_signature_info` (`lsp_signature_help` at `intelligence.rs:1262` is Parameter Info, another feature).
- Verdict: not built.

## lsp_virtual_document

- Site `src/platform/virtualDocs.ts:97` (`virtualDocument`): opening a library class (`jdt://…`) or other virtual document in an editor, and reading its lines for previews.
- Site `src/features/debugger/classImports.ts:140`: the debugger's evaluate/import fix reads a library class's source to check it is accessible.
- virtualDocs: NOT BUILT for the editor: no native editor opens a virtual URI (`app-gpui/src/editor_model/virtual_input.rs:18` is called only by its test). DIFFERENT FUNCTION for two other features: Super Method (`app-gpui/src/workbench/refactor_actions.rs:644`) and File Structure supertypes (`app-gpui/src/everywhere/structure_view.rs:196`) open a library class through `ops::doc::doc_open_virtual`, which calls `lsp_virtual_document` inside (`crates/ide-host/src/ops/doc/commands.rs:21`).
- classImports: NOT BUILT. Grepped `virtual_document`, `doc_open_virtual`, `is_virtual_uri`.
- Verdict: not built.

## lsp_workspace_symbols

- Site `src/features/debugger/classImports.ts:148`: debugger import fix finds classes by simple name.
- Site `src/features/debugger/memory/jumpToClass.ts:22`: Memory view › Jump to Source of a class.
- Site `src/features/run/navigate.ts:75`: clicking a stack-trace frame of a library class in run output opens it.
- Site `src/features/search/everywhere/deps.ts:21`: Search Everywhere's Symbols tab adds the servers' workspace symbols.
- All NOT BUILT. Search Everywhere answers `SeFetch::WorkspaceSymbols` with `None` (`app-gpui/src/everywhere/view.rs:383`), so only the index's `find_symbols` results show; nothing else calls `lsp_workspace_symbols`. Grepped `lsp_workspace_symbols`, `workspace_symbols`.
- Verdict: not built.

## maven_dependency_completion

- Site `src/features/build-tools/pomCompletion.ts:30`: completion of groupId/artifactId/version inside a `pom.xml` `<dependency>`.
- NOT BUILT. Grepped `maven_dependency_completion`, `PomCompletion`.
- Verdict: not built.

## maven_dependency_declaration

- Site `src/features/build-tools/DependencyAnalyzer.tsx:133`: Dependency Analyzer › Jump to declaration of a Maven dependency; the `pom.xml` opens at it.
- NOT BUILT (no Dependency Analyzer). Grepped `dependency_declaration`, `dependency_analyzer`.
- Verdict: not built.

## maven_exclude_dependency

- Site `src/features/build-tools/DependencyAnalyzer.tsx:154`: Dependency Analyzer › Exclude on a transitive dependency; an `<exclusion>` is written into `pom.xml`.
- NOT BUILT. Grepped `exclude_dependency`.
- Verdict: not built.

## maven_goto_declaration

- Site `src/features/build-tools/pomNavigation.ts:60`: Cmd-click a dependency/parent/module in `pom.xml`; navigates to its pom or module.
- NOT BUILT. Grepped `maven_goto_declaration`, `PomGoto`.
- Verdict: not built.

## mcp_agent_run_closed

- Site `src/features/run/store.ts:1621` (`closeSession`): closing the run tab of an agent's run (started over MCP); the MCP server forgets that run.
- NOT BUILT. Backend `host.mcp_agent_runs.closed`. Grepped `mcp_agent_runs`, `agent_run_closed`.
- Verdict: not built.

## mcp_regenerate_token

- Site `src/features/settings/pages/mcpServer.tsx:68`: Settings › IDE MCP Server › Regenerate token; the page shows the new token and configs.
- NOT BUILT. The native page (`app-gpui/src/settings_model/mcp_server.rs`, `mcp_server_page`) has the enable toggle and a Custom "Endpoint" entry with no control behind it; nothing touches `ide_mcp::token_path`. Grepped `token_path`, `regenerate_token`, `Regenerate`.
- Verdict: not built.

## mcp_respond

- Site `src/features/mcp/requests.ts:11`: an MCP client's tool call asks for confirmation; the user's Allow/Deny goes back to the server.
- NOT BUILT. Backend `host.mcp_requests.answer`. `app-gpui/src/features_b/mcp.rs` has a doc-commented model only. Grepped `mcp_requests`, `mcp_respond`.
- Verdict: not built.

## mcp_server_status

- Site `src/features/settings/pages/mcpServer.tsx:36`: the MCP Server page shows running/error/port/URL/token and the Copy HTTP/Stdio Config buttons.
- NOT BUILT. `app-gpui/src/settings_model/mcp_server.rs:11,30,40` define the status type and config strings (unit-tested), but nothing in non-test code builds a status or renders it. Grepped `McpServerStatus`, `http_config`, `Copy HTTP Config`.
- Verdict: not built.

## mcp_set_focused_editor

- Site `src/features/mcp/index.ts:67`: when the focused editor or selection changes, the MCP server learns it (agents' "current file/selection" tools answer with it).
- NOT BUILT. The native window reports open editors (`host.mcp_reports.set_open_editors`, `app-gpui/src/workbench/plugin_runtime.rs:1517`) and problems (`app-gpui/src/workbench/code_insight.rs:603`), but never `set_focused_editor`; `app-gpui/src/platform_more/remote.rs:469-474` only lists the command name for remote forwarding. Grepped `set_focused_editor`, `mcp_reports`.
- Verdict: not built.

## mcp_ui_ready

- Sites `src/features/mcp/index.ts:18,23`: a project window tells the MCP server it can show confirmations (ready on load, not ready on unload).
- NOT BUILT. Grepped `set_ready`, `mcp_ui_ready` (doc comment in `app-gpui/src/features_b/mcp.rs:35` only).
- Verdict: not built.

## nav_index_definitions

- Site `src/editor/lsp/syntaxFallback.ts:149`: Go to Declaration when the server finds nothing falls back to the project index's definitions of the name.
- NOT BUILT. The native fallback (`app-gpui/src/editor/intelligence.rs:875` `syntax_fallback`) asks `ops::syntax::syntax_locals` only (local variables); no index lookup. Grepped `nav_index_definitions`, `index_definitions`.
- Verdict: not built.

## nav_inheritance

- Site `src/editor/navigation/inheritanceGraph.ts:33`: Python files get subclass/override gutter icons and hierarchy from the project index (Python servers answer no type hierarchy).
- NOT BUILT. Grepped `nav_inheritance`, `ClassInheritance`.
- Verdict: not built.

## nav_resolve_module

- Site `src/editor/lsp/navigation.ts:771`: Go to Declaration on a dotted module path opens its file.
- Sites `src/editor/navigation/implementationGutter.ts:788`, `src/editor/navigation/inheritanceGraph.ts:192`: Python inheritance gutter resolves base names through imports.
- All NOT BUILT. Grepped `nav_resolve_module`.
- Verdict: not built.

## notebook_cell_docs

- Site `src/features/notebook/session.ts:11` (used by `syncCellDocs`, `session.ts:176`): code cells of a notebook get language-server documents, so completion/diagnostics work in them.
- NOT BUILT. The native notebook (`app-gpui/src/workbench/notebook.rs`) uses other `ops::notebook` calls; nothing calls `cell_docs`. Grepped `cell_docs`, `NotebookCellDoc`.
- Verdict: not built.

## path_completions

- Site `src/editor/lsp/fileReferences.ts:66`: completion of file paths inside string literals / references in the editor.
- Sites `src/features/agents/Composer.tsx:394,428`: `@` with a path query in the agent composer lists the typed directory; `@` alone lists the top-level entries.
- All NOT BUILT. Grepped `path_completions`, `PathCompletion`.
- Verdict: not built.

## plugin_answer_commit

- Site `src/features/plugins/backend.ts:297`: a plugin asks for the Commit view's selection (files, message); the webview answers with what the user has selected.
- DIFFERENT behaviour, same backend function. `app-gpui/src/workbench/plugin_runtime.rs:528-535` answers every `PluginEvent::CommitRequested` with `host.answer_commit(request_id, CommitAnswer::NoRepository)` (comment: "The Commit view does not hand its selection to plugins"). The user loses every plugin feature that reads the commit selection (the plugin is told there is no repository).
- Tests: none found (`grep -i -E "commit_request|answer_commit|no_repository|plugin.*commit" rust_tests.txt` gives only unrelated tests).
- Rules: a plugin test fixture with an in-process plugin host would be allowed.
- Verdict: built, no test (and only the "no repository" answer is built).

## plugin_answer_run_target

- Site `src/features/plugins/backend.ts:337` (caller `src/features/plugins/runTargets.ts:43`): a plugin offers a run target (e.g. a remote host) and the user answers Use / Don't use in the notification; settings are updated.
- NOT BUILT. Grepped `run_targets::answer`, `answer_run_target`.
- Verdict: not built.

## plugin_choose_program

- Site `src/features/plugins/backend.ts:212` (caller `src/features/plugins/PluginsPage.tsx:533`): Plugins page › a plugin's program capability › Choose…; the chosen executable is granted for global or project scope.
- NOT BUILT. `app-gpui/src/workbench/plugins_window.rs` handles `SetCapability` (`:455`) but has no `choose_program`. Grepped `choose_program`.
- Verdict: not built.

## plugin_color_presentations

- Site `src/features/color-preview/pluginColors.ts:75`: picking a colour in the colour preview popup of a plugin-reported colour rewrites it in the plugin's format.
- NOT BUILT. `app-gpui/src/features_b/color_preview/plugin_colors.rs:119` is a model with a doc comment; no non-test code outside the file uses it and nothing calls `ops::lsp::plugin_color_presentations`. Grepped `color_presentations`, `plugin_colors::`.
- Verdict: not built.

## plugin_commit_checks

- Site `src/features/plugins/backend.ts:299` (caller `src/features/plugins/uiPoints.tsx:223`): before a commit, plugins' commit checks run and their problems show in the commit dialog.
- NOT BUILT. The native commit (`app-gpui/src/workbench/commit.rs:286`) runs `shelf::run_commit_checks`, the IDE's own checks; nothing calls `ide_host::plugins::commit::commit_checks`. Grepped `commit_checks`.
- Verdict: not built.

## plugin_content_tab_action

- Site `src/features/plugins/backend.ts:291` (caller `src/features/plugins/views/contentTab.ts:303`): a button in a plugin's content tab (e.g. Rerun, Close) is sent to the plugin.
- NOT BUILT. Grepped `content_tab_action`.
- Verdict: not built.

## plugin_debug_attach_target

- Site `src/features/run/pluginAttach.ts:24`: running a plugin's "debug attach" configuration starts it and attaches the debugger to the process the plugin names.
- NOT BUILT. Grepped `debug_attach_target`.
- Verdict: not built.

## plugin_decoration_sets

- Site `src/features/plugins/decorations.ts:242`: plugins' file decorations (badges/colours in the project tree and tabs) load on start and project change.
- NOT BUILT. `app-gpui/src/plugin_models/decorations.rs:111` is a model with a doc comment; not used outside its file. Grepped `decoration_sets`, `plugin_models::decorations`.
- Verdict: not built.

## plugin_endpoints

- Site `src/features/plugins/backend.ts:338` (caller `src/features/plugins/points.tsx:164`): the Endpoints tool window lists the HTTP endpoints plugins report.
- NOT BUILT. Grepped `plugin_endpoints`, `index_ops::`.
- Verdict: not built.

## plugin_installed_intellij_themes

- Site `src/features/plugins/backend.ts:327` (caller `src/features/plugins/IntellijThemesDialog.tsx:44`): Plugins › Import IntelliJ Theme › from an installed JetBrains IDE lists the themes found.
- NOT BUILT. `app-gpui/src/workbench/plugins_window.rs:479-482` logs `ImportInstalledIntellij` / `ImportIntellijFile` as "not available in this window"; `plugin_models/plugins_page.rs:3186` `set_themes` is called only by tests.
- Rules: read: `intellij::installed_themes` (`crates/ide-host/src/plugins/intellij.rs:80`) scans `local_ide_dirs()`, the JetBrains settings folders of this user (home); cannot be tested under the rules unless that directory list is injectable (it is not at this call).
- Verdict: not built.

## plugin_line_markers

- Site `src/features/plugins/lineMarkerGutter.ts:160`: plugin gutter markers (icons with actions) in an editor.
- NOT BUILT. `app-gpui/src/plugin_models/line_markers.rs:128` (`LineMarkerGutter`) is not used outside its file. Grepped `plugin_line_markers`, `LineMarkerGutter`.
- Verdict: not built.

## plugin_navigation_files

- Site `src/features/plugins/navigation.ts:131`: on project open, discovery of the files plugins index for navigation.
- NOT BUILT. `app-gpui/src/plugin_models/navigation.rs:44` (`NavigationIndexer`) is not used outside its file. Grepped `plugin_navigation_files`, `NavigationIndexer`.
- Verdict: not built.

## plugin_navigation_goto

- Site `src/features/plugins/navigation.ts:199`: Cmd-click in a file a plugin indexes (e.g. a MyBatis mapper) goes to the plugin's target.
- DIFFERENT FUNCTION, partly. `ops::lsp::lsp_goto` for Go to Declaration asks `plugins.navigation_at` itself (`crates/ide-host/src/ops/lsp.rs`, the block under "⌘B on a declaration shows its usages") and the native editor calls `lsp_goto` (`app-gpui/src/editor/intelligence.rs:829`). Hypothesis: that reaches plugin targets only when the plugin index has been built, and nothing native builds it (see `plugin_navigation_index`), so the user gets no plugin targets; check by a window test with a plugin fixture that runs `navigate.declaration` on an indexed name.
- Verdict: not built.

## plugin_navigation_index

- Site `src/features/plugins/navigation.ts:82`: after a plugin-indexed file changes (or on discovery), it is re-indexed.
- NOT BUILT. Grepped `plugin_navigation_index`.
- Verdict: not built.

## plugin_navigation_invalidate

- Sites `src/features/plugins/navigation.ts:81,165`: invalidate the changed files' index entries before re-indexing (81), or all of them when plugins change (165).
- NOT BUILT. Grepped `plugin_navigation_invalidate`.
- Verdict: not built.

## plugin_navigation_markers

- Site `src/features/plugins/markerStore.ts:33`: navigation gutter markers of plugin-indexed files in visible editors.
- NOT BUILT. `app-gpui/src/plugin_models/marker_store.rs` is not used outside its file. Grepped `plugin_navigation_markers`, `marker_store::`.
- Verdict: not built.

## plugin_parse_test_results

- Site `src/features/plugins/backend.ts:281` (caller `src/features/plugins/runtimeUi.tsx:707`): a plugin's test runner output is parsed into the test tree of the Run window.
- NOT BUILT. Grepped `parse_test_results`.
- Verdict: not built.

## plugin_program

- Site `src/features/plugins/backend.ts:217` (caller `src/features/plugins/PluginsPage.tsx:504`): Plugins page shows the resolved path of a plugin's program capability.
- NOT BUILT. Grepped `program_for_project`. (`host.set_program_path` at `app-gpui/src/workbench/plugins_window.rs:1025` sets the host's own program path, not this.)
- Verdict: not built.

## plugin_read_intellij_theme

- Site `src/features/plugins/backend.ts:326` (caller `src/features/plugins/intellijImport.ts:23`): Import IntelliJ Theme from a `.jar`/`.zip`/`.json` file.
- NOT BUILT (refused, `app-gpui/src/workbench/plugins_window.rs:479-482`). Grepped `read_source`, `intellij::`.
- Verdict: not built.

## plugin_revoke_net_host

- Site `src/features/plugins/backend.ts:224` (caller `src/features/plugins/store.ts:197`): Plugins page › a plugin's network permission › revoke one host.
- NOT BUILT. Grepped `revoke_net_host`.
- Verdict: not built.

## plugin_select_tests

- Site `src/features/plugins/backend.ts:280` (caller `src/features/plugins/runtimeUi.tsx:711`): rerunning selected/failed tests of a plugin's test configuration asks the plugin for the filter.
- NOT BUILT. Grepped `select_tests`.
- Verdict: not built.

## plugin_service_detail

- Site `src/features/plugins/backend.ts:335` (caller `src/features/plugins/views/ServicePane.tsx:43`): selecting a plugin service in Services shows its detail tabs.
- NOT BUILT. Grepped `service_detail`, `views::service`.
- Verdict: not built.

## plugin_service_log

- Site `src/features/plugins/backend.ts:343` (caller `src/features/plugins/views/ServicePane.tsx:96`): a plugin service's Log tab streams its lines.
- NOT BUILT. Grepped `service_log`.
- Verdict: not built.

## plugin_set_theme_frame

- Site `src/features/plugins/backend.ts:377` (caller `src/features/plugins/bridge.ts:193`): the active plugin theme's frame colour is reported so the next start paints the window frame in it.
- NOT BUILT. `app-gpui/src/plugin_models/startup_theme.rs:146` is a model not used outside its file. Grepped `set_theme_frame`, `startup_theme::`.
- Verdict: not built.

## plugin_stage_package

- Site `src/features/plugins/backend.ts:328` (caller `src/features/plugins/intellijImport.ts:24`): an imported IntelliJ theme is staged as a plugin package before install.
- NOT BUILT. Grepped `stage_package`.
- Verdict: not built.

## plugin_terminal_session_close

- Site `src/features/plugins/backend.ts:283` (caller `src/features/plugins/runtimeUi.tsx:743`): closing (stopping) a terminal a plugin opened tells the plugin its session ended.
- NOT BUILT. Plugin terminals are refused: `PluginEvent::OpenTerminal` is logged "which this window cannot run" (`app-gpui/src/workbench/plugin_runtime.rs:537-539`). `terminal.closeSession` (`app-gpui/src/workbench/terminal_actions.rs:103,185`) closes the IDE's own terminal tab, not a plugin session. Grepped `close_terminal_session`, `spawn_terminal_session`.
- Verdict: not built.

## plugin_terminal_session_spawn

- Site `src/features/plugins/backend.ts:286` (caller `src/features/plugins/runtimeUi.tsx:754`): a plugin opens a terminal running its operation; a Terminal tab shows it.
- NOT BUILT (refused, see above).
- Rules: spawns a shell/program of the machine.
- Verdict: not built.

## plugin_web_page

- Site `src/features/plugins/backend.ts:349` (caller `src/features/plugins/views/WebView.tsx:183`): a plugin's web view tool window loads its page.
- NOT BUILT. Grepped `web_page`.
- Verdict: not built.

## plugin_web_pty_spawn

- Site `src/features/plugins/views/ptyBackend.ts:12`: a terminal inside a plugin web view (frame terminals, log tabs) runs the plugin's granted command.
- NOT BUILT. Grepped `spawn_web_pty`.
- Rules: spawns a program of the machine.
- Verdict: not built.

## plugin_web_read_file

- Site `src/features/plugins/backend.ts:364`: a plugin web view's Open File picks a file and reads it (caller `WebView.tsx:390`).
- Site `src/features/plugins/backend.ts:367`: a plugin custom editor reads its file's bytes (caller `src/features/plugins/views/customEditor.tsx:148`).
- Both NOT BUILT. Grepped `web_read_file`, `MAX_WEB_VIEW_FILE`.
- Verdict: not built.

## plugin_web_save_file

- Site `src/features/plugins/backend.ts:356` (caller `WebView.tsx:395`): a plugin web view's Save File writes the bytes to the chosen path.
- Site `src/features/viewers/markdown/mermaidTools.ts:183`: Markdown preview › a Mermaid diagram › Save as PNG; the PNG is written and "Diagram saved" is notified.
- Both NOT BUILT. The native Markdown viewer draws a Mermaid fence as a code block (`app-gpui/src/features_b/viewers/markdown_view.rs:529`), with no diagram or Save as PNG. Grepped `web_save_file`, `mermaid`.
- Verdict: not built.

## Summary

Counts (110 commands): every site covered 0; some covered 0; built, no test 2 (`lsp_goto_implementation` through `lsp_goto`, `plugin_answer_commit` answering "no repository" only); cannot be tested under the rules 1 (`docker_snapshot`, built); not built 102; no webview call site at all 5 (`gh_pr_comment`, `gh_pr_list`, `gh_pr_review`, `gh_pr_review_comments`, `gh_pr_view`).

Commands with every site covered (claimable now): none.

Built with no test, where one window test would do, by the module with the harness:

- `editor::intelligence::tests` (stand-in language server, `f.server.answer`, `f.run(cx, "navigate.implementation")`): `lsp_goto_implementation`. One test answering `textDocument/implementation` with two locations and asserting the chooser lists both (and one location opening the file) would cover it; the row should say it is reached through `lsp_goto` without streaming.
- `workbench` plugin tests (`plugins_window::test_support`, an in-process plugin host): `plugin_answer_commit`. A test would only prove the plugin hears `NoRepository`; the webview's behaviour (handing over the Commit view's selection) is not built, so the row stays open after such a test.

Built but not testable as is: `docker_snapshot` (runs the `docker` CLI; the existing `proofs11_docker` tests answer the store's read themselves). Wired but unreachable: `docker_save_settings` (the window carries out the save, no UI asks for one until the connection/registry dialogs exist).

Not built, by feature (what the webview user sees):

- Docker (Services): registry Test Connection (`docker_registry_login`), delete container/image/volume/network (`docker_remove`), Docker run configurations' environment (`docker_run_env`), saving connections/registries (`docker_save_settings`, handler only), registry password (`docker_set_registry_password`).
- Search and navigation: folders in Search Everywhere and the agent composer (`find_dirs`), symbol links in agent replies (`find_symbols_named`), file references Cmd-click in editors and `.http` files (`fs_resolve_reference`), index fallback of Go to Declaration (`nav_index_definitions`), Python inheritance gutter and module navigation (`nav_inheritance`, `nav_resolve_module`), path completion in strings and the composer (`path_completions`), workspace symbols in Search Everywhere / stack traces / debugger (`lsp_workspace_symbols`), old-remote fallback of run-output file links (`index_status`).
- Diff: Compare Directories and its Copy actions (`fs_compare_directories`, `fs_copy_merge`).
- Files and platform: Open in Associated Application (`fs_open_with_system`), Linux GTK picker fallback (`file_dialog_needs_gtk`, `gtk_pick_paths`; GPUI's own picker instead), JVM file icons (`jvm_file_presentations`), New File from the welcome screen in LightEdit (`light_edit_open_untitled`), webview error log forwarding (`log_frontend`, no counterpart needed), End Process in the Memory popup (`end_process`), Reload Environment (`env_reload`).
- GitHub: Get from Version Control › GitHub repository list (`gh_account_repos`).
- Build tools: Gradle Dependency Analyzer (`gradle_dependencies`), published version catalogs (`gradle_published_catalog`), pom.xml completion / Cmd-click / Dependency Analyzer jump and exclude (`maven_dependency_completion`, `maven_goto_declaration`, `maven_dependency_declaration`, `maven_exclude_dependency`).
- HTTP client: Convert to cURL and Copy (`http_curl`), paste curl (`http_from_curl`), Add Environment to Public/Private File (`http_environment_file`), `{{var}}` navigation (`http_variable_definitions`).
- Code analysis: Inspect Code (`inspect_code`).
- JDK and Java tooling: Download JDK dialog (`jdk_download`, `jdk_download_location`, `jdk_download_releases`, `jdk_download_vendors`; `java.downloadJdk` has no handler), jdtls required Java (`jdtls_required_java`), restart servers after a tool update (`lsp_restart_named`).
- Editor language features: code lens (`lsp_code_lens`), document links (`lsp_document_links`), inlay hints (`lsp_inlay_hints`), inline completion (`lsp_inline_completion`), semantic tokens (`lsp_semantic_tokens`), implementation gutter (`lsp_implementations`, `lsp_implementation_locations`), Type Hierarchy root detail (`lsp_prepare_type_hierarchy`), interactive code actions (`lsp_resolve_command`, `lsp_execute_code_action`).
- Library classes: decompiled-class sources banner and download (`lsp_can_download_sources`, `lsp_library_sources`), opening `.class` files (`lsp_resolve_class_file`), opening library classes in an editor and the debugger import check (`lsp_virtual_document`; Super Method and File Structure reach it through `doc_open_virtual`).
- Refactorings: Change Signature (`lsp_signature_info`, `lsp_change_signature`), Extract Interface (`lsp_extract_interface_info`, `lsp_extract_interface`), Pull Up / Push Down (`lsp_hierarchy_members`, `lsp_hierarchy_refactor`), Java Move (`lsp_java_move`, `lsp_java_move_destinations`, `lsp_java_source_types`).
- MCP server: agent run closed (`mcp_agent_run_closed`), token regeneration and status page (`mcp_regenerate_token`, `mcp_server_status`), confirmations (`mcp_respond`, `mcp_ui_ready`), focused editor report (`mcp_set_focused_editor`).
- Notebook: language-server documents for code cells (`notebook_cell_docs`).
- Plugins, editor side: colour presentations (`plugin_color_presentations`), line markers (`plugin_line_markers`), file decorations (`plugin_decoration_sets`), plugin navigation index, goto and markers (`plugin_navigation_files`, `plugin_navigation_index`, `plugin_navigation_invalidate`, `plugin_navigation_goto`, `plugin_navigation_markers`).
- Plugins, Plugins page: Choose program / program path (`plugin_choose_program`, `plugin_program`), revoke a network host (`plugin_revoke_net_host`), IntelliJ theme import (`plugin_installed_intellij_themes`, `plugin_read_intellij_theme`, `plugin_stage_package`), startup frame colour (`plugin_set_theme_frame`).
- Plugins, run/test/services: run targets (`plugin_answer_run_target`), debug attach (`plugin_debug_attach_target`), test selection and parsing (`plugin_select_tests`, `plugin_parse_test_results`), service detail and log (`plugin_service_detail`, `plugin_service_log`), endpoints (`plugin_endpoints`), commit checks (`plugin_commit_checks`).
- Plugins, views and terminals: content tab buttons (`plugin_content_tab_action`), web views and their files and terminals (`plugin_web_page`, `plugin_web_read_file`, `plugin_web_save_file`, `plugin_web_pty_spawn`), plugin terminals (`plugin_terminal_session_spawn`, `plugin_terminal_session_close`); Mermaid Save as PNG in the Markdown preview (`plugin_web_save_file`).

Uncertain points, stated as hypotheses in their blocks: what `env_reload` (it calls `host.environment.reload`, `crates/ide-host/src/ops/env.rs:15`, not read further) and `jdk_download` touch (home, keychain, network), GPUI's Linux picker without a portal, and whether `lsp_goto` reaches plugin navigation targets without the native index.
