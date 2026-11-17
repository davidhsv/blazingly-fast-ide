# Preliminary measurement: native GPUI app vs Tauri app, release builds

## How this was made

This design was made by reading the code. Nothing was built or run: no cargo, rustc, pnpm, or either app.

- **Worktree:** `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`, branch `gpui`.
- **Commits:**
  - HEAD was `dcba373ca` when reading started and `6bf48f7b6` when it ended.
  - The two commits in between change only `docs/gpui-port/DECISIONS.md` and `docs/gpui-port/notes/backlog.md`. This was measured with `git diff --stat`.
  - Files under `app-gpui/` were read with `git show HEAD:`. Two read-only helpers did part of the reading; their `file:line` claims that this design depends on were re-read here.

**Labels.**

- "Measured" means a command was run in this session and its output is quoted: `ps`, `defaults read`, `ls`, `df`, `stat`, `find` counts, `strings`, and `lockdiff.py` on the two lock files.
- "Read" means the line was read in the source.
- Everything about run-time behaviour that was not observed is marked **Hypothesis:**.

**Files.**

- Scripts: `scripts/`.
- Optional app changes: `hunks.md`. None is required.

**Measured facts about the machine.**

- **The owner's app is not running.** At about 13:25 on 2026-11-17, `ps -axo pid,command` showed no process from `/Applications/Blazingly Fast IDE.app/`.
  - The brief says it runs, so the design assumes it may run at any time.
- **The installed bundle** is `ai.l0ng.openidea`, version 0.1.40 (`defaults read …/Info CFBundleIdentifier`, `CFBundleShortVersionString`).
- **Two `com.apple.WebKit.WebContent` processes with ppid 1 were running.** Neither belongs to the owner's app, which was not running. So WebKit helpers cannot be found by parent pid. Whose they are was not established.
- **Language servers.** `[ -e ]` was run in `/usr/local/bin`, `/opt/homebrew/bin` and `/usr/bin` for each name.
  - Not found: `jdtls`, `lemminx`, `kmp-lsp`, `vtsls`, `typescript-language-server`, `basedpyright`, `pyright`.
  - Found: `/usr/local/bin/node`, `/opt/homebrew/bin/mvn` (Maven 3.9.16 in the Cellar), `/opt/homebrew/bin/gh`, `/usr/bin/java`, and docker in `/usr/local/bin`.
- **JDK:** the only one in `/Library/Java/JavaVirtualMachines` is `adoptopenjdk-8.jdk`.
- **The Java bundles are in git.** The jars that the backend embeds with `include_bytes!` (`java/*/…jar`, read in `crates/ide-sdk/src/java.rs:1577-1633` and `crates/ide-tools/src/registry.rs:854`) are tracked blobs, not LFS pointers: `git check-attr filter` reports unspecified, and `git cat-file -s` gives 172300 bytes for one of them. So a `git archive` export holds every build input. The tracked `build.rs` files of the two builds read only files inside the repository and run no commands (read: `app-gpui/build.rs` reads `../src/ui`, `crates/ide-language/build.rs` reads `languages/` and the root `Cargo.lock`, and `src-tauri/build.rs` and `crates/ide-index/build.rs` were also read).
- **The project `/Users/davidvieira/code/Allocadia`:**
  - 1,207,480 files outside `.git`, 16 GiB on disk, and a 1.0 GiB `.git` directory.
  - 17 top-level `node_modules` trees; 122,325 files lie outside them.
  - 56,191 `.java` and 1,921 `.xml` files; 1,811 of the XML files are outside `node_modules`.
  - A root `pom.xml`, `MODULE.bazel`, `WORKSPACE` and `package.json`. There are 270 `pom.xml` files and no Gradle files.
  - Both `.openidea/` and `.idea/` exist. The project and `/private/tmp` are on the same APFS volume (device 16777233), which has 337 GiB free.
  - Only file counts by type were read from the project (`$S/dmeasure/allocadia_types.txt`), no contents.

## Fairness caveats

Every number must be read next to these. Each "less" favours the native app unless it says otherwise.

