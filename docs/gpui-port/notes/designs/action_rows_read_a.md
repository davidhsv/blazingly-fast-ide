# Partial action rows (set A): which clauses an existing native test closes

Read at HEAD 444f30969 with `git grep`/`git show`; no build or test run. Test names are lines of rust_tests.txt.

## `action:(dynamic) src/features/plugins/points.tsx:200`

- Toolbar button shows the click was taken while the command runs: OPEN, not built. The webview registers plugin commands with `indicatesRunning: true` (`src/features/plugins/points.tsx:213`) so its toolbar button shows it is running. Grepped `indicates_running|in_flight|pending_command|running_command|spinning|busy` under `app-gpui/src/workbench/plugin_contributions*`, `context_menus*`, `title_bar*`, `plugins*` and `platform/actions.rs`: no running state for a plugin command's button. The only test of the button, `workbench::context_menus::tests::proofs7_toolbar_slots::a_plugins_main_toolbar_entry_is_a_title_bar_button_that_runs_its_command`, asserts only that the command ran.
- Verdict: nothing closed.

## `action:(dynamic) src/features/templates/index.ts:81`

- Plugin-contributed file templates in effect (in lists, menus, actions): OPEN, not built. The webview's template list includes "user or plugin templates" (`src/features/templates/index.ts:97`, actions from `fileTemplates()` at `:71` and `newFileAction` `:81`). Natively `templates_in_effect` (`app-gpui/src/workbench/template_files.rs:61`) builds the set from `FileTemplates::load` over UI storage only; `git grep -i plugin` in `workbench/template_actions.rs` and `template_files.rs` finds no plugin source (only `plugin_text.rs:122` translates a `fileTemplates` contribution's name). The test `workbench::tests::proofs34_template_files::a_plugin_templates_package_goes_into_the_body_never_into_the_path` hands a plugin-shaped template to the creation step; it does not make a plugin's template be in effect.
- Actions synced again when a window closes without closing its project: OPEN, not built. `sync_template_actions` is deferred only from `close_project` (`app-gpui/src/workbench/mod.rs:3251`), project open (`mod.rs:3524`) and UI-storage changes (`template_actions.rs:63`); the window-release path removes the workbench (`mod.rs:1180`) without calling it.
- Verdict: nothing closed.

## `action:agents.dockTask`

- A conversation shown as an editor tab (webview `dockTask(arg)`, `src/features/agents/taskActions.ts:265`, docks the task and records the tool-window placement, closing its editor tab when it was one): OPEN, not built. Natively the handler exists (`app-gpui/src/workbench/agents.rs:301`) but `agent_editors` (`app-gpui/src/workbench/agents.rs:476`) reports an editor area in which no tab is a conversation (`Editors { active: ..AgentTab::Other, open: Vec::new() }`), and `git grep -i 'AgentEditor|TabKind::Agent|EditorKind::Agent'` under `app-gpui/src` finds no conversation editor kind. The cited test asserts the opposite of the webview on purpose: `"{action} leaves the conversation docked"` (`app-gpui/src/workbench/mod.rs:9050`). The row's text is accurate.
- Verdict: nothing closed.

## `action:agents.openTask`

- A conversation shown as an editor tab (webview `openTask(arg)`, `src/features/agents/taskActions.ts:256`, opens the task where sessions are placed, an editor tab when the placement is Editor): OPEN, not built. Natively the handler exists (`app-gpui/src/workbench/agents.rs:297`) but `agent_editors` (`app-gpui/src/workbench/agents.rs:476`) reports an editor area in which no tab is a conversation (`Editors { active: ..AgentTab::Other, open: Vec::new() }`), and `git grep -i 'AgentEditor|TabKind::Agent|EditorKind::Agent'` under `app-gpui/src` finds no conversation editor kind. The cited test asserts the opposite of the webview on purpose: `"{action} leaves the conversation docked"` (`app-gpui/src/workbench/mod.rs:9050`). The row's text is accurate.
- Verdict: nothing closed.

## `action:agents.openTaskInEditor`

- A conversation shown as an editor tab (webview `openTaskInEditorLocation(arg)`, `src/features/agents/taskActions.ts:247`, opens the task in an editor tab and makes Editor the placement): OPEN, not built. Natively the handler exists (`app-gpui/src/workbench/agents.rs:314`) but `agent_editors` (`app-gpui/src/workbench/agents.rs:476`) reports an editor area in which no tab is a conversation (`Editors { active: ..AgentTab::Other, open: Vec::new() }`), and `git grep -i 'AgentEditor|TabKind::Agent|EditorKind::Agent'` under `app-gpui/src` finds no conversation editor kind. The cited test asserts the opposite of the webview on purpose: `"{action} leaves the conversation docked"` (`app-gpui/src/workbench/mod.rs:9050`). The row's text is accurate.
- Verdict: nothing closed.

## `action:agents.quoteReply`

- The session's selection toolbar has Quote: OPEN, not built. In the webview, selecting text in a reply shows a floating toolbar whose Reply button runs `agents.quoteReply` with the selected text (`src/features/agents/SelectionToolbar.tsx:141`). Natively the action quotes (`app-gpui/src/workbench/agents.rs:374`) and a per-reply Quote Reply button quotes the whole reply (`app-gpui/src/agents/session_view.rs:1126`), but `git grep -i 'selection.toolbar|selection_toolbar|side conversation'` under `app-gpui/src` finds no selection toolbar. The cited test runs the action by the registry only (`app-gpui/src/workbench/mod.rs:9005`).
- Verdict: nothing closed.

## `action:agents.undockTask`

- A conversation shown as an editor tab (webview `undockTask()`, `src/features/agents/taskActions.ts:274`, moves the docked conversation into an editor tab): OPEN, not built. Natively the handler exists (`app-gpui/src/workbench/agents.rs:308`) but `agent_editors` (`app-gpui/src/workbench/agents.rs:476`) reports an editor area in which no tab is a conversation (`Editors { active: ..AgentTab::Other, open: Vec::new() }`), and `git grep -i 'AgentEditor|TabKind::Agent|EditorKind::Agent'` under `app-gpui/src` finds no conversation editor kind. The cited test asserts the opposite of the webview on purpose: `"{action} leaves the conversation docked"` (`app-gpui/src/workbench/mod.rs:9050`). The row's text is accurate.
- Verdict: nothing closed.

## `action:agents.useEditor`

- A conversation shown as an editor tab (webview `setSessionLocation("editor")`, `src/features/agents/taskActions.ts:240`, moves the conversation on show into an editor tab): OPEN, not built. Natively the handler exists (`app-gpui/src/workbench/agents.rs:288`) but `agent_editors` (`app-gpui/src/workbench/agents.rs:476`) reports an editor area in which no tab is a conversation (`Editors { active: ..AgentTab::Other, open: Vec::new() }`), and `git grep -i 'AgentEditor|TabKind::Agent|EditorKind::Agent'` under `app-gpui/src` finds no conversation editor kind. The cited test asserts the opposite of the webview on purpose: `"{action} leaves the conversation docked"` (`app-gpui/src/workbench/mod.rs:9050`). The row's text is accurate.
- Verdict: nothing closed.

## `action:agents.useToolWindow`

- A conversation shown as an editor tab (webview `setSessionLocation("toolWindow")`, `src/features/agents/taskActions.ts:233`, moves a conversation out of its editor tab into the tool window; natively the move to the tool window happens, but there is never an editor-tab conversation to move out of): OPEN, not built. Natively the handler exists (`app-gpui/src/workbench/agents.rs:288`) but `agent_editors` (`app-gpui/src/workbench/agents.rs:476`) reports an editor area in which no tab is a conversation (`Editors { active: ..AgentTab::Other, open: Vec::new() }`), and `git grep -i 'AgentEditor|TabKind::Agent|EditorKind::Agent'` under `app-gpui/src` finds no conversation editor kind. The cited test asserts the opposite of the webview on purpose: `"{action} leaves the conversation docked"` (`app-gpui/src/workbench/mod.rs:9050`). The row's text is accurate.
- Verdict: nothing closed.

## `action:app.about`

