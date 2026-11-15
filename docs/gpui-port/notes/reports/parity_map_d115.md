# Parity map D115: rows earned by 767e28d2 and cfe32bcc and not recorded

## Proposals (4, all new keys, `replaces: false`)

| Prefix                              | done                 | partial                                                        |
| ----------------------------------- | -------------------- | -------------------------------------------------------------- |
| `boundid:`                          | 1 (`toolwindow.run`) | 0                                                              |
| `point.registerEditorContribution:` | 0                    | 1 (`database.console`)                                         |
| `module:`                           | 0                    | 2 (`src/ui/Input.tsx`, `src/workbench/toolwindows/actions.ts`) |

Validation (`$S/validate_d115.py`): every key is a blank row of INVENTORY.md in its section (lines 954, 1094, 1872, 1846) and none is in parity.json yet; all 20 cited test names are in `rust_tests.txt`; problems: 0.

## Least sure, and why

1. **`module:src/workbench/toolwindows/actions.ts`**: most of what it cites was earned before these commits; the proofs18 tests add only the ⌘4 / Shift+Escape / ⌘⇧F12-restore flow. The row was simply never filled. Leave it out if the bar is "earned by these two commits" in the strict sense.
2. **`module:src/ui/Input.tsx`**: the webview module is Input plus SearchField. Only the plain field is proven. Four of the six tests are `ui::text_field` window tests on a bare field, not in the app. The app-level evidence is the docked Database filter field and the New Branch dialog's paste.
3. **`point.registerEditorContribution:database.console`**: the native console wraps the editor (a `ConsoleView` around it), not as a CodeMirror extension. Completion is only shown to be loaded (the result's model has `main.users`); no test opens a lookup in the console.
4. **`boundid:toolwindow.run`**: solid (key pressed, show/hide/show asserted). One weakness: the "focus if visible but not focused" branch is not exercised by this test.

## Leads checked and rejected (do not re-propose)

- **`action:toolwindow.closeContent`** (blank): the native code does not register it. It appears only as a constant in `vcs_diff/dialog_view.rs:29` and a menu entry. ⌘W in the e2e editor test is `editor.closeTab`.
- **`toolwindow.project`, `.terminal`, `.problems`, `.hideAll`, `.hideActive`**: already `[x]`.
- **`boundid:toolwindow.debug` and `toolwindow.services`**: registered natively (TOOL_WINDOWS), but no test presses their keys.
- **Partial actions the flows drive** (`run.run`, `run.stop`, `code.completion`, `code.showIntentions`, `navigate.declaration`, `find.usages`, `navigate.searchEverywhere`, `navigate.findAction`, `vcs.newBranch`, `vcs.showDiff`, `file.settings`, `project.paste`): each recorded "Missing" names something these tests do not cover.
- **`point.registerOverlay:search.everywhere`**: already updated with the Settings-pages test; its other gaps remain.
- **`e2e:search.spec.ts`, `e2e:plugin-settings.spec.ts`, `e2e:keymap.spec.ts`**: still partial or blank for the reasons in D115.
- **`action:db.sqlScript`** (blank): the test calls `tree.perform(TreeAction::SqlScript)`, not the registry or a key.
- **`ipc:db_connect`, `db_databases`, `db_schema_objects`** (blank): called natively (`workbench/database.rs:180`, `:192`, `:1516`), but no test asserts state that only these operations give (no routines row, no database list). They were also not touched by these commits.
- **`point.registerEditorKind:db-table`, `point.registerServiceView:dbResults`**: the table editor and results stay in the tool window, not an editor tab or service view.
- **`point.registerOverlay:welcome.recentProjects`**: this is the Recent Projects popup, which is not built. The open-project flow uses the Welcome screen.
- **`module:src/ui/Tooltip.tsx`**: its Missing is placement and flip, which nothing here asserts.
- **`module:src/ui/Dialog.tsx`, `unit:src/ui/Dialog.test.tsx`**: Import Data's Enter/Escape is one dialog, not the shared Dialog component.
- **`unit:src/platform/keymapMouse.test.ts`**: the mouse-button listener change has no dedicated test.
- **`context:findBarHasResults`, `context:popupOpen`**: no new test reads them.
- **`settings:appearance`**: the colon fix does not touch the listed Missing items.
- **`unit:…/settings/__tests__/dialog.test.tsx`, `pages.test.ts`, `search/__tests__/dialogs.test.tsx`**: no case for "reopens on the last page" or "Settings pages in Search Everywhere".
- **`e2e:databaseDialogLayout.spec.ts`**: it covers the password, SSH secret and params dialogs, none of which these commits test.
