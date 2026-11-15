# Build tools spec rows: parity map

Proposals: `parity_map_bt.json`; all cases: `bt_map/all_cases.json`. Spec and native test bodies were both read.

## Per spec

| Spec                        | Proven   | Missing | Webview-only | Proposed    |
| --------------------------- | -------- | ------- | ------------ | ----------- |
| model.test.ts               | 14 of 15 | 1       | 0            | partial     |
| ExecutePopup.test.tsx       | 9 of 9   | 0       | 0            | done        |
| actions.test.tsx            | 2 of 4   | 2       | 0            | partial     |
| linkage.test.tsx            | 4 of 16  | 12      | 0            | partial     |
| analyzer.test.ts            | 0 of 7   | 7       | 0            | report only |
| DependencyAnalyzer.test.tsx | 0 of 6   | 6       | 0            | report only |
| autoReload.test.ts          | 0 of 3   | 3       | 0            | report only |
| firstImport.test.ts         | 0 of 2   | 2       | 0            | report only |
| profileReload.test.ts       | 0 of 3   | 3       | 0            | report only |
| pomNavigation.test.ts       | 0 of 7   | 7       | 0            | report only |
| versionCatalog.test.ts      | 0 of 5   | 5       | 0            | report only |
| workspace.test.tsx          | 0 of 4   | 4       | 0            | report only |

In total 29 of 81 cases are proven and 52 are missing. No case is webview-only.

Other rows:

- `feature:build-tools`: partial, with the nine window tests.
- `point.registerOverlay:build-tools.execute`: stays partial. Its proof now adds the `build_tools::execute::tests`; the chooser as a select is still missing.
- The inventory has no `settings:` or `module:` row for `src/features/build-tools`.
- The context-key rows (`build.maven`, `build.gradle`, `MAVEN_WHEN`, `GRADLE_WHEN`) were out of scope and are not proposed.

## Missing behaviour, by feature

- **Dependency Analyzer (14):** analyzer (7), DependencyAnalyzer (6), and model's POM resolution. `maven_pom_at_path` has no caller because the pom.xml menus are missing.
- **Automatic syncs (9):**
  - autoReload (3): syncing a stale build after an outside change; telling the IDE's own saves apart.
  - firstImport (2): the Deep-mode first import and its cancel.
  - profileReload (3): a profile check box only stores the choice.
  - linkage (1): applied scripts. `is_build_file` ignores `gradle/dependencies.gradle`.
- **Add and Unlink Maven Projects (8):** linkage's eight Maven cases.
- **POM navigation (7):** no go-to-declaration in pom.xml; `maven_goto_declaration` has no caller.
- **Workspace windows (5):** workspace (4), plus Link and Unlink still offered in a Workspace window.
- **Version catalog (5):** no accessors in build scripts; `gradle_published_catalog` has no caller.
- **Action list (1):** actions. Add Maven Projects and Analyze Dependencies are absent.
- **Implemented but untested (3):** these need a test, not a feature.
  - The Maven window leaving with the POM (`sync_build_tool_windows`).
  - Dropping a late link result (the `gradle_linked` epoch).
  - Reloading one chosen build (`gradle.reload` `buildDir`).

## Five mappings I am least sure of

1. **ExecutePopup is done on state tests.** The tests drive `ExecuteState`, which `ExecutePopup::claim_key`, `click_row` and `choose_project` call. The window tests drive up, down, typing and Enter. They do not drive Tab, PageUp, PageDown or a section click.
2. **model is 14 of 15.** The POM case was left out only because its function is unreached.
3. **linkage "applied script" is missing.** The `is_gradle_build_input` test covers those paths, but that function feeds only the libraries refresh.
4. **linkage "duplicate task paths" is proven** by a model test and an execute test, reached through `node_config` and `execute_config`. No window test has two builds.
5. **actions "Maven window only while there is a POM" is missing.** Absent with no model and present with one are each seen. The switch from one to the other, and the window's title, icon and anchor, are not tested.