- The logo: OPEN, not built. The webview draws the 64px app icon (`src/features/app/AboutDialog.tsx:87`); the native header row (`app-gpui/src/app_views/about_dialog.rs:207-230`) has only text children, no image or icon.
- The Keymap Reference link: OPEN, testable, and WRONG TEXT in part. The link is drawn when `help.keymapReference` is registered (`app-gpui/src/app_views/about_dialog.rs:162`) and its event runs the action from the window (`app-gpui/src/workbench/app_actions.rs:735-739`). "No test reads or clicks it" is false: `app_views::about_dialog::tests::the_links_report_what_they_open` clicks it, `click(cx, "about-link-keymap")`, and asserts `AboutEvent::RunAction(KEYMAP_REFERENCE)` (`about_dialog.rs:311-317`). That is a view test with the action list handed in; no window test clicks the link and ends in Keymap Reference shown (`workbench::app_actions::tests::about_shows_the_version_of_the_build`, `app_actions.rs:1623`, checks only the product, the facts and Close).
- Show Log (help.showLog) not registered: OPEN, not built. `git grep '"help.showLog"'` finds it only in the menu spec (`app-gpui/src/workbench/menu_spec.rs:822`) and the About constant; no `registry.handle`/registration. The view test asserts its absence: `assert!(cx.debug_bounds("about-link-log").is_none(), "Show Log is not registered")` (`about_dialog.rs:310`).
- Verdict: nothing closed.

## `action:app.restart`

- The relaunch itself: OPEN, testable only outside a test build. The behaviour exists: without a `Relaunch` global, `relaunch` calls `restart_app`, which is `cx.restart()` in a non-test build (`app-gpui/src/workbench/app_actions.rs:77-87`); in a test build `restart_app` panics on purpose (`app_actions.rs:92-95`), so no `#[gpui_kit::test]` can assert it. `workbench::app_actions::tests::just_restart_cancels_a_pending_invalidation_cancel_does_nothing` asserts only the recorder count. Text accurate.
- Verdict: nothing closed.

## `action:build.buildArtifacts`

- A caller can wait for the build's end: OPEN, not built. The webview returns the build's promise when the argument has `wait: true` (`src/features/build/index.ts:126-127`). Natively every build handler only calls `start_build` and returns (`app-gpui/src/workbench/build_window.rs:47-81`); the registry offers no completion to wait on, and the run driver reads `build.lastStatus` right after `execute_action` returns (`app-gpui/src/run/driver.rs:896-899`). Hypothesis (not run): a before-launch build therefore reads no status and reports "Build did not start"; check with a window test whose configuration has a Build before-launch task. Side note: the native handler passes no module target (`build_window.rs:79`, `None`) where the webview packages `moduleTarget(arg)` (`index.ts:126`).
- Verdict: nothing closed.

## `action:build.buildModule`

- A caller can wait for the build's end: OPEN, not built. The webview returns the build's promise when the argument has `wait: true` (`src/features/build/index.ts:85-86`). Natively every build handler only calls `start_build` and returns (`app-gpui/src/workbench/build_window.rs:47-81`); the registry offers no completion to wait on, and the run driver reads `build.lastStatus` right after `execute_action` returns (`app-gpui/src/run/driver.rs:896-899`). Hypothesis (not run): a before-launch build therefore reads no status and reports "Build did not start"; check with a window test whose configuration has a Build before-launch task.
- Verdict: nothing closed.

## `action:build.buildProject`

- A caller can wait for the build's end: OPEN, not built. The webview's handler always awaits `startBuild` ("Awaitable: Run's Build before launch task waits", `src/features/build/index.ts:63-72`). Natively every build handler only calls `start_build` and returns (`app-gpui/src/workbench/build_window.rs:47-81`); the registry offers no completion to wait on, and the run driver reads `build.lastStatus` right after `execute_action` returns (`app-gpui/src/run/driver.rs:896-899`). Hypothesis (not run): a before-launch build therefore reads no status and reports "Build did not start"; check with a window test whose configuration has a Build before-launch task.
- Verdict: nothing closed.

## `action:build.rebuildProject`

- A caller can wait for the build's end: WRONG TEXT. The webview's Rebuild does not offer it: `run: () => { ... void startBuild("rebuild", ...) }` returns nothing to wait on (`src/features/build/index.ts:110-113`). Natively it starts the rebuild into the Build window (`app-gpui/src/workbench/build_window.rs:70-74`), which is what the webview does. No clause remains open on this ground.
- Verdict: nothing closed (the only clause is wrong text; with it removed the row has no missing clause in its text).

## `action:code.cleanupFile`

- Its reformat step never uses a plugin-contributed formatter: OPEN, not built. The webview's `format` prefers the formatter a plugin contributes to the editor's language (`contributedAt(view.state, CONTRIBUTED_FORMAT, 0)` then `applyContributedFormat`, `src/editor/lsp/format.ts:119-122`). Natively `git grep -i 'contributed_format|CONTRIBUTED_FORMAT|plugin_format'` under `app-gpui/src` finds only the settings model of `languages.pluginFormatters` (`app-gpui/src/settings_model/language_pages.rs`, `settings_view/pages/languages_controls.rs:480`) and a model test in `editor_lsp/format.rs:349`; no formatting path reads it.
- Verdict: nothing closed.

## `action:code.completeStatement`

- With the completion lookup open, its chosen item is inserted first: OPEN, not built. The webview accepts the active completion before completing the statement (`if (completionStatus(view.state) === "active") acceptCompletion(view);`, `src/editor/core/actions.ts:385`). Natively `complete_statement` (`app-gpui/src/editor/structure/commands.rs:118-124`) goes straight to the text rules or the structural request; `git grep -i 'complete_statement|completeStatement'` under `app-gpui/src` finds no lookup acceptance on that path.
- Verdict: nothing closed.

## `action:code.completion`

- Tab through a snippet's fields: CLOSED. `editor::intelligence::tests::proofs33_template_keys::tab_walks_the_placeholders_of_a_servers_snippet_item`, `app-gpui/src/editor/intelligence/tests/proofs33_template_keys.rs:116`: `assert_eq!(selected(cx), "b", "Tab moved to the second placeholder");` (after Enter accepts the server's snippet item `call(${1:a}, ${2:b})$0` from the lookup and typing replaces the first field, `:110-115`).
- Live templates in the list: OPEN, not built. The server's answer becomes the list with only `LookupItem::Lsp` items (`app-gpui/src/editor/intelligence.rs:553-554`, `completion_options(..., &[])`); `LookupItem::Contributed` rows are built only by Insert Live Template's own lookup (`app-gpui/src/workbench/editor_templates.rs:107`).
- Contributed rows in the list: OPEN, not built. Same place: the extra-items argument of `completion_options` is `&[]` (`intelligence.rs:553`); `apply_contributed` exists (`editor_lsp/completion.rs:526`) but nothing feeds contributed rows into Code Completion's list.
- Word completion without a server: OPEN, not built. When the server gives no list (`Answered::Fallback | Answered::Nothing`) the lookup closes (`intelligence.rs:539-543`). The word-completion model (`editor_commands::word_completion`) backs a different action (cyclic expand word); it is not used as the lookup's fallback.
- Verdict: some closed.

## `action:code.expressionTypeInfo`

- The type in the documentation popup as highlighted code: OPEN, not built. The webview opens a `kind: "doc"` popup whose entry is the type as a fenced code block in the file's language (`src/editor/lsp/typeInfo.ts:67-78`). Natively `expression_type_info` calls `hint(&editor, info.text, ...)` (`app-gpui/src/workbench/editor_extras.rs:446`), a plain one-line hint. Text accurate.
- Verdict: nothing closed.

## `action:code.moveStatementDown`

- Moving a statement towards a block moves it into the block, re-indented, in the languages the webview parses itself: OPEN, not built in the window. The webview's Lezer planner moves "into a block when the neighbour is a block header or closing brace ... The moved lines take the indentation of their new place" (`src/editor/core/cm/editing.ts:411-418`), used whenever the document has a Lezer tree (`src/editor/core/actions.ts:365-366`). Natively a port of that planner exists (`plan_statement_move` with `shift_to_indentation`, `app-gpui/src/editor_commands/editing.rs:682-760`), but the window path calls it only with `SyntaxTree::default()` (several carets or a selection, `app-gpui/src/editor/structure/commands.rs:105-112`); with one caret it takes the backend's structural ranges, which pass a neighbouring block whole. Its tests (`editor_commands::editing::tests::moves_a_multi_line_statement_as_a_whole_never_one_of_its_lines`) are model tests and none is named for moving into a block. Text accurate.
- Verdict: nothing closed.

## `action:code.moveStatementUp`

