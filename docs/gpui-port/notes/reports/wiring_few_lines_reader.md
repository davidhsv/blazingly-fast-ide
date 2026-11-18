I checked all 103 "a few lines" entries at HEAD `00bd90824`. Only 12 are genuinely a call plus a test, so they make a single batch. Nothing was left unchecked, but eight entries still rest on the audit's own text, and each of those lines carries the command that would settle it.

**Verdict counts**

| Verdict                                  | Entries |
| ---------------------------------------- | ------- |
| Few lines                                | 12      |
| Already wired or superseded              | 4       |
| Needs a view                             | 22      |
| Larger than a few lines, but no new view | 11      |
| Needs a backend call                     | 4       |
| Not a wiring item                        | 26      |
| Needs care                               | 23      |
| Windows-only                             | 1       |

"Larger" and "Windows-only" are two categories you didn't define. I added them so items wouldn't be forced into "few lines".

- **Larger:** more than a call, but nothing new to draw. Examples are an event field that several commands must set, a new completion source, or menu state that isn't kept.
- **Windows-only:** the ConPTY hint. The call is small, but no test on macOS or Linux can prove its effect.

**Batch 1: 12 items**
It touches the editor, the debugger gutter, the theme, stashes, Find, Search Everywhere and the project view. The order is by how directly each item lifts a parity row:

1. Back/Forward and Last Edit Location forget deleted files. Raises `unit:src/editor/core/model/editHistory.test.ts`.
2. Selected text stays visible inside a find match. Raises `unit:src/editor/core/cm/selectionMarks.test.ts`.
3. The TODO preview becomes editable. Raises `unit:src/features/todo/TodoView.test.tsx`, possibly `PreviewEditor.test.tsx`. A ready test exists in `proofs16_todo_run_pending.rs` but is not declared.
4. and 5. Breakpoints move with edits. Raises `unit:src/features/debugger/__tests__/breakpoints.test.ts`. The edit event is sent from one place, so adding the text before the edit is one field.
5. "Reload Changed Classes" in the editor menu. Raises `unit:src/features/debugger/__tests__/hotswap.test.ts`.
6. Stash rows show the message and the branch apart. Raises `unit:src/features/vcs/__tests__/decorations.test.ts`.
7. The editor colour scheme follows a light/dark theme switch. Raises `module:src/platform/colorScheme.ts`.
8. The hover shows a quick-fix link. No row found.
9. Search Everywhere re-asks for endpoints when they change. No row found.
10. Find in Files shows the replacement inline. No row found.
11. Paste follows files copied in the file manager. No row found.

For each item the file gives the function where the call belongs (with a quoted line), what is or isn't at hand there, the webview behaviour with `file:line`, and an existing window test to copy the setup from.

**Needs care (kept out of the batch)**

- **Network:** remote development, GitHub pull requests, Download JDK, Clone, GraphQL introspection.
- **Running a program:** External Tools, Go module and Go Generate actions, running with an executor.
- **Deleting:** Drop Stash.
- **Writing outside the project:** installing the `openidea` command.
- **Trust:**
  - Reopening the last project at startup: native never records the last project, so this needs more than the call.
  - OS file drops: a dropped folder opens a project.
  - Gradle build inputs: the call is a few lines, but the reload needs trust, and whether it runs Gradle is unverified.

**What in the audit turned out false**

- **Already wired since the audit:** the UI language switch, the Git log's remembered branches, the log refresh signature, and the settings runtime.
- **Format on save is not one call.** The save entry point has 7 callers, so it needs a step every save goes through. I first planned it as a few lines and moved it to "larger".
- **Commit-time reformat is half wrong.** The commit box already draws a "Reformat code" check, marked unavailable at `workbench/shelf.rs:281-287`.
- **The Inspections page exists** at `inspections_model.rs:632`, but it has no body.
- **Seven entries only look wired.** Their callers are themselves unreached, so a text match counts them wrongly. These are the remote connect dialog, GitHub review diff, repository data, Inspect Code, log loading, the push-rejected dialog and the terminal's `LiveSession`.
- **Mis-sized entries:**
  - Git log speed search needs a typed-text label that no native view draws.
  - Ruby test navigation depends on the test tree.
  - Editor scrollbar marks need a strip the editor doesn't draw.
- **The webview doesn't wire it either:** plugin message catalogs (`registerMessages`) have no production caller there.
- **Ready tests the work list doesn't mention:** undeclared `*_pending.rs` window tests exist for the TODO preview, the Find popup size, merge-view annotate and the GitHub timeline.
- **Line numbers have drifted.** For example, the Find popup's scope-editor arm is now at `workbench/mod.rs:3124`, not :3044.

**Found on the way:** the hover also ignores documentation-link clicks (`editor/intelligence.rs:1214`), so links in a hover do nothing. This is outside the entry.

The file has the summary table, the batch section, and the lists for already wired, needs a view, larger, needs a backend call, not a wiring item, needs care and Windows-only. It is at `$S/drafts/wiring_few_lines_batches.md`.
