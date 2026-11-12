# GPUI port — decisions

Every non-obvious call made while porting the webview UI to GPUI, with the reason. "Measured" means the command was run in the port's worktree and its output is what is quoted.

## D1. `app-gpui/` is its own Cargo workspace

The GPUI app lives in `app-gpui/` and is listed under `exclude` in the root `Cargo.toml`. The root workspace, its `Cargo.lock` and its clippy / deny / nextest runs stay exactly what the Tauri app is built from, so the final comparison builds the baseline from an unchanged tree. The cost is a second lock file; D9 keeps the shared backend dependencies on the same versions.

## D2. GPUI comes from crates.io as `gpui-pre =0.3.12`, a snapshot of zed@d2a4fb6

- Pinned Zed commit: `d2a4fb6ded7c470be25d1541281da3882495b65b` (committed 2026-11-08).
- The official `gpui` crate on crates.io is 0.2.2 from 2025-10-22, more than a year behind Zed's `main`. `gpui-pre` republishes `crates/gpui` weekly under Apache-2.0.
- Provenance, measured: a shallow fetch of zed at that commit and `diff -rq crates/gpui/src <registry>/gpui-pre-0.3.12/src` reports 1 differing file of 96, `action.rs`, where the `actions!` macro derives `$crate::Action` instead of `gpui::Action` (needed because the package is renamed).
- A git dependency on zed itself was the alternative. It was not taken because `gpui-component` (D3) depends on `gpui-pre` by name and Cargo cannot patch a package with one of a different name; two GPUI copies in one binary do not interoperate.

## D3. Widgets come from `gpui-kit` / `gpui-component` 0.9.1 (Apache-2.0)

The webview UI's `src/ui` primitives (inputs with IME, lists, trees, virtual lists, menus, popovers, tooltips, tabs, splitters, scrollbars, dialogs) each need a GPUI counterpart. `gpui-component` provides them; writing them again would add months and nothing the user sees. Its look is replaced by the IDE's own tokens (D6). It is used for primitives only: the dock/tool-window model, the editor and the terminal are the port's own code, because their behaviour is specified by this repository's tests.

`gpui-component`'s `tree-sitter` feature is off: it links tree-sitter 0.26 while the backend links 0.25 (`links = "tree-sitter"` allows one). Highlighting comes from `ide-syntax`, as in the Tauri app.

## D4. No Zed crate other than `gpui`

Measured at the pinned commit: `crates/editor`, `crates/terminal` and `crates/ui` are `GPL-3.0-or-later`; only `crates/gpui` is Apache-2.0. This repository is Apache-2.0, so the editor and terminal views are written here.

## D5. The app calls `ide_host::ops` directly

`src-tauri/src/commands/*.rs` are thin adapters over `ide_host::ops::<domain>` (the same functions `openidea-server` dispatches for remote development) plus desktop-shell commands. The GPUI app links `ide-host` and calls those functions with typed arguments: no JSON, no IPC. `ide_core::EventSink` is implemented by a channel drained on the GPUI main thread. Desktop-shell commands (windows, menus, updates) are reimplemented against GPUI; where a shell command holds logic both apps need, it moves into a backend crate rather than being copied.

## D6. Design data is read from the webview sources, not retyped

`src/styles/tokens.css`, the keymap presets, the text catalogs, the icons and the static metadata of every action and contribution (id, title, category, `when` clause, icon, anchor, order…) are the source of truth while both apps exist. `tokens.css`, the locale JSON files and the SVG files are embedded as they are. What lives in TypeScript is extracted by `docs/gpui-port/gen-data.mjs` (it evaluates the keymap modules and resolves object literals through the TypeScript program) into `app-gpui/assets/gen/*.json`, which is checked in; `gen-data.mjs --check` fails when a file is stale. A Rust test then fails for every action in `actions.json` without a handler, which is how "every action is ported" is enforced rather than eyeballed.

## D7. Terminal emulation: `alacritty_terminal` 0.26 (Apache-2.0)

`ide-terminal` owns the PTY and streams bytes; the webview parses them with xterm.js. The GPUI app feeds the same bytes to `alacritty_terminal`'s grid and paints the grid.

Status: a terminal view exists and runs a real shell (D27). It is not in a tool window yet.

## D8. HTML surfaces

| Surface                                                                                                            | GPUI app                                                                                    |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| Markdown (preview, agent chat, GitHub, LSP hover)                                                                  | Rendered natively                                                                           |
| Plugin web views (`openidea-plugin:` scheme), the Web editor's browser bar, notebook HTML output, Mermaid diagrams | A WKWebView created through `wry` when the surface is first shown, destroyed when it closes |

Plugin web views are HTML by contract (plugin API 1.4), so a web view cannot be removed without breaking plugins. It is created on demand, so a session that opens none of these surfaces has no WebKit process; the final measurement reports both cases.

Status: the web view works inside a GPUI window and the plugin host runs in the GPUI app (D26). `wry` is pinned to 0.57.0, the version the Tauri build's lock holds. No plugin web view, browser bar, notebook output or Mermaid diagram is shown through it yet.

## D9. Shared dependencies resolve to the root lock's versions

`app-gpui/Cargo.lock` was seeded from the root `Cargo.lock`, so Cargo only added what GPUI needs. `node docs/gpui-port/lockcheck.mjs` compares the dependency closure of `ide-host` in both locks and fails on any package the two builds use at different versions. Measured after seeding: 12 packages differ and are allowlisted in the script, all forced by `gpui-kit`'s optional `gpui-fast-web` (never compiled here, but part of lock resolution): the ICU 2.3 → 2.4 family (`icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`, `tinystr`, `yoke`, `zerovec`, `zerovec-derive`) and `unicode-properties` 0.1.4 → 0.1.3. Before seeding, a fresh resolution had moved roughly a hundred backend dependencies to newer patch releases.

## D10. Commits go through the repository's pre-commit hook with pnpm 10

The hook runs `pnpm lint-staged`; the `pnpm` on `PATH` here is 9.15, which rejects this repository's `pnpm-workspace.yaml`. Commits run with a shim that executes `corepack pnpm@10.34.9` (the version that installed the main checkout's `node_modules`) rather than skipping the hook.

## D11. Per-window state replaces per-webview singletons

