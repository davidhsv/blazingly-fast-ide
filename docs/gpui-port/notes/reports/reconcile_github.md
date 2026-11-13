I wrote the reconciled notes file. Nothing was compiled or run: no cargo, node, gh or git writes. Every "compiles" or "passes" below is from reading.

**Notes file:** `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/github_reconciled.md` (46,772 bytes).

**Hunks (20):** `mod.rs` 12, `branches.rs` 2, `crates/ide-github/src/lib.rs` 5, `inventory.mjs` 1. `scratchpad/gh/anchors.py` recounts them all and prints ALL OK: each anchor occurs once, no pending integration inserts a duplicate, and the saved copy of the drafted file is byte-identical.

- **Rewritten (6):**
  - `TOOL_WINDOWS` is now 18 entries and ends with `DEBUG_TOOL_WINDOW`.
  - `editor_all_integration.md` also inserts after the draft's `mod` anchor.
  - The mount arm needed a single-line anchor.
  - `drop_vcs()` occurs twice, so each site got its own anchor.
- **Unchanged (7).**
- **New (7):** the gh seam (5), a `branches.rs` helper that makes the checkout wait for and block other Git operations, as commit `88677f14` requires, and the `inventory.mjs` hunk.
- **Dropped:** the test module the draft pasted into `mod.rs`. The tests now live in `github.rs`.

**Edits to `app-gpui/src/workbench/github.rs`** (copy in `scratchpad/gh/files/`):

- Ten actions registered through the manifest. The draft's hand-copied metadata had drifted, and `createPullRequest` and `selectOpenedPullRequest` are removed because nothing can run them.
- `github.refresh` reconnects before reloading the list. Checkout uses the webview's exact notice texts.
- Credentials masked everywhere they could leak.
- 14 end-to-end tests and 1 plain test, all against a fake gh.

**How gh is located:** once per process, through `OPENIDEA_GH`, then `PATH`, then fixed paths. There is no per-call setting, so a test could only use `set_var`. The new seam is a `GhCli` value in `ide-github` (`located()`, `at()`, `not_installed()`). The existing functions delegate to it, so Tauri is unchanged. Each test sets its own fake `sh /bin/sh <script>` in its own app. A test without one runs no gh at all, and the gh-not-installed state can now be tested.

**"Run gh auth login":** in the webview it runs `terminal.runCommand` with `gh auth login`, which opens a Terminal tab running it. Natively, production does the same. In test builds the request is only recorded and nothing can start. The one test that clicks the button checks the recorded request and that no Terminal opened.

**Tokens:** nothing in the backend, Tauri or webview code reads one (searched). gh keeps its own; where (keyring, a config file or an env var) is a hypothesis. The real leak was a remote URL carrying a credential, which flowed into structs that derive `Debug`, notification bodies and logs. URLs are now masked before they are kept, and every word of gh or git output is masked. A test formats the state with `{:?}` and checks the notifications and clipboard for a sentinel token.

**`inventory.mjs`:** the hunk fills in actions missing from `actions()` with the ids from `actions.json`. Actions goes from 763 to 781 rows (+18), five GitHub ids leave the "Keymap ids" section, so the total grows by 13.

**Not verified:**

- Whether `git remote -v` prints URLs after `insteadOf` rewriting. Assumption 1 in the notes gives the check and the fix, and two assertions fail loudly if it is wrong.
- That ⌘R and Ctrl+F5 reach the GitHub action among the many actions on that key.
- Focus and `activeToolWindow` in tests.
- `classify_failure` against gh's real "no GitHub remote" message.
- Whether rustc warns on the five `Task` fields that are only ever assigned (`branches.rs` does the same and builds clean).
- The inventory row counts, which need node.
