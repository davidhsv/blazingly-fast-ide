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

Status: planned. The crate is a dependency, resolves next to the backend (`lockcheck.mjs` passes) and compiles; no terminal view exists yet.

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
