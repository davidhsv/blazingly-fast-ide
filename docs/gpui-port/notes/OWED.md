# What the final hand-over must say

- Parity is not reached; the measurement and `REPORT.md` are not written until it is. The inventory's first line has the count.
- Early terminal tests of this work appended about 150 lines to the owner's `~/.zsh_history`. Later tests use no interactive shell.
- The macOS menu's key-equivalent path has no test.
- Nobody has looked at the real window: the binary was launched once on a temp project for 25 s with an isolated home (no panic, 121 MB RSS), no key pressed.
- `crates/ide-language`, `crates/ide-workspace` and the root `Cargo.toml` were changed additively by early commits; `src/` and `src-tauri/` are untouched. The Tauri app's build and tests must be re-run and the commands recorded before claiming it still passes.
- Before measuring idle CPU, fix or disclose: branches and tags are listed on every VCS refresh; the TODO feed scans while its window is hidden; publishing the context locks and clones the repository list every time.
- Before-commit checks: see the shelf integration notes; until that lands the rows show ticked although nothing runs.
- No test performs a successful file delete (the backend sends to the system Trash).
- Light mode, the default, starts no language server, and there is no word completion without one.
- The merge editor and the diff are front windows, not editor tabs.
- Text added natively is not in `src/platform/locales`.
- `DataSource` in `crates/ide-db` derives `Debug` with its URL, which can hold a password in URL-only mode.
- The action manifest generator drops conditions set through helpers (D6, D80; `reports/audit_action_conditions.md`).
- Plugins (`reports/plugin_host_assessment.md`): the native app loads the Bazel plugin's code but surfaces none of what it does; its nine commands are registered and probably always disabled (`workspaceContains:` keys are never set). The native app keeps its own app folders and keychain entries, so the installed app's plugin links and grants are not touched. Three ways the native app can still affect the installed one, all through the project or the machine, not the app folders: a Bazel sync writes `.openidea/settings.toml` in the project with jdtls's `-data` inside the syncing app's plugin folder (the installed app picks that up without a restart); with syncOnOpen and a Bazel grant, opening a never-synced workspace syncs without a click; switching to Blaze mode runs `bazel shutdown` in each workspace, stopping the server the installed app uses. The measurement on the owner's large project must therefore run without a Bazel grant, or on a copy.
- Tauri side checked on 2026-11-13 (D82): crate tests of the four changed crates, `cargo check -p openidea`, clippy on two crates. Still owed before claiming "the Tauri app builds and passes": the whole workspace test suite, the frontend's `pnpm` checks and a bundle build.
- Suite stability seen on 2026-11-13: one whole-suite run hung for over ten minutes (the test was not identified), and two Git tests each timed out once in runs that took about 50 s instead of 17 s; each passes alone.