- Moving a statement towards a block moves it into the block, re-indented, in the languages the webview parses itself: OPEN, not built in the window. The webview's Lezer planner moves "into a block when the neighbour is a block header or closing brace ... The moved lines take the indentation of their new place" (`src/editor/core/cm/editing.ts:411-418`), used whenever the document has a Lezer tree (`src/editor/core/actions.ts:365-366`). Natively a port of that planner exists (`plan_statement_move` with `shift_to_indentation`, `app-gpui/src/editor_commands/editing.rs:682-760`), but the window path calls it only with `SyntaxTree::default()` (several carets or a selection, `app-gpui/src/editor/structure/commands.rs:105-112`); with one caret it takes the backend's structural ranges, which pass a neighbouring block whole. Its tests (`editor_commands::editing::tests::moves_a_multi_line_statement_as_a_whole_never_one_of_its_lines`) are model tests and none is named for moving into a block. Text accurate.
- Verdict: nothing closed.

## `action:code.parameterInfo`

- Tab and Shift+Tab between arguments: OPEN, not built in the window. The parameter-info view takes Tab/Shift+Tab only when an owner hands it `ArgumentKeys` (`app-gpui/src/editor_lsp/signature_view.rs:66`), and `git grep set_argument_keys` finds no caller outside the view's own test (`signature_view.rs:313`). `editor_lsp::signature_view::tests::escape_closes_it_and_tab_goes_to_the_owner_while_it_shows` and `editor_lsp::signature::tests::tab_moves_while_parameter_info_is_shown_and_is_left_alone_otherwise` drive the view and the model with a stand-in owner, not an editor in a window. Text accurate.
- Verdict: nothing closed.

## `action:code.pasteFromHistory`

- The preview's line numbers and special-character marks: OPEN, not built. The webview's preview is a read-only CodeMirror view with `lineNumbers()` and `highlightSpecialChars()` (`src/editor/core/view/PasteHistoryDialog.tsx:232-236`). The native preview draws the row's lines as styled text in the target editor's font and grammar (`app-gpui/src/editor/paste_history_view/preview.rs:1-3`, `:163`); `git grep -i 'line.number|gutter|special|whitespace|marks'` in `paste_history_view.rs` and `preview.rs` finds no line numbers or marks. Text accurate.
- Verdict: nothing closed.

## `action:code.quickDocumentation`

- The Documentation popup with its history and tool window: OPEN, not built in the window. The view exists: `QuickDocView` with Back, Forward, Edit Source and Show in Documentation Tool Window (`app-gpui/src/editor_lsp/hover_view.rs:557-572`), tested only as a view (`editor_lsp::hover_view::tests::moves_the_popup_to_the_documentation_tool_window`). `git grep QuickDocView` finds no constructor outside `hover_view.rs`'s tests (`:1002`); the action runs `quick_documentation`, which asks the hover (`app-gpui/src/editor/intelligence.rs:1206-1212`). Text accurate.
- Verdict: nothing closed.

## `action:code.reformat`

- A plugin-contributed formatter for the language is used first: OPEN, not built. The webview prefers it (`src/editor/lsp/format.ts:119-122`). Natively `git grep -i "contributed_format|CONTRIBUTED_FORMAT|plugin_format"` under `app-gpui/src` finds only the settings model of `languages.pluginFormatters` (`app-gpui/src/settings_model/language_pages.rs`, `app-gpui/src/settings_view/pages/languages_controls.rs:480`) and a model test (`app-gpui/src/editor_lsp/format.rs:349`); no formatting path reads it.
- Verdict: nothing closed.

## `action:code.reformatDialog`

- Optimize imports left out when a plugin's formatter formats the file: OPEN, not built. The webview passes `canOptimizeImports: !contributedAt(view.state, CONTRIBUTED_FORMAT, 0)` (`src/editor/lsp/format.ts:283`). Natively the dialog supports the flag (`app-gpui/src/editor_lsp/reformat_dialog.rs:74-92`) but the window always passes `true`: `ReformatDialog::new(&name, has_selection, true, initial, cx)` (`app-gpui/src/workbench/editor_extras.rs:486`), and no plugin formatter exists natively to decide it. Text accurate.
- Verdict: nothing closed.

## `action:code.reformatFile`

- A plugin-contributed formatter is used: OPEN, not built. Same as Reformat Code (`src/editor/lsp/format.ts:119-122`). Natively `git grep -i "contributed_format|CONTRIBUTED_FORMAT|plugin_format"` under `app-gpui/src` finds only the settings model of `languages.pluginFormatters` (`app-gpui/src/settings_model/language_pages.rs`, `app-gpui/src/settings_view/pages/languages_controls.rs:480`) and a model test (`app-gpui/src/editor_lsp/format.rs:349`); no formatting path reads it.
- Verdict: nothing closed.

## `action:code.showIntentions`

- The bulb: OPEN, not built in the window. `BulbView` exists (`app-gpui/src/editor_lsp/intentions_view.rs:943`) with a view test (`editor_lsp::intentions_view::tests::the_bulb_shows_red_for_a_fix_and_asks_for_the_intentions`), but `git grep 'BulbView|bulb_for('` finds no use outside `intentions_view.rs`/`intentions.rs`: no editor draws it.
- The IDE's own intentions: OPEN, not built. The webview merges `intentionProviders` contributed by features (cucumber, dependencies, Spring HTTP: `src/features/cucumber/index.ts:185`, `src/features/dependencies/index.ts:174`, consumed in `src/editor/lsp/intentions.ts`); `git grep -i 'intention_provider|IntentionProvider'` under `app-gpui/src` finds nothing.
- The editor's underline shown to follow a disabled inspection: OPEN, testable. `editor::intelligence::tests::proofs_editor_lsp::a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls` ends at the stored profile (`proofs_editor_lsp.rs:372`, `assert!(format!("{:?}", inspections(cx)).contains("MOCK001"), ...)`). `editor::intelligence::tests::proofs_editor_lsp::the_editor_hides_and_restyles_a_servers_problems_by_the_inspection_profile` proves the editor filters a server's problems by the profile, but only for diagnostics that arrive after the profile is set (`proofs_editor_lsp.rs:390-405`); no test asserts that the problem already underlined disappears after Disable inspection.
- Verdict: nothing closed.

## `action:db.ddlToClipboard`

- The password question and retry around the request: OPEN, not built. The webview wraps both DDL requests in `withConnection` (`src/features/database/treeActions.ts:109`, `:113`), which asks for a password or host-key trust and retries (`src/features/database/store.ts:517-539`). Natively the DDL request goes through `db_call` (`app-gpui/src/workbench/database_extras.rs:744-756`), which authorizes and runs the call and hands any error to `then` (`app-gpui/src/workbench/database.rs:1174-1199`); the error ends in the notice "Cannot generate the DDL" (`database_extras.rs:753`). The password question and retry exist natively only on the store's own requests (`Prompt::Password`, `database.rs:1255`; proven for expanding the tree by `workbench::tests::proofs20_database::the_password_a_connection_asks_for_reaches_the_backend_and_the_retry_connects`), not on `db_call`.
- Verdict: nothing closed.

## `action:db.ddlToConsole`

- The password question and retry around the request: OPEN, not built. Same as Generate DDL to Clipboard (`src/features/database/treeActions.ts:104-119`, `withConnection`). Natively the DDL request goes through `db_call` (`app-gpui/src/workbench/database_extras.rs:744-756`), which authorizes and runs the call and hands any error to `then` (`app-gpui/src/workbench/database.rs:1174-1199`); the error ends in the notice "Cannot generate the DDL" (`database_extras.rs:753`). The password question and retry exist natively only on the store's own requests (`Prompt::Password`, `database.rs:1255`; proven for expanding the tree by `workbench::tests::proofs20_database::the_password_a_connection_asks_for_reaches_the_backend_and_the_retry_connects`), not on `db_call`.
- Verdict: nothing closed.

## `action:db.editData`

- The table editor opens in an editor tab: OPEN, not built. The webview opens the table's input in the editor area (`openTableData` → `editorService().openInput(tableInput(...), { focus: true })`, `src/features/database/treeActions.ts:92-94`, run by `db.editData` at `:291-299`). Natively `open_table_editor` selects or adds a `Lower::Table` page of the Database panel (`app-gpui/src/workbench/database.rs:1865-1880`), and the tests reach it as `w.database_table_editor(cx)`, the panel's active editor (`database.rs:939-942`). Text accurate.
- Verdict: nothing closed.

