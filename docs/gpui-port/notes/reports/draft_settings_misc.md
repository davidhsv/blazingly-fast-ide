I've written all 11 pages as new files, but nothing has been compiled or run yet. My estimate after integration: 6 pages fully match the webview and 5 are partial (list at the end). Integration needs edits to 7 existing files, all written as exact hunks in the notes. One slip: a bulk find-and-replace briefly changed `settings_view/pages/vcs_git.rs` (line 181). I reverted it to the original text right away and the line count is unchanged, but it's worth a glance in the diff. No other existing file was touched.

**1. Files written** (all under `app-gpui/src/`)

- `settings_view/pages/tools_record_table.rs` (510 lines): `RecordTable::new(&binding, TableSpec, ReadRows, WriteRows, cx)`, plus `set_source`, `rows`, `selected`, `editing`, `lines`. It's a shared +/−/edit table; File Types, Issue Navigation and the CMake environment use it, because the existing `ListEditor` can't hold those values. Events: Cancel.
- `general_date_formats.rs` (251): `DateFormatsView::new` / `at(binding, now, cx)`, with `lines`, `options`, `preview`, `pattern_field`.
- `general_http_proxy.rs` (715): `HttpProxyView::new(binding, Option<Checker>, cx)`, plus `ProxyForm`, `CheckState`, a `ConnectionCheck` global as the test seam, and `checker(cx)`. The password field shows only bullets.
- `general_updates.rs` (152): `UpdateStatusView::new(binding, version, cx)` and `follow_backend`.
- `editor_files.rs` (453): two list specs (ignored names, exclude patterns) and `RecognizedFileTypesView`.
- `tools_actions_on_save.rs` (82), `tools_external_tools.rs` (559): `ExternalToolsView`.
- `vcs_mappings.rs` (132): `MappingsView`, `project_mapping`. `vcs_issue_navigation.rs` (232): `IssueNavigationView`, `IssuePresetsView`.
- `build_sdk_field.rs` (~400): the toolchain drop-down (`SdkFieldView`), an `SdkDetection` global as the test seam, and `ToolPage`, a container for the three Build pages. Detection starts only when the field is first drawn, and in the test build it finds nothing unless a test supplies a detector.
- `build_cmake.rs` (439), `build_gradle.rs` (~140), `build_maven.rs` (150).
- `workbench/tests/proofs19_settings_misc.rs` (~660): 12 end-to-end tests.
- Each page file has its own view tests.

The `mod` lines and every hunk are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/settings_misc_integration.md`. Hunks to existing files:

- `settings_view/pages/mod.rs`: the `mod` lines.
- `workbench/mod.rs`: the test `mod` line, inserted after `mod proofs18_e2e_search;`.
- `settings_view/dialog.rs`: a `page_view()` getter, so tests can read the CMake/Gradle/Maven pages back.
- `settings_view/custom_controls.rs`: a new `Slots::page_view` method and the install block, inserted after the `"Scheme"` line.
- `settings_model/staging.rs`: a hand-written `Debug` for staged changes that hides `proxy.password`.
- `workbench/tests/smoke_settings_pages.rs`: must also accept `<custom>` lines. Without this the existing smoke test breaks on seven pages that only have their own controls. The languages writer's pages hit the same lines.

**2. Actions**
None registered. The tests open each page with `file.settings` and the page id through the registry. Updates' Check Now runs `app.checkForUpdates`, which the tests register as a stub.

Backend calls each page stands for:

- `sdk_detect` (Gradle/Maven), called with the kind inferred by type because `ide-sdk` isn't a dependency of the crate.
- `proxyCheckConnection` (HTTP Proxy, through the seam only).
- `appBuildInfo` (replaced by the crate version), `appUpdateStatus`, and the update-status event (Updates).
- `languages` (File Types, from the backend's language list).
- Apply goes through the existing settings write, and the backend already moves the proxy password into its saved-passwords file.

**3. Spec files**
The only spec beside these pages, `deploymentModel.test.ts`, belongs to another page. All tests here are named "Not from a spec".

**4. Not built, assumptions, likely first failures**

- **Not built:**
  - A real Check connection: it needs `ide-http` added to `app-gpui/Cargo.toml`; the exact hunk is in the notes.
  - Download JDK… (network).
  - The Build type and Generator suggestion lists in CMake.
  - Trimming of the Gradle/Maven path fields as you type.
  - A blank date pattern is flagged red instead of snapping back to the stored one.
  - The popup for Check connection is an inline panel.
  - The Updates page stays hidden in the app, because there is no native `app.checkForUpdates` handler and no update-status source.
- **Not fixed (existing):** the settings dialog model's `Debug` output already includes the saved proxy password in its base settings.
- **Assumptions:**
  - `cx.defer` is reached through `App`.
  - Setting a text field's text does not emit a change event.
  - The CMake profile, external tool and proxy settings structs read and write camelCase with defaults.
  - Clicks land on controls near the top of each page.
  - Issue patterns are checked with Rust's `regex` crate rather than the webview's JavaScript `RegExp`, so look-around patterns are refused here.
- **Likely first failures:**
  - The proxy test: it needs the `Debug` hunk, typing into the password field, and the password landing only in `proxy-password.json`.
  - The File Types test: it needs language id `python` and the scope switch drawn on the page.
  - Gradle and Maven: they need the `page_view` getter.
  - Updates: it needs the stub action and an event emitted with no window reaching listeners.
  - Clippy: `radio` and `text_row` in the proxy view are at 7 parameters, the limit.

**Expected state after integration**

- general.dateFormats → done
- general.httpProxy → partial: Check connection needs the `ide-http` hunk
- general.updates → partial: hidden in the app (no native action or status source)
- editor.files → done
- build.cmake → partial: no Build type or Generator suggestion lists
- build.gradle → partial: no Download JDK…, no trimming
- build.maven → partial: no Download JDK…, no trimming
- tools.actionsOnSave → done
- tools.externalTools → done
- vcs.mappings → done (the Git case is proven only by the view test, since no git runs)
- vcs.issueNavigation → done
