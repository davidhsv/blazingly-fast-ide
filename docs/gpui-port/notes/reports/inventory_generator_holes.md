The inventory now has 3905 rows, up from 3807: 98 rows added, none removed or renamed, and the generator still ends with no problems. Rows at parity stay at 1195 (9 model only, 936 partial, 14 with nothing to port). The patch applies with `git apply --check` from the worktree root. Applying it to a fresh `git archive HEAD` copy and running the generator gives exactly `INVENTORY.after.md`. Both files pass the project's `oxfmt --check`. Nothing in the worktree was edited.

One change you should know about before applying: `POINT_FILES` is left as it was. Widening it changed `app-gpui/assets/gen/contributions.json`, which `gen-data.mjs` writes from `POINT_FILES` and the Rust app reads (`workbench/mod.rs`, `status_items.rs`, `title_bar.rs`). The new registries are in a separate `REGISTRY_FILES` list that only the inventory reads. With it, `gen-data.mjs --out` gives output byte-identical to HEAD's. If you want these registries in the manifest too, that is a separate change and needs `gen-data.mjs` re-run.

## Proof that no key was lost

- **Key sets:** `scripts/dumpkeys.mjs` runs a copy of `inventory.mjs` that records every row. `scripts/compare.mjs` then compares the key sets: 3807 keys before, 3905 after, 0 lost, 98 added, 0 existing rows moved to another section. I ran it after each fix as well as end to end.
- **Cells:** every line of `INVENTORY.before.md` that is missing from `INVENTORY.after.md` (10 in all) is the overall total, a section count or heading, or the modules section's note. No existing row's cells changed.

## 1. Settings pages (+3)

**Change:** `settingsPages()` now takes an id from either of two forms:

- a line holding only `id: "…"`, as before;
- a whole entry on one line, `{ id: "…", title: …`.

Ids may now contain `-`.

**Rows added:**

- `settings:vcs` (`vcs.tsx:358`), the Version Control parent page in `VCS_PAGES`; this one was missed because of the one-line form.
- `settings:editor.colorScheme.language-defaults` and `settings:editor.colorScheme.console-colors` (`editor/colorScheme.tsx:1741` and `:1776`). These are real `editorPage({...})` pages in `COLOR_SCHEME_PAGES`. They were missed because of the hyphen, not the one-line form.

`scripts/settings_ids.cjs` lists every id the wider scan takes that the old one did not; these three are the only ones. The other one-line `{ id:` entries in that folder are dialog buttons such as `{ id: "connection", label: … }` in `deployment.tsx`. They have no `title`, so they are not picked up.

**Not resolved:** the per-language colour-scheme pages (`languagePageId(language)`) have ids built at run time and still have no rows of their own.

## 2. Context keys (+49: 43 keys, 6 dynamic rows)

**Change in `tsmodel.mjs`:** a new `stringValues(expr)` resolves an expression to the strings it can take. It follows:

- string literals;
- `const` identifiers, in the same file or imported;
- conditionals, `??` and `||`;
- `+` concatenations and template literals, as every combination of their parts;
- parameters and destructured props, to the call arguments and JSX attributes at their call sites (`jsxOf` finds the `<Tree focusContext=…>` sites);
- helper functions' return values;
- as a last step, a type made only of string literals.

The cycle guard is per path, so a constant used twice is not wrongly marked dynamic.

`contextKeyUses()` gathers two kinds of site:

- every `setContext` call whose callee resolves to `src/platform/context.ts` and whose first argument is not a literal; this leaves out `services/dialogs.tsx:291`, a React state setter with the same name;
- every non-literal `when:` property, shorthand `{ when }` included; arrays such as the Gherkin dialects, objects, functions and numbers are left out.

**Change in `inventory.mjs`:** the operand parsing is shared as `noteClause`. The literal text scan runs first, so existing "First source" cells are unchanged. A resolved key's source is the file where its literal is written.

**Counts:** 280 sites (21 `setContext`, 259 `when`). None resolves to nothing without being marked dynamic. The per-site table is in `context_uses.txt`.

**Keys added:**