## `action:db.explainPlan`

- The project change while the data sources load, driven for this action: CLOSED. The webview drops a database request whose project changed (`projectRequest`/`databaseRequest`, `src/features/database/requests.ts:20-31`). `workbench::tests::proofs43_database_results::explain_plan_waiting_for_the_data_sources_is_dropped_when_the_project_changes` runs `db.explainPlan` while the first project's sources load, opens a second project, and asserts at `app-gpui/src/workbench/tests/proofs43_database_results.rs:160`: `assert!(workbench.read_with(cx, |w, _| w.database_plan()).is_none(), "the first project's plan is not shown");` then the second project's own Explain Plan shows its statement (`:166`). The row's "Read, not tested" sentence is superseded.
- Verdict: all clauses closed.

## `action:db.goToColumn`

- Confirming the cell being edited before Go to Column: CLOSED for a console result grid. The webview's `activeTableEditor` blurs the cell editor first so its value counts (`src/features/database/tableEditorFocus.ts:90-93`). `workbench::tests::proofs43_database_results::go_to_column_confirms_the_result_cell_being_edited_first` edits a result cell to `amy`, runs `db.goToColumn`, and asserts at `app-gpui/src/workbench/tests/proofs43_database_results.rs:238-241`: `assert!(results.read_with(cx, |results, _| results.is_dirty()), "the typed value waits to be submitted");` and `assert!(results.read_with(cx, |results, _| results.columns_query().is_some()), "the column list opens");`. Not driven: the same in the Edit Data table editor (the webview's rule covers both); the row's text names only the console grid, so I count the clause closed.
- Verdict: all clauses closed.

## `action:db.importData`

- The password question and retry around the request: OPEN, not built, and WRONG TEXT in part. In the webview only the dialog's column load goes through `withConnection` (`ImportDialog` → `loadDetails`, `src/features/database/DataDialogs.tsx:138-141`, `src/features/database/store.ts:738`); the import itself calls `commands.dbApplyEdits` directly with no password retry (`DataDialogs.tsx:176`). So "around the request" holds for the column read only. Natively both the column read (`"db_table_details"`, `app-gpui/src/workbench/database_extras.rs:863`) and the insert (`"db_apply_edits"`, `:934`) go through `db_call`, which has no password question (`app-gpui/src/workbench/database.rs:1174-1199`).
- Verdict: nothing closed.

## `action:db.jumpToConsole`

- With several consoles of the source, a chooser lists them and New Query Console: OPEN, not built. The webview's `jumpToConsole` opens the only console directly, else `chooseItem` with the consoles' names plus New Query Console (`src/features/database/treeActions.ts:548-556`). Natively `TreeAction::OpenConsole | TreeAction::JumpToConsole` share one branch, `store.open_console(&target)` (`app-gpui/src/workbench/database.rs:1659-1664`); `git grep -i 'Query Consoles|newConsole|new_console'` finds no chooser. Text accurate.
- Verdict: nothing closed.

## `action:db.modifyTable`

- The data-type suggestions of the type field: OPEN, not built. The webview's type field offers the driver's types through a `<datalist>` (`src/features/database/ObjectDialogs.tsx:485`, `:538-542`, `TYPES[driver]`). In `app-gpui/src/database/modify_table_dialog.rs`, `git grep -i "type"` filtered for `popup|list|complet|menu|option|choose` finds no suggestion list for the type field.
- Verdict: nothing closed.

## `action:db.newColumn`

- The data-type suggestions: OPEN, not built. The webview's type field offers the driver's types through a `<datalist>` (`src/features/database/ObjectDialogs.tsx:485`, `:538-542`, `TYPES[driver]`). In `app-gpui/src/database/modify_table_dialog.rs`, `git grep -i "type"` filtered for `popup|list|complet|menu|option|choose` finds no suggestion list for the type field.
- Verdict: nothing closed.

## `action:db.newDataSource`

- A test that the dialog of one project does not act on the next: OPEN, testable. The webview ties database requests to the project's lifetime (`src/features/database/requests.ts:20-31`). Natively the Data Source dialog is held in the Database slot (`source_dialog: Option<SourceDialogSlot>`, `app-gpui/src/workbench/database.rs:719`), which `reset_database` drops when the project goes (`database.rs:874-875`), so the behaviour exists by construction; `rust_tests.txt` has no test that opens the dialog, changes project and asserts the dialog is gone or writes nothing (grepped `data_source` with `project|next|dialog`: only `workbench::database_extras::tests::trust_host_in_the_data_source_dialog_asks_the_backend_for_that_source` and `workbench::tests::dropping_a_data_source_asks_and_removes_it_from_the_project_and_the_tree`, neither changes project).
- Verdict: nothing closed.

## `action:db.newTable`

- The data-type suggestions: OPEN, not built. The webview's type field offers the driver's types through a `<datalist>` (`src/features/database/ObjectDialogs.tsx:485`, `:538-542`, `TYPES[driver]`). In `app-gpui/src/database/modify_table_dialog.rs`, `git grep -i "type"` filtered for `popup|list|complet|menu|option|choose` finds no suggestion list for the type field.
- Verdict: nothing closed.

## `action:db.openConsole`

- New ▸ Query Console in the + menu and the node menu creates another console (`db.newConsole`): OPEN, not built. The webview registers `db.newConsole` (`src/features/database/index.ts:243`) and uses it for New ▸ Query Console. Natively the tree's New ▸ Query Console item is `act(TreeAction::OpenConsole)` (`app-gpui/src/database/tree_view.rs:1205`, `:1239`, `:1260`, `:1271`), which opens the existing console (`app-gpui/src/workbench/database.rs:1659-1662`); `git grep '"db.newConsole"'` under `app-gpui/src` finds nothing. Text accurate.
- Verdict: nothing closed.

## `action:db.queryHistory`

- The project change while the data sources load, driven for this action: CLOSED. `workbench::tests::proofs43_database_results::query_history_waiting_for_the_data_sources_is_dropped_when_the_project_changes` runs `db.queryHistory` while the first project's sources load and opens a second project; `app-gpui/src/workbench/tests/proofs43_database_results.rs:177`: `assert!(workbench.read_with(cx, |w, _| w.database_history_dialog()).is_none(), "no history dialog opens");`, and the second project's history shows no row of the first (`:194`). The row's "Read, not tested" sentence is superseded.
- Verdict: all clauses closed.

## `action:db.rename`

- The key pressed with the Database view holding the keyboard, ending in the rename: OPEN, testable. The native handler exists and asks for the name (`workbench::tests::the_database_trees_actions_and_shortcuts_reach_the_store_and_the_backend` runs it by the registry only: `run_action(cx, "db.rename", ...)`, `app-gpui/src/workbench/mod.rs:13499`, then the Rename Data Source prompt). `platform::input::tests::the_eclipse_rename_key_renames_in_the_view_that_claims_it_before_editing_a_commit_message` presses Alt+Shift+R with `databaseViewFocus` set by hand on an empty window and stand-in handlers (`app-gpui/src/platform/input.rs:446-472`). No test presses the key in the real Database view and ends in the rename prompt.
- Verdict: nothing closed.

## `action:db.revertChanges`

- Confirming the cell being edited before Revert: CLOSED for a console result grid. `workbench::tests::proofs43_database_results::revert_confirms_the_result_cell_being_edited_and_drops_it` edits a result cell to `amy`, runs `db.revertChanges`, and asserts at `app-gpui/src/workbench/tests/proofs43_database_results.rs:259-261`: `assert!(!results.read_with(cx, |results, _| results.is_dirty()), "nothing waits to be submitted");` and `assert_eq!(grid.read_with(cx, |grid, _| grid.cell_text(0, 1)), "ann");`. Webview rule: `activeTableEditor` blurs the cell editor first (`src/features/database/tableEditorFocus.ts:90-93`). As for Go to Column, the Edit Data table editor case is not driven, but the row names only the console grid.
- Verdict: all clauses closed.

## `action:db.runSqlScript`

- The picker's .sql/.ddl filter: OPEN, not built. The native picker is `cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None })` (`app-gpui/src/workbench/database_extras.rs:968-969`); `PathPromptOptions` has no filter field. Text accurate.
- Verdict: nothing closed.

## `action:debug`

