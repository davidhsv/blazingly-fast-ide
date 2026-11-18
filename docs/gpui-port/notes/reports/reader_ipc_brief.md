# Reader brief: the empty backend-call rows, judged by what the webview does at each call site (reading only, no compiler)

You read; you do not compile, run tests, or edit any tracked file. Work in the git worktree `$W`. Read the native app (`app-gpui/`) and the webview (`src/`) at HEAD with `git grep -n <pattern> HEAD -- <path>` and `git show HEAD:<path>` (another agent is editing test files in the working tree: do not read the working tree's `app-gpui/`). Never run `cargo`. Write your results to the one file named at the end and nowhere else.

## Background

The inventory (`docs/gpui-port/INVENTORY.md`, section of the backend commands) has one row per Tauri command of the webview (`src-tauri/src/commands/<domain>.rs`, bound in `src/bindings.ts`). The rewrite is a native Rust UI (`app-gpui/`) that calls the same backend functions directly (`ide_host::ops::<domain>::<function>`), with no IPC. A row is at parity when named native tests, starting from a window, a registered action or an event and ending in what the user sees or what is on disk, cover what the webview does at each place it calls the command. The native app may reach the same behaviour through a different backend function: that still counts, and the difference is said.

The rows in `$S/read/ROWS_FILE` (one command name per line) are empty today: nothing is recorded for them. Earlier judgements of such rows by their names alone were wrong about half the time, so everything here is checked in the code.

The full list of native test names is `$S/rust_tests.txt` (one per line, without the crate prefix).

## For each command

1. The webview's call sites: `git grep -n "commands\.<camelCaseName>\b\|\"<snake_name>\"" HEAD -- src` (the binding is camelCase of the command's name; skip `src/bindings.ts` itself and test files). For each site, one sentence on what the user does to get there and what they see as the result, with `file:line`.
2. The native side: `git grep -n "<function name>" HEAD -- app-gpui/src` for the `ide_host::ops` function of the same name (and, if there is none, for how the native app does the same thing: the feature's module usually calls a sibling function). Say for each webview site whether the native app has the behaviour, where (`file:line`, non-test code), and through which backend function.
3. Tests: for each site the native app covers, look for a window test that reaches it (grep `$S/rust_tests.txt` by the feature's words, read the body at HEAD). A test counts only if an assertion of it would fail without the call's effect.
4. Whether a test could drive it at all under the project's rules: tests may not run a program of the machine (no `ps`, Docker, real toolchains, `gh`, SSH to anything but an in-process stand-in), use the network beyond loopback stand-ins, or touch the user's home, keychain or SSH configuration. Say when a command's backend function does one of these (read it under `crates/`).

## What to return, per command

One block per command, in the list's order:

- the command;
- each webview site with its sentence;
- per site one of: COVERED (test's full name, file and line of the assertion, the assertion quoted); BUILT, NO TEST (native `file:line`; what a test would drive; whether the rules allow it); NOT BUILT (what you grepped); DIFFERENT FUNCTION (which, and what the user loses, if anything);
- a verdict: `every site covered`, `some covered`, `built, no test`, `not built`, `cannot be tested under the rules`.

Then at the end: the commands with every site covered (these can be claimed now); the commands built with no test where one window test would do (candidates for a tests slice), grouped by the test module that has the harness; the commands not built, grouped by feature.

Be exact rather than generous: when you are not sure, say so. Every test name you give must be a line of `$S/rust_tests.txt`. If you run short of budget, stop at a command boundary and say which were not reached.

Your results go to `$S/read/OUT_FILE` (Markdown, one paragraph per line, no hard wrapping). The repository is public: write no name, path or content of any other project of the machine's owner. Your final message must be a short summary: how many commands read, how many in each verdict, and the output file's path.