- the four `focusContext` values given to `Tree`: `projectViewFocus`, `testTreeFocus`, `servicesViewFocus`, `databaseViewFocus`;
- `build.running`, `distractionFree` and `inlineCompletionVisible`;
- six `github.*` handler keys, from the literal names passed to `provideGithubHandler`;
- 30 `merge.<command>` keys, from the template ``setContext(`merge.${command}`)`` and the typed `MergeCommand`.

**Not resolved (6 `(dynamic)` rows, with the source `file:line`):**

- `workbench/toolwindows/store.ts:455`: `` `toolWindow.${tw.id}.visible` ``, one key per tool window.
- `plugins/runtimeUi.tsx:222`: keys set by plugins, `plugin.<id>.<key>`.
- `plugins/workspaceContains.ts:64`: keys from plugin manifests.
- `plugins/points.tsx:210` and `plugins/uiPoints.tsx:190`: `when` clauses from plugin manifests.
- `settings/window/bridge.ts:48`: passes each action's `when` on to the settings window; it is a forwarding site, and those keys are counted where they are written.

**Rows that look wrong but were left:** `context:command` and `context:merge.` (`mergeActions.ts`) were already there. They come from the old text scan reading the template `` when: `mergeEditorFocus && merge.${command}` `` and are not real keys.

## 3. Registries outside the scanned files (+36)

**Change:** a new `REGISTRY_FILES` list holds the files below. `pointName(id, registries)` and `isPointFile()` take it into account for the inventory only. `KEYED_BY_ARGUMENT` (`registerClientCommand`, `registerLanguageSemantics`) takes the id from the first argument. I first applied that to every point, and it changed two existing keys: `registerMenuSlotItem` takes a slot as its first argument, not an id. So it is limited to those two.

| Registry                     | Rows | Examples                                                                                                    |
| ---------------------------- | ---- | ----------------------------------------------------------------------------------------------------------- |
| `registerRelatedProvider`    | 2    | `(at) src/features/rails/index.ts:53`, `(at) src/editor/navigation/index.ts:246`                            |
| `registerClientCommand`      | 1    | `java.action.applyRefactoringCommand`                                                                       |
| `registerLanguageSemantics`  | 4    | `go`, `kotlin`, `php`, `python`                                                                             |
| `registerDiffViewerProvider` | 2    | `github.review`, `vcs.annotateRevision`                                                                     |
| `registerDropTarget`         | 11   | `(at)` per call site                                                                                        |
| `registerCommitCheck`        | 7    | `vcs.rebase`, `vcs.detachedHead`, `vcs.fileNames`, `vcs.largeFiles`, `vcs.analysis`, `vcs.todo`, `vcs.crlf` |
| `registerExternalIcons`      | 4    | plugin icon themes and plugin icons, `(at)`                                                                 |
| `registerCellGrid`           | 1    | `(at) database/Grid.tsx:432`                                                                                |
| `registerTableEditor`        | 2    | `(at) ResultsView.tsx:487`, `(at) TableEditor.tsx:254`                                                      |
| `registerValueEditorActions` | 1    | `(at) JsonValueEditor.tsx:150`                                                                              |
| `registerContentTabSender`   | 1    | `(at) plugins/views/WebView.tsx:311`                                                                        |

**The other exported `register*` functions outside `POINT_FILES`:** there are 52 in all (`scripts/register_survey.mjs`, output in `register_survey.txt`), and 11 of them are the registries above. The other 41 are already rows of another section:

- **They wrap `registerActions` or `registerAction` (Actions section):**
  - editor: `registerPasteHistoryActions`, `registerEditorActions`, `registerLibrarySourcesActions`, `registerInlineCompletionActions`, `registerImportsOnPasteActions`, `registerLspActions`;
  - project tree: `registerProjectActions`;
  - vcs: `registerCompareActions`, `registerVcsActions`, `registerHistoryActions`, `registerLogPaneActions`, `registerLocalHistoryActions`, `registerMergeActions`;
  - run: `registerExternalTools`, `registerRunTabActions`, `registerRunServiceActions`;
  - debugger: `registerDebugActions`, `registerDebugTabActions`;
  - app: `registerInvalidateCachesActions`, `registerLocalizedActions`;
  - plugins: `registerLogActions`, `registerContentActions`;
  - tools: `registerJdkActions`;
  - database: `registerResultTabActions`, `registerDatabaseTreeActions`;
  - deployment: `registerDeploymentActions`.
