# Empty backend-call rows, batch 3 (102 commands), judged at each webview call site

Read at HEAD `67c5e41f1` with `git grep`/`git show` only. Webview sites: `git grep -n -w -e <camelCase> -e <snake_name> HEAD -- src`, minus `src/bindings.ts` and test files. Native: `git grep -n -w <snake_name> HEAD -- app-gpui/src`, then the feature's own words when that found nothing. A doc comment naming the command in a native model (`/// … (\`window_take_launch\`)`) is not counted as built unless a non-test executor of that step was found.

## Commands

### 1. prepare_workspace_edit

- Site `src/features/analysis/AnalysisDialogs.tsx:405`: Edit › Find › Replace Structurally…, after a search, opens the Replace Preview dialog; the backend adds the files' preconditions (so a file changed since is refused) before the preview shows.
- NOT BUILT. `prepare_workspace_edit` has no hit in `app-gpui/src`. Structural search/replace exists only as a model (`app-gpui/src/features_b/analysis/structural.rs`) and two menu entries (`app-gpui/src/workbench/menu_spec.rs:242-243`, `find.structural`, `find.replaceStructural`) with no handler registered anywhere (`git grep '"find.replaceStructural"'` finds only the menu line).
- Verdict: not built.

### 2. profiler_collect

- Site `src/features/profiler/cpuProfiles.ts:30`: opening a CPU-profile snapshot folder of the project reads it into the profiler view (flame graph / call tree).
- Site `src/features/profiler/executor.ts:86`: Run with Profiler, when the profiled run ends, collects the profile and opens it in the Profiler tool window.
- NOT BUILT. No hit for `profiler_collect`/`ide_profiler::collect`; `app-gpui/src/features_b/profiler.rs` holds only the profile types, `profiler_supports` and the flame layout, and calls no backend.
- Verdict: not built.

### 3. profiler_gc_before_stop

- Site `src/features/profiler/executor.ts:108`: stopping a live-objects profiled JVM run first forces a GC so the written profile holds only reachable objects.
- NOT BUILT (no hit; profiler executor absent as above). A test would need a real JVM with an attach listener: not allowed under the rules.
- Verdict: not built.

### 4. profiler_prepare

- Site `src/features/profiler/executor.ts:61`: Run with Profiler asks the backend for the wrapped command line (async-profiler / node --cpu-prof …) before starting the run.
- NOT BUILT (no hit for `profiler_prepare`/`ide_profiler::prepare`).
- Verdict: not built.

### 5. project_current

