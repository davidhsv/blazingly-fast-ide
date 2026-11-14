# Hard rules for an integrator (the only agent that runs cargo)

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01` (branch `gpui`); the native app is `app-gpui/` (its own Cargo workspace; run cargo from `app-gpui/`). The webview app it must match 1:1 is `src/` (React/TypeScript) with tokens in `src/styles/tokens.css`. Shared scratch folder `S=/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`; work in your own subfolder of it. Also read `$S/view_rules.md` and `$S/wiring_brief.md`.

1. NEVER run `cargo fmt`. Format only the files you edited: `rustfmt --edition 2024 <files>`. Take that list from `git diff --name-only` plus the files your brief hands you by name, never from `git status` (it lists other agents' untracked drafts, which you must not format). A file that declares child modules pulls them in: use `$S/fmt.sh` (stdin mode) for those.
2. NEVER restore or overwrite a file from git or from a backup (`git checkout`, `git restore`, `git show X > file`, `git stash`, `cp` of an old copy). No git command that changes anything: no add, commit, reset, stash, clean, push. Read-only git is fine.
3. No edits under `docs/`, `src/`, `src-tauri/`, `e2e/`. No edits under `crates/` unless this brief names the seam; if it does: no `///` doc comment on a type that derives `specta::Type`, then `cargo test -p <crate>` and `cargo clippy -p <crate> --all-targets -- -D warnings` from the worktree root, `cargo test -p openidea export_bindings`, and `git status --short src/ src-tauri/` must print nothing.
4. No new dependencies, no new `unsafe`.
5. Never touch files of other agents that are committed but not declared with `mod` (drafts), unless this brief hands them to you.
6. Tests must not use the network, the real `gh`, the user's agent binaries, an interactive or login shell, the user's git config, the system Trash, the keychain, the Bazel plugin folder, or anything under the user's home. The test build already panics or refuses on most of these; do not weaken a guard.
7. A secret is never drawn, copied or logged in clear.
8. Comments say why the code is as it is. No history, no "port", "webview", "TypeScript", "moved from", task or ticket names in code comments.
9. If you launch the binary, for any reason, a `--self-check` included: always with `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR` on fresh temp folders, on a temp project, in the background; stop it by its PID (never by name pattern), and never use the app's Restart. Never send input to, quit or restart the owner's running "Blazingly Fast IDE".
10. Non-trivial behaviour ships with a Rust test that drives the app the way a user does (registry action, key, click) and asserts what the window shows or what the backend did. No ignored tests, no stubs, no TODO for core logic.
11. Timing: waits use the existing 30 s helpers (`settle`, `wait_long`, `wait_for_notice`); never a fixed sleep as the proof that something did not happen, unless there is no other way and the comment says why.
12. At the end, from `app-gpui/`: the whole suite five times (`cargo test --lib`), `cargo test --test web_view` once, `cargo clippy --all-targets -- -D warnings`, `cargo build` with 0 warnings. Report the five results exactly.

## Report format (final message)

1. Gates: the five suite results, web_view, clippy, build, exactly as measured; any failure seen at any time, with the test name and what you know of the cause.
2. For each part of the brief: what changed (files), the webview source it follows (`file:line`), the test that proves it (full path of the test), and what was left out and why.
3. Inventory rows you believe changed state: key, new state, the named test. A row is done only with a named test that proves it.
4. Mark every statement you did not measure as "Hypothesis:" with the command that would confirm it.
5. Anything false or stale that you found in notes or briefs.