- **C1. Deep mode starts no language server in either app on this machine.** None is installed where the apps look once `HOME` is the fake home (measured, see above).
  - Both apps search the inherited `PATH` and the home tool folders, then `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin` and `/bin` (read: `crates/ide-core/src/tool_path.rs:16-31`).
  - The home tool folders come from `$HOME` (read: line 17), and `HOME` is the fake home.
  - So Blaze → Deep → Blaze measures the switch itself, plus what Deep starts without servers.
  - The Tauri app's Deep mode also starts a Maven import of every never-synced build. That runs `mvn`, which is found in `/opt/homebrew/bin`; the project has a root `pom.xml` (read: `src/features/build-tools/index.ts:185-200`; ide-build runs `mvn`).
  - The native app only parses build files and starts no `mvn` (helper's reading of `workbench/build_tools.rs:351,401-406`).
  - The Maven child processes are reported in the "child" column, apart from the app and WebKit columns.
  - If the owner wants Deep mode with real servers, they must be installed first (decision D1). Nothing may be downloaded.
- **C2. Native features absent from the measured paths.** Neither the helper nor this design ran these; they come from the helper's grep and `UNWIRED.md`.
  - Inlay hints, code lens and semantic tokens are not used outside their own modules. This is moot while there are no servers.
  - The caret does not blink (inferred), which means less idle repainting.
  - Find in Files has a preview, but only one Find window, no tabs, no pinning and no scope editor.
  - The Git log draws the graph but has no speed search, no per-branch log tabs and no multi-commit selection.
- **C3. Syntax colouring is the same in kind for the files measured.** All 10 files are `.java`. The native app colours Java with tree-sitter (`languages/java.toml:8`); the webview uses CodeMirror.
  - Languages without a native grammar (`.properties`, Groovy, SQL, JSP…) are avoided on purpose. The natively coloured set is `crates/ide-language/src/syntax/grammars.rs:84-115`.
- **C4. Things only the Tauri app does here.** Each is switched off when it can be; otherwise it is counted where it shows up.
  - **`gh` is called when a project opens**, eagerly (read: `crates/ide-github/src/lib.rs:120-131`). `gh` is installed. The fake home leaves it without credentials, and its requests go to a dead proxy.
  - **Changelists are saved automatically** when the tree has changes. This is a write into the clone, and the run records it.
  - **The login-shell `PATH` read** is skipped with `OPENIDEA_SKIP_SHELL_ENV` (read: `crates/ide-host/src/shell_env.rs:26`).
  - **The incoming-commits fetch** is turned off with `vcs.git.incomingCheck = "never"` (read: `crates/ide-core/src/workflow_settings.rs:55-56`; `src/features/vcs/incoming.ts:29`).
  - **Its log default is `info,openidea_lib=debug`** (read: `src-tauri/src/logging.rs:26-28`). Both apps run with `RUST_LOG=info`.
- **C5. Release profiles differ, as shipped.**
  - Native: `lto = true` (fat), `app-gpui/Cargo.toml:96-101`.
  - Tauri: `lto = "thin"`, root `Cargo.toml:143-153`.
  - Both use `codegen-units = 1` and `opt-level = 3`.
  - Decision D5: keep the profiles as shipped (the default) or set `CARGO_PROFILE_RELEASE_LTO=thin` for the native build.
- **C6. Same backend crates, not fully the same dependency versions.**
  - Both builds come from one `git archive` of one commit, and app-gpui uses `../crates/*` by path (read: `app-gpui/Cargo.toml:24-49`).
  - The two lock files share 888 packages; 98 shared names differ in version (measured, `lockdiff.txt`). Most differences are extra versions that GPUI brings in.
  - Of the backend's direct dependencies, only `rustix` (1.1.6 vs 1.1.5) and `toml` 0.8 (0.8.2 vs 0.8.23) differ. `tokio`, `gix`, `tree-sitter`, `serde`, `reqwest`, `rayon` and `redb` are not in the diff list.
  - The native build enables `ide-language/native-highlights` (`app-gpui/Cargo.toml:43`).
- **C7. Window size.**
  - The Tauri window is 1440×900 by config (`src-tauri/tauri.conf.json:13-14`).
  - The native window is GPUI's default size: `window_options` sets only the title bar, at `workbench/mod.rs` (`window_options`, about line 484). The default is unread (Hypothesis: smaller).
  - The run records both sizes (`events.jsonl`, `stopped.window`). Decision D6.
- **C8. File cache.** The OS file cache cannot be purged without `sudo`. Only the very first launch of the campaign reads the project cold from disk.
  - The repetitions alternate the order, so this falls on each app equally often. The first repetition is flagged.
- **C9. The same driver for both.** Key events are posted by PID, with the same keys and the same waits.
  - Neither app's own automation is used: the native `inspect.rs` and the Tauri MCP bridge are debug-only.
  - The waits depend on what each app shows. The time to get there is a result, not a fixed sleep.

## 1. Native app: is each scenario reached today?

The keys come from `app-gpui/assets/gen/keymaps.json`, section `intellij-macos`, which is the default on macOS (helper: `platform/keymap.rs:70-71,332`). These bindings were re-read at HEAD:

- `navigate.gotoFile` Meta+Shift+O (lines 22-25)
- `ide.toggleMode` Ctrl+Alt+Shift+M (145-148)
- `find.inPath` Meta+Shift+F (389-392)
- `toolwindow.git` Meta+9 (1276-1279)

The Tauri bindings are identical (read: `src/platform/keymaps/intellij-macos.ts:10, 29, 72, 238`; `editor.closeTab` Meta+W at 186).

All seven scenarios are reached from `main.rs` through registered actions, per the helper's reading and the `file:line` citations below. **Hypothesis:** each also runs end to end in a release build. A pilot run settles it (Go/no-go G9).

### S1. Cold start

|               | Native                                                                                                                                                                                  | Tauri                                                                                                                                                                               |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entry         | Launch with no argument: Welcome screen (`main.rs:28-33`, `Target::Empty`). `cx.activate(true)` at `main.rs:39` makes it frontmost.                                                     | Launch with no argument: Welcome screen. Settings set `general.reopenLastProject = false`, so no session is restored (default `true`, read: `crates/ide-core/src/settings.rs:644`). |
| Driven        | Spawn; wait until a normal-layer window is on screen; CPU below 3% of a core for 5 s; hold 30 s; stop (part 3)                                                                          | Same                                                                                                                                                                                |
| Less natively | Hypothesis: nothing measured here differs. Both start the plugin host, whose thread wakes every 250 ms (`app-gpui/src/plugins.rs:242-248`; `src-tauri/src/commands/plugin.rs:134-148`). | —                                                                                                                                                                                   |

### S2. Open the project (the clone, given on the command line)

|               | Native                                                                                                                                                                                                                                                                                                                                                             | Tauri                                                                                                                                                                                                                                                                                                            |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entry         | argv folder → `Target::Project` (`main.rs:28-30`) → `open_project_asking` → `Workbench::open_project` → `Host::open_project` (helper: `workbench/windows.rs:85`, `workbench/mod.rs:3338-3467`). Trust is decided beforehand in `<home>/data/project-trust.json`, or the "Trust and Open Project?" dialog blocks it (helper: `workbench/app_actions.rs:1056-1075`). | `windows::cli_launch` parses argv; a folder becomes a project launch (helper: `src-tauri/src/windows.rs:513-552,1263-1278`, `windows/cli.rs:70-105`). Then `openProject` → `Host::open_project`. Pre-trusted the same way (read: `crates/ide-host/src/trust.rs:37-39`, the file is `<data>/project-trust.json`). |
| Driven        | Wait for the window, then the log line `opened <root> in window` (read: `crates/ide-host/src/project.rs:557`), then `indexed N files` or `index snapshot: warm start` (read: `crates/ide-index/src/worker.rs:578`, `:304`), then CPU below 3% for 10 s; hold 30 s                                                                                                  | Same lines: same backend. Tauri also logs to stdout (read: `src-tauri/src/logging.rs:31`).                                                                                                                                                                                                                       |
| Less natively | Hypothesis: nothing for the open itself, since it is the same backend `Host::open_project`. The Tauri app also starts its changelist tracking, the `gh` probe and the git status refresh in the UI (C4).                                                                                                                                                           | —                                                                                                                                                                                                                                                                                                                |

### S3. Open 10 files and scroll each

|               | Native                                                                                                                                                                                                                                                                                                                                            | Tauri                                                                                                                                                                                                                            |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entry         | Cmd+Shift+O (`navigate.gotoFile`, Search Everywhere on its Files tab; helper: `workbench/mod.rs:773-786`). Type the name, Enter (`everywhere/view.rs:505`). PageDown moves one viewport; Cmd+Down goes to the end of the document (read: `app-gpui/src/editor/view.rs:1765`).                                                                     | Cmd+Shift+O (`src/features/search/index.ts:273-280`); Enter in the popup. PageDown and Cmd+Down fall through to CodeMirror's keymap in an editor (helper: `src/platform/keymap.ts:617-625`, `src/editor/core/cm/baseKeymap.ts`). |
| Driven        | Per file: Cmd+Shift+O; type `<Name>.java`; CPU below 10% for 2 s; Enter; wait until the window title ends with ` – <Name>.java` (both apps format the title as `<project> – <file>`, read: native `workbench/mod.rs` `window_title`, about lines 451-471; Tauri `src/features/app/title.ts:12-22`); 40 × PageDown; Cmd+Down; CPU below 5% for 3 s | Same                                                                                                                                                                                                                             |
| Less natively | Highlighting is equivalent for Java (C3). The native editor shapes only the visible rows (helper: `editor/view.rs:2302-2311`) and does not blink the caret. The 10 files are chosen at run time from the clone: the largest `.java` files of 20-200 KB whose basename is unique (`measure.py --select-files`).                                    | —                                                                                                                                                                                                                                |

### S4. Find in Files

|               | Native                                                                                                                                                                    | Tauri                                                                                                                                    |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Entry         | Cmd+Shift+F (`find.inPath`, helper: `workbench/mod.rs:926-933`). The popup has a debounced preview and caps at 2000 matches (helper: `find/popup_view.rs:59-62,634-638`). | Cmd+Shift+F (`src/features/search/index.ts:350-358`) → `searchStart` → `ide_host::ops::search::start`; cap 2000 (`FindInFiles.tsx:102`). |
| Driven        | Cmd+Shift+F; type `import java.util`; CPU below 5% for 5 s; 10 × Down (the preview follows the selection); CPU below 5% for 5 s; Escape                                   | Same                                                                                                                                     |
| Less natively | One Find window: no tabs, pinning or scope editor; the popup's size is not remembered (C2). The search engine is the same backend crate.                                  | —                                                                                                                                        |

### S5. The Git log

|               | Native                                                                                                                                                                                                                                        | Tauri                                                                                                                                     |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Entry         | Cmd+9 (`toolwindow.git` → `mount_git_log`, helper: `workbench/mod.rs:711-721,2440`). The graph is drawn in a `uniform_list` 300 commits at a time, with the next page fetched near the end (helper: `vcs/log_view.rs:12,38,378-394,517-523`). | Cmd+9; the Log tab is the default (helper: `src/features/vcs/index.ts:269-277`, `logStore.ts:188`). Data from `vcsLog` → `ide-vcs` (gix). |
| Driven        | Cmd+9; CPU below 5% for 5 s; 20 × PageDown; settle; Cmd+9 to hide it                                                                                                                                                                          | Same                                                                                                                                      |
| Less natively | No speed search, no branch log tabs, no multi-commit selection (C2). Hypothesis: PageDown reaches the log list in both apps after Cmd+9 (the pilot checks it).                                                                                | —                                                                                                                                         |

### S6. Blaze → Deep → Blaze

|               | Native                                                                                                                                                                                                            | Tauri                                                                             |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Entry         | Ctrl+Alt+Shift+M (`ide.toggleMode`, read: `app-gpui/src/workbench/mod.rs:981-989`). It writes the global `ide.mode`, plus the project file only when the project overrides the mode (helper: `mod.rs:1273-1294`). | Ctrl+Alt+Shift+M (`src/features/app/projectActions.ts:182-195`). Same two writes. |
| Driven        | Ctrl+Alt+Shift+M; wait until the app's global `settings.toml` says `mode = "full"`; CPU below 3% for 10 s; hold 60 s; Ctrl+Alt+Shift+M; wait until it no longer does; settle; hold 60 s                           | Same file, under the Tauri config folder                                          |
| Less natively | No language server in either app (C1). Tauri-only: the Maven import with `mvn`, offline through the fake `~/.m2/settings.xml`, counted as "child". Native-only: nothing.                                          | —                                                                                 |

### S7. Ten minutes idle

|               | Native                                                                                    | Tauri                                                                                           |
| ------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| State         | The project open, 10 tabs, Blaze mode, no popup                                           | Same                                                                                            |
| Driven        | No input for 600 s; 1 Hz samples                                                          | Same                                                                                            |
| Less natively | The plugin host ticks every 250 ms in both. The caret does not blink natively (inferred). | The Tauri app adds the WebKit processes (WebContent, GPU, Networking) and any timers of its UI. |

### What `OPENIDEA_GPUI_HOME` covers, and what the native app still reads

- **Covered.** `OPENIDEA_GPUI_HOME` redirects `config/`, `data/` and `local-data/` and marks the run isolated (read: `app-gpui/src/backend.rs` `AppDirs::resolve`, `under`).
  - Isolated means secrets go to files, never to the keychain (read: `ide_ssh::secrets::keep_secrets_in_files()` in `Backend::start`).
  - It also means no `~/.ssh` configuration is read (`ssh_files`).
  - Without the variable, a release build would use `ai.l0ng.openidea.gpui` (read: `APP_IDENTIFIER`). That differs from the owner's `ai.l0ng.openidea`.
- **Still read from the environment.** The helper found these (`tool_path.rs`; `ide-sdk/src/java.rs:170-190`; `ide-build/src/maven.rs:502`; `ide-index/src/store.rs:62-78`; `ide-fonts/src/lib.rs:202-205`):
  - `HOME`, which determines `~/.gitconfig`, `~/.m2`, `~/.jdks` and the home tool folders;
  - `PATH`, `JAVA_HOME`, the locale variables and the font folders.
- **How the run handles them.** It starts each app with `env -i` and only these variables (`measure.py` `prepare`):
  - `HOME` and `CFFIXED_USER_HOME` set to a fresh fake home;
  - `PATH=/usr/bin:/bin:/usr/sbin:/sbin`;
  - no `JAVA_HOME`;
  - `GIT_CONFIG_NOSYSTEM=1`, `GIT_OPTIONAL_LOCKS=0` and `GIT_TERMINAL_PROMPT=0`.

  So neither app reads the user's git configuration, Maven repository, recent projects or settings. **Hypothesis:** `gix` honours `HOME` for its global configuration, as git does.

- **Fonts.** `~/Library/Fonts` is read through the real home if CoreText ignores `HOME`. That is reading, not writing, and the same for both apps.

## 2. Tauri app: entry points, and how both are driven the same way

- **The entry points** are in the tables above. The native menu drops IDE menu items fired by a key press, so a key reaches the webview's own keymap (helper: `src-tauri/src/lib.rs:367-373`, `macos.rs:209-214`). The window must therefore be key, and the webview focused.
- **The driver** is `scripts/macos.py`. It uses `CGEventPostToPid` for keys: a key code, modifier flags and the character, on a US layout. The real keyboard and pointer are not used, and no other process receives anything.
  - The existing `$S/insp8/click` only posts mouse events (`click.swift` read; `strings` on the binary shows no keyboard symbols). It is not used.
- **Waits** use only what can be observed from outside:
  - the window list (`CGWindowListCopyWindowInfo`), for the first window and the title;
  - the backend's log lines on stdout;
  - the settings file on disk;
  - the CPU of the process set falling below a threshold for N seconds.

  Every wait has a ceiling. A timeout is recorded and the run goes on.

- **Before every key** the guard checks three things:
  - the PID is the recorded measured app (pid and start time), with its executable under the base folder;
  - it is the frontmost application (`lsappinfo front`; no Apple event to any app);
  - the owner's app is not frontmost.

  If any check fails, the run stops. Nothing is posted and nothing is activated, unless the manager passes `--allow-activate` (decision D3). With that flag, `osascript` makes only our PID frontmost through System Events.

- **Focus is handed back only to a checked app.** `$S/insp8/run.sh` ends by making whichever process was frontmost before frontmost again, and that could be the owner's app. The run instead hands focus back only to the app it recorded before the launch, and only if that app is not the owner's (part 3, "Focus at the start and end of each launch").

## 3. Isolation and safety

### The owner's app cannot be reached by the measured Tauri build

Each mechanism below is keyed by the identifier, unless it says otherwise.

| Mechanism                                                               | Owner's app                                                                         | Measured build                                                                                       | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Identifier compiled into the app                                        | `ai.l0ng.openidea` (measured, its Info.plist)                                       | `ai.l0ng.openidea.measure`, merged at build time by `pnpm tauri build --config`                      | Read: release builds have no `dev::configure` (`src-tauri/lib.rs:9-10,87-88`, `cfg(debug_assertions)`). The CLI's `TAURI_CONFIG` "represents the configuration to be merged" (read: `node_modules/@tauri-apps/cli/CHANGELOG.md:2032`). **Hypothesis:** `generate_context!` takes the merged identifier. Gate: `build.sh` stops unless `strings` finds `ai.l0ng.openidea.measure` in the binary and the embedded Info.plist (if any) does not name `ai.l0ng.openidea`.                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Single-instance lock (a second launch hands its arguments to the first) | Would be `/private/tmp/ai_l0ng_openidea_si.sock` (absent now, measured)             | `/private/tmp/ai_l0ng_openidea_measure_si.sock`                                                      | Read: lib.rs:125-127 ("Debug builds have a separate identifier per worktree"); `windows/single_instance.rs:11-14` (the plugin "names its receiver using the configured identifier", Windows). Measured: `/private/tmp/ai_l0ng_openidea_bench_si.sock` exists, left by an earlier build with the identifier `ai.l0ng.openidea.bench`. This shows the macOS naming `<identifier with _>_si.sock`. **Hypothesis:** the plugin 2.5.4 (`Cargo.lock`) keys only on that. Its source is in the cargo registry, which was not read. Run-time gate: the measured app must still be alive after start, and no owner process may appear during the run (`Launch.check`).                                                                                                                                                                                                                              |
| Settings, data, local data, logs                                        | `~/Library/Application Support/ai.l0ng.openidea`, `~/Library/Logs/ai.l0ng.openidea` | `<run>/tauri/{config,data,local,logs}/ai.l0ng.openidea.measure`                                      | Read: `crates/ide-host/src/data_dirs.rs:8-15,19-52` (each `OPENIDEA_*_DIR` root is joined with the identifier; a relative path is an error, never a fallback).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Login-shell `PATH` cache                                                | `~/Library/Caches/ai.l0ng.openidea/login-path`                                      | Not used: `OPENIDEA_SKIP_SHELL_ENV=1`                                                                | Read: `shell_env.rs:26-28,135-137`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| The `openidea` CLI socket                                               | `~/Library/Caches/openidea-<fnv(ai.l0ng.openidea)>.sock`                            | `<fakehome>/Library/Caches/openidea-<fnv(measure id)>.sock`                                          | Read: `crates/ide-cli/src/socket.rs:23-33`. **Hypothesis:** `dirs` takes `HOME` from the environment, as `tool_path.rs:17` does. The run never passes `--cli`, so the only path that runs `open -a <bundle>` is never taken (read: `src-tauri/src/cli.rs:67-79`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Restart                                                                 | `open -n <bundle>`                                                                  | Not a bundle (`--no-bundle`), so `tauri::process::restart` re-executes itself                        | Read: `src-tauri/src/relaunch.rs:16-24,27-34`. It is only reached through a restart request (`lib.rs:338-341`), which the run never sends.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Keychain: saved passwords                                               | Service `ai.l0ng.openidea/<uuid>`. **Not keyed by the identifier.**                 | Never opened                                                                                         | Read: `crates/ide-ssh/src/secrets.rs:25,184-186`. The vault is asked only for a key that the index file lists (`:223-230`). A missing index file gives an empty index (`:315-318`). Every run starts with an empty data folder.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Keychain: plugin secrets                                                | Service `ai.l0ng.openidea.plugin.<id>`. **Not keyed by the identifier.**            | Never opened                                                                                         | Read: `src-tauri/src/commands/plugin.rs:755-784`. Only a plugin calls it, and a fresh data folder has none installed. The catalog only lists `<resource_dir>/plugins` on request (`:305-316`). Measured by grep: no other source in `src-tauri/src` or `crates/*/src` uses `security_framework` or `keyring`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| WebKit storage (localStorage holds open tabs and tool-window layout)    | `~/Library/WebKit/ai.l0ng.openidea/`                                                | **Hypothesis:** a folder under `<fakehome>/Library/WebKit/`                                          | Read: the code sets no website data store (helper: `src-tauri/Cargo.toml:92`). **Hypothesis:** WebKit names the folder after the main bundle identifier. That is the embedded Info.plist's `ai.l0ng.openidea.measure` if the binary has one. Otherwise it is the process name, `openidea`, which other unbundled dev runs share but the owner's bundled app does not. Home is resolved through `CFFIXED_USER_HOME`. Either way, the name is never `ai.l0ng.openidea`: the build gate refuses a binary whose embedded Info.plist names it. Check after the pilot: the `fakehome_library` event lists what appeared under the fake home's `Library/{WebKit,Caches,HTTPStorages,…}` (G10). If nothing appeared there, the storage went to the real `~/Library/WebKit/<that name>`. That is not the owner's folder, but tabs would then carry over between repetitions (risk R4, decision D7). |
| Launch Services                                                         | The installed `.app`                                                                | No `.app`, so no registration and no file associations (`tauri.macos.conf.json` would register many) | Read: `src-tauri/tauri.macos.conf.json`. `--no-bundle`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Menu bar and Dock name                                                  | "Blazingly Fast IDE"                                                                | "OpenIDEA Measure" (`productName`)                                                                   | Config merge                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

**Processes.** The run signals only PIDs it recorded: the pid and start time from `proc_pidinfo`, and an executable under the base folder (`Guard`). Before every signal and every posted event it refuses any process whose executable is under `/Applications/Blazingly Fast IDE.app/`.

**Native isolation.**

- **Folders:** `OPENIDEA_GPUI_HOME=<run>/gpui-home`.
- **Instance and identifier:** the native app has no single-instance plugin (measured by grep: only a comment at `app-gpui/src/backend.rs:87`), and its own identifier is `ai.l0ng.openidea.gpui`.
- **Plugin secrets:** go to files under an isolated home (read: `app-gpui/src/plugins.rs:191-194`).

### Writes into the project

The run uses an APFS clone of the project, `cp -Rc`, at `<base>/project/Allocadia`. On the same volume a clone shares blocks until written, so it costs little disk.

**Hypothesis:** cloning 1.2M files takes 10-30 min.

The owner's project is opened only by `cp` (read) and by `stat`. Neither app ever sees it.

Writes to expect in the clone:

- **Tauri `.openidea/changelists.json` and `.openidea/.gitignore`** when the tree has uncommitted changes (helper: `src/features/vcs/changelists.ts:199-205` → `ops/vcs/commands.rs:1420-1424`).
- **The native changelists save** (inferred).
- **Project settings (`.openidea/settings.toml`)**, only if the project overrides `ide.mode`.
- **Git's `.git/index` refresh**, prevented by `GIT_OPTIONAL_LOCKS=0`. **Hypothesis:** gix's status writes nothing.

Everything else (index, caches, Local History, jdtls data) goes to the apps' own folders (read: `crates/ide-core/src/project_dir.rs:6-11`; `project.rs:114-136`).

How writes are checked:

- Before each run `measure.py --project-stamp` touches a stamp. After the run, `--project-check` lists every path in the clone newer than the stamp (`find -newer`).
- **Writes under `.openidea/` are expected.** The owner's `.openidea` was moved out before the first run, so whatever is there afterwards the app wrote, for example `changelists.json`. That folder is moved into the run's folder, and the clone is as before.
- **Any write elsewhere** gives the next run a fresh clone, and the written one is kept for inspection. Each re-clone adds 10-30 min (Hypothesis) to the campaign.
- The clone's copy of the owner's `.openidea/` (one `.toml`, one `.json` and others, counted by type only) is moved out of the clone by default. A project-level `ide.mode` or project name there would otherwise steer both apps (decision D4).

### No network

There is no way to block the network for a process without root. The design layers these instead:

| What                                                                                                                                | Native                                                             | Tauri                                                                                         | Switched off by                                                                                                                                                                                                                                                                                                                                                                                            |
| ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Plugin marketplace, 60 s after start then daily (`https://github.com/l0ng-ai/openidea/releases/latest/download/plugins-index.json`) | Release only (read: `app-gpui/src/plugins.rs:186-188,242-248`)     | Always (read: `src-tauri/src/commands/plugin.rs:115`)                                         | `<data>/plugins/state.json` = `{"market_url": "<run>/no-market/index.json"}`. That is a local path that does not exist, so the read fails on disk, never over the network (read: `crates/ide-host/src/plugins/market.rs:96-100,115-127`; the user's URL wins over the default, `:229-232`; the field is `market_url`, `plugins/mod.rs:104-113`).                                                           |
| App updater                                                                                                                         | No handler (helper: `workbench/menu.rs:48`)                        | Only with `general.autoCheckUpdates = true`; the default is `false` (read: `settings.rs:657`) | Settings file: `false`                                                                                                                                                                                                                                                                                                                                                                                     |
| What's New release notes                                                                                                            | —                                                                  | First start of a new version                                                                  | `general.showWhatsNewAfterUpdate = false`                                                                                                                                                                                                                                                                                                                                                                  |
| Incoming-commits `git fetch` (20 s after open, then every 15 min)                                                                   | None found                                                         | Yes (read: `incoming.ts:13-14`)                                                               | `vcs.git.incomingCheck = "never"`                                                                                                                                                                                                                                                                                                                                                                          |
| `gh repo view` / `gh api user` when a project opens                                                                                 | Not found                                                          | Yes (read: `ide-github/src/lib.rs:120-131`)                                                   | Fake home without gh credentials (`GH_CONFIG_DIR`); proxies to `127.0.0.1:9`. **Hypothesis:** an unauthenticated gh stops before any request.                                                                                                                                                                                                                                                              |
| Dependency versions (Maven Central, npm)                                                                                            | On build files                                                     | When `pom.xml` / `package.json` is opened                                                     | Not triggered: only `.java` files are opened                                                                                                                                                                                                                                                                                                                                                               |
| Maven import in Deep mode                                                                                                           | No                                                                 | Yes (C1)                                                                                      | `MAVEN_ARGS=-o`, which Maven 3.9 reads, plus a fake `~/.m2/settings.xml` with `<offline>true</offline>`. **Hypothesis:** the sync may pass its own `-s` settings file (read: `crates/ide-build/src/sync.rs:320-330` mentions "`<localRepository>` of the `-s` settings"), which would replace the user settings file. That is why `MAVEN_ARGS` is set too. That the sync's `mvn` inherits it was not read. |
| Temurin download for jdtls                                                                                                          | Only if jdtls is found                                             | Same                                                                                          | jdtls is not installed. The download needs the launcher to be available (read: `crates/ide-lsp/src/lib.rs:862-868`; `ide-host/src/project.rs:282-295`).                                                                                                                                                                                                                                                    |
| Telemetry, crash upload, CDN fonts                                                                                                  | None found                                                         | None found (helper; CSP `font-src 'self' data:`)                                              | —                                                                                                                                                                                                                                                                                                                                                                                                          |
| Agents, Docker, remote                                                                                                              | Only when opened (Docker polls while the Services window is shown) | Same                                                                                          | Not opened; short `PATH`                                                                                                                                                                                                                                                                                                                                                                                   |

**Detector.** Every 10 s, `lsof -nP -a -p <set> -i` lists the sockets of the measured process set. Any non-loopback line is recorded in `events.jsonl` (`stopped.network`). The aggregate refuses no run because of it, but the manager must reject any run that has such lines.

**Proxies.** `HTTP(S)_PROXY` and `ALL_PROXY` point at `127.0.0.1:9`, in both cases. **Hypothesis:** reqwest, curl, git and gh honour them.

### Language servers in Deep mode

- **Which servers the apps would start** for this project:
  - Java: jdtls, with kmp-lsp as fallback (read: `languages/java.toml:26-30,69-74`);
  - XML: lemminx (`languages/xml.toml:11-14`);
  - JS/TS: vtsls, else typescript-language-server, only for open JS/TS documents. None is opened.
- **Missing servers:** none of them is installed where the apps look (measured). A missing server fails as Missing and shows an "Install" notification. Nothing installs without a click (helper: `ide-lsp/src/lib.rs:870-874`; `src/features/tools/missing.ts:44-57`). The run never clicks.
- **Result:** both apps are measured without servers, equally (C1).

### Display, input and ending the run

- **Before the run:** `preflight.sh` checks that the screen is unlocked, the display awake, and Accessibility and Screen Recording granted. It uses `CGSessionCopyCurrentDictionary`, `CGDisplayIsAsleep`, `AXIsProcessTrusted` and `CGPreflightScreenCaptureAccess`.
- **During the run:** the sampler rechecks lock and sleep every second and aborts if either occurs.
- **Keeping the display awake:** `caffeinate -d -i -m -w <run_all pid>` runs for the length of the campaign. It changes no setting.
- **Focus at the start and end of each launch.**
  - The app activates itself when it starts (natively at `main.rs:39`). When the active app ends, macOS activates another one, possibly the owner's.
  - So a launch is refused while the owner's app is frontmost. The pid and start time of the app that is frontmost before the launch are recorded.
  - Before ours is stopped, that recorded app is made frontmost again through System Events, by its unix id. That app is never the owner's, which the guard checks.
  - Residual (Hypothesis): if that app has quit meanwhile, or the activation fails, macOS chooses the next app itself when ours ends. That could be the owner's app, if it runs and is next in line. The run logs this as a `warning`.
  - This is the one way the owner's app could gain focus. It is activation by macOS, not input from the run.
- **At the end of each launch:**
  - SIGTERM, then SIGKILL after 10 s, each to our PID only. Cmd+Q is not posted, because a quit from the front would hand focus to whatever macOS picks.
  - The recorded descendants (git, mvn, gh) get SIGTERM and then SIGKILL, by pid and start time.
  - Our WebKit helpers are waited for (30 s) and reported if still alive, never killed.
  - Our own stale `/private/tmp/ai_l0ng_openidea_measure_si.sock` is removed when no process holds it.
  - `stop.py` is the emergency stop: recorded PIDs only.

## 4. The metrics

- **Process set of one launch:**
  - the `/usr/bin/time` launcher;
  - all its descendants (by ppid, from `proc_pidinfo` at every sample), each kept as (pid, start time);
  - every `com.apple.WebKit.*` process that is new since just before the launch and whose responsible process is the launcher.
- **How the responsible process is set and found:**
  - The launcher is spawned with `posix_spawn` and `responsibility_spawnattrs_setdisclaim`, so it is responsible for itself and its child.
  - The responsible process is read with `responsibility_get_pid_responsible_for_pid`, the SPI that `crates/ide-workspace/src/memory.rs:296-318,444-458` uses.
  - This excludes the owner's helpers and other apps' helpers. The run stops if the app's responsible pid is not the launcher.
- **Per process, every second:** `proc_pid_rusage(RUSAGE_INFO_V4)` gives:
  - `ri_phys_footprint`, the number `footprint -p` and Activity Monitor report (word 9, as `memory.rs:331-334,410-414` reads it);
  - `ri_resident_size` (RSS);
  - user plus system time, in ticks converted with `mach_timebase_info` (words 2-3, as `memory.rs:416-430`);
  - idle and interrupt wakeups;
  - `ri_lifetime_max_phys_footprint` (word 30). **Hypothesis:** the word index, from `<sys/resource.h>`; the sampler flags it if it is ever below the current footprint.

  A process of the set that is alive but unreadable aborts the run. Undercounting the Tauri helpers silently is the one error that would invalidate the comparison.

- **Cross-checks at each scenario's end** (raw, in `tools-*.txt`):
  - `footprint -p <each pid>`;
  - `ps -o pid=,rss=,time=,comm=`;
  - `top -l 2 -s 1 -stats pid,cpu,mem,command -pid …`.

  `/usr/bin/time -l -o time-<launch>.txt` wraps each launch. It reports the app process alone, not its helpers, and is shown apart.

- **Definitions:**
  - **Peak:** the largest 1 Hz sample of the summed footprint (and RSS) in a scenario. The sum of each process's lifetime maximum is also given as an upper bound.
  - **Steady:** the median of the summed footprint over the last 10 samples of a scenario, or the last 60 for idle.
  - **CPU time:** the increase of the summed CPU time over a scenario. Exited processes keep their last reading, so a short-lived child's final slice is missed; at most one sample.
  - **Idle CPU:** idle CPU seconds / 600 s, as a percentage of one core, plus wakeups per second.
  - Every figure is also split into app, WebKit and child.
- **Sampling interval:** 1 s. The sampler is the same in both apps and its cost is equal. `lsof` runs every 10 s.
- **Repetitions:** 3 per app by default (`--reps`), alternating. Repetition 1 runs native then Tauri, repetition 2 Tauri then native, and so on. Each run gets fresh homes and 60 s of rest between runs.
- **Spread:** median [min-max] ±(max-min)/median per metric and app, and native / Tauri of the medians (`aggregate.py`). The run's load average is recorded.
- **Left out:** anything that needs root (`sudo`, `powermetrics`, `purge`, packet filters).

## 5. The builds

Run `scripts/build.sh <base>` (`--dry-run` prints the steps):

1. Check that at least 80 GiB are free.
2. Export HEAD: `git -C <worktree> archive HEAD | tar -x -C <base>/src`. This changes nothing in the worktree and is unaffected by the other agent's uncommitted edits.
3. Install frontend dependencies from the pnpm store only, failing instead of downloading: `pnpm install --offline --frozen-lockfile`.
4. Build the Tauri app: `CARGO_NET_OFFLINE=true pnpm tauri build --no-bundle --config <base>/measure.tauri.conf.json -- --locked`.
   - `scripts/tauri.ts` passes `build` straight to the Tauri CLI (read: lines 51-52).
   - The CLI runs `beforeBuildCommand` (`pnpm build`: tsc, vite build, check-webkit) and builds the release binary `<base>/src/target/release/openidea` with the frontend embedded.
   - A bare `cargo build --release` would not do: **Hypothesis**, it compiles with `cfg(dev)` and loads `devUrl` `http://localhost:1420`. The CLI changelog line 1327 says the CLI now uses `cfg(dev)` instead of the `custom-protocol` feature.
   - `--no-bundle` exists in this CLI (2.12.3): `CHANGELOG.md:1291`.
5. Build the native app: `cd <base>/src/app-gpui && CARGO_NET_OFFLINE=true cargo build --release --locked --bin openidea-gpui`, giving `<base>/src/app-gpui/target/release/openidea-gpui`.
6. Run the gates, then write `config.json`, `binaries.sha256`, `lockdiff.txt` and `disk.txt`. The gates check:
   - the identifier is in the Tauri binary;
   - the embedded Info.plist, if there is one, does not name the owner's identifier.

**Same commit.** Both builds come from the one export, and app-gpui reaches the backend by path. The other differences are in C6.

**Disk.** These are Hypothesis, estimated from measured sizes:

- `app-gpui/target/debug` is 45 GiB and the partial `app-gpui/target/release` is 1.7 GiB, without a binary (measured, `du`).
- The Tauri debug target was 135 GiB (brief).
- A release target carries no debug info, so its size is mostly dependency rlibs:
  - Tauri: 15-30 GiB;
  - native: 10-20 GiB;
  - `node_modules` from the store: a few GiB;
  - the project clone: near 0 at first.
- Total: 30-55 GiB. `build.sh` requires 80 GiB free (337 GiB measured).

**Offline.** `CARGO_NET_OFFLINE=true` and `--locked` (cargo), `--offline --frozen-lockfile` (pnpm). If a crate or package is missing from the caches, the build fails instead of fetching.

- The root lock file pins `sqlx` to a git revision (read: `Cargo.toml:116`), which must already be in cargo's git cache.
- **Hypothesis:** both caches hold everything, since this worktree built both apps before. The pnpm store and the cargo registry are outside the worktree and were not read. A failed offline build is a no-go, not a reason to go online.

## Risks that could invalidate a run

| Risk                                                                        | What happens                                                                          | How it is caught                                                                                  | Remedy                                                                                |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| R1 Posted keys do not reach the window: not key, or the webview not focused | Nothing opens                                                                         | Title waits time out; `file_not_opened` and `mode_not_switched` events                            | Pilot first (G9). `--allow-activate` (D3). For the native app only, H1 in `hunks.md`. |
| R2 Window titles unreadable                                                 | Open-file waits time out                                                              | Preflight requires Screen Recording                                                               | Grant it to the terminal that runs the scripts                                        |
| R3 `--config` does not reach the compiled identifier                        | The Tauri build would be the owner's identity                                         | `build.sh` gate (`strings`, `launchctl plist`)                                                    | No-go                                                                                 |
| R4 WebKit storage not under the fake home                                   | Residue in `~/Library/WebKit/ai.l0ng.openidea.measure`; tabs carry across repetitions | G10 after the pilot                                                                               | D7: delete that one folder between runs, or accept it                                 |
| R5 Disclaim SPI missing                                                     | Helper attribution falls back to responsible pid plus start time                      | Warning event                                                                                     | Accept with a note, or no-go                                                          |
| R6 WebKit helpers unreadable by `proc_pid_rusage`                           | Totals would undercount                                                               | The run aborts                                                                                    | No-go for the comparison                                                              |
| R7 Indexing never logs completion                                           | 30 min ceiling, then the run goes on                                                  | `timeout` event                                                                                   | Read the log; raise the ceiling                                                       |
| R8 Network contact despite the switches                                     | Would add CPU and maybe downloads                                                     | `lsof` lines in `stopped.network`                                                                 | Reject the run; find the source                                                       |
| R9 The machine is used or busy during a run                                 | Noise, or a focus change                                                              | The frontmost check aborts; load average recorded                                                 | The machine is reserved for the campaign (G5)                                         |
| R10 Exact-name ranking in Go to File                                        | The wrong file opens                                                                  | Title check                                                                                       | Recorded per file                                                                     |
| R11 SIGTERM skips the apps' exit cleanup                                    | Tauri and native children may be orphaned                                             | Recorded children are killed by pid and start time; `stop.py` and the `ps` check end the campaign | None needed                                                                           |
| R13 Focus goes to the owner's app when ours ends                            | The owner's app is activated by macOS                                                 | A `warning` event when the focus cannot be handed back                                            | Close the owner's app for the campaign (owner's choice)                               |
| R12 The first launch reads the project cold, the rest warm                  | Order effect                                                                          | Alternation; repetition 1 flagged                                                                 | Discard repetition 1 or add a warm-up run                                             |

## Go / no-go list for the manager

Automated by `preflight.sh`: G2, G3, G4 (partly), G6, G7 and G8.

- **G0.** The scripts run before anything is measured. They were parsed (`ast.parse`, `sh -n`) but never executed, so every ctypes signature in `macos.py` is a Hypothesis until this passes:
  - `macos.py session` prints the session, permissions and frontmost pid without an error;
  - `macos.py windows <pid of any own window>` lists windows with titles;
  - `build.sh <base> --dry-run`, `run_all.sh <base> --dry-run` and `measure.py --base <base> --app native --rep 1 --dry-run` print their plans.
- **G1.** The owner approved:
  - the clone of Allocadia under `/private/tmp/openidea-measure-<random>/project` (D2);
  - moving the clone's `.openidea` aside (D4);
  - Deep mode without servers (D1);
  - focus being taken from whatever is frontmost when each measured app starts (both apps activate themselves, `main.rs:39` natively). This deactivates, but does not focus, the owner's app if it is in front.
- **G2.** `build.sh` passed all gates. The commit is recorded, and `lockdiff.txt` has been read.
- **G3.** No process from the base is running, and our single-instance socket is not held.
- **G4.** The owner's app is either not running, or running and listed (it is never touched). No `.app` from this build exists.
- **G5.** No other heavy work is running on the machine: other agents' cargo and builds are stopped, since the GPUI agent compiles now. The owner will not use the machine for the length of the campaign.
- **G6.** The screen is unlocked, the display awake, and Accessibility and Screen Recording are granted to the terminal that runs the scripts.
- **G7.** The responsibility SPI is present. The disclaim SPI is present, or R5 is accepted.
- **G8.** At least 80 GiB are free before `build.sh` and 20 GiB before the runs.
- **G9.** The pilot passed. It is `run_all.sh <throwaway base> --reps 1` and must show:
  - no `abort`, `file_not_opened`, `mode_not_switched` or `unreadable` events;
  - `timeout` events reviewed one by one: a settle threshold the webview never meets must be raised for both apps, never for one;
  - no `network` lines;
  - a non-empty window title for each app;
  - helpers attributed to the Tauri app (`procs` with `webkit:` kinds);
  - status `ok` in `status.json`. `aggregate.py` counts only `ok` runs unless `--include-degraded` is passed.
- **G10.** After the pilot, the `fakehome_library` event of the Tauri run shows a WebKit folder under the fake home, and none is named `ai.l0ng.openidea` (R4).
- **G11.** After the pilot, `--project-check` lists only the expected writes, or none.

## Decisions for the manager or the owner

- **D1.** Deep mode without language servers (none is installed for either app), or install jdtls and lemminx beforehand where both apps would find them, from a source the owner trusts. They must not be downloaded during the run. A JDK 21+ is also needed, or jdtls would make the apps download Temurin.
- **D2.** Measure on an APFS clone of Allocadia (the default) or not at all. The original must not be opened, because the apps write `.openidea/changelists.json` when the tree is dirty.
- **D3.** Whether the driver may make the measured app frontmost (`--allow-activate`). Off by default: a lost focus stops the run.
- **D4.** Whether the clone keeps the owner's `.openidea/`. The default moves it aside, so both apps start from the same defaults.
- **D5.** LTO as shipped (the default), or thin for both.
- **D6.** Accept different window sizes (recorded), or equalize them. Equalizing needs an Accessibility resize of the measured window, which is not designed here.
- **D7.** If R4 occurs: delete `~/Library/WebKit/ai.l0ng.openidea.measure` between runs, or accept that tabs carry over.
- **D8.** The number of repetitions (3 by default), and whether repetition 1 counts.

## Estimated wall time

Hypothesis, from the waits in `measure.py` and the measured project size.

- **Builds:**
  - export and pnpm install: 5-10 min;
  - Tauri release (frontend, then thin LTO with codegen-units 1): 30-60 min;
  - native release (fat LTO): 30-60 min.
- **Clone:** 10-30 min.
- **One run (one app, one repetition):** 25-45 min:
  - cold launch, about 2 min;
  - project open and index, 2-15 min;
  - 10 files, 3-5 min;
  - Find in Files, 1-3 min;
  - Git log, 2-3 min;
  - mode switch, 3-5 min;
  - idle, 10 min;
  - stop and project check, 2-4 min;
  - rest, 1 min.
- **Pilot:** 2 runs, 1-1.5 h.
- **Campaign of 3 repetitions:** 6 runs, 2.5-4.5 h.

**Total:** 5-8 h from an empty base to the table, of which 3.5-6 h need the machine unattended and awake.