- Sites (all re-read the project the backend holds for this window into the webview's cached `useProject` store): `src/features/app/windows.ts:202` after attaching a folder (Project view shows the new member); `src/features/project-tree/index.ts:86` refreshes Unity metadata after a relevant file change; `src/features/project-trust/index.tsx:205` after Trust/Restrict, so the window's restricted state updates; `src/features/settings/index.ts:252` the separate Settings window loads the project's settings; `src/platform/project.ts:72` after a failed open, the window shows what the backend still holds; `src/platform/remote.ts:201` after connecting a remote project; `src/platform/remote.ts:113` the remote proxy rewrites its answer; `src/features/deployment/window.tsx:88`, `src/features/vcs/ComparisonWindow.tsx:109`, `src/features/vcs/DiffWindow.tsx:200` separate windows read their project.
- Native: the backend function is `ide_host::ops::project::current`, called live (no cache to refresh) at `app-gpui/src/workbench/app_shell.rs:173` (the window's workspace members for opening decisions), `app-gpui/src/workbench/agents_views.rs:40`, `app-gpui/src/workbench/http_client.rs:1187`. The refresh-a-cache sites (`windows.ts:202`, `project-tree/index.ts:86`, `project-trust/index.tsx:205`, `project.ts:72`) have no counterpart to build because the native window reads the backend each time; whether Unity metadata is shown natively at all was not checked. Separate Settings, deployment, comparison and diff windows: the native deployment window model (`app-gpui/src/features_b/deployment/window.rs:28`) only names the command in a doc comment; no executor found. Remote (`remote.ts`): `app-gpui/src/platform_more/remote.rs:150` is a routing table for a remote window, and the native open flow answers a remote folder with "is on another machine" (`app-gpui/src/workbench/app_shell.rs:902-905`), so remote is not built.
- Tests: no window test found whose assertion depends specifically on `ops::project::current` (the attach tests below assert the manifest on disk, not this read).
- Verdict: built, no test (for the in-window reads); not built for the separate-window and remote sites. Not claimable as a row as it stands.

### 6. project_rename

- Site `src/features/app/projectActions.ts:159`: File › Rename Project… asks for a name; the window title and Recent Projects show it (and a workspace manifest is rewritten with it).
- NOT BUILT. `app-gpui/src/app_models/project_actions.rs:135` has `rename_prompt` (the starting name), but nothing outside that file uses it (`git grep 'rename_prompt' HEAD -- app-gpui/src`), and no call of `save_workspace` exists in `app-gpui/src`.
- Verdict: not built.

### 7. remote_download_into

- Site `src/features/deployment/actions.tsx:231`: Tools › Deployment › Download from… a chosen server into a chosen local folder.
- NOT BUILT (no hit). The native deployment actions call `deploy_download`, `remote_*` siblings, but not this one.
- Verdict: not built.

### 8. run_application_build

- Site `src/features/run/applicationUpdate.ts:55`: Update Running Application (Spring Boot), "Update resources/classes": the backend plans the build that processes the module's resources, then runs it.
- NOT BUILT. `app-gpui/src/run/application_update.rs` is a step machine (`UpdateStep::PlanBuild`, `RunBuild`, `Publish`, `SyncTomcatOutputs`, `BuildTomcatArtifact`, `TomcatUpdate`, lines 214-266) with no executor: `git grep 'PlanBuild\|SyncTomcatOutputs\|TomcatUpdate\|BuildTomcatArtifact' HEAD -- app-gpui/src` finds nothing outside that file, and `ApplicationUpdates`/`advance` are used only in its own tests.
- Verdict: not built.

### 9. run_application_publish

- Site `src/features/run/applicationUpdate.ts:57`: the same Update, publishing the processed resources/classes into the running app's output.
- NOT BUILT (as 8: `UpdateStep::Publish` has no executor).
- Verdict: not built.

### 10. run_env_files

- Site `src/features/debugger/launch.ts:48`: starting Debug on a configuration that names `.env` files loads them under the configuration's own variables.
- BUILT through the same function: `app-gpui/src/debugger/driver.rs:906-912` (`BackendCall::RunEnvFiles` → `run::run_env_files`), asked from `app-gpui/src/debugger/actions.rs:946`.
- COVERED: `workbench::debug::tests::proofs6_debug::a_debug_start_reads_the_configurations_env_files_and_one_that_cannot_be_read_stops_it`, `app-gpui/src/workbench/debug/tests/proofs6_debug.rs:583` `long(cx, "the variables in the debug console", &|cx| fixture.console(cx).contains("GREETING=hello SHARED=own"));` and `:597` `assert!(body.starts_with("cannot read .env file") && body.contains("missing.env"), "{body}");`. Both would fail without the call (no GREETING, no notice). Not sure: the test's fake adapter (`FAKE_ADAPTER`) is a script; whether it runs through a machine interpreter was not checked here.
- Verdict: every site covered.

### 11. run_import_configs

- Site `src/features/run/importConfigs.tsx:114`: Edit Configurations › Import from another IDE's/tool's configurations (e.g. VS Code launch.json) converts them into run configurations in the draft.
- NOT BUILT. `app-gpui/src/run/store.rs:510` declares `edit_import: Option<RunImportSource>`, but nothing reads or sets it outside the declaration (`git grep 'edit_import' HEAD -- app-gpui/src`), and no `run_import_*` call exists.
- Verdict: not built.

### 12. run_import_sources

- Site `src/features/run/EditConfigsDialog.tsx:186`: the Edit Configurations dialog lists importable sources (shows the Import button). Site `src/features/run/importConfigs.tsx:35`: the import chooser lists them.
- NOT BUILT (as 11).
- Verdict: not built.

### 13. run_save_templates

- Site `src/features/run/EditConfigsDialog.tsx:314`: Edit Configurations › "Edit configuration templates…", OK/Apply saves the edited per-type templates to the project.
- NOT BUILT. The native app only reads templates (`app-gpui/src/run/driver.rs:559-562`, `ops::run::run_templates`) and applies them to added configurations (`app-gpui/src/run/edit_configs_view.rs:1271`); no template editing or saving.
- Verdict: not built.

### 14. run_system_env

- Site `src/features/run/EnvDialog.tsx:76`: the run configuration's Environment variables dialog shows the system environment ("Show"/include system variables list).
- NOT BUILT. No hit for `run_system_env`; `include_system_env` is only a flag of the configuration (`app-gpui/src/run/edit_configs_view.rs:1425`), no list of system variables is read.
- Verdict: not built.

### 15. run_test_reports

- Site `src/features/run/store.ts:1195`: when a JVM (Maven Surefire/Gradle) test run ends, the per-test results (including passed ones) are read from the XML reports into the test tree.
- NOT BUILT. The native test tree parses the console (`app-gpui/src/run_testing/jvm.rs`, Surefire lines) and, for Gradle, an init script's messages (`app-gpui/src/run/gradle_run.rs:38`); no XML report reading (no hit for `run_test_reports`, `TestCaseReport`). What the user loses: Maven passed tests are not listed per test (hypothesis from reading; confirm by reading `run_testing/jvm.rs` for a pass event).
- Verdict: not built.

### 16. run_tomcat_artifacts

- Sites `src/features/run/TomcatSettings.tsx:178` (Tomcat run configuration's Deployment tab lists the artifacts to deploy), `src/features/run/tomcat.ts:162` (starting a Tomcat run picks the artifact), `src/features/settings/languages/ProjectStructurePage.tsx:1578` (Project Structure › Artifacts lists web artifacts).
- NOT BUILT (no hit for `tomcat_artifacts`/`WebArtifact`; no Tomcat settings form or launcher in `app-gpui/src/run`).
- Verdict: not built.

### 17. run_tomcat_build

- Site `src/features/run/tomcat.ts:148`: starting a Tomcat run builds the exploded/WAR artifact first.
- NOT BUILT (no hit).
- Verdict: not built.

### 18. run_tomcat_home

- Sites `src/features/run/TomcatSettings.tsx:200`, `:1142`, `:1442`: choosing/typing a Tomcat home in the Tomcat configuration or Application Servers page validates it and shows its version (or "missing catalina.jar"). Site `src/features/run/tomcat.ts:141`: a run checks the home before starting.
- NOT BUILT (no hit for `tomcat_home`/`TomcatHome`).
- Verdict: not built.

### 19. run_tomcat_installations

- Site `src/features/run/TomcatSettings.tsx:144`: the Tomcat home field offers detected installations.
- NOT BUILT (no hit).
- Verdict: not built.

### 20. run_tomcat_sync

- Site `src/features/run/tomcat.ts:171`: Update Running Application on Tomcat copies the compiled outputs into the deployment.
- NOT BUILT (`UpdateStep::SyncTomcatOutputs` has no executor; see 8).
- Verdict: not built.

### 21. run_tomcat_update

- Sites `src/features/run/applicationUpdate.ts:97`, `:103`: Update Running Application on Tomcat reloads/redeploys the app (redeploy when hot swap did not apply).
- NOT BUILT (`UpdateStep::TomcatUpdate` has no executor; see 8).
- Verdict: not built.

### 22. scaffold_catalog

- Site `src/features/app/NewProject.tsx:131`: File › New Project… lists the generators (Empty, Spring Initializr, Vite, Cargo, …) in the dialog's left column.
- NOT BUILT. The native New Project dialog (`app-gpui/src/app_views/new_project_dialog.rs`) has name, location and "Create Git repository" only; no catalog (no hit for `scaffold`).
- Verdict: not built.

### 23. scaffold_create

- Site `src/features/app/NewProject.tsx:229`: Create runs the chosen generator (streaming its steps), optionally `git init`, closes the dialog and opens the project.
- DIFFERENT FUNCTION, for the empty-project case only: `app-gpui/src/workbench/app_actions.rs:1146-1176` creates the folder with `ide_vfs::create_dir` (after `ops::fs::read_dir` refuses a non-empty one) and runs `ops::vcs::vcs_init` when Git is checked, then opens it. The user loses every generator other than an empty folder, and the progress view.
- COVERED for the empty case: `workbench::app_actions::tests::new_project_creates_the_folder_and_opens_it`, `app-gpui/src/workbench/app_actions.rs:1676` `assert!(created.is_dir() && workbench.read_with(cx, |w, _| w.new_project_dialog()).is_none());` and `:1669` (the non-empty folder error). The Git-on case has a test file `app-gpui/src/workbench/app_actions/tests/proofs6_app_actions.rs` (`new_project_with_git_creates_a_repository_in_the_new_folder`, asserting `created.join(".git").is_dir()`), but that file is not declared as a module (`app_actions.rs:1213` declares only `mod proofs20_layouts;`) and its test is not in `rust_tests.txt`: it never compiles or runs. The test at `:1663` turns Git off because "the backend's git would read this machine's configuration".
- Verdict: some covered (empty generator without Git only; generators not built).

### 24. scaffold_options

- Site `src/features/app/NewProject.tsx:151`: a generator with dynamic options (e.g. Spring Initializr) fetches its metadata from its server and shows the extra fields.
- NOT BUILT (no hit). A test would need the network beyond a loopback stand-in unless the server field points at one: allowed only with an in-process stand-in server.
- Verdict: not built.

### 25. scaffold_requirements

- Site `src/features/app/NewProject.tsx:167`: the dialog lists the tools a generator needs (e.g. cargo, node) and disables Create while one is missing.
- NOT BUILT (no hit). The backend probes the machine's toolchains: a test of the real function would run/locate machine programs, not allowed.
- Verdict: not built.

### 26. sdk_container_python

- Site `src/features/settings/languages/api.ts:165`: Settings › Languages › Python shows the interpreter of the project's dev container.
- NOT BUILT (no hit for `container_python`/`ContainerPython`). The backend reads Docker: not testable under the rules.
- Verdict: cannot be tested under the rules (and not built).

### 27. sdk_container_python_refresh

- Site `src/features/settings/languages/api.ts:170`: Settings › Python, "Refresh" copies the container interpreter's libraries again after an image rebuild.
- NOT BUILT (no hit). Backend drives Docker: not testable under the rules.
- Verdict: cannot be tested under the rules (and not built).

### 28. sdk_jdk_at

- Site `src/features/settings/languages/ProjectStructurePage.tsx:1801`: Project Structure › SDKs › Add JDK from disk: the picked folder is resolved into a named JDK (version) or refused.
- NOT BUILT. No hit; the native SDK combo (`app-gpui/src/settings_view/pages/languages_sdk.rs`) lists `sdk_detect` results and keeps a hand-set value as typed, without resolving it (read from the module doc, lines 1-13; not traced further).
- Verdict: not built.

### 29. sdk_php_composer_install

- Sites `src/features/run/composerInstall.ts:34`, `:131`: after `composer.json`/`composer.lock` changes, the editor offers "Run composer install" and tracks whether it is needed.
- NOT BUILT (no hit for `composer_install`; `composer` hits are the agents' composer box). The backend runs/probes Composer: a test of the real function would need a machine toolchain.
- Verdict: not built.

### 30. sdk_php_language_level

- Site `src/features/settings/languages/api.ts:175`: Settings › PHP shows the language level detected from the project's `composer.json` next to the "auto" choice.
- NOT BUILT. Only a default exists (`settings_model::language_pages::tests::defaults_the_php_language_level_to_auto`, a model test); no hit for the function.
- Verdict: not built.

### 31. sdk_project_libraries

- Site `src/features/libraries/index.ts:193`: the Project view's External Libraries node lists the interpreter's/SDK's library roots (streamed). Site `src/features/settings/languages/ProjectStructurePage.tsx:1077`: Project Structure › Libraries lists them.
- NOT BUILT. `app-gpui/src/features_b/libraries.rs` holds the tree model (`EXTERNAL_LIBRARIES_LABEL`, `sdk_key`), but nothing outside the file uses it (`git grep 'features_b::libraries'` finds nothing) and no backend call exists.
- Verdict: not built.

### 32. sdk_python_create_venv

- Site `src/features/settings/languages/api.ts:107`: Settings › Python › Add Interpreter › New environment (venv/uv/poetry/conda) creates it with streamed output.
- NOT BUILT (no hit). Runs machine toolchains: not testable under the rules.
- Verdict: cannot be tested under the rules (and not built).

### 33. sdk_python_interpreter

- Site `src/features/settings/languages/api.ts:112`: Add Interpreter › Existing environment resolves a hand-picked interpreter into a named SDK (version).
- NOT BUILT (no hit; the native combo keeps a typed/browsed path as is, see 28). Resolving runs the interpreter for its version: a real-function test would run a machine program.
- Verdict: not built.

### 34. sdk_python_packages

- Site `src/features/settings/languages/api.ts:121`: Settings › Python lists the interpreter's installed packages.
- NOT BUILT (no hit). Runs pip: not testable under the rules.
- Verdict: cannot be tested under the rules (and not built).

### 35. sdk_python_pip

- Site `src/features/settings/languages/api.ts:131`: the package list's Install/Uninstall/Upgrade buttons run pip with streamed output.
- NOT BUILT (no hit). Runs pip and the network: not testable under the rules.
- Verdict: cannot be tested under the rules (and not built).

### 36. sdk_python_project_interpreters

- Site `src/features/settings/languages/api.ts:117`: the interpreter combo also offers the interpreters of other recent projects.
- NOT BUILT (no hit).
- Verdict: not built.

### 37. settings_list_jetbrains_ides

- Site `src/features/app/projectActions.ts:120`: File › Import Settings from a JetBrains IDE lists the IDEs installed on this computer (or warns none were found).
- NOT BUILT. `app-gpui/src/app_models/jetbrains_settings.rs` exists but nothing outside it uses it (`git grep jetbrains HEAD -- app-gpui/src` outside the file finds only `mod` and plugin-theme tests). The backend reads the user's JetBrains configuration folders in the home: a real-function test would touch the user's home, not allowed (a test-home stand-in might do; not checked).
- Verdict: not built.

### 38. settings_read_jetbrains

- Site `src/features/app/jetbrainsSettings.tsx:386`: after choosing an IDE (or a settings.zip), reads its settings bundle to preview what will be imported.
- NOT BUILT (as 37).
- Verdict: not built.

### 39. settings_removed_theme

- Site `src/features/plugins/oneDarkNotice.ts:25`: at startup, a settings file still naming the removed `one-dark` theme gets a one-time notice offering the One Dark Pro plugin.
- NOT BUILT. No hit; `app-gpui/src/plugin_models/startup_theme.rs:183-188` only has a One Dark theme in a test fixture.
- Verdict: not built.

### 40. show_system_notification

- Site `src/workbench/notificationStore.ts:165`: a balloon notification also shows as an OS notification (when the app is in the background and system notifications are on).
- NOT BUILT (no hit for `system_notification` or an OS notification API). Posting a real OS notification from a test is a machine side effect; a stand-in would be needed.
- Verdict: not built.

### 41. spell_check

- Site `src/editor/lsp/spelling.ts:221`: while editing, typos are underlined and listed in Problems ("Typo: In word …").
- NOT BUILT. `app-gpui/src/code_insight/problems.rs:699-718` only shows a "spelling" producer in a model test; no producer calls `spell_check` (no hit for `ops::spell`).
- Verdict: not built.

### 42. spell_suggest

- Site `src/editor/lsp/spelling.ts:118`: the typo's quick fix lists suggested words.
- NOT BUILT (no hit).
- Verdict: not built.

### 43. ssh_askpass_answer

- Site `src/features/ssh/askpass.ts:41`: an `ssh`/`git` child process asks for a passphrase/password/host-key answer; the dialog's OK/Cancel (and Remember) answers it.
- NOT BUILT. `app-gpui/src/features_b/ssh.rs` is the prompt model (`AskpassPrompt`, the reply documented at line 26) with no user outside the file (`git grep 'AskpassPrompt\|features_b::ssh'`). `app-gpui/src/backend.rs:204` installs a credential store so saved HTTPS credentials are answered silently, which is not the prompt. A test could drive it with an in-process askpass event, if the window showed it.
- Verdict: not built.

### 44. ssh_open_terminal

- Site `src/features/ssh/session.ts:66`: Tools › Start SSH Session… opens a terminal tab connected to the chosen host.
- NOT BUILT (no hit for `ssh_open_terminal`/`spawn_ssh`). Real SSH is not allowed in tests; an in-process stand-in server would be needed.
- Verdict: not built.

### 45. ssh_password_saved

- Site `src/features/settings/pages/deployment.tsx:308`: Settings › Deployment, an SFTP server's password field shows whether a password is saved.
- NOT BUILT (no hit). The backend reads the keychain: not testable under the rules unless the secret store is the test stand-in.
- Verdict: not built.

### 46. structural_languages

- Site `src/features/analysis/AnalysisDialogs.tsx:200`: the Search Structurally dialog lists its languages.
- NOT BUILT (see 1: model only, menu entries without handlers).
- Verdict: not built.

### 47. structural_search

- Site `src/features/analysis/structural.ts:111`: Find runs the structural search; matches stream into the Find tool window.
- NOT BUILT (see 1).
- Verdict: not built.

### 48. structural_validate

- Site `src/features/analysis/AnalysisDialogs.tsx:220`: typing a pattern shows whether it parses.
- NOT BUILT (see 1).
- Verdict: not built.

### 49. syntax_expression_at

- Site `src/editor/structure/index.ts:92` is the wrapper; its callers: `src/editor/refactor/refactorThis.ts:221` (Refactor This with a caret lists the expression refactorings that apply), `src/editor/refactor/serverIntroduce.ts:65` and `:266` (Introduce Variable/Constant/Parameter… with a caret offers the enclosing expressions to choose from), `src/editor/refactor/introduce.ts:193` (the syntax-only Introduce Variable when no language server answers).
- Native: same function. `app-gpui/src/workbench/refactor_actions.rs:426-431` (`FactRequest::ExpressionAt`, Refactor This facts) and `:289-298` (`MenuStep::ExpressionAt`, then `MenuStep::ChooseExpression`, the expression chooser) for the server-backed Introduce. The syntax-only Introduce is NOT BUILT: `MenuStep::IntroduceFallback` only shows the server's reason as a hint (`refactor_actions.rs:282-286`).
- BUILT, NO TEST for the chooser and Refactor This facts: the window tests of `refactor_actions` (`workbench::refactor_actions::tests::lists_only_the_refactorings_applicable_at_the_caret_in_idea_order`, `…offer_idea_s_occurrences_choice_when_the_server_has_both_variants`) assert the lists and occurrences, not the expression chooser; no test name in the list mentions the expression choice. A window test would put the caret inside a nested call in a parsed language with a stand-in server, run Introduce Variable and assert the "Expressions" chooser's rows: allowed (tree-sitter in-process; an in-process stand-in server).
- Verdict: built, no test (and the syntax-only fallback not built).

### 50. system_probe_hotkeys

- Site `src/features/settings/keymap/systemHotkeys.ts:54`: on Windows only, once per session, warns which IDE shortcuts the system has grabbed.
- NOT BUILT (no hit). Probing registers real OS hotkeys: not testable under the rules.
- Verdict: not built.

### 51. tools_install

- Site `src/features/tools/backend.ts:37`: Settings › Tools (managed toolchains/language servers) Install downloads and installs a tool with progress.
- NOT BUILT (no hit for `ops::tools`). Downloads from the network: not testable beyond a loopback stand-in.
- Verdict: not built.

### 52. tools_list

- Site `src/features/tools/backend.ts:33`: the Tools page lists managed tools and their state. Site `src/features/debugger/java.ts:92`: the Java debugger checks whether its adapter is installed.
- NOT BUILT (no hit).
- Verdict: not built.

### 53. tools_uninstall

- Site `src/features/tools/backend.ts:39`: the Tools page's Uninstall.
- NOT BUILT (no hit).
- Verdict: not built.

### 54. tools_updates

- Site `src/features/tools/backend.ts:40`: the Tools page checks for updates.
- NOT BUILT (no hit). Network.
- Verdict: not built.

### 55. user_font_data

- Site `src/platform/fonts.ts:69`: a font family the macOS webview cannot resolve by name (a user-installed font) is read from disk and registered under an alias, so the editor/UI font setting takes effect.
- DIFFERENT FUNCTION (GPUI itself): the native text system resolves installed fonts by name, so no read is needed; `app-gpui/src/theme/fonts.rs:65-80` keeps the webview's alias model (`UserFonts`, `load: Option<UserFont>`), but nothing executes a load. Hypothesis: the user loses nothing (confirm with a window test that sets a user-installed family and reads the editor's resolved font; such a test needs a font in the user's font folder, which the rules forbid touching).
- Verdict: cannot be tested under the rules (no backend call needed natively).

### 56. vcs_clone

- Site `src/features/vcs/clone.tsx:191`: Git › Clone… clones a URL into a folder with streamed output, then offers to open it.
- NOT BUILT. `app-gpui/src/vcs_ops/clone.rs` is the dialog model (depth validation, line 64) with no `vcs_clone` call. A test would need a loopback git server or a local path as URL and runs real `git` (a machine program).
- Verdict: not built.

### 57. vcs_compare_paths

- Site `src/features/vcs/operations.tsx:657`: Git › Compare with Branch or Revision… on a folder or several files opens the comparison of every changed file.
- NOT BUILT. The native action uses `ops::vcs::vcs_compare_scope` and then compares only a single file; folders and several files are refused on purpose (`app-gpui/src/workbench/diff_tools.rs:336-341`: "Folders and several files are compared in the path comparison, which this window does not have").
- Verdict: not built.

### 58. vcs_delete_tag

- Site `src/features/vcs/branchOps.ts:423`: Branches popup › a tag › Delete deletes it and shows a notice with Restore.
- NOT BUILT (no hit; native has `vcs_create_tag`, `vcs_tags` only).
- Verdict: not built.

### 59. vcs_file_mode

- Site `src/features/vcs/diffInput.ts:316`, `:318`: a diff shows the file-mode change (e.g. 100644 → 100755) of each side.
- NOT BUILT (no hit for `file_mode` outside unrelated run-config fields).
- Verdict: not built.

### 60. vcs_ignore_blame_revision

- Site `src/features/vcs/blame.ts:334`: the annotations' context menu › Ignore Revision adds the commit to the ignore-revs file and re-annotates.
- NOT BUILT (no hit; native blame calls `vcs_blame` only, `app-gpui/src/workbench/annotate.rs:100`).
- Verdict: not built.

### 61. vcs_init

- Site `src/features/vcs/createRepository.ts:14`: VCS › Create Git Repository… asks for a folder, runs `git init`, says "Created Git repository in …" and adds the root to the VCS roots.
- NOT BUILT for this site: no `vcs.createRepository` action in `app-gpui/src` (`git grep -i 'create_repository\|createRepository\|Create Git Repository'` finds only New Project's checkbox). The native app calls the same function from a different place, New Project's "Create Git repository" (`app-gpui/src/workbench/app_actions.rs:1170-1175`), whose only test is in the undeclared module `proofs6_app_actions.rs` (see 23). Real `git` reads the machine's configuration (the test at `app_actions.rs:1663` says so), so a test needs a stand-in home.
- Verdict: not built.

### 62. vcs_log_details_batch

- Site `src/features/vcs/logDetails.ts:59`: the Git log loads commit details (full message, containing branches, signature) in batches of up to 64 commits per repository as rows and the details pane need them.
- DIFFERENT FUNCTION: the native log reads one commit at a time with `git::vcs_log_details` (`app-gpui/src/workbench/log_window.rs:499-502` for the details pane, `app-gpui/src/workbench/log_actions.rs:496`, `:815`, `:842` for actions). The user sees the same details; what differs is the number of backend calls when many rows need details (hypothesis, from reading).
- Tests: `vcs::log_details::tests::loads_a_full_body_containing_branches_and_signature_only_when_details_are_visible` exists; I did not read its body, so whether it starts from a window and passes through `log_window.rs:501` is unconfirmed.
- Verdict: built, no test (unconfirmed; read that test before claiming).

### 63. vcs_log_refs

- Sites `src/features/vcs/GitLog.tsx:248` (when branches change, the log reloads if refs moved), `:269` (a read-only/external log polls refs to pick up outside changes), `src/features/vcs/logStore.ts:976` (row decorations refresh).
- DIFFERENT FUNCTION for the labels: the native log gets the refs with each commit from `ops::vcs::vcs_log` (`app-gpui/src/vcs/log_view.rs:303`) and draws them (`log_view.rs:544`, `ref_labels`). NOT BUILT for the change detection and the polling: no hit for `log_refs` and no reload on a ref change was found.
- COVERED for the labels: `vcs::log_view::tests::shows_a_repositorys_commits_with_their_graph`, `app-gpui/src/vcs/log_view.rs:821` `assert!(merge.refs.iter().any(|name| name == "HEAD") && merge.refs.iter().any(|name| name == "tag: v1"));` (the test opens a `GitLog` window view). Not sure: the repository it reads is made by running real `git`, which may be outside the rules.
- Verdict: some covered.

### 64. vcs_move_worktree

- Site `src/features/vcs/gitMenuDialogs.tsx:776`: Git › Worktrees › Move… moves a worktree to a new folder.
- NOT BUILT. Native has `git::vcs_add_worktree`, `vcs_remove_worktree`, `vcs_worktrees`, but nothing for move.
- Verdict: not built.

### 65. vcs_restore_tag

- Site `src/features/vcs/branchOps.ts:438`: the "Deleted tag" notice's Restore puts the tag back.
- NOT BUILT (no hit; tag deletion itself is not built, see 58).
- Verdict: not built.

### 66. vcs_revision_children

- Sites `src/features/vcs/RevisionBrowser.tsx:80`, `:118`: Browse Repository at Revision shows the tree of a revision and expands folders.
- NOT BUILT (no hit for `revision_children`/`RevisionBrowser`).
- Verdict: not built.

### 67. vcs_revision_files

- Sites `src/features/vcs/PathFilterDialog.tsx:76` (the log's path filter lists HEAD's files), `src/features/vcs/RevisionBrowser.tsx:96` (revision browser search), `src/features/vcs/fileRevisions.ts:25` (file chooser of a revision).
- NOT BUILT (no hit).
- Verdict: not built.

### 68. vcs_stage_hunks

- Site `src/features/vcs/DiffEditor.tsx:995`: the diff viewer's per-hunk Stage button stages just that hunk.
- NOT BUILT. Native stages whole files only (`git::vcs_stage`, `app-gpui/src/workbench/shelf.rs:1764`, `conflicts.rs:169`); `vcs_line_hunks` is read for gutter change bars (`app-gpui/src/workbench/editor_gutter.rs:501`) but there is no hunk staging.
- Verdict: not built.

### 69. vcs_stash_drop

- Site `src/features/vcs/changeOps.tsx:542`: Stashes › Drop asks to confirm, drops `stash@{n}` and refreshes the list.
- NOT BUILT. `app-gpui/src/vcs_ops/change_ops.rs:279` (`drop_stash`, the confirmation texts) is used only by its own test (`:483`); native has `vcs_stashes`, `vcs_stash_apply`, `vcs_stash_push` only.
- Verdict: not built.

### 70. vcs_status_paths

- Site `src/features/vcs/store.ts:294`: after file events, only the changed paths' status is re-read and merged, so the Commit view and file colours update.
- DIFFERENT FUNCTION: the native app re-reads the whole status with `ops::vcs::status` (`app-gpui/src/vcs/commit_view.rs:488`; `git::status` elsewhere). The user sees the same result; a large repository costs more per refresh (hypothesis).
- Tests: none found whose assertion is a status update after a file change (grepped `rust_tests.txt` for status/changes with save/refresh/modified).
- Verdict: built, no test.

### 71. web_browsers

- Site `src/features/web/browsers.ts:18`: the HTML editor's floating browser icons and Open in Browser list the installed browsers. Site `src/features/debugger/browser.ts:104`: the JavaScript debugger picks an installed browser.
- NOT BUILT (no hit for `ide_web`/`web_browsers`). Detecting real browsers reads the machine.
- Verdict: not built.

### 72. web_goto

- Sites `src/editor/lsp/navigation.ts:676`, `:887`: Go to Declaration (and the Ctrl-hover underline) on a web reference (`href`/`src`/CSS `url()`) opens the target file.
- NOT BUILT (no hit; native `ops::nav` calls are `find_files`, `find_symbols`, `recent_files`, …).
- Verdict: not built.

### 73. web_open

- Site `src/features/web/browsers.ts:42`: Project view/editor › Open 'x.html' in Browser opens the file's URL in the chosen browser.
- NOT BUILT. The context menu row exists (`app-gpui/src/workbench/context_menus.rs:347-357`, action `web.openInBrowser`), but no handler is registered for `web.openInBrowser` anywhere (`git grep '"web.openInBrowser"'` finds only that row). Opening a real browser is a machine program.
- Verdict: not built.

### 74. web_serves_page

- Site `src/features/debugger/state.ts:607`: the JavaScript debugger asks the dev server whether a URL is a page before opening the browser on it.
- NOT BUILT (no hit). A loopback stand-in server would make it testable.
- Verdict: not built.

### 75. web_unresolved_references

- Site `src/editor/lsp/fileReferences.ts:122`: unresolved file references in HTML/CSS are underlined and listed in Problems.
- NOT BUILT (no hit).
- Verdict: not built.

### 76. web_url

- Site `src/features/web/browsers.ts:35` (used by `src/features/web/WebEditor.tsx:62` for the HTML editor's preview and by Open in Browser): the built-in web server's URL of a project file.
- NOT BUILT (no hit for the function; `app-gpui/src/features_b/deployment/actions.rs:291` `open_in_browser` uses the deployment server's configured `web_url` setting, a different feature).
- Verdict: not built.

### 77. window_complete_launch

- Site `src/features/app/windows.ts:283`: after a window has opened what it was launched for (a project, files, a diff from `openidea …`), it releases the launch so the next one is delivered.
- NOT BUILT. `app-gpui/src/app_models/windows.rs:717-718` (`LaunchStep::Complete`) is a model step; its only executor is the test driver in the same file (`windows.rs:1270-1290`, inside `mod tests` from line 989). There is no launch queue natively (see 95).
- Verdict: not built.

### 78. window_flash_taskbar

- Sites `src/features/debugger/taskbarFlash.ts:20`, `:37`: a breakpoint hit in a background window flashes the taskbar/Dock; focusing or resuming stops it.
- NOT BUILT (decision built, effect not). `app-gpui/src/debugger/taskbar_flash.rs` decides when to ask, but the driver answers every request as a failure without flashing (`app-gpui/src/debugger/driver.rs:947-952`: "The window cannot ask the shell for attention"). The `debugger::state::tests::flashes_when_a_breakpoint_stops_an_unfocused_window` family are model tests of the decision only.
- Verdict: not built.

### 79. window_group_cancel_close

- Sites in `src/features/app/closeGuard.ts`: `:222` a detached document window refused to close; `:239` the last project window went back to Welcome instead of closing; `:256` the user cancelled the close question; `:265` saving failed. In each the window (group) stays open.
- DIFFERENT FUNCTION (GPUI itself): `app-gpui/src/workbench/app_shell.rs:385-401` `window_should_close` returns `false` to keep the window; there are no window groups natively, so nothing else to call off.
- COVERED for `:256` and `:265`: `workbench::tests::proofs13_close_guard::keeps_the_window_when_saving_fails_or_the_user_cancels`, `app-gpui/src/workbench/tests/proofs13_close_guard.rs:223` `assert!(!cx.simulate_close());` then `:229` `assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()), "the project stays");` (save fails) and `:234`/`:238` the same after `Cancel`. COVERED for `:239`: `workbench::tests::proofs13_close_guard::asks_before_terminating_a_running_process`, `proofs13_close_guard.rs:92` `assert!(!cx.simulate_close());` then `:98` waits for the Welcome screen. Site `:222` (detached document windows): not checked whether the native app has such windows; detached editor tabs are not built (see 91).
- Verdict: some covered.

### 80. window_group_close

- Site `src/features/app/windows.ts:426`: a project tab of a grouped window › Close closes that project window.
- NOT BUILT. `app-gpui/src/platform/window_groups.rs` and `app_models/windows.rs:603` (`tabs_to_close`) are models with no window-group feature; no `window.closeTab` handler is registered (the only `window.*` handlers are `window.new` and `window.toggleFullScreen`, `app-gpui/src/workbench/windows.rs:187`, `:250`).
- Verdict: not built.

### 81. window_group_close_tabs

- Site `src/features/app/windows.ts:311`: Close Other Tabs / Close Tabs to the Left/Right on the project tabs of a grouped window.
- NOT BUILT (as 80).
- Verdict: not built.

### 82. window_group_current

- Site `src/platform/windowGroups.ts:19`: the project tab strip of a grouped window shows its tabs.
- NOT BUILT (as 80).
- Verdict: not built.

### 83. window_group_detach

- Site `src/features/app/windows.ts:409`: Window › Move Tab to New Window takes the project tab out of the group.
- NOT BUILT. `window.detachTab` is in the menu (`app-gpui/src/workbench/menu_spec.rs:799`) with no handler.
- Verdict: not built.

### 84. window_group_merge

- Site `src/features/app/windows.ts:401`: Window › Merge All Windows groups every project window as tabs of one.
- NOT BUILT (as 80).
- Verdict: not built.

### 85. window_group_reorder

- Site `src/features/app/windows.ts:456`: dragging a project tab reorders the group (`reordered_tabs` model in `app-gpui/src/platform/window_groups.rs:17`, unused).
- NOT BUILT.
- Verdict: not built.

### 86. window_group_restore

- Site `src/platform/windowGroups.ts:41`: at startup, the window groups of the last session are restored.
- NOT BUILT.
- Verdict: not built.

### 87. window_group_select

- Sites `src/features/app/windows.ts:417`, `src/platform/windowGroups.ts:24`: clicking a project tab (or Next/Previous Project Tab) shows that project window.
- NOT BUILT as a group. Hypothesis: native cycles between separate windows instead (`app-gpui/src/workbench/windows.rs:235-247` activates the next/previous window); not traced to an action id or a test.
- Verdict: not built.

### 88. window_is_fullscreen

- Site `src/workbench/useFullscreen.ts:17`: in native full screen the title bar drops the room kept for the window buttons.
- NOT BUILT. No `is_fullscreen` read in `app-gpui/src` (only `window.toggle_fullscreen()`, `windows.rs:251`); hypothesis: the native title bar keeps the room in full screen (confirm with a screenshot of the real window in full screen).
- Verdict: not built.

### 89. window_open_comparison

- Site `src/features/vcs/operations.tsx:550`: Compare with Branch on a folder / Changes between revisions opens a separate comparison window listing the changed files.
- NOT BUILT. No comparison window (`app-gpui/src/features_b/deployment/actions.rs:275` `open_comparison` is deployment's, a model); the path comparison is refused on purpose (see 57).
- Verdict: not built.

### 90. window_open_diff

- Site `src/features/vcs/DiffWindow.tsx:272`: Show Diff (modal interface) opens the diff in a window in front of the project window.
- DIFFERENT FUNCTION (GPUI itself): the native diff window is a layer over the project window, not an OS window (`app-gpui/src/workbench/diff_window.rs:1-8`), loading its sides with `ops::vcs::vcs_file_at` (`diff_window.rs:126`). The user loses moving the diff to another screen (a separate OS window).
- COVERED: `workbench::tests::proofs12_diff_window::opens_the_context_menus_show_diff_in_a_front_dialog_not_in_an_editor_tab`, `app-gpui/src/workbench/tests/proofs12_diff_window.rs:184` `assert_eq!(dialog.read_with(cx, |dialog, _| dialog.data().path.clone()), text_of(&root.join("a.txt")));` and `:185` `assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "no editor tab is opened");`.
- Verdict: every site covered (as a front layer, not a separate window: say so on the row).

### 91. window_open_file_in_new_window

- Site `src/editor/core/controller.ts:1845`: an editor tab's Open in New Window saves the file, opens it in a new window (in the project's window when it is inside the project) and closes the tab here.
- NOT BUILT. `app-gpui/src/editor_model/detach.rs` is the model (its tests `editor_model::detach::tests::keeps_the_tab_and_opens_no_window_when_text_saving_fails` and `…keeps_a_pinned_text_tab_if_window_creation_fails` are model tests); nothing outside it uses it (`git grep 'detach::'`).
- Verdict: not built.

### 92. window_open_projects

- Site `src/features/welcome/ProjectPopup.tsx:28`: the project switcher popup lists the projects open in other windows.
- NOT BUILT (no hit for the popup or the list).
- Verdict: not built.

### 93. window_open_remote

- Sites `src/features/welcome/RecentList.tsx:134` (a recent remote project › Open in New Window), `src/workbench/projectWindow.ts:61`, `:65` (opening a remote project focuses its window or opens one).
- NOT BUILT. The native open flow answers a remote folder with "… is on another machine" (`app-gpui/src/workbench/app_shell.rs:902-905`).
- Verdict: not built.

### 94. window_set_titlebar_height

- Site `src/workbench/nativeTitlebar.ts:26`: the macOS window buttons are vertically centred on the app's title bar, following the UI zoom.
- DIFFERENT FUNCTION (GPUI itself): the buttons are placed once when the window is made, at a fixed point (`app-gpui/src/workbench/mod.rs:498-506`, `traffic_light_position: (13, 13)`). The user loses re-centring when the UI zoom changes the bar's height (hypothesis; confirm with a screenshot at a non-default zoom).
- Tests: none found.
- Verdict: built, no test (a screenshot check, not a unit test, would show it).

### 95. window_take_launch

- Sites: `src/features/app/windows.ts:246` (a project window takes what it was launched with: project, files with positions, a diff), `src/features/settings/index.ts:176` (the Settings window), `src/features/deployment/window.tsx:79`, `src/features/vcs/ComparisonWindow.tsx:107`, `src/features/vcs/DiffWindow.tsx:198` (separate windows take their contents).
- DIFFERENT FUNCTION for the first launch only: `app-gpui/src/main.rs:10-33` opens the first command-line argument directly (a folder as a project, a file as a file window). NOT BUILT: later launches into a running app, positions, the CLI diff and every separate-window launch; `LaunchQueue` (`app-gpui/src/app_models/windows.rs:692-740`) is executed only by its own tests (`app_models::windows::tests::opens_launched_files_in_the_launched_project` and siblings, model tests).
- Tests: no window test of `main.rs`'s argument handling found.
- Verdict: built, no test (first launch only); not built for the rest.

### 96. window_toggle_zoom

- Site `src/main.tsx:48`: a double click on the title bar fills the work area (zoom), as macOS windows do.
- DIFFERENT FUNCTION (GPUI itself): Window › Zoom calls `window.zoom_window()` (`app-gpui/src/workbench/menu.rs:589`). A double click on the app's own title bar was not found to zoom (`git grep 'zoom_window'` has only the menu).
- Tests: none found (`grep -i zoom rust_tests.txt` lists only editor/image/profiler zoom).
- Verdict: built, no test (menu only; the title bar double click is not built).

### 97. workspace_attach

- Site `src/features/app/windows.ts:201`: opening a folder and answering Attach (in the "Open Project" question) asks about trust, adds the folder to the window's workspace, shows it in the Project view, reveals it and opens the files.
- NOT BUILT for users, built for tests: the executor exists (`app-gpui/src/workbench/app_shell.rs:946-958`, `OpenStep::Attach` → `ops::project::attach_workspace`), but the Attach answer is removed from the question users get (`app_shell.rs:863` `prompt.buttons.retain(|button| button.id != ATTACH)`, reason at `:52-53`: "the window does not read its project again once a folder is attached, so the Project view would not show it"). Tests reach it through a `#[cfg(test)]` entry (`app_shell.rs:840-850`, `attach_as_answered`): `workbench::tests::proofs44_attach_trust::attaching_a_trusted_folder_attaches_it_without_a_question` (`app-gpui/src/workbench/tests/proofs44_attach_trust.rs:139` `assert!(attached(&root, &other));`, the manifest on disk) and its three siblings. They prove the backend effect and the trust question, not a user path.
- Verdict: not built (for the user; the hidden path is tested).

### 98. workspace_create

- Site `src/features/app/WorkspaceDialog.tsx:102`: File › New Workspace… creates a workspace manifest with the chosen projects and opens it.
- NOT BUILT. `app-gpui/src/app_models/workspace_dialog.rs` (`WorkspaceDialog`, `DialogStep`) is used only by its own tests; `workspace.create` / `workspace.manage` are menu entries without handlers (`app-gpui/src/workbench/menu_spec.rs:184`, `:186`).
- Verdict: not built.

### 99. workspace_discover_projects

- Site `src/features/app/WorkspaceDialog.tsx:76`: the workspace dialog searches picked folders for projects to add.
- NOT BUILT (as 98).
- Verdict: not built.

### 100. workspace_update

- Sites `src/features/app/WorkspaceDialog.tsx:114` (Manage Workspace… saves the member list) and `src/features/app/windows.ts:332` (Project view › Remove from Workspace / Detach, after confirming).
- NOT BUILT. `RemoveFromWorkspace` (`app-gpui/src/app_models/windows.rs:622-676`) is used only by its tests; the Project view offers `workspace.removeProject` / `project.detach` rows (`app-gpui/src/project_view/context_menu.rs:388`) but no handler is registered for either (`git grep '"project.detach"'` finds only the row, the model's context-key table and tests).
- Verdict: not built.

### 101. wsl_distros

- Site `src/features/remote-dev/ConnectDialog.tsx:67`: Remote Development's connect dialog lists installed WSL distributions as targets.
- NOT BUILT. `app-gpui/src/features_b/remote_dev/connect_dialog.rs` is the model (line 38 names the step), used by nothing outside it. Listing distributions runs `wsl.exe`: not testable under the rules.
- Verdict: cannot be tested under the rules (and not built).

### 102. wsl_resolve

- Sites `src/features/app/windows.ts:59`, `:140` (opening a `\\wsl$\…` folder connects to that distribution instead of opening it as a network folder; a new window asks trust only for local folders), `src/platform/remote.ts:281` (a remote location typed as a WSL path).
- NOT BUILT, on purpose: the native open flow answers every WSL question with "not WSL" (`app-gpui/src/workbench/app_shell.rs:900-901`, `OpenStep::ResolveWsl { .. } => flow.wsl_resolved(None)`, "this window does not run projects elsewhere").
- Verdict: not built.

## Summary

All 102 commands were read.

Tally by block verdict: every site covered 2 (`run_env_files`, `window_open_diff`); some covered 3 (`scaffold_create`, `vcs_log_refs`, `window_group_cancel_close`); built, no test 7 (`project_current`, `syntax_expression_at`, `vcs_log_details_batch`, `vcs_status_paths`, `window_set_titlebar_height`, `window_take_launch`, `window_toggle_zoom`; several of these are partial, as each block says); cannot be tested under the rules 7 (`sdk_container_python`, `sdk_container_python_refresh`, `sdk_python_create_venv`, `sdk_python_packages`, `sdk_python_pip`, `user_font_data`, `wsl_distros`); not built 83.

### Can be claimed now (every site covered)

- `run_env_files`: `workbench::debug::tests::proofs6_debug::a_debug_start_reads_the_configurations_env_files_and_one_that_cannot_be_read_stops_it` (check first that its fake adapter runs no machine interpreter).
- `window_open_diff`: `workbench::tests::proofs12_diff_window::opens_the_context_menus_show_diff_in_a_front_dialog_not_in_an_editor_tab`; the row must say the diff is a layer over the window, not an OS window.

### Partly claimable (some covered)

- `window_group_cancel_close` (GPUI's should-close returns false): `workbench::tests::proofs13_close_guard::keeps_the_window_when_saving_fails_or_the_user_cancels`, `workbench::tests::proofs13_close_guard::asks_before_terminating_a_running_process`.
- `scaffold_create` (empty folder only): `workbench::app_actions::tests::new_project_creates_the_folder_and_opens_it`.
- `vcs_log_refs` (labels only): `vcs::log_view::tests::shows_a_repositorys_commits_with_their_graph`.

### Built, no test: candidates for a tests slice, by harness

- `workbench::refactor_actions` tests (window + stand-in server, `app-gpui/src/workbench/refactor_actions.rs` mod tests): `syntax_expression_at`, the Introduce expression chooser.
- `vcs::log_view` / `vcs::log_details` window tests: `vcs_log_details_batch` (read `vcs::log_details::tests::loads_a_full_body_containing_branches_and_signature_only_when_details_are_visible` first; it may already count).
- Commit view tests (`app-gpui/src/vcs/commit_view.rs`): `vcs_status_paths` (edit a file on disk, assert the Commit view lists it).
- `workbench::app_actions` tests: `window_take_launch` first-launch path would need a test of `main.rs`'s argument handling, which is outside the window harness; probably not worth a unit test.
- Screenshot checks rather than tests: `window_set_titlebar_height`, `window_toggle_zoom` (menu Zoom), `window_is_fullscreen` (not built, confirm visually).
- Also found: `app-gpui/src/workbench/app_actions/tests/proofs6_app_actions.rs` (New Project with Git) is not declared as a module, so it never compiles or runs; declaring it (`mod proofs6_app_actions;` in `app_actions.rs` tests) would cover the Git half of `scaffold_create`/`vcs_init`, if real `git` under the test home is allowed.

### Not built, by feature

- Profiler: `profiler_collect`, `profiler_gc_before_stop`, `profiler_prepare` (models only, `features_b/profiler.rs`).
- Structural search/replace: `prepare_workspace_edit`, `structural_languages`, `structural_search`, `structural_validate` (menu entries without handlers).
- Update Running Application and Tomcat: `run_application_build`, `run_application_publish`, `run_tomcat_artifacts`, `run_tomcat_build`, `run_tomcat_home`, `run_tomcat_installations`, `run_tomcat_sync`, `run_tomcat_update` (step machine with no executor).
- Run configurations: `run_import_configs`, `run_import_sources`, `run_save_templates`, `run_system_env`, `run_test_reports`.
- New Project generators: `scaffold_catalog`, `scaffold_options`, `scaffold_requirements`.
- SDKs and Python/PHP settings: `sdk_jdk_at`, `sdk_php_composer_install`, `sdk_php_language_level`, `sdk_project_libraries` (External Libraries model unused), `sdk_python_interpreter`, `sdk_python_project_interpreters` (plus the four untestable ones above).
- Project and workspace: `project_rename`, `workspace_attach` (hidden from users), `workspace_create`, `workspace_discover_projects`, `workspace_update`.
- Settings import and notices: `settings_list_jetbrains_ides`, `settings_read_jetbrains`, `settings_removed_theme`, `show_system_notification`, `system_probe_hotkeys`.
- Spelling: `spell_check`, `spell_suggest`.
- SSH: `ssh_askpass_answer` (model only), `ssh_open_terminal`, `ssh_password_saved`.
- Tools page: `tools_install`, `tools_list`, `tools_uninstall`, `tools_updates`.
- Deployment: `remote_download_into`.
- Git: `vcs_clone`, `vcs_compare_paths`, `vcs_delete_tag`, `vcs_restore_tag`, `vcs_file_mode`, `vcs_ignore_blame_revision`, `vcs_init` (VCS › Create Git Repository), `vcs_move_worktree`, `vcs_revision_children`, `vcs_revision_files`, `vcs_stage_hunks`, `vcs_stash_drop`.
- Web: `web_browsers`, `web_goto`, `web_open` (menu row without handler), `web_serves_page`, `web_unresolved_references`, `web_url`.
- Windows: `window_complete_launch`, `window_flash_taskbar` (decision only, no flash), `window_group_close`, `window_group_close_tabs`, `window_group_current`, `window_group_detach`, `window_group_merge`, `window_group_reorder`, `window_group_restore`, `window_group_select`, `window_is_fullscreen`, `window_open_comparison`, `window_open_file_in_new_window`, `window_open_projects`, `window_open_remote`.
- Remote/WSL: `wsl_resolve` (refused on purpose).