- Rerun in the debugger from Services: OPEN, not built. The webview's Services Debug item reruns a live debug session in place (`debug(r)`, `src/features/run/services.tsx:624`). Natively the model returns `ServiceCommand::RerunDebugger` (`app-gpui/src/run_testing/services.rs:579`, model test `services::model::tests::reruns_a_debug_service_in_place_from_the_services_panel`), but the window only logs it: `ServiceCommand::RerunDebugger(id) => tracing::info!(id = id.0, "Services: this window has no debugger")` (`app-gpui/src/workbench/services_window.rs:677`). Text accurate.
- Verdict: nothing closed.

## `action:debug.evaluate`

- A multi-line field: OPEN, not built. The native dialog's module doc says "The field is one line: an expression seeded from a selection over several lines is joined with spaces" (`app-gpui/src/debugger/evaluate_dialog.rs:9-10`); the field is a plain `TextField::new` (`:144`).
- Completion: OPEN, not built. `git grep -i completion` in `evaluate_dialog.rs` finds nothing.
- An expandable result: OPEN, not built. The result is drawn as one Variables-style row (`name = {type} value`, `evaluate_dialog.rs:1-2`); `git grep -i 'expand|tree|children'` finds only layout `.children(...)` calls (`:451`, `:453`).
- Verdict: nothing closed.

## `action:debug.pluginStart`

- Debug from the configuration chooser goes through the Java attach: OPEN, not built. The webview sends a chosen configuration through Run's debug flow, where "Maven / Gradle / scripts start the JVM with a JDWP agent" (`src/features/debugger/actions.ts:345-347`). Natively the waiting-JDWP-agent attach (`AgentDebug::start`) is reached only from a plugin's debug request (`app-gpui/src/workbench/plugin_java_debug.rs:58`, called from `plugin_contributions.rs:356`); `git grep AgentDebug` finds no other caller, and the chooser's `run.debugConfig` handler is in `app-gpui/src/workbench/debug.rs:187`. Text accurate.
- Verdict: nothing closed.

## `action:debug.quickEvaluate`

- The value as an expandable tree: OPEN, not built. The webview's value hint has a "+" that "expands the object into a variables tree in place" (`src/features/debugger/editor/valueTooltip.ts:52-58`, used by `quickEvaluate` at `src/features/debugger/actions.ts:211`). Natively `value_hint` computes `expandable: value.variables_reference > 0` (`app-gpui/src/debugger/editor/value_tooltip.rs:118`), but `git grep '\.expandable|expandable:'` finds no reader of it in the debugger: the hint is drawn as one line (`app-gpui/src/debugger/editor/quick_evaluate.rs:81`). Text accurate.
- Verdict: nothing closed.

## `action:debug.rerun`

- The rerun session takes its own tab's place: CLOSED. `workbench::debug::tests::proofs40_debug_guards::rerun_stops_the_live_session_first_and_takes_its_own_tabs_place`, `app-gpui/src/workbench/debug/tests/proofs40_debug_guards.rs:255-261`: after `run(cx, "debug.rerun")`, `assert!(!cx.has_pending_prompt(), "Rerun asks nothing");` and the waits (which fail the test on timeout) `long(cx, "the finished session's tab to be taken", &|cx| project.debug_tabs(cx) == ["main"]);`. The row's text already says so in its last sentences; its "missing:" clause is stale for this part.
- ...and keeps its pin: OPEN, not built. The webview returns the replaced tab's `pinned` (`src/features/debugger/state.ts:318-324`); `git grep -i pinned` under `app-gpui/src/debugger` (non-test) finds nothing.
- Verdict: some closed.

## `action:debug.smartStepInto`

- The dialog form: OPEN, not built in the window. The webview opens `openSmartStepChooser` when inline picking cannot start (`src/features/debugger/actions.ts:174`). Natively the debugger model opens an overlay chooser (`app-gpui/src/debugger/actions.rs:421-422`), but the window shows the calls as a popup menu of `SmartStepChoice` (`app-gpui/src/workbench/debug_extras.rs:241-262`), and the window test asserts there is no overlay: `assert!(!overlay, "the menu holds the choice; the debugger keeps no overlay open");` (`debug_extras.rs:555`).
- Inline picking in the editor: OPEN, not built in the window. `start_inline_smart_step` needs the paused line (`actions.rs:421`), and the driver always passes `None`: `debugger.step_in_targets_loaded(ticket, result, None, host)` (`app-gpui/src/debugger/driver.rs:745`). The inline model is tested on its own (`debugger::editor::smart_step::tests::highlights_the_calls_moves_with_arrows_and_steps_in_with_enter`).
- The direct step for exactly one call: OPEN, testable. Built: `if targets.len() <= 1 { return self.step_into(title, targets.first().map(|target| target.id), host); }` (`actions.rs:418-419`), matching the webview (`actions.ts:163-171`). Only the model bench drives it (`debugger::actions::tests::smart_step_into_chooser_for_several_calls_direct_step_for_one_step_into_without_support`); the window tests cover no calls and two calls, not one.
- Verdict: nothing closed.

## `action:debug.startConfig`

- The driver's call to the backend for the `.env` files run by a test: OPEN, testable. The driver carries out `BackendCall::RunEnvFiles` with `run::run_env_files` (`app-gpui/src/debugger/driver.rs:906-912`); the only tests are on the debugger's actions with a stand-in backend (`debugger::actions::tests::a_debug_launch_loads_the_configurations_env_files_under_its_own_variables`, `debugger::actions::tests::a_debug_launch_without_env_files_reads_none_and_one_whose_file_is_unreadable_does_not_start`); `rust_tests.txt` has no window test naming env files and debug.
- A JavaScript Debug (browser) configuration starts a browser debug session: OPEN, not built. The webview branches on `isBrowserConfig` (kind `js-browser`) to `startBrowser` (`src/features/debugger/actions.ts:355-357`, `src/features/debugger/browser.ts:10`, `:60`). `git grep -i 'js-browser|jsBrowser|JS_BROWSER'` under `app-gpui/src` finds nothing.
- Verdict: nothing closed.

## `action:deployment.configure`

