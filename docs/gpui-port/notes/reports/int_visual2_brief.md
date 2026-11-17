# Integrator brief: second slice of defects seen in the real window, and two unproven save cases (no new feature)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`. Your own scratch subfolder: `$S/dvis2/`.

Format only files you edited, taken from `git diff --name-only`. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff.

This slice fixes what is wrong in the native app's real window against the webview app (`src/`, tokens in `src/styles/tokens.css`; the webview is upstream's v0.1.46 plus the fork's changes: read every component at HEAD, not from an older note). For each item: read the webview's component and its CSS module first and write down the numbers (sizes, gaps, colours as tokens, which states show what), then fix the native view, then prove it with a test that asserts what the window draws (element bounds through `debug_bounds`, the colour a thing is drawn in, presence or absence) or does, and that fails without the fix, then look at it in your own binary. One item at a time; if one does not converge, undo it by editing, say where it stands, and go on. Do not restyle anything the list does not name.

## Part 0: data first (before any visual item)

A. Two cases of saving are not proven by a test (`docs/gpui-port/DECISIONS.md`, D134's "Not proven by a test"): quitting the app with an edited code preview that has no tab (hypothesis: it is counted and saved, or asked about, as closing the window does), and the TODO preview's late close while a Replace in Files changes the same file (hypothesis: the late close can still drop the change). Write each test on the real backend in a temp project, asserting the file's bytes on disk at the end; if either fails, fix the cause (read how `Workbench::save_all_documents` and the documents' reference counts treat a preview) and say what it was.

B. Shift+Tab in the editor outside a template session types a tab character; the webview unindents (read `src/editor/` for the key's binding and command at HEAD, and what it does with a selection over several lines, with no selection, at column 0, with tabs or spaces by the file's indent settings). Failing test first, through the window's key dispatch; then the command, in one undo step.

C. Show Gutter Icons in the gutter's context menu reports unchecked while icons are shown (hypothesis: the key is missing from the JSON the menu reads). Test first: the check mark follows the setting both ways and toggling it hides and shows the icons.

## The visual items, most visible first

1. **Tool window headers show Options and Hide always; the webview shows them on hover or focus of the header.** The Terminal's header was fixed; every other tool window still draws them always, and the Terminal's buttons vanish while its own Options menu is open (they must stay while a menu of the header is open). Read the webview's header (`src/workbench/toolwindows/`, the header component and its CSS at HEAD: which buttons are always shown, which on hover, focus-within or an open menu) and do it once in the shared native header, not per window.
2. **The HTTP Client's tabs are on a row of their own under the Services tool window's header; the webview puts a tool window's own tabs or title content inside its header.** The shared header needs a content slot: read how the webview's tool windows hand content to their header (Services, Run, Debug, Find, Git: which do) and give the native header the same slot; move the HTTP Client's tabs into it, and any other window the webview does this for and the native app does not (list them; move those that are a few lines, list the rest).
3. **The main toolbar's ⋮ menu opens over the toolbar's buttons.** It opens under its button, aligned as the webview's is (read the menu's anchor and alignment in `src/workbench/` toolbar at HEAD).
4. **The Settings tree does not scroll to its selected row** when the dialog opens on a page far down the tree or the selection changes by search or by an action that opens a page (seen with the Vim page and an Agents page). The selected row is brought into view as the webview does (read whether it centres or only reveals).
5. **Vim page:** the rc file's path runs to the edge without an ellipsis (read which end the webview cuts); the conflicts table shows a raw action id where the webview shows the action's title; the Escape row's icon differs from the webview's.
6. **Find Usages preview:** the first row of the preview is cut by the pane's top edge.
7. **Bookmarks popup:** a bookmark on a comment line is labelled "(empty line)". Find the cause (what text the label is taken from and when) and fix it at the cause.
8. **Notebook toolbar:** the Save button shows the fallback glyph; use the icon the webview's button uses (the generated icon set: if the icon is not in it, read `docs/gpui-port/gen-data.mjs` and say what is missing instead of drawing one).
9. **Project Trust page:** its two headings are styled differently from each other where the webview's are alike; in the trust question dialog the "Trust projects in…" label is cut off.
10. **An empty line with a breakpoint or the execution point is tinted across the row** since code previews became editors: compare with the webview (does an empty line get the line tint?) and say which it is; fix only if it differs.
11. **Small ones, each a number:** placeholder text and list rows drawn with another colour token than the webview's (name the tokens on both sides before changing; one shared place); schema-driven Settings hints at a 28 px line where the webview's are another height; the Agents pickers' 4 px margin.

## Tests and shots

Every item has a test that fails without the fix. Shots, light and dark where colours are involved, through `$S/insp8/run.sh` only, with a temp `HOME`, `OPENIDEA_GPUI_HOME` and project under a neutral path `/private/tmp/openidea-inspect-<random>/` (never under the scratch folder: views draw paths; removed afterwards). Never open the Agents tool window or the Agents pages of Settings (item 11's pickers and item 4's Agents page are proven by tests only), never the Services window's Services tab (item 2: the HTTP Client tab only if it opens without the Services tab being shown first; else tests only), no notebook cell is run and no interpreter picked (item 8: the toolbar only), no debug session (item 10: a breakpoint only), nothing is sent, synced, downloaded or signed in to.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell, and do not leave a polling loop behind (a loop on `pgrep -f "cargo test"` matches its own command line and never ends). If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: Part 0 first (each case: the test, whether it failed, the cause, the fix); then one section per item: the webview's numbers with `file:line`, the native numbers before and after, the test and whether it failed without the fix, the shot's absolute path and what you saw; items not fixed with why; anything else that looked wrong in the shots, as a list (do not fix what the list does not name).

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): name the undeclared drafts of other agents then in the tree and anything the integrations before this one changed in the shared tool window header, the Settings tree or the editor's key bindings.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