Each Tauri window is its own webview, so the webview UI keeps context keys, tabs, tool-window layout and the open project in module-level stores. The GPUI app is one process with many windows: that state lives in one `Workbench` entity per window, and only what really is process-wide (the action registry's definitions, the keymap, settings, the backend `Host`) is a GPUI global. An action's handler receives the window it was invoked in.

## D12. The keymap dispatcher is the port's own, not GPUI's

GPUI has a key-binding system, but this IDE's binding syntax (two-stroke chords with a 2 s timeout, double-modifier gestures such as `Shift Shift` and `Alt Alt+Up`, mouse-button and swipe bindings, `when` / `keyWhen` clauses over context keys, plugin bindings that yield to the preset) is specified by `src/platform/keymap.test.ts`. The dispatcher is ported as a state machine fed from GPUI's keystroke, modifier and mouse events at the window root, so the same tests apply.

## D13. The GPUI app has its own directories

The Tauri app keeps settings, recent projects, credentials and caches under the identifier `ai.l0ng.openidea`. The GPUI app uses `ai.l0ng.openidea.gpui` (`.gpui.dev` in debug builds), so it can run next to an installed Tauri build without the two processes writing the same files, and `OPENIDEA_GPUI_HOME` puts every directory under one folder (tests, and the final measurement, which starts both apps from empty directories). The cost: the GPUI app does not pick up the Tauri app's existing settings; importing them is a copy of the configuration folder.

## D14. The editor shows the backend's rope; edits are synchronous calls

The webview keeps its own copy of every open document in CodeMirror and reconciles it with the backend's through an OT client (`src/editor/core/sync/DocSync.ts`), because edits cross an IPC boundary. In process there is no boundary: the editor holds a clone of the backend's rope at a known version (a `ropey` clone shares its chunks, so the text exists once) and applies each edit with `Documents::edit(doc, version, changes)` before repainting. Edits the backend makes itself arrive as `DocPatched` and move the selection; an edit rejected as out of sync (a backend edit not delivered yet) refreshes the text and is re-made at the caret's new place. `Documents::rope_snapshot` is the one backend API added for this (`crates/ide-workspace/src/documents.rs`, additive).

Undo stays where the webview has it, on the UI side: the editor records each edit with its inverse and the selections around it (`editor/history.rs`, grouped like CodeMirror's history: continuous typing within 500 ms is one step, a caret move starts another). An edit the backend makes is a step too, as it is in the webview, except a workspace edit, which is undone as a whole elsewhere and resets the editor's history.

Open risk: `Documents::edit` takes the store's operation lock on the main thread. A long backend operation holding that lock (save all, sync from disk) would stall typing; if a measurement shows it, edits move to a queue with the rebase logic `DocSync` has.

## D15. Highlighting comes from tree-sitter, not Lezer

The webview highlights most languages with CodeMirror's Lezer grammars, which do not exist outside a browser; only languages without a CodeMirror mode use the backend's tree-sitter queries (`doc_highlights`). The GPUI editor uses `doc_highlights` for every language and maps its kinds to the `--syn-*` tokens exactly as the webview's highlight style does.

For the languages CodeMirror covers, `ide-language` gains a cargo feature, `native-highlights`, that the GPUI app enables: it supplies each grammar crate's own `highlights.scm` (26 grammars) through the same code path. The Tauri build leaves the feature off, so its binary and behaviour are unchanged (checked: clippy and the highlight tests pass in both configurations). Those upstream queries capture declared and called functions alike; for Rust, JavaScript/TypeScript/TSX, Python, Go, Java, C and C++ a few extra patterns mark declarations, because the colour scheme distinguishes them. The other languages colour declarations like calls for now. Protocol Buffers has no query in the locked grammar version. Token boundaries will differ from Lezer's in places; that is a deviation this port cannot avoid.

## D16. Keys are named by the layout, not by the physical key

The webview names a key from `KeyboardEvent.code` (physical position) with a fallback to the typed letter, so that ⌥-letter works on macOS and AZERTY's Ctrl+Z is not Ctrl+W. GPUI reports the layout-resolved key (the character the key types without modifiers, or its ASCII equivalent on non-Latin layouts), which gives the same answers for both cases; it has no physical code, so numeric-keypad operators are not told apart from the main-row ones. Bindings that name `NumpadAdd` / `NumpadSubtract` / `NumpadMultiply` / `NumpadDivide` therefore fire from neither key until a platform hook supplies the distinction.

## D17. Fonts: not embedded yet

The webview bundles Inter Variable and JetBrains Mono Variable as WOFF2 subsets per Unicode range (`@fontsource-variable/*`). GPUI's text system loads TTF/OTF data, and several same-named subset fonts do not combine into one family there. The editor currently uses the first installed family of `--font-mono` (falling back to Menlo) and the UI uses the system font. Embedding the two families as full TTFs is open.

## D18. Pure-logic ports are checked against the original's own engine where one exists

Number formats and plural categories are rules the webview gets from `Intl`. `gen-data.mjs` writes what Node's `Intl.NumberFormat` and `Intl.PluralRules` answer for the seven UI locales to `assets/gen/intl.json`, and a Rust test compares every row. It caught two differences on its first run (negative zero keeps its sign; Spanish treats -1 as `one`).

## D19. Caret movement and typing are the editor's own keys, not actions

The manifest has no actions for arrows, Home/End, Backspace or Enter: in the webview they are CodeMirror's base keymap (`src/editor/core/cm/baseKeymap.ts`), below the IDE keymap. The GPUI editor handles them in its own key handler for the same reason, after the dispatcher has had the keystroke. Undo, redo, cut, copy, paste and select-all are both: registered actions (`edit.*`, what the Edit menu runs) and the editor's ⌘-keys, since the webview gets those shortcuts from the native menu's accelerators and the GPUI app has no native menu yet.

## D20. What has been seen on screen

A window-only capture of the running debug build (2026-11-11) shows the editor drawing a Zig file with gutter numbers, caret, current-line band and syntax colours in Islands Light. Nothing else has been looked at: the window tests run on GPUI's test text system, whose glyph metrics are not the real shaper's, so pixel positions asserted there prove the mapping logic, not the rendering.

## D21. `localStorage` becomes one JSON file

The webview keeps session-outliving UI state (tool window layouts, named layouts, search history, dialog choices) in `localStorage` under string keys. The GPUI app keeps the same keys with the same JSON values in `ui-storage.json` in its configuration directory, written through on every change by rename, so a layout stored by one app parses in the other. The two apps do not share the file (D13).

## D22. A tool window appears on a stripe only once it has a view here

The webview registers 36 tool windows. The GPUI stripes show a button only for those this app can draw (today: Project), so no button opens an empty pane. Their declarations (title, icon, side, order) come from the contributions manifest, and each gets its `toolwindow.<id>` action when its view is ported.

## D23. Unsaved text is never dropped

Measured in the webview's sources: when a text document's last tab closes, `DocumentManager.release` saves it if `files.autoSave` is on (the default) and then disposes it whether or not the save worked; with `files.autoSave` off it is disposed unsaved. Switching tabs and the window losing focus also autosave. Closing a window with unsaved documents asks "Unsaved Changes" with Save All / Don't Save (`closeGuard.ts`).

The GPUI app saves on the same three occasions and asks the same question on window close. It differs in two places, both on the side of keeping the text: a tab whose save fails reopens with its editor and an error notification instead of being disposed, and with `files.autoSave` off closing an unsaved tab asks the same "Unsaved Changes" question (Save All / Don't Save / Cancel, Cancel bringing the tab back) where the webview discards the edits without asking. With autosave off nothing is saved on leaving a tab or the window, as in the webview.

The editor marks itself unsaved on its own successful edit rather than waiting for the backend's `DocDirty` event, so typing and closing in the same instant cannot slip between the two.

## D24. One collation, on ICU, checked against `Intl.Collator`

The webview sorts every list with `localeCompare` or `Intl.Collator`, in four option sets: default, `sensitivity: "base"`, `numeric: true`, and both. `platform::collate` gives the same four on `icu_collator` 2.4.0, the ICU version GPUI already locks (no new version in the lock; `lockcheck.mjs` passes). It is one new direct dependency, justified because code-point ordering visibly differs from the webview's for case, accents and punctuation (`_build` before `1`, `main` before `Main`, `résumé` next to `resume`).

`gen-data.mjs` writes `assets/gen/collation.json`: 75 awkward names sorted by Node's `Intl.Collator("en", …)` under each option set. It also holds the sign of every pair of those names under each option set (22,500 comparisons), so names the collator holds equal are told from names that were merely listed in order. `platform::collate::tests::orders_names_as_intl_collator_does` requires the same order and the same signs from the Rust side. The webview passes `undefined` as the locale, so it follows the system's; the GPUI app uses the root collation, which is what English and most European locales resolve to. Locale tailorings (Swedish `ä` after `z`, for example) are not applied.

The six hand-written orderings this replaces (tool windows, tabs, main toolbar, project tree, themes and fonts, version control lists) are gone.

## D25. One settings service; the theme is per process

`platform::settings` is the only way the UI reads and writes settings: it calls the backend's store directly and then announces `SettingsChanged` to the windows the value concerns, the writing window included (the Tauri glue did the announcing; here `Backend::emit` puts UI-side events on the same ordered queue as the backend's). What depends on a setting listens for that event, so it is refreshed the same way whoever wrote the value. The four ported copies of `SettingsWrite` and the theme module's own variant are this module's one type now. A write runs on the main thread: it is one small TOML file, and the work a value may trigger (language servers, file types) is already spawned by the backend.

Not carried over from the Tauri glue, because their features are not ported yet: re-applying the MCP server on `mcpServer.*` and waking the update checker on a global write.

`theme::appearance` applies the `appearance` settings: the chosen theme, or with Sync with OS the preferred light/dark theme for the OS mode (GPUI's window-appearance observer), a custom accent, and a session override when the choice cannot be saved. It reads the **global** settings and sets one theme for the process, because the theme is a process global here (D11). The webview applies each window's effective settings, and a project's settings layer may hold `appearance.*` (measured: `remove_global_settings` in `crates/ide-workspace/src/settings.rs` strips only `mcpServer` and `editor.customSchemes` from it), so there a project can give its window its own theme; the GPUI app ignores such a project-level override. That is a parity gap, listed in the inventory under `src/platform/themes.test.ts`. Plugin themes resolve to their light/dark base until the plugin host is connected.

Seen on screen (D20): the first frame of a window with `appearance.theme = "dark"` is dark. Nothing after the first frame could be captured in that session: the display was asleep and the screen locked (measured with `CGDisplayIsAsleep`), and GPUI draws further frames from the display link.

## D26. The web view and the plugin host are proven in the GPUI app

**Web view (confirms D8).** GPUI's `Window` implements `HasWindowHandle`, and `wry` builds a WKWebView as a child of it (`web_view::WebSurface`). GPUI's test windows have no native view, so this is checked in the real application: `openidea-gpui --self-check=web-view` opens a window, loads a page, receives `window.ipc.postMessage` from it, runs script in it, resizes the surface and waits for the page to report the new size. The integration test `tests/web_view.rs` starts the binary and requires `web-view: ok`. It passed with the display asleep and the screen locked, so it does not depend on frames being drawn. `inventory.mjs` accepts such tests as proofs, cited as `tests::<file>::<case>`.

**Plugin host (no extraction needed).** Measured: the plugin host is `crates/ide-host/src/plugins/` (about 20,800 lines); `src-tauri/src/commands/plugin.rs` (1,067 lines) only creates it and forwards commands. `plugins::Plugins::init` in the GPUI app does the same creation (data directory, language store, external syntax and run detection, workspace, trust policy, secret store, the load/flush/supervise thread) and brings `PluginEvent`s to the main thread. Nothing under `src-tauri` was changed. Left out because the features are not ported: syncing plugins to remote-development servers.

Plugin secrets go to the login keychain under this app's own identifier (`<APP_IDENTIFIER>.plugin.<id>`), not the installed app's `ai.l0ng.openidea.plugin.<id>`: like the data directories (D13), a development build does not read or overwrite the installed app's entries. At switchover the service name is the one line to change.

First contribution projected: the `themes` point. A plugin's theme is offered next to the built-in ones, the host's selected theme follows `appearance.theme`, and its tokens are applied over the built-in base of its kind, under a custom accent. `plugins::tests::a_linked_plugins_theme_is_offered_and_applied` does this with `plugins/official/harbor-night`. Project plugins (`<project>/.openidea/plugins`) are not listed yet: the listing is per process, not per window.

**The Bazel plugin.** `/Users/davidvieira/code/openidea-bazel` was linked into a temporary home by the opt-in test `a_plugin_folder_named_by_the_environment_loads` (run with `OPENIDEA_GPUI_TEST_PLUGIN`). Measured result: `davidhsv.bazel 0.1.0: Inactive` (contributions live, no Wasm instance yet), 9 commands, 6 menus, 1 settings contribution, and the languages `starlark`, `bazelproject`, `bazelrc` registered. With `OPENIDEA_GPUI_TEST_COMMAND=open-sync-log` the same test runs that command without a project: the plugin's Wasm loaded (status `Active`) and answered `no project is open`, its own message. The installed app's folders were not touched.

**Commands.** Every declared command of a live plugin is an action `plugin.<plugin id>.<command id>` with its title, category and `when`, registered before any of its code is loaded. Running one passes the window's project, its active editor (path, language, selection as lines and UTF-16 columns) and the selected paths; a failure is a notification in that window unless the plugin was stopped or lacks a permission. `plugins::tests::a_plugins_command_runs_its_wasm_on_the_editors_selection` does this with `plugins/official/string-tools`: the command turns the selected `hello big world` into `hello_big_world` through the plugin's Wasm, the backend's document and the editor's patch path. A command's own image icon is not loaded yet (a named icon is used, otherwise the package icon).

## D27. The terminal: emulator, keys and view

Three parts, each tested on its own:

- `terminal::emulator` wraps `alacritty_terminal`'s `Term`: output in, rows of styled runs out (a wide character is its own two-cell run, so text stays on the grid), cursor, scrollback of 10,000 lines as in the webview, selection by cell, word and line, and the answers a program is owed (device reports, colour queries, text-area size) as effects to write to the PTY. A program may set the clipboard (OSC 52) but never read it.
- `terminal::encode` is what xterm.js did implicitly: xterm's key sequences (cursor keys in normal and application mode, modifiers, function keys, control characters, Option as Meta from `terminal.optionAsMeta`, Option+Left/Right as word movement) and paste encoding (Enter line ends, bracketed-paste markers, a marker inside the pasted text removed so it cannot end the bracket early). Bold text is not brightened, as the webview sets `drawBoldTextInBrightColors: false`.
- `terminal::view::TerminalView` spawns the shell through `ide_host::ops::term`, feeds its output to the emulator, sizes the grid to the room it is painted in and tells the PTY, and reports the exit with the webview's `[Process completed (exit code N)]` line.

`terminal::view::tests::a_shell_runs_in_the_terminal_and_reports_its_exit` runs the login shell on a real PTY: typed text and Enter reach it, its output is on the grid, `stty size` prints the fitted grid's size, ⌘V pastes, and `exit 3` is reported as exit code 3.

Not done: the Terminal tool window with its tabs and groups, restart on Enter after exit, the find bar, links, mouse reporting to programs, the resize debounce, cursor blink, the Nerd-font fallbacks, and the derived palette for themes that define neither `--terminal-ansi-*` nor `--ansi-*` (every built-in theme defines them). The cell is the font's `M` advance by its ascent plus descent times `terminal.lineHeight`; xterm.js measures its cells its own way, so the grid may differ by a column or a row from the webview's at the same pixel size.

## D28. Tool windows dock on three sides; a hidden one keeps its view

The workbench lays tool windows out as the webview's grid does: the editor between the left and right sides, the bottom side spanning below all three, each side with a primary and a split area sharing it by the stored ratio. The left stripe lists Left, Left Bottom and at its foot Bottom Left; the right stripe (shown only when it has a window) lists Right, Right Bottom and Bottom Right. Supersedes D22's "today: Project": the Terminal is the second window.

A window's view is created the first time it is shown and kept while hidden, as the webview keeps mounted windows: hiding the Terminal does not end its shell. The Terminal starts as Settings › Terminal says (start directory, shell command line, environment, shell integration).

Not carried over yet: resize handles, maximize, the widescreen layout (side windows at full height), tool window names under the icons, drag and drop on the stripes, header actions and menus.

## D29. The editor session: same stored JSON, tabs loaded when shown

`editor::session` stores a project's tabs, splits, active pane and per-file caret and scroll position under the webview's key (`openidea.editorSession.v1:<project folder>`) in the webview's JSON shape, validated on the way in. It is written 400 ms after the tabs last changed, when the window loses focus and when it goes away.

A restored tab is only a title until it is shown: its document is opened in the backend when the tab becomes the active one, and its saved selection and first visible line are applied then. The webview does the same (`ensureSlot` on the active tab of each pane). Opening a tab's document is idempotent per tab: the first version of this change opened it twice, and the second arrival took the focus back from wherever the user had gone (caught as a 1-in-4 test failure).

Only one pane is drawn so far: a saved split layout is restored into the model and kept on save, but its other panes are not visible. Tabs of non-file editor kinds are not saved, because no such kind exists here yet.

## D30. The Git log's first slice, and what the session restore must not do

The Git tool window (bottom, as declared) holds `vcs::log_view::GitLog`: commits read 300 at a time through `ide_host::ops::vcs::vcs_log` as the list nears its end, the graph from the ported `vcs::graph::compute_graph` drawn as the webview draws it (14 px lanes, 26 px rows, straight edges, dashed for commits left out, a ring for HEAD), reference labels by the webview's rules (`vcs::log_options::ref_labels`), author and date in the user's date format and local time. It is read again on the backend's `VcsChanged`, keeping the selected commit selected. The commit and graph rows are shared with the rows being drawn, so a frame copies no commit: this is a measured scenario on a large repository.

`ide-vcs` is a direct dependency now; the ported VCS models still carry their own mirrors of its types and are to be moved onto it as they are wired. `chrono` (the version the backend locks, `clock` only) gives the local UTC offset at a commit's moment, so a winter and a summer date each show their own local time.

Restoring a project's tabs does not take the keyboard: a restored tab's editor loads without focus, which stays in the Project view as on any open (the webview focuses only when tabs were already live). The first version of the restore did take it; an assertion in `workbench::tests::a_projects_tabs_and_carets_are_restored` holds this now.

Not in this slice: the branches tree, filters, commit details and changed files, context menus, column headers and resizing, long-edge and linear-branch options, multiple repositories.

## D31. A focused view may claim keys before the keymap; the terminal does

The shortcut dispatcher sees every keystroke before the focused view (D12). The terminal needs the opposite for its own keys, as the webview's terminal key handler runs before its keymap: Ctrl+C must interrupt the program, not run an IDE action. `platform::input::claim_keys` lets a view see keys first while its focus handle is focused; the terminal's claim applies the ported rules (`terminal::keys::route_terminal_key`): copy and paste, ⌘K, ⌘W, ⌘F, the shell's line-editing strokes for ⌘ and ⌥ with the arrows, Ctrl with a letter by the letter it types, Escape, and Settings › Terminal › Override IDE shortcuts and Escape-to-editor.

⌘K clears inside the emulator as xterm.js does (the cursor's line becomes the first, the rest and the scrollback go) and does not involve the shell. A first version sent the shell a form feed after clearing the scrollback; measured in the real-shell test, the shell's redraw scrolled the old screen back into the scrollback.

The terminal's colours now come from the ported `terminal::theme::build_theme` (including the palette derived for themes without ANSI tokens), replacing the view's own reduced copy. Still the view's own and to be replaced by the ported modules as the tool window grows: tabs and groups (`terminal::store`), the resize debounce (`terminal::fit`), links, the find bar, drop and image paste.

## D32. Find in Files: the models are in; their search types are the UI's own, converted at the edge

`find/` holds the ported Find in Files and Replace in Files models (query, scopes and file masks, a find session, the Find tool window's tree, the replace flow, the search popup's keys): 60 spec cases, no view yet.

`find/ipc.rs` keeps UI-side copies of the backend's search types (`ide_search`, `ide_index`, `ide_lsp`), with paths as the `/`-separated strings the models compare and with equality and defaults the tests rely on. They are not replaced by the backend's types: the copies are what the models were specified against. The view that will call the backend converts in both directions in one place, where the compiler checks every field.

Delivered by the same subagent but not wired: its Search Everywhere controller, routes and a second copy of the model, ranking and filters, which duplicate the Search Everywhere batch another subagent is still writing. They stay out of the build until the two are reconciled into one.

## D33. An own one-line text field; Find in Files starts in the Find tool window

`ui::text_field::TextField` is the app's single-line input (query boxes, later dialog fields): its own text, caret, selection, clipboard, word and line movement, and the OS input method through GPUI's input handler. The component library's input was not used: it brings its own styling and key contexts, and the dispatcher already knows `TextField` as the context where plain typing stays in the field.

`find.inPath` shows the Find tool window with the query focused, where the webview opens the Find in Files popup (query, scope, file mask, preview) and only sends results to the tool window on request. This is a deliberate first slice, not parity: the popup, scopes, file masks, the preview, Replace in Files and result tabs are not there. What is there is driven by the ported models: `FindSession` validates and runs the search through `ide_host::ops::search`, results stream in by file, the case, word and regex options search again, and Enter or a double click opens the match with the caret on it. `find::backend` is the one place the UI's search types and `ide_search`'s are converted (D32).

## D34. The Terminal tool window has tabs; a tool window says where its keyboard goes

`terminal::panel::TerminalPanel` drives the ported tab model (`terminal::store::Terminals`): it creates a terminal for each session the model adds, drops it when the model disposes the session (which ends the shell), and writes the remembered tabs under the webview's key (`openidea.terminal.tabs.v1:<project>`). New tabs take the next default name (`Local`, `Local (2)`), a shell that exits closes its tab when Settings › Terminal › Close session when it ends is on, the last tab closing hides the tool window, and showing the Terminal without a tab starts one: all as the webview does.

The workbench no longer keeps a fixed focus handle per tool window. It asks the window's view each time (`Focusable`), because the Terminal hands the keyboard to whichever tab is showing.

A tab of a shell keeps its own name at the prompt and while a command runs. The webview shows the running command's title, which it knows from shell integration's command marks (OSC 133); `alacritty_terminal` does not report those, so this is a gap until the marks are parsed here. Splits within the Terminal are in the model and restored from storage, but only the active group is drawn. Not there yet: renaming, drag and drop between groups, the tab context menu, predefined sessions, plugin sessions, restart on Enter after exit when Close on exit is off.

## D35. Tests that start a shell give it a home of its own (after they had written to the owner's history)

What happened: the terminal tests start the account's login shell on a real PTY and type commands into it. Until this change the shell was the owner's own interactive zsh with the owner's startup files, so every run appended the tests' commands to `~/.zsh_history`. Measured on 2026-11-12 with `grep -cF`: 34 lines `echo marker-$((6*7)); stty size`, 34 `echo pasted-$((40+2))`, 30 `exit 3`, 29 starting `echo in-$(basename`, 10 `sleep 30`, 10 `echo back-$((1+1))`, 2 `echo second-$((1+1))` (about 150 lines of 10,840), plus a few bare `exit`. Nothing else of the owner's was written. The history file was not edited: it is the owner's, and the lines are harmless to leave or to delete.

The fix: `terminal::test_support::isolate_shells` sets the global `terminal.env` of the test's temporary settings to a `HOME` and `ZDOTDIR` under the test's temporary directory (with an empty `.zshrc`) and `HISTFILE=/dev/null`, before any terminal starts. Every test that starts a shell calls it. Verified: the marker count stayed at 34 across the full suite run twice.

The app itself is unaffected: a real terminal is meant to be the user's own shell.

## D36. The diff models bring their own comparison engine

`vcs_diff/` holds the diff viewer's models (hunks and gutter marks, navigation, options, scroll sync, stripes, conflict markers, inline blame, compare) and `vcs_diff::comparison`, a port of the webview's IntelliJ-style comparison (by line, word and character, with its chunk optimiser and move detection), checked against the webview's 172 fixture cases (`ideaCases.json`, read as-is). No diff view is drawn yet.

Choices the subagent made, kept: the web editor's own fallback diff is not ported, so a comparison too large for the engine degrades to one coarse chunk between the shared leading and trailing lines; blame ages are English "N units ago" through the text catalog instead of the browser's relative-time formatter, so other locales read the catalog's wording; three-way merge chunks are not computed here (the merge-stripe cases use hand-built changes).

Subagent choices in earlier batches, recorded here as they are judgment calls too. Search Everywhere: a translated action's English title matches through its keywords, with their lower weight, where the webview has a branch of its own for the English title. Version control: commit messages wrap at the margin counting characters, where the webview counts UTF-16 units (they differ only beyond the Basic Multilingual Plane), and commits not loaded yet sort after loaded ones when ordering newest first.

Search types still exist twice (`find/ipc.rs` and `everywhere/model.rs`: symbol and line matches, workspace symbols). D32 justifies one UI-side copy, not two: they are to be merged into one when Search Everywhere gets its view.

## D37. Costs that grow with the project are paid once

Three places did work proportional to everything loaded each time a little more arrived; all three are measured scenarios or count towards them.

- A terminal tab's shell starts when the tab first shows, as in the webview ("hidden tabs stay unopened"): tabs remembered from the last run cost no process until selected. `terminal::panel::tests::remembered_tabs_start_their_shells_when_first_shown`.
- The Git log continues the graph from the lanes left open by the last page instead of recomputing it from the first commit. `vcs::log_view::tests::a_graph_continued_page_by_page_is_the_whole_graph` checks that the two are the same for every way of splitting a branching history.
- Find in Files adds rows for the files found since the last batch instead of rebuilding every row. A new search keeps the previous rows until it has a result of its own or ends, as the session model intends.

## D38. Search Everywhere, and the popup slot it opens in

`everywhere/view.rs` is the first overlay in the app, opened by the six `navigate.*` actions on their tabs. `SeController` decides which sources a query needs; the view asks `ide_host::ops::nav` (`find_files`, `find_symbols`, `recent_files`) and `ide_host::ops::search::start` for them and feeds the answers back. The webview's rules that decide what Enter opens are kept: the query rests 60 ms before it is searched, the selection is the top result until the user moves it, and Enter typed before the top result is final waits for it instead of opening a row of the previous query.

The workbench has one popup slot (`open_popup` / `close_popup`) for this and every overlay to come. It draws the popup over the whole window (`deferred`, on an occluding backdrop that closes it on a click beside it), remembers what had the keyboard and gives it back on closing (the editor when nothing had it), and publishes the context keys as it closes. The last part fixed a bug the first version had, found in review and reproduced by a test: an action chosen in the popup ran while the keys still said the popup was open and nothing was focused, so one that needs `terminalFocus` was refused. `search_everywhere_gives_the_keyboard_back_and_runs_actions_where_it_was_opened` fails without the republish.

Choices:

- The popup claims Enter, Tab, the arrows and Page Up/Down in every modifier combination while its query has the keyboard, as the webview does (`claimSearchKeys`), so the keymap's bindings for them do not run. The combinations whose behaviour is not ported (⌥↵ menu, ⌘↵ to Find in Files, ⌥↑/⌥↓ history, ⇧↵ to the side, which opens in place) are claimed all the same: doing nothing is closer to the webview than running an editor action behind the popup.
- Sources the app cannot answer yet reply at once instead of staying "loading": language-server workspace symbols answer "no server", endpoints and settings pages answer "failed", which the controller treats as empty. Recent actions are an empty list. Each is listed as a gap in the `search.everywhere` overlay row.
- An action that shows a state (a toggle) leaves the popup open after it runs, as in the webview; any other result closes the popup first and then acts. Whether an action is a toggle is asked, and the action run, after the workbench's own update is over (`window.defer`): both may read the workbench, and asking inside the update panicked in the test.
- A plain click on a row selects and opens it, as in the webview (`List` calls `onActivate` on a single-selection click). Its modifier clicks, which select several rows, are not ported.
- The search types that existed twice are now one: `everywhere::model` uses `find::ipc::LineMatch`, and the copies of `SymbolMatch` and `WorkspaceSymbol` nothing in `find/` used are gone. `ide-index` is a direct dependency, to name the types `ide_host::ops::nav` returns.

## D39. The menu bar is drawn from the Tauri app's table

`src-tauri/src/menu.rs` already holds the menu bar as a pure table (`spec(os)`) with the functions that route an item and say whether it is enabled; the rest of the file draws it with Tauri. `docs/gpui-port/menuspec.mjs` writes those pure parts to `app-gpui/src/workbench/menu_spec.rs`, and with `--check` fails when the written file differs, so both apps show the same menus and the table is edited in one place. The alternatives were to move the table into a crate both apps use, which means editing the Tauri app that this work has left untouched, or to copy it by hand, which drifts. The script formats what it writes with rustfmt, as the commit hook does, so the comparison is byte for byte against the committed file. When the Tauri app is retired the table simply stays where the script wrote it.

`workbench/menu.rs` does what the webview's `menuBridge.ts` and the drawing half of `menu.rs` did between them: the focused window's state (`MenuState`) plus the table give the bar (`model`), which is installed with GPUI's `set_menus` only when it differs from the one shown, 120 ms after the window last changed.

Choices:

- An item shows a shortcut only through a GPUI key binding for its command, so the bar binds each shown shortcut to `MenuCommand`. The keymap stays the only dispatcher: the keystroke interceptor marks a key the keymap let pass as "in flight", and a command that arrives while one is in flight is dropped and the key goes on to whatever has the keyboard. Only the items whose shortcut the table itself gives (Quit, window cycling) and the OS roles the keymap never binds (Hide, Hide Others, Minimize) run from the key. `the_menu_bar_follows_the_window_and_runs_chosen_items` fails without the drop. That covers the key GPUI dispatches itself. On macOS there is a second path, read in `gpui-pre-macos` (`handle_key_event`): a key equivalent that nothing in the window handled is reported unhandled to AppKit, which then performs the menu item with that shortcut, in a later cycle when the flag is already cleared. So a chosen item is also dropped when AppKit's current event is a key press, which is how the Tauri app tells the two apart (`handling_key_press`). This second path is not exercised by any test: the test platform has no AppKit menu. It is a reading of the source plus the Tauri app's own practice, to be confirmed by pressing a declined shortcut in the running app.
- The bar follows the focused window only when something it shows could have changed: the windows' context keys are counted as they change, as are settings changes and the recent projects, and a draw that finds the same counts asks for nothing. The table for the OS is built once and the labels are shared, not copied. `the_menu_bar_follows_the_focused_window_when_it_changes`.
- Bindings from an earlier keymap are left bound: GPUI shows the last binding of a command, and a stale one can only fire a command that is then dropped. Clearing all bindings would also remove any other GPUI bindings the app gains later.
- Undo, Redo, Cut, Copy, Paste and Select All are the OS's roles on macOS. Chosen from the bar they run the editor's `edit.*` action in the focused window. A text field that is not the editor does not get them from the bar yet (its own keys work).
- Check for Updates is left out, as the Tauri app does when no updater is configured: this app has none.
- `app.quit` is registered here because the bar is its first user: each window is asked to close, and the app quits when all agree. A window with unsaved changes asks its question and, once answered, closes itself, but the app does not go on to quit. Marked partial.

## D40. Windows are labelled and opened as the Tauri shell does it

`workbench/windows.rs` opens the app's windows (`main`, then `project-<n>` with the lowest free number, the Tauri shell's labels and order) and decides where a project opens: the window that already has it comes to the front, a window showing nothing takes it, and one with a project follows `general.openProjectIn` or asks. `main.rs` opens its first window through it.

Choices:

- The question is the OS's own prompt, so it has This Window, New Window and Cancel but no "Don't ask again" box, and no Attach (attaching a project to a workspace is not ported). The stored setting is honoured when it is set elsewhere.
- Replacing a window's project saves what is unsaved and writes the editor session first, then lets the backend swap the project (`Host::open_project` closes the previous one itself). Close Project uses `close_project_detached`: it runs on the UI thread and must not wait for language servers to stop.
- An opened project is recorded as recent where it is opened (in the workbench, after the backend opened it), with its branch and display name as `AppState::record_project` does, and Open Recent is read again then.

## D41. The Project view's operations are models first

`project_view/` holds what the Project view does beyond listing folders: its panes and view modes, selection and inline editing, clipboard, contributed roots, the file operations behind its actions (new, rename, move, copy, delete with usages, undo), its context and New menus, Select In and module groups. It is a subagent's port of fifteen specs (122 tests), written without a compiler and wired here; it built on the first try and needed three lint fixes. `ide-build` is a dependency for `GradleProject`.

Choices the subagent made, kept:

- The state machine of the tree (load, file-system change, reveal, toggle) exists twice for now: `project_view::store::ProjectViewState`, which the operation specs drive, and the one `project_tree.rs` draws from. The view is to move onto the store when its actions get handlers; until then the overlap is a known cost, not a design.
- The import and Unity refresh specs mock backend calls; here they are checked on the pure models (`LocalImport`, `UnityRefresh`) and marked partial.
- Menu rows reuse `vcs_ops::menu::MenuItem` and are identified by command or label, since it has no id; the Select In header is a title field.
- Not ported: the file refactoring dialogs (only their request and the undo ordering), the React component specs, and every action handler: `actions.rs` computes the actions' titles and targets but registers nothing.

## D42. Appearance options are attributes of the theme

The Appearance options the webview sets as attributes on the document root (`data-compact`, contrast scrollbars, red-green palette, a different tool window background, project colours, menu icons) are passed as attributes when the theme is cascaded, so the stylesheet's own blocks for them apply and nothing is restated in Rust. The four View › Appearance toggles flip their global setting; the status bar and the tool window stripes are drawn only when theirs is on. The four Stretch actions have ids built from a direction in the webview, so they are registered with their metadata written out instead of read from the generated list.

IDE zoom is not ported: sizes come from tokens and from literals in the views, and scaling only the tokens would zoom half the UI. It needs one scale applied where GPUI resolves pixels, which is a change of its own.

## D43. Views written by subagents: File Structure

`everywhere/structure_view.rs` is the first view (not model) written by a subagent without a compiler, against `everywhere/view.rs` as its template; it was wired here, built on the first try and its twelve tests passed, one of them against the real backend (a Makefile's outline, which needs no language server). It is the ⌘F12 popup only: the Structure tool window is a separate component in the webview and is not ported.

Choices the subagent made, kept:

- The popup loads the outline once when it opens and again when inherited members are toggled; the webview also reloads it 500 ms after an edit, which cannot happen under a popup that has the keyboard.
- The selection goes back to the symbol at the caret (or the best match) whenever the shown tree changes, as the webview's effect does: after a sort or visibility option, and when inherited members arrive.
- Keys are claimed where the webview prevents the default: Enter, Up and Down always; Right when the selected node has children; Left when something is selected; Escape only while there is a query, which it clears.
- The selected row uses the inactive selection colour, since the keyboard is in the query field, not in the tree. Search Everywhere's rows use the active one; one of the two is to change when the popups get a common list.
- A supertype's file is opened and closed again in the backend to read its outline, which marks it as recent. (The subagent built the type-hierarchy request from JSON because `ide-lsp` was not a dependency then; it is now, and the view uses its types.)
- An outline that fails to load shows "No symbols" and logs the error, as the webview's popup does.

## D44. Recent Files and the Switcher

`everywhere/recent_view.rs` is one view for Recent Files (⌘E) and the Switcher (⌃⇥), as `RecentPopups.tsx` is one component; a subagent wrote it without a compiler. It built on the first try; one of its seventeen tests failed, and the measured cause was the test's hand-traced expectation (which file row the presses last touched), not the code, so the test was corrected.

What the workbench does for it:

- A tab's editor becoming the active one is counted (`activations`), which is the order the Switcher lists open editors in, and its file is marked recent in the backend then, as the webview does on every change of the active editor. Without the second, ⌘E and Enter would not go back and forth between two open files. `recent_files_and_the_switcher_go_between_the_editors` checks both.
- Closing a tab from the popup does not move the keyboard out of it; hiding a tool window from it gives the keyboard back to the popup after the layout handed it to the editor.

Choices the subagent made, kept: the popup's keys are claimed ahead of the keymap, so ⌃⇥ inside it is always the popup's; releasing Ctrl opens the selection only when Ctrl was seen held, so a Switcher opened from the menu stays open; tool window titles are shown untranslated, as the tool window headers are.

Not there: Recent Locations (its link closes the popup), the set of files edited in this session, opening to the side, and keeping a row when its tab refuses to close (with autosave on, the default, a tab never refuses).

## D45. Settings and templates as models; the backend's settings are the schema

`settings_model/` (22 files) and `templates/` (17 files) are a subagent's port of 29 specs, 260 tests, written without a compiler; wiring them needed two one-line compile fixes and eleven lint fixes. No Settings dialog and no template expansion in the editor exist yet: these are the models they will be drawn from.

- The webview validates settings itself (`validateSettings`: a malformed value becomes the default, text is trimmed, numbers clamped, unknown names dropped). Here the settings are `ide_core::settings::Settings`, the backend's own types, read through serde: a value of the wrong type is refused instead of defaulted, and text and numbers are kept as stored. The specs that test the webview's repairs (`agents`, `cmake`, `goBuildTags`, `behavior`) are marked partial for that reason rather than given a second validator that the backend would not agree with.
- Syntax-tree cases of the templates (the context a template applies in, postfix targets) are driven with hand-built node chains, not a parser; the editor is to pass its real nodes.
- Emmet expansion is behind an `EmmetEngine` trait with no implementation: the webview uses the `emmet` package's tables, and porting those is a job of its own.

## D46. Run configurations as models

`run/` (37 files, 268 tests) is a subagent's port of twenty of the run feature's specs: the configuration store and its persistence, the built-in configuration types for the JVM, Go, CMake, Gradle and .NET targets, the console with its ANSI handling and links, build commands and tasks, update policies of a running application, and navigation from console output. It built on the first try and needed six lint fixes. `ide-lsp` is a dependency now (Java launch information and hierarchy items); the lock check still passes.

Not ported, and the largest gap in this area: the producers that make a configuration from a file or from the caret (`producers`, `producers.ruby`, `contextRun`, `npmGutter`, 113 cases), the Ruby, PHPUnit, .NET test and Tomcat types, and the test runner's parsers. No Run tool window, run widget or gutter icon is drawn yet.

Choices the subagent made, kept: the store hands completed console lines back to its caller instead of keeping test parsers itself; console links reuse the terminal's URL trimming, which also drops an unbalanced closing bracket; index and language-server results the specs mock are plain structs of this module (`FoundSource`, `SymbolHit`) until the views pass the real ones.

## D47. A spec row is checked against the tests it cites

Subagents report how many cases of each spec they ported, and the rows were first marked from those reports with the test modules guessed from file names. `inventory.mjs` now refuses a spec row marked done when the tests it cites are fewer than the spec's cases, counted over all rows that cite the same tests, unless the proof says after a semicolon why fewer suffice. Running it found four groups: three where the cases existed but in modules the proof did not cite (corrected), and one of my own early rows where several cases are one test (now said in the proof). A second pass matched every case title against Rust test names; it found no missing case in the subagent batches, and two rows whose "covered elsewhere" claims could not be found by name, which are now partial (`settings keymap`, `settings languages`). The check is a lower bound on honesty, not a proof of equivalence: it cannot tell a test that asserts the wrong thing.

## D48. The debugger as models

`debugger/` (34 files, 274 tests) is a subagent's port of 28 of the debugger's specs: the session state with its console, frames, values and execution point, the actions (start, attach, step, rerun, hot swap), breakpoints of every kind with their store, the Java launch path, remote and attach configurations, start failures and the missing-adapter flow, and the editor-side models (gutter, run to cursor, smart step, quick evaluate, value tooltips). It built on the first try and needed four lint fixes. `ide-dap` is a dependency now; the lock check passes. Its rows were marked from a match of every spec case title against the test names, not from the subagent's counts (D47).

Not ported: the Debug tool window and its views, the console's links and find bar, expression completion, and the cases that test the web editor's DOM (the breakpoint click area, an inline rendering error). No debugger UI is drawn yet.

It depends on `run::{java_launch, ansi, store, config_types, configs}`: the subagent dropped its own copies of those in favour of the run batch's.

## D49. Editor panes; editors of one document tell each other of their edits

The editor area draws the tab model's layout: every pane has its tab strip and the editor of its selected tab, splits share their room evenly with a line between the panes, and the pane whose editor takes the keyboard becomes the active one. The split actions (Split Right and Down, Unsplit, Unsplit All, Change Splitter Orientation, Move to and Open in Opposite Group, Go to Next and Previous Splitter) run the model's operations.

A file shown in two panes has two editors on one backend document. I first assumed they would follow each other through the backend's `DocPatched` event and the test refuted it: the backend announces only the edits it makes itself, not the ones an editor sends. So an editor now tells the other editors of its document about each edit it made (`DocumentEditors`), and they follow it exactly as they follow a backend edit, selection included. Each editor keeps its own undo history, in which the other pane's edits are entries of their own.

Not there: splitters cannot be dragged (the model keeps no sizes; the panes share evenly), and tabs cannot be dragged between panes.

## D50. One popup menu for every context menu

`ui/popup_menu.rs` draws the menu model the ports already share (`vcs_ops::menu::MenuItem<C>`): rows bound to a registered action, which take the action's title, icon, shortcut, enabled and toggled state, and rows of the menu's own that carry a command `C` back to whoever opened the menu. The workbench opens one with `show_menu` in the popup slot, so it gets the slot's focus restore and context republish, and a chosen action runs through the same deferred path as one chosen in Search Everywhere.

The editor tab's menu is the first user (`tab_menu`, `run_tab_command`), with the webview's rows in its order. Rows bound to actions that are not registered yet (Compare With, Copy Path, Rename File, Configure Editor Tabs) do not show, exactly as the webview's `resolveMenuItems` drops an unknown action; a submenu left with no row is not offered either, which the webview never has to decide because there all of them are registered. Not in the menu yet: Close All Read-Only Tabs (no document metadata is kept per tab), Open in New Window, and the groups other features contribute (Run, Local History, Git, Open In, Bookmarks).

The pointer opens a submenu after 120 ms on its row and closes another after 200 ms, as in the webview. The webview also keeps a submenu open while the pointer moves diagonally towards it; that needs the submenu's drawn rectangle, which this view does not measure yet.

## D51. Presets keep their declaration order (a bug in rows that were marked done)

Where two actions share a key, the webview tries the one declared later first (`effectiveBindings` walks the preset in object order, and dispatch takes the last pair whose action is enabled). `gen-data.mjs` sorted each preset by action id before writing it, and the port held presets in a sorted map, so shared keys resolved alphabetically: for example New File could take ⌘N from Generate, and New Branch ⌥⌘N from Inline. The six "Keymap presets" rows were marked at parity on a test that only checked that every binding is well formed. A subagent porting the keymap specs found it by reading; I confirmed it in the generator's source.

Fixed: the generator writes each preset as pairs in declaration order, `preset_order` exposes that order, and the effective keymap walks it. A user override of an action the preset binds keeps the preset's place with the user's keys, which is what JavaScript's object spread does in `{...preset, ...overrides}` (my earlier test asserted the opposite and is corrected); an action only the user binds comes last. `lets_scoped_actions_win_shared_keys` carries the spec's fourteen pairs for both IntelliJ presets.

Lesson kept in D47's spirit: a row is not at parity because a test exists that mentions it. The preset rows now cite the order tests too.

## D52. Platform and workbench models; specs that test the browser

`platform_more/` (24 modules, 142 tests) is a subagent's port of the platform and workbench specs that were still open: inspections and their controls, remote sessions and which settings go to the remote side, deployment and workflow settings, the .NET project reading, the project session, boot failures, distraction-free mode on the real tool window layout, file drops, file editor and provider registries, running processes, pointer drags and copyable text. It built on the first try with no lint fixes.

Some specs test the web platform itself and have nothing to port: WebKit's double-click and tap ordering quirks (`doubleClick`, `reordered-tap-click`), DOM caret handling on focus changes, script load errors, CSS class names, Tauri channel encoding. Those rows stay unchecked with that reason where they are whole files, and are named in the proof where they are cases of a ported spec. GPUI reports click counts and has no such reordering.

As in D45, settings validators are normalisers over the backend's typed settings; a value of the wrong type is refused by serde rather than defaulted.

The same subagent found the keymap order bug (D51).

## D53. Go to Line and bookmarks

`editor/goto_line.rs` and `everywhere/bookmarks_popup.rs` (the Bookmarks popup and the mnemonic chooser) are a subagent's views; both built on the first try and their 33 tests passed, including the ones the subagent expected to fail. The workbench keeps the project's bookmarks (`BookmarkStore`, read from the UI storage when a project opens) and registers the actions that are not popups: toggle at the caret or at a given line, toggle and go to the ten digit bookmarks, next and previous, file bookmarks.

- The popups work on their own copy of the store and report what to save; the workbench takes the copy back and writes it. That is how the store model is meant to be used and keeps the views free of the workbench.
- Go to Line is a popup in the popup slot, so a click beside it closes it; the webview's is a dialog that stays. Numbers typed beyond half of `u32` saturate, where the webview passes any digits on and clamps when it opens the place.
- The Bookmarks preview shows the file as plain text with the bookmarked line marked. The webview's preview is a second editor on the document. The file is read without opening it as a document, so that previewing does not make it a recent file.
- Editing a description happens in a row of the popup instead of a second dialog, since the slot holds one popup.
- After Delete the selection stays at the same index, as in the webview, even when that is now a header.

Not there: the Bookmarks tool window, bookmark marks in the gutter, and bookmarks following lines as the text is edited (the model has `set_file_lines`; the editor does not call it).

## D54. Find in Files is the popup again

`find/popup_view.rs` is a subagent's view of the Find and Replace in Files popup on the ported models (`FindSession`, `store`, `replace_flow`, scopes). About 3,800 lines written without a compiler built on the first try and its 23 tests passed. `find.inPath` and `find.replaceInPath` now open it, as in the webview; before, `find.inPath` showed the Find tool window (D33), which stays as the place results are kept: Open in Find Window hands the popup's query to the panel (`FindPanel::run_query`), with its scope and masks for that one search.

Choices, the subagent's unless noted:

- What the popup remembers between openings (options, scope, mask) is kept per window in a process global and written when the popup goes away, because the popup slot drops a view without asking it. If the workbench is to own that state later, it is one input field and one accessor away.
- Replace All asks through the OS prompt and checks afterwards that the results are still the ones asked about.
- The popup's keys are claimed ahead of the keymap; ⌘G, ⇧⌘G, F4 and ⌘↓ are left to the `find.popup.*` actions and the `findPopupOpen` context key, which the workbench now publishes.
- A pinned popup stays open when a match is opened and when the click is beside it (mine: the popup slot's backdrop asks the Find popup whether it is pinned).
- The view's tests were written from the component; the webview's own popup spec (25 cases) was not in the subagent's brief and is marked partial, not done.

Not there: see the `search.findInFiles` overlay row; the largest are the preview editor and multi-line queries (the text field is one line).

## D55. Status bar items are buttons

The editor's three status bar items answer a click as in the webview: the position opens Go to Line, the line separator and the encoding open a menu at the pointer (the popup menu keeps itself inside the window, so it opens upwards from the bar). The rows call `Documents::set_line_ending` and `set_encoding` through the editor, and a refusal by the backend (reloading with unsaved changes, a read-only file) is told in a notification under the webview's titles.

The check mark of the current line separator is drawn as the row's icon: the shared menu model has no "checked" for rows of a menu's own, and adding the field while subagents write against the type would have broken their files. It is to become a field when the menus with real toggles arrive.

## D56. Run producers, and a stop that was not honoured

`run_producers/` (14 files, 184 tests) completes the run feature's models: the producers that make a configuration from a file, the caret or a directory for every language the webview knows, the context Run/Debug menu, npm script gutters, runtime and tool lookup, Ruby tests, stack traces, service order, console folds and external tools. It built on the first try.

Its one failing test was a real bug in the earlier run batch, which the subagent had predicted from reading: in `StartRun::java_prepared`, a successful classpath answer that arrived after Stop was pressed went on to launch the program. The webview's spec says the opposite ("cancels the request and ignores a late success response"). Fixed in `run/store.rs`: whether the launch was aborted is asked before the answer is looked at.

Choices the subagent made, kept: what the producers read from global stores in the webview (Go modules, Python packages, the Gradle model, tools on the PATH) is passed in as data; the context menu has rows of its own, since the shared menu model has no header rows; the test-state icons use a small model fed with results, because the test tree is not ported yet (the test runner batch is still being written).

## D57. The rest of the version-control logic, with the three-way merge

`vcs_more/` (28 files, 185 tests) is a subagent's port of 26 of the 27 version-control specs that were open: the three-way merge (its diff3 and the model of the merge editor with accept, ignore, undo and redo), loading the Git log page by page and going to a ref, its filters, search and branch memory, several repositories in one project, show diff, push targets and a rejected push, worktrees, recent changes. It needed one compile fix and one lint fix, and four small changes in existing modules (two comparison modules and one function made visible, `GitLog::view_of_mut`). `pluginCommit` (3 cases) is not started.

Choices the subagent made, kept, with the one that matters most first:

- The webview's diff3 runs on CodeMirror's diff; here it runs on the crate's own comparison (`comparison::lcs` and the chunk optimiser). Where a difference has several equally short answers the boundaries of the clusters may differ from the webview's. The 23 spec cases pass, including the symmetry one; a merge that is ambiguous in that way could still be cut differently. The webview's 3 s and 1 s diff timeouts are not kept.
- Every transaction of the merge model is one undo step; CodeMirror groups adjacent typing.
- White space in word and blank patterns is JavaScript's set written out, not the `regex` crate's.
- An issue-link pattern that needs look-around or backreferences is skipped, as an invalid one is.
- Loading the log is a set of step machines the view drives (`PageLoad`, `GoToRef`); session restore, bookmark sync and revealing a commit are left out of the store.

No merge editor, diff viewer or multi-repository UI is drawn from these yet.

## D58. Notifications are drawn

`workbench/notification_balloons.rs` and `workbench/notifications_view.rs` are a subagent's views on the notifications model; both built on the first try and their 25 tests passed. The workbench owns the model and keeps the two views in step with one routine (`sync_notifications`), called wherever a notification is raised, dismissed or fades and whenever the Notifications tool window shows or hides. A timer wakes it for the next balloon due to fade.

- The views follow the component, not my brief, in three places the subagent checked in the source: a long body scrolls instead of expanding, the list is flat and newest first, and the tool window is reached from its stripe button and `view.notifications`; there is no bell in the status bar.
- Settings › Notifications are read now (they were passed as "none" before): the effective settings, serialised, give the model's settings. "Don't Show Again" writes the group's display to the global settings; the workbench test checks that the next notification of the group raises no balloon.
- A link of a notification runs its command when the command is a registered action. Commands that carry arguments in their own form (a run configuration, a repository) have no runner yet and are logged; the notification still shows the link.
- Balloons are drawn in a deferred layer below the popup slot, so a popup covers them, as the stylesheet orders them.