- "That page is not built natively (it says it is not available yet)": WRONG TEXT. The Deployment servers page is built: `DeploymentServersView` is put in the `build.deployment` page's slot (`app-gpui/src/settings_view/custom_controls.rs:225-233`, `app-gpui/src/settings_view/pages/build_deployment.rs:45`). `workbench::deployment::tests::deployment_configuration_opens_the_deployment_settings_page` asserts the action opens Settings on that page (`app-gpui/src/workbench/deployment.rs:1973`: `assert_eq!(page.as_deref(), Some(SETTINGS_PAGE));`), and window tests drive the page itself, e.g. `workbench::tests::proofs28_settings_deployment::deployment_a_local_server_tested_by_the_backend_is_written_to_the_project`, whose `open_page` downcasts the page's control `.expect("the page has its Deployment servers editor")` (`app-gpui/src/workbench/tests/proofs28_settings_deployment.rs:55-60`) and asserts `assert_eq!(run_test(&view, cx), Some(TestState::Connected));` (`:125`). (`open_page` opens it with `file.settings` and the page id rather than `deployment.configure`; the action-to-page link is the first test's assertion.)
- Verdict: all clauses closed (the only missing clause is wrong text).

## `action:deployment.uploadTo`

- Choosing among several servers and uploading to the chosen one: OPEN, testable. Built: `("deployment.uploadTo", TransferDirection::Upload, Pick::Choose)` shares the chooser and transfer with Download from (`app-gpui/src/workbench/deployment.rs:498`). The chooser is driven only for download (`workbench::deployment::tests::download_from_chooses_the_server_and_replaces_the_local_file`) and sync (`workbench::deployment::tests::sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row`); the upload tests are `workbench::deployment::tests::upload_to_default_server_copies_the_file_to_the_server_and_reports_it` (default server, no chooser) and `workbench::deployment::tests::upload_to_explains_the_rejected_paths_and_links_settings` (no server). Text accurate.
- Verdict: nothing closed.

## `action:diff.applyLeftSide`

- In Show Diff, not only in a comparison opened by the compare actions: OPEN, not built. The webview runs it in any focused diff editor (`when: "diffEditorFocus"`, `focusedDiffNavigator()?.applyChange(...)`, `src/features/vcs/compare.tsx:262-269`). Natively `accept_diff_side` acts only on the compare dialog's viewer: `let Some(viewer) = self.diff_dialog().map(...) else { return };` (`app-gpui/src/workbench/diff_tools.rs:484-488`); Show Diff's window is not reached. Text accurate.
- Verdict: nothing closed.

## `action:diff.applyRightSide`

- In Show Diff, not only in a comparison opened by the compare actions: OPEN, not built. The webview runs it in any focused diff editor (`when: "diffEditorFocus"`, `focusedDiffNavigator()?.applyChange(...)`, `src/features/vcs/compare.tsx:271-278`). Natively `accept_diff_side` acts only on the compare dialog's viewer: `let Some(viewer) = self.diff_dialog().map(...) else { return };` (`app-gpui/src/workbench/diff_tools.rs:484-488`); Show Diff's window is not reached. Text accurate.
- Verdict: nothing closed.

## `action:diff.compareWith`

- Comparing folders (the row hidden on a folder): OPEN, not built. The project view offers Compare With… on a folder only when `diff.compareDirectories` is registered (`app-gpui/src/project_view/context_menu.rs:503`), and `git grep compareDirectories` finds no `registry.handle` for it (only its id, `app-gpui/src/vcs_diff/compare.rs:28`, and menu code). In a window, `workbench::diff_tools::tests::compare_with_asks_for_the_other_file_and_shows_both` asserts the absence: `assert!(!folder.contains(&"Compare With…".to_owned()), "{folder:?}");` and that a folder argument asks for nothing (`app-gpui/src/workbench/diff_tools.rs:960-963`). The view test `project_view::context_menu::tests::offers_directory_comparison_only_for_two_folders_and_compare_with_for_a_single_folder` shows the row on a folder only with the action registered by the test. Text accurate.
- Verdict: nothing closed.

## `action:edit.selectAll`

- Selects the focused reading pane's text: OPEN, not built. The webview's `selectAll` selects the text of the focused `[data-selection-scope]` pane (`selectReadingPane`, `src/features/app/appActions.ts:53`, `src/ui/useSelectionBoundary.ts:117-127`), which the agent session, the HTTP client and source previews mark (`useSelectionBoundary` in `src/features/agents/SessionEditor.tsx`, `src/features/http-client/HttpClientView.tsx`, `src/ui/SourcePreview.tsx`). Natively the Edit commands reach only `TextField`, `TextArea`, `SecretField`, `ProjectTree` and `ServicesView` (`impl EditTarget for …`, `app-gpui/src/workbench/edit_actions.rs:69-141`), then the editor.
- Verdict: nothing closed.

## `action:edit.undo`

- ⌘Z pressed in the tree: OPEN, not built. `git grep '"edit.undo"'` under `app-gpui/src` finds the handler (`app-gpui/src/editor/actions.rs:24`), the Edit routing (`app-gpui/src/workbench/edit_actions.rs:23`) and the Keymap page's list, but no default key binding; the tree passes no modified keys on (row text). No test presses ⌘Z with the Project view focused.
- The Project view's Undo of a move: CLOSED. `workbench::project_view_actions::tests::proofs20_tree::undo_in_the_project_view_takes_back_its_last_move_and_rename` moves a file with `project.move`, runs `edit.undo` with `projectViewFocus` on, and asserts at `app-gpui/src/workbench/project_view_actions/tests/proofs20_tree.rs:89-91`: `wait_long(cx, "the file to move back", |_| data.is_file() && !moved.exists());` and `assert_eq!(std::fs::read_to_string(&data).expect("the file is back"), "{}\n");` (and the rename's undo at `:107-111`).
- Verdict: some closed.

## `action:editor.copyPathReference`

- Path From Source Root in the popup: OPEN, not built in this popup. `show_copy_path_popup` passes no source roots: `copy_path_items(&path, line, project, &[], &self.shortcuts(cx))` (`app-gpui/src/workbench/project_files.rs:1402`), though the window knows them (`settings::effective(...).project_structure.source_roots`, used by `project.copySourceRootPath`, `app-gpui/src/workbench/project_view_actions.rs:169`). Text accurate.
- Verdict: nothing closed.

## `action:editor.decreaseFontSize`

- The focused font zoom surface (a log, terminal, agent conversation, Markdown preview or plugin content view) is stepped first: OPEN, not built. The webview registers zoom surfaces with `registerFontZoomSurface` (`src/features/terminal/session.ts:382`, `src/features/agents/fontZoom.ts`, `src/features/viewers/markdown/MarkdownEditor.tsx`, `src/features/plugins/views/WebView.tsx`) and the font actions act on the focused one before text editors. `git grep -i 'font_zoom_surface|FontZoomSurface|zoom_surface'` under `app-gpui/src` finds nothing; the native handlers act on editors only (`app-gpui/src/workbench/editor_core.rs:107` and neighbours). Text accurate.
- Verdict: nothing closed.

## `action:editor.increaseFontSize`

- The focused font zoom surface (a log, terminal, agent conversation, Markdown preview or plugin content view) is stepped first: OPEN, not built. The webview registers zoom surfaces with `registerFontZoomSurface` (`src/features/terminal/session.ts:382`, `src/features/agents/fontZoom.ts`, `src/features/viewers/markdown/MarkdownEditor.tsx`, `src/features/plugins/views/WebView.tsx`) and the font actions act on the focused one before text editors. `git grep -i 'font_zoom_surface|FontZoomSurface|zoom_surface'` under `app-gpui/src` finds nothing; the native handlers act on editors only (`app-gpui/src/workbench/editor_core.rs:107` and neighbours). Text accurate.
- Verdict: nothing closed.

## `action:editor.pinTab`

- Pinned tabs drawn differently: OPEN, not built (in part WRONG TEXT). The webview draws a pinned tab with a pin button (Unpin Tab) in its close slot and no close button (`src/ui/Tabs.tsx:389-392`, `:493-508`). Natively `git grep '"pin"|tab.pinned'` in the workbench finds the pin icon only on the tab menu's Pin/Unpin row (`app-gpui/src/workbench/mod.rs:3686`); the tab element has no pin mark. "Not drawn differently" is not quite true any more: with "Show pinned tabs in a separate row" the pinned tabs take a row of their own (`app-gpui/src/workbench/mod.rs:1567-1573`, `:1725-1726`), proven by `workbench::tests::proofs44_editor_tabs::pinned_tabs_take_a_row_of_their_own_when_the_setting_asks_for_it` (`app-gpui/src/workbench/tests/proofs44_editor_tabs.rs:248`: `assert_eq!(rows(cx), [1, 2]);`). The pin mark itself is still missing.
- Verdict: nothing closed.

## `action:editor.resetFontSize`

- The focused font zoom surface (a log, terminal, agent conversation, Markdown preview or plugin content view) is reset first: OPEN, not built. The webview registers zoom surfaces with `registerFontZoomSurface` (`src/features/terminal/session.ts:382`, `src/features/agents/fontZoom.ts`, `src/features/viewers/markdown/MarkdownEditor.tsx`, `src/features/plugins/views/WebView.tsx`) and the font actions act on the focused one before text editors. `git grep -i 'font_zoom_surface|FontZoomSurface|zoom_surface'` under `app-gpui/src` finds nothing; the native handlers act on editors only (`app-gpui/src/workbench/editor_core.rs:107` and neighbours). Text accurate.
- Verdict: nothing closed.

## `action:editor.toggleZen`

- The menu bar item follows the mode (Exit Distraction Free Mode while on): OPEN, not built. The action's own title flips (`distraction_free_title`, `app-gpui/src/platform_more/distraction_free.rs:41`, `:125`), but the menu bar takes its item's label from the static menu spec, `act("editor.toggleZen", "Enter Distraction Free Mode")` (`app-gpui/src/workbench/menu_spec.rs:345`), read once per locale into `MenuBar.labels` (`app-gpui/src/workbench/menu.rs:418-433`); no dynamic item or title lookup for it. Text accurate.
- Verdict: nothing closed.

## `action:editor.unsplit`

- With autosave off, one question about the pane's unsaved tabs first: OPEN, not built. The webview removes the pane through `removeGuarded`, which asks once for the tabs it closes (`src/editor/core/controller.ts:1805-1807`). Natively `editor.unsplit` is `tabs.remove_pane(pane)` (`app-gpui/src/workbench/mod.rs:1039-1041`), and `tabs_changed` then asks per closed dirty editor with a count of 1 (`mod.rs:1833-1849`, `("count", 1usize.into())`). Text accurate.
- Verdict: nothing closed.

## `action:editor.unsplitAll`

