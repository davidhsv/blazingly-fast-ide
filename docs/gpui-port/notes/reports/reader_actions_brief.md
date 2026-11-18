# Reader brief: which partial action rows are already closed by an existing test (reading only, no compiler)

You read; you do not compile, run tests, or edit any tracked file. Work in the git worktree `$W`. Read the native app (`app-gpui/`) and the webview (`src/`) at HEAD with `git grep -n <pattern> HEAD -- <path>` and `git show HEAD:<path>` (another agent is editing test files in the working tree: do not read the working tree's `app-gpui/`). Never run `cargo`. Write your results to the one file named at the end and nowhere else.

## Background

`docs/gpui-port/parity.json` maps inventory rows to `{status, proof}`. A row is at parity only when named native tests start from a window, a registered action or an event and end in what the user sees or what is on disk, covering what the webview's item does. A partial row's text ends in what is still missing (a "Missing:" clause, or older wording such as "not exercised", "no test", "model only", "Not yet:"). Row texts go stale: tests written later for other rows often close a clause without the row being updated, and sometimes the text claims behaviour that is not there.

The full list of native test names is `$S/rust_tests.txt` (one per line, without the crate prefix; a name maps to a `#[gpui_kit::test]` or `#[test]` function under `app-gpui/src`, modules following the path).

## The rows

`$S/read/ROWS_FILE` holds the rows, one per line: the key, then the whole current text.

## For each row

1. Split what is still open into its separate clauses.
2. Read what the webview does for each clause (the row's source is in `docs/gpui-port/INVENTORY.md`: grep the action id there, then read the handler in `src/`). Write down the exact behaviour in one sentence with `file:line`.
3. Look for an existing native test that drives that behaviour through a window, the action registry, a key or a click: grep `$S/rust_tests.txt` for likely words, then read the test's body at HEAD. A test counts only if an assertion of it would fail without the behaviour; a test of a model alone does not count.
4. Also check that the native handler exists at all for the clause (grep the action id under `app-gpui/src`, non-test code).

## What to return, per row

One block per row, in the rows' order:

- the key;
- per clause, one of:
  - CLOSED: the test's full name, the file and line of the assertion, and the assertion quoted (one or two lines);
  - OPEN, testable: the behaviour exists natively (say where, `file:line`) and no test asserts it;
  - OPEN, not built: the native code for it does not exist (say what you grepped);
  - WRONG TEXT: the row claims something that is false (say what is true, with `file:line`);
- a verdict: `all clauses closed`, `some closed`, or `nothing closed`.

Then two lists at the end: the rows whose every clause is closed; the rows where the text is wrong.

Be exact rather than generous: when you are not sure an assertion covers the clause, say OPEN and why. Do not guess a test name: every name you give must be a line of `$S/rust_tests.txt`. If you run short of budget, stop at a row boundary and say which rows were not reached.

Your results go to `$S/read/OUT_FILE` (Markdown, one paragraph per line, no hard wrapping). The repository is public: write no name, path or content of any other project of the machine's owner. Your final message must be a short summary: how many rows read, how many with every clause closed, how many with wrong text, and the output file's path.