- **Actions plus a point:** `registerNavigationActions` (plus `registerRelatedProvider`, now rows), `registerGoActions` (plus `registerMenuGroup`), `registerComposerInstall` (plus `registerEditorBanner`).
- **They call a point that already has rows:**
  - `registerFormatOnSave` and `registerActionsOnSave` (`registerSaveParticipant`);
  - `registerOpenOnRemote` (`registerMenuGroup`);
  - `registerPowerSaveBanner` (`registerEditorBanner`);
  - `registerMarkdownLiveSource` (`registerEditorContribution`);
  - `registerFileNavigation` and `registerVariableNavigation` (`registerNavigationProvider`);
  - `registerSpringEndpointProvider` (`registerEndpointProvider`);
  - `registerBuiltinTypes` and `registerDockerRunTypes` (`registerRunConfigType`);
  - `registerBuiltinPages` (`registerSettingsPage`, the Settings pages section);
  - `registerWindowView` (`registerToolWindow`, row `(at) plugins/views/viewWindows.tsx:118`).

## 4. `registerMessages` (no rows)

It exists for features that ship keyed catalogues of their own (`src/platform/i18n.ts:129`), but nothing in `src/` calls it except `i18n.test.ts`, so there are no registering features to give rows. Strings are counted only in "Design tokens, icons and locales", one row per catalogue file with its entry count. With fix 5 those rows cover both kinds of catalogue that `i18n.ts` loads, so `registerMessages` stays in `COVERED_ELSEWHERE`. If a feature calls it later, a row per call site would be the consistent treatment.

## 5. Locale catalogues and `global.css` (+8)

The design section now reads two more sources:

- The keyed catalogues `src/platform/locales/*.json`, with keys `design:messages.<file>` because `locale.<file>` is already used by the source-text catalogues of the same name:
  - `en.json` 527, `zh.json` 527, `ko.json` 462, `es.json` 456, `pl.json` 454, `fr.json` 452, `pt.json` 451.
- `design:global.css`: `src/styles/global.css`, 44 rule blocks.

## 6. Sources no section covered (+2)

The modules section now also lists the source files sitting directly in `src/` and in `src/features/` (`bindings.ts` and `.d.ts` aside): `module:src/main.tsx` (120 lines) and `module:src/features/index.ts` (201 lines). Its note now says it holds the entry point and the feature list.

`scripts/coverage.mjs` maps every `.ts`/`.tsx` file under `src/` (tests, fixtures and `bindings.ts` aside) to the row that accounts for it. Of 1421 files, 1023 fall to a feature row and 398 to a module row; none is left uncovered. Not counted: `src/emmet.d.ts` and `src/vite-env.d.ts`, which are type declarations only, and non-TS files inside feature or module folders (CSS modules, SVGs, grammar files).

## 7. The two `registry.tsx` rows (left as they are)

- `settings:(at) src/features/settings/pages/registry.tsx:64` is the single call that registers every built-in page except Updates.
- `:71` registers the Updates page when the build reports updates.

Both pages also have rows of their own by id, so these two count them a second time. `:84`, the per-language Code Style pages built at run time, is not a duplicate.

Files are in `$S/gen/`:

- `inventory.patch`
- `added_rows.txt` (98 lines: key, section, source file)
- `INVENTORY.after.md`
- `INVENTORY.before.md`
- `context_uses.txt`
- `register_survey.txt`
- `keys.before.json`
- `keys.final.json`
- `scripts/dumpkeys.mjs`
- `scripts/compare.mjs`
- `scripts/coverage.mjs`
- `scripts/context_uses.mjs`
- `scripts/register_survey.mjs`
- `scripts/settings_ids.cjs`
- `scripts/fix2_apply.cjs` (an intermediate step; the patch is the final version)