- One question about the unsaved tabs first: OPEN, not built. Webview: `unsplitAll` → `removeGuarded` (`src/editor/core/controller.ts:1808-1814`). Natively `tabs.unsplit_all(pane)` (`app-gpui/src/workbench/mod.rs:1042-1044`), each closed dirty editor asked on its own as for Unsplit (`mod.rs:1833-1849`).
- Verdict: nothing closed.

## `action:file.exportSettings`

- The save dialog's OpenIDEA settings file-type filter: OPEN, not built. The webview's save dialog has `filters: [{ name: "OpenIDEA settings", extensions: ["json"] }]` (`src/features/app/projectActions.ts:63`). Natively `cx.prompt_for_new_path(&start, Some(EXPORT_FILE_NAME))` (`app-gpui/src/workbench/app_actions.rs:883`) takes no filter. Text accurate.
- Verdict: nothing closed.

## `action:file.importSettings`

- A JetBrains settings.zip is imported: OPEN, not built in the window. The webview routes a `.zip` to `importJetBrainsSettings` (`src/features/app/projectActions.ts:89`). Natively a model exists (`app-gpui/src/app_models/jetbrains_settings.rs:1-3`, model tests `app_models::jetbrains_settings::tests::maps_a_webstorm_export_to_the_settings_it_has_and_lists_what_is_left_out` and others), but `git grep 'jetbrains_settings::'` finds no caller outside its own module: Import Settings never reaches it.
- The open dialog's file-type filter: OPEN, not built. Webview: `filters: [{ name: "OpenIDEA or JetBrains settings", extensions: ["json", "zip"] }]` (`projectActions.ts:84`). Natively `cx.prompt_for_paths(options)` (`app-gpui/src/workbench/app_actions.rs:937`); `PathPromptOptions` has no filter field.
- Verdict: nothing closed.

## `action:file.newProject`

- Create Git repository run by a test: OPEN, testable. Built: `CreateStep::GitInit(dir)` in `create_project` (`app-gpui/src/workbench/app_actions.rs:1152-1170`). The window test turns it off on purpose: "Git is left out: the backend's git would read this machine's configuration" and `assert!(!created.join(".git").exists(), "no repository was asked for");` (`app_actions.rs:1663-1682`); the view test `app_views::new_project_dialog::tests::create_reports_the_folder_and_whether_git_is_wanted` checks only the event.
- Browse… starts in the typed folder: OPEN, not built. `browse_project_location`'s doc says the folder dialog "is not told the dialog's location" and uses `PathPromptOptions { ..., prompt: None }` (`app_actions.rs:780-789`), though the dialog emits the current folder (`NewProjectEvent::Browse(current)`, `app-gpui/src/app_views/new_project_dialog.rs:143-145`).
- Verdict: nothing closed.

## `action:file.openProject`

- "Not the webview's own Open dialog with its recent locations": WRONG TEXT. The webview has no Open dialog of its own: `openProjectDialog` calls the desktop's folder chooser (`@/platform/dialog`, the Tauri dialog plugin, `src/platform/dialog.ts:1-14`) with `{ directory: true, multiple: false, title: "Open Folder", defaultPath }` (`src/workbench/shellActions.ts:25-27`), then `file.openProjectPath`. The one real difference: it starts in Settings › Default Project Directory (`defaultPath`, `shellActions.ts:26`); the native folder prompt cannot be given a start folder (`PathPromptOptions` has no such field, see `app-gpui/src/workbench/app_actions.rs:780-788`). That start folder is OPEN, not built.
- Verdict: nothing closed.

## `action:file.openProjectPath`

- Attach: WRONG TEXT, and OPEN, testable. Attach is built: the "Open project in" question offers Attach in a local project window (`app-gpui/src/app_models/windows.rs:293`, `:485-490`), and the window carries out `OpenStep::Attach` with `ide_host::ops::project::attach_workspace` (`app-gpui/src/workbench/app_shell.rs:946-957`). Only model tests drive it (`app_models::windows::tests::attaches_in_place_without_reopening_or_asking_to_leave`); no window test.
- Remote projects: OPEN, not built. `OpenStep::ConnectRemote` ends in the error "{path} is on another machine" (`app-gpui/src/workbench/app_shell.rs:903-906`), and the window reports `remote: false` (`app_shell.rs:185`); the webview connects (`connectRemote`, `src/features/app/windows.ts:62`).
- Verdict: nothing closed.

## `action:file.openRecent`

- The Recent Projects popup (in a window with a project): OPEN, not built. The webview runs `showRecentProjectsPopup` (`src/features/welcome/index.ts:28-33`). Natively `file.openRecent` shares `welcome.recentProjects`'s handler, which only focuses the Welcome screen's search and does nothing otherwise ("a window with a project has no Recent Projects popup to show", `app-gpui/src/workbench/mod.rs:1061-1070`). Text accurate.
- Verdict: nothing closed.

## `action:file.openRecentProject`

- Same as Open Project Path: the policy part is what the cited test drives; Attach is built but only model-tested (WRONG TEXT inherited from that row, see above), remote projects are not built (`app-gpui/src/workbench/app_shell.rs:903-906`). Webview: `openProjectWithPolicy(c.arg)` (`src/features/app/windows.ts:529-536`).
- Verdict: nothing closed.

## `action:file.settings`

