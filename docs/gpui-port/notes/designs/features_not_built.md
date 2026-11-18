# Backend commands with no native behaviour, by feature

Merged from the three readings of the empty backend-command rows (`ipc_rows_read_1.md`, `_2.md`, `_3.md` beside this file, which hold for each command what the webview's user sees at every call site). This is the work list for feature writers: a feature is picked by the rows it raises, after checking that its backend function runs no program of the machine and needs no network a test cannot stand in for.

## From batch 1

- Agents › changes review: agent_accept_changes, agent_base_text, agent_grouped_changes, agent_rollback_changes.
- Agents › side conversations and external sessions: agent_create_ephemeral, agent_external_sessions, agent_import_session.
- Agents › queue and background tasks (dispatch exists, no UI): agent_send_queued_now, agent_stop_background, agent_unqueue.
- Agents › New Task page checks: agent_probe, agent_refresh_models.
- Agents › MCP servers (page and per-task menu): agent_mcp_discover_sources, agent_mcp_import, agent_mcp_import_selected, agent_mcp_list, agent_mcp_remove, agent_mcp_save, agent_mcp_save_json, agent_mcp_set_enabled, agent_mcp_test, agent_set_mcp_overrides.
- Settings › Agents (drafts at HEAD, not compiled): agent_custom_import, agent_custom_remove, agent_custom_save, agent_registry_install, agent_registry_list, agent_registry_remove, agent_set_command, agent_set_env.
- Code analysis: analysis_locate_duplicates, doc_python_facts.
- Android Logcat: android_devices, android_logcat.
- Updates (model at `app_models/updates.rs`, not wired): app_check_update, app_install_update, app_release_notes, app_update_announcement, app_update_status, app_update_window_ready.
- App and Help actions: app_cli_install, app_cli_uninstall, app_log_location.
- Memory / caches: app_metrics, clear_caches, caches_local_data_info, caches_set_local_data_dir.
- Build: build_compile_file.
- OS clipboard file list (project tree, Markdown paste): clipboard_file_list, clipboard_has_files, clipboard_has_image, clipboard_set_file_list.
- Coverage: coverage_collect, coverage_import, coverage_prepare.
- Cucumber: cucumber_sources.
- Database: db_consoles, db_database_catalog, db_retain_catalog, db_saved_catalog (Snowflake catalog), db_session_context, db_test_tunnel; plus db_connect's plugin connect site.
- Debugger: debug_completions, debug_free_port, debug_send_input, debug_unity_processes.
- Debugger › heap snapshots: debug_heap_capture, debug_heap_classes, debug_heap_close, debug_heap_compare, debug_heap_dominators, debug_heap_open, debug_heap_path, debug_heap_save.
- Debugger › JavaScript (network, overrides, event breakpoints, profiler): debug_js_custom_breakpoints, debug_js_response_body, debug_js_response_overrides, debug_js_start_profile, debug_js_stop_profile.
- Debugger › JVM memory view: debug_jvm_memory_classes, debug_jvm_memory_fields, debug_jvm_memory_instances, debug_jvm_memory_referrers.
- Dependencies (model at `features_b/dependencies.rs`, not wired): dependency_versions.
- Diff: diff_external_tool.
- Documents: doc_open_untitled, doc_save_as, doc_set_bom, doc_snapshot, doc_sync_from_disk, doc_ui_mode (the preview site; the function is used for structural editing instead); plus doc_open_virtual's editor-tab site.
- Services › Docker (store models exist; the window carries out only settings, snapshots and refreshes): docker_compose_op, docker_container_op, docker_container_stats, docker_container_top, docker_events, docker_image_history, docker_inspect, docker_logs, docker_prune, docker_pull.

## From batch 2

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

## From batch 3

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