- The pages listed as showing "not available yet": WRONG TEXT in part, the rest OPEN and not verified page by page. Fonts is built: `settings_view::custom_controls::tests::editor_font_family_and_weight_apply_and_cancel_discards` opens `pages::editor_font::PAGE` through `open_page`, which asserts every control is drawn: `assert!(missing.is_empty(), "{page} draws every control: {missing:?}");` (`app-gpui/src/settings_view/custom_controls.rs:577`, call at `:850`); the same holds for the Code Style page (`code_style_visual_guides_and_do_not_format_apply`, `:868`). Languages are built and driven in a window (`workbench::tests::proofs19_settings_languages::reset_and_cancel_discard_an_edit_on_each_language_page` and the other `proofs19_settings_languages` tests). For colour scheme, code style schemes, file types, inspections and SSH I did not establish whether their pages still draw `UNAVAILABLE` (`app-gpui/src/settings_view/controls.rs:733`); hypothesis: some still do. Check: a window test calling `open_page` on each page id under `app-gpui/src/settings_model/builtin_pages.rs:187-235`, or `git grep -n UNAVAILABLE HEAD -- app-gpui/src/settings_view` plus the slot fills in `custom_controls.rs`.
- Verdict: nothing closed (the row's open clause is a list; its fonts and languages items are false).

## `action:file.toggleReadOnly`

- A virtual document refused as the webview refuses it: OPEN, not built. The webview toggles the active slot's model (`toggleReadOnly(slot.model)`, `src/editor/core/actions.ts:1009-1018`), whose model can be a virtual document. Natively, per the row, the target is always built as a file on disk; I found no virtual-document branch for this action and no test (`rust_tests.txt` read-only tests are `workbench::editor_core::tests::toggle_read_only_asks_then_locks_the_file_and_its_editor` only). Hypothesis on the native side (not read in full): check with `git grep -n -A20 'fn toggle_read_only' HEAD -- app-gpui/src/workbench/editor_core.rs`.
- Verdict: nothing closed.

## `action:find.inPath`

- Queries of several lines: OPEN, not built. The popup's fields hold one line: `find::popup_view::tests::a_text_of_several_lines_is_taken_on_one` documents that a text of several lines "is taken with spaces for its line breaks" (`app-gpui/src/find/popup_view.rs:3601-3604`, joining at `:387`).
- The undo history shared between the preview and a tab of the same file: OPEN, not built. The nearest test, `workbench::tests::proofs26_code_preview::keeps_dirty_text_and_undo_when_the_preview_closes_and_reopens_without_a_tab`, is about the preview alone, "without a tab"; no test or code found that shares one history with an open tab.
- Syntax colours in result rows: OPEN, not built. `git grep -i "colour|highlight_spans|syntax"` in `app-gpui/src/find/popup_view.rs` finds only a theme test (`:3686`); the `ui::code_preview::proofs26_code_preview::*` colour tests are of the preview, not the rows.
- Resizing and the remembered size: OPEN, not built. `git grep -i "resiz|size_key|POPUP_SIZE"` in `popup_view.rs` finds nothing.
- The scope editor: OPEN, not built in the window. A scopes dialog exists (`find::scopes_dialog::tests::the_scope_editor_needs_unique_names_and_a_pattern`), but `git grep -i "ScopesDialog|scopes_dialog"` finds no user outside `app-gpui/src/find/scopes_dialog.rs` and its `mod` line (`app-gpui/src/find/mod.rs:13`).
- Opening to the side: OPEN, not built. No split/side open in `popup_view.rs`; the window opens a match with `FindPopupEvent::Open { path, line, col, keep_open, .. }` (`app-gpui/src/workbench/mod.rs:3104`).
- The selection end of an opened match: OPEN, not built. The open event carries only `line` and `col` (`mod.rs:3104`); `find::query::tests::selection_of_maps_to_an_editor_selection` is a model test whose function is not used on this path.
- Verdict: nothing closed.

## `action:find.openInWindow`

- The bar's In Selection scope, excluded occurrences and unsaved text carried into the Find window: OPEN, not built. The webview hands the bar's own results source to the Find window (`target.results?.({ ...opts })` then `showFindResults(source)`, `src/editor/core/view/FindBar.tsx:174-183`). Natively `FindBar::results` builds a fresh `SearchQuery` over the file with `scope: SearchScope::Files { paths: vec![path.clone()] }` (`app-gpui/src/editor/find_bar.rs:830-841`) and the window "searches the bar's file with the bar's query" through the backend (`app-gpui/src/workbench/editor_find.rs:148-154`). Text accurate.
- Verdict: nothing closed.

## `action:find.replaceInPath`

Same "Not yet" list as Find in Files, same findings:

- Queries of several lines: OPEN, not built. The popup's fields hold one line: `find::popup_view::tests::a_text_of_several_lines_is_taken_on_one` documents that a text of several lines "is taken with spaces for its line breaks" (`app-gpui/src/find/popup_view.rs:3601-3604`, joining at `:387`).
- The undo history shared between the preview and a tab of the same file: OPEN, not built. The nearest test, `workbench::tests::proofs26_code_preview::keeps_dirty_text_and_undo_when_the_preview_closes_and_reopens_without_a_tab`, is about the preview alone, "without a tab"; no test or code found that shares one history with an open tab.
- Syntax colours in result rows: OPEN, not built. `git grep -i "colour|highlight_spans|syntax"` in `app-gpui/src/find/popup_view.rs` finds only a theme test (`:3686`); the `ui::code_preview::proofs26_code_preview::*` colour tests are of the preview, not the rows.
- Resizing and the remembered size: OPEN, not built. `git grep -i "resiz|size_key|POPUP_SIZE"` in `popup_view.rs` finds nothing.
- The scope editor: OPEN, not built in the window. A scopes dialog exists (`find::scopes_dialog::tests::the_scope_editor_needs_unique_names_and_a_pattern`), but `git grep -i "ScopesDialog|scopes_dialog"` finds no user outside `app-gpui/src/find/scopes_dialog.rs` and its `mod` line (`app-gpui/src/find/mod.rs:13`).
- Opening to the side: OPEN, not built. No split/side open in `popup_view.rs`; the window opens a match with `FindPopupEvent::Open { path, line, col, keep_open, .. }` (`app-gpui/src/workbench/mod.rs:3104`).
- The selection end of an opened match: OPEN, not built. The open event carries only `line` and `col` (`mod.rs:3104`); `find::query::tests::selection_of_maps_to_an_editor_selection` is a model test whose function is not used on this path.
- Verdict: nothing closed.

## `action:github.checkoutPullRequest`

- Without a number or a selected pull request, ask for one: OPEN, not built. The webview's description: "Checks out the selected pull request, a number given as argument, or asks for one." (`src/features/github/index.ts:80-87`). Natively the handler logs and stops: `None => tracing::info!("github: Checkout Pull Request… without a pull request needs a number prompt")` (`app-gpui/src/workbench/github.rs:601-603`).
- The window's progress line with gh's output and its Cancel button: OPEN, not built. `run_checkout` marks the window busy with "Checking out #n…" (`begin_outside_git_operation`, `app-gpui/src/workbench/github.rs:546-548`), but gh's output only goes to the debug log (`tracing::debug!(... "gh pr checkout")`, `:558`) and its `CancellationToken` is created and never cancelled (`:557`); no Cancel is offered.
- Verdict: nothing closed.

## `action:github.pullRequest.checkoutBranch`

- The window's progress line with gh's output and its Cancel button: OPEN, not built. `run_checkout` marks the window busy with "Checking out #n…" (`begin_outside_git_operation`, `app-gpui/src/workbench/github.rs:546-548`), but gh's output only goes to the debug log (`tracing::debug!(... "gh pr checkout")`, `:558`) and its `CancellationToken` is created and never cancelled (`:557`); no Cancel is offered. Same `checkout_pull_request` path (`github.rs:607-613`).
- Verdict: nothing closed.

## `action:github.pullRequest.refresh`

- The timeline and review threads reloaded: OPEN, not built. The pull request's timeline editor is not built at all natively: `tracing::info!(number, "github: the timeline editor of PullRequestOpened is not built")` (`app-gpui/src/workbench/github.rs:1004-1006`); review-thread models exist (`app-gpui/src/github/review_mode.rs`, `review_editor.rs`) but `git grep 'review_editor::|ReviewEditor::new|review_mode::'` finds no window use beyond an error type in `details_view.rs:13`, so there is nothing for Refresh to reload.
- Verdict: nothing closed.

## `action:github.pullRequest.show`

- The timeline editor the webview opens beside the tab: OPEN, not built (`app-gpui/src/workbench/github.rs:1004-1006`, logged as not built).
- Review threads and the pending review: OPEN, not built in the window (models only, see Refresh above).
- Reloading details already loaded: OPEN, not verified. I did not find whether `open_pull_request_tab` (`github.rs:1003`) asks gh again for a tab already loaded; `workbench::github::tests::proofs15_github_refresh::refresh_pull_request_replaces_what_the_tab_shows_by_the_new_answer` covers the Refresh action, not Show. Check: `git grep -n -A30 'fn open_pull_request_tab' HEAD -- app-gpui/src/workbench/github.rs`.
- Verdict: nothing closed.

## `action:gradle.executeTask`

- The project chooser as a select: OPEN, not built in that form. The webview's Execute popup has a `<Select>` of projects (`src/features/build-tools/ExecutePopup.tsx:211`). Natively the popup draws "Project:" followed by one clickable chip per project (`execute-project-<value>`, `app-gpui/src/build_tools/execute.rs:601-614`, `choose_project` at `:409`); choosing a project works and is driven in the view test `build_tools::execute::tests::a_typed_line_runs_in_the_chosen_project_and_is_kept_without_its_launcher`. Only the control's form differs. Text accurate.
- Verdict: nothing closed.

## Rows whose every clause is closed

- `action:db.explainPlan`
- `action:db.goToColumn` (for the console result grid the row names)
- `action:db.queryHistory`
- `action:db.revertChanges` (for the console result grid the row names)
- `action:deployment.configure` (its only missing clause is wrong text: the Deployment page is built and driven)

Rows with some clauses closed: `action:code.completion` (Tab through a snippet's fields), `action:debug.rerun` (takes its own tab's place), `action:edit.undo` (the Project view's Undo of a move).

## Rows whose text is wrong

- `action:app.about`: a test does click the Keymap Reference link (`app_views::about_dialog::tests::the_links_report_what_they_open`), though only as a view test.
- `action:build.rebuildProject`: the webview's Rebuild offers no wait either (`src/features/build/index.ts:110-113`).
- `action:db.importData`: in the webview only the column read has the password retry; the import request does not (`src/features/database/DataDialogs.tsx:176`).
- `action:deployment.configure`: the Deployment servers page is built (`app-gpui/src/settings_view/custom_controls.rs:225-233`).
- `action:editor.pinTab`: pinned tabs can take a row of their own (setting); only the pin mark is missing.
- `action:file.openProject`: the webview has no Open dialog of its own; it uses the desktop chooser starting at the default project directory.
- `action:file.openProjectPath` and `action:file.openRecentProject`: Attach is built natively (`app-gpui/src/workbench/app_shell.rs:946-957`), only model-tested.
- `action:file.settings`: Fonts, Code Style and the language pages are built and drawn with every control.
- `action:db.explainPlan`, `action:db.queryHistory`: their "Read, not tested" sentences are stale (tests now exist).
- `action:db.goToColumn`, `action:db.revertChanges`, `action:debug.rerun`: their "missing" clauses are stale (tests now exist).
