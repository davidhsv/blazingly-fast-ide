# Integrating the Settings pages: Date Formats, HTTP Proxy, Updates, File Types, CMake, Gradle, Maven, Actions on Save, External Tools, Directory Mappings, Issue Navigation

Drafted without a compiler. Run `cargo fmt` first: several lines (mostly in tests) run past 120 columns. No existing file was edited. The page specs already exist in `settings_model/builtin_pages/{behavior,build,editor,tools,vcs}.rs` with the webview's ids, titles, order, keywords, scope flags and slot labels; nothing there changes. What was missing is the views drawn in the slots (and, for CMake / Gradle / Maven, which have no sections, the page views).

New files (all under `app-gpui/src/`):

| File                                           | Lines | What                                                                                                                                                                                                                                               |
| ---------------------------------------------- | ----- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings_view/pages/tools_record_table.rs`    | 510   | `RecordTable`: a +/−/edit table over rows read and written through closures (`ReadRows`, `WriteRows`); 1 column = pattern list (entries kept once), N columns = records. Shared by File Types, Issue Navigation and the CMake environment. 2 tests |
| `settings_view/pages/general_date_formats.rs`  | 251   | `DateFormatsView` (slot "Override system date and time format"). 2 tests                                                                                                                                                                           |
| `settings_view/pages/general_http_proxy.rs`    | 715   | `HttpProxyView` (slot "Manual proxy configuration"), `ProxyForm`, `Checker`, `ConnectionCheck` global seam, `checker(cx)`. 3 tests                                                                                                                 |
| `settings_view/pages/general_updates.rs`       | 152   | `UpdateStatusView` (slot "Current version"), `follow_backend`. 1 test                                                                                                                                                                              |
| `settings_view/pages/editor_files.rs`          | 453   | `lists()` (Ignore files and folders, Exclude patterns), `RecognizedFileTypesView` (slot "File name patterns"), `backend_file_types`, `recognized_file_types`. 2 tests                                                                              |
| `settings_view/pages/tools_actions_on_save.rs` | 82    | `forms()` (slots "Reformat code", "Save"). 2 tests                                                                                                                                                                                                 |
| `settings_view/pages/tools_external_tools.rs`  | 559   | `ExternalToolsView` (slot "External Tools"), `MACROS`, `with_macro`. 3 tests                                                                                                                                                                       |
| `settings_view/pages/vcs_mappings.rs`          | 132   | `MappingsView` (slot "Directory"), `project_mapping(window, cx)`. 1 test                                                                                                                                                                           |
| `settings_view/pages/vcs_issue_navigation.rs`  | 232   | `IssueNavigationView` (slot "Issue ID"), `IssuePresetsView` (slot "Add JIRA Pattern"). 2 tests                                                                                                                                                     |
| `settings_view/pages/build_sdk_field.rs`       | ~400  | `SdkFieldView` (toolchain combo), `Detector`, `SdkDetection` global seam, `detector(window, cx)`, `backend_detector`, `ToolPage` (page container). 2 tests                                                                                         |
| `settings_view/pages/build_cmake.rs`           | 439   | `CMakeProfilesView` (page view of `build.cmake`). 2 tests                                                                                                                                                                                          |
| `settings_view/pages/build_gradle.rs`          | ~140  | `page(binding, detector, cx) -> Entity<ToolPage>`. 1 test                                                                                                                                                                                          |
| `settings_view/pages/build_maven.rs`           | 150   | `page(binding, detector, cx)`, `rest()`. 1 test                                                                                                                                                                                                    |
| `workbench/tests/proofs19_settings_misc.rs`    | ~660  | 12 end-to-end tests (one per page + Reset/Cancel across kinds)                                                                                                                                                                                     |

## 1. `mod` lines — `app-gpui/src/settings_view/pages/mod.rs`

Pure insertions (rustfmt's `reorder_modules` keeps them sorted either way; the languages writer inserts `languages_*` lines after the same or nearby anchors).

After `pub mod appearance;` insert:

```rust
pub mod build_cmake;
pub mod build_gradle;
pub mod build_maven;
pub mod build_sdk_field;
```

After `pub mod deployment_options;` insert:

```rust
pub mod editor_files;
```

After `pub mod general;` insert:

```rust
pub mod general_date_formats;
pub mod general_http_proxy;
pub mod general_updates;
```

After `pub mod terminal;` insert:

```rust
pub mod tools_actions_on_save;
pub mod tools_external_tools;
pub mod tools_record_table;
```

After `pub mod vcs_git;` insert:

```rust
pub mod vcs_issue_navigation;
pub mod vcs_mappings;
```

`app-gpui/src/workbench/mod.rs`, inside `mod tests`, after `    mod proofs18_e2e_search;` insert:

```rust
    mod proofs19_settings_misc;
```

## 2. `app-gpui/src/settings_view/dialog.rs`: read a page view back (tests)

After the method that ends

```rust
    pub fn custom_control(&self, page: &str, label: &str) -> Option<&AnyView> {
        self.custom_views.get(&(page.to_owned(), label.to_owned()))
    }
```

insert:

```rust

    /// The view drawn for the whole of `page`, when it has one.
    pub fn page_view(&self, page: &str) -> Option<&AnyView> {
        self.page_views.get(page)
    }
```

(The languages writer needs the same getter; apply it once.)

## 3. `app-gpui/src/settings_view/custom_controls.rs`

### 3a. A `Slots` method for page views

Before the line `    fn put<V: Render + EventEmitter<ControlEvent>>(` insert:

```rust
    /// `view` drawn for the whole of `page` (a page without sections), its requests answered.
    fn page_view<V: Render + EventEmitter<ControlEvent>>(&mut self, page: &'static str, view: &Entity<V>, cx: &mut App) {
        self.listen(view, cx);
        self.dialog.update(cx, |dialog, cx| dialog.set_page_view(page, view.clone().into(), cx));
    }

```

(If the languages writer adds the same method, keep one.)

### 3b. The views of the 11 pages

After the line `    slots.put(pages::code_style::PAGE, "Scheme", &scheme, cx);` insert:

```rust

    // Appearance & Behavior › System Settings › Date Formats, HTTP Proxy and Updates.
    let date_formats = binding(pages::general_date_formats::PAGE);
    let view = cx.new(|cx| pages::general_date_formats::DateFormatsView::new(date_formats, cx));
    slots.put(pages::general_date_formats::PAGE, pages::general_date_formats::SLOT, &view, cx);
    let proxy = binding(pages::general_http_proxy::PAGE);
    let checker = pages::general_http_proxy::checker(cx);
    let view = cx.new(|cx| pages::general_http_proxy::HttpProxyView::new(proxy, checker, cx));
    slots.put(pages::general_http_proxy::PAGE, pages::general_http_proxy::SLOT, &view, cx);
    let updates = binding(pages::general_updates::PAGE);
    let view = cx.new(|cx| {
        let mut view = pages::general_updates::UpdateStatusView::new(updates, env!("CARGO_PKG_VERSION"), cx);
        view.follow_backend(cx);
        view
    });
    slots.put(pages::general_updates::PAGE, pages::general_updates::SLOT, &view, cx);

    // Editor › File Types.
    let files = binding(pages::editor_files::PAGE);
    slots.lists(files.clone(), pages::editor_files::lists(), cx);
    let view = pages::editor_files::recognized_file_types(files, cx);
    slots.put(pages::editor_files::PAGE, "File name patterns", &view, cx);

    // Tools › Actions on Save and External Tools.
    slots.forms(binding(pages::tools_actions_on_save::PAGE), pages::tools_actions_on_save::forms(), cx);
    let tools = binding(pages::tools_external_tools::PAGE);
    let view = cx.new(|cx| pages::tools_external_tools::ExternalToolsView::new(tools, cx));
    slots.put(pages::tools_external_tools::PAGE, pages::tools_external_tools::SLOT, &view, cx);

    // Version Control › Directory Mappings and Issue Navigation.
    let mapping = pages::vcs_mappings::project_mapping(install.window_label, cx);
    let mappings = binding(pages::vcs_mappings::PAGE);
    let view = cx.new(|_| pages::vcs_mappings::MappingsView::new(mappings, mapping));
    slots.put(pages::vcs_mappings::PAGE, pages::vcs_mappings::SLOT, &view, cx);
    let issues = binding(pages::vcs_issue_navigation::PAGE);
    let table_binding = issues.clone();
    let view = cx.new(|cx| pages::vcs_issue_navigation::IssueNavigationView::new(table_binding, cx));
    slots.put(pages::vcs_issue_navigation::PAGE, pages::vcs_issue_navigation::TABLE_SLOT, &view, cx);
    let view = cx.new(|_| pages::vcs_issue_navigation::IssuePresetsView::new(issues));
    slots.put(pages::vcs_issue_navigation::PAGE, pages::vcs_issue_navigation::PRESETS_SLOT, &view, cx);

    // Build, Execution, Deployment › CMake, Gradle and Maven: pages drawn by views of their own.
    let detector = pages::build_sdk_field::detector(install.window_label, cx);
    let cmake = binding(pages::build_cmake::PAGE);
    let view = cx.new(|cx| pages::build_cmake::CMakeProfilesView::new(cmake, cx));
    slots.page_view(pages::build_cmake::PAGE, &view, cx);
    let view = pages::build_gradle::page(binding(pages::build_gradle::PAGE), detector.clone(), cx);
    slots.page_view(pages::build_gradle::PAGE, &view, cx);
    let view = pages::build_maven::page(binding(pages::build_maven::PAGE), detector, cx);
    slots.page_view(pages::build_maven::PAGE, &view, cx);
```

Update the doc comment of `install` ("on the 13 pages it covers") to 24 if the reviewer cares. No `ControlEvent` variant is added; every view's requests are the existing `Submit` / `Cancel` / `Browse` / `OpenPage`. Check connection and SDK detection go through the seams below, not through `answer`.

## 4. `app-gpui/src/settings_model/staging.rs`: a staged proxy password stays out of `Debug`

Replace

```rust
/// An edit waiting for Apply.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedChange {
```

with

```rust
/// An edit waiting for Apply. Its `Debug` output hides the value of a password setting.
#[derive(Clone, PartialEq)]
pub struct StagedChange {
```

and after the struct's closing lines

```rust
    /// The page that made the change (Reset discards per page).
    pub page: String,
}
```

insert:

```rust

/// Settings that hold a password; the backend keeps them outside the settings file.
const SECRET_KEYS: [&str; 1] = ["proxy.password"];

impl fmt::Debug for StagedChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A change of a password, or of an object holding one, shows no value.
        let secret = SECRET_KEYS.iter().any(|key| *key == self.key || key.starts_with(&format!("{}.", self.key)));
        let value: &dyn fmt::Debug = if secret { &"…" } else { &self.value };
        f.debug_struct("StagedChange")
            .field("scope", &self.scope)
            .field("key", &self.key)
            .field("path", &self.path)
            .field("value", value)
            .field("page", &self.page)
            .finish()
    }
}
```

`proofs19_settings_misc::http_proxy_manual_configuration_applies_and_the_password_stays_secret` asserts this. Not fixed here (pre-existing, outside this assignment): `SettingsDialog` derives `Debug` over `base` / `global_base`, which hold the effective settings with the saved proxy password filled in (`ops::project::settings` calls `with_proxy_password`); a `{:?}` of the dialog model prints it.

## 5. `app-gpui/src/workbench/tests/smoke_settings_pages.rs`: pages of own controls only

Seven pages now draw only controls of their own (`Label: <custom>` lines): Date Formats, HTTP Proxy, File Types, Actions on Save, External Tools, Directory Mappings, Issue Navigation. Replace

```rust
                let unavailable = lines.iter().all(|line| line.starts_with('—') || line.ends_with(UNAVAILABLE));
```

with

```rust
                // A page of controls of its own only is proven by its own tests.
                let unavailable = lines
                    .iter()
                    .all(|line| line.starts_with('—') || line.ends_with(UNAVAILABLE) || line.ends_with("<custom>"));
```

The languages writer's pages hit the same lines. The smoke test also draws Gradle and Maven, whose SDK fields detect: in test builds `build_sdk_field::detector` finds nothing unless a test sets `SdkDetection`, so no JDK / Maven scan or `java_home` runs there.

## 6. Seams (production wiring not done)

- **Check connection…** (`proxyCheckConnection` → `ide_http::client::check_connection`): `ide-http` is not an `app-gpui` dependency, so the view takes a `Checker` from the `ConnectionCheck` global and the button is disabled without one (as the webview disables it outside Tauri). To enable it in the app, add `ide-http = { path = "../crates/ide-http" }` to `app-gpui/Cargo.toml` and set at start (e.g. in `main`/app init after `Backend::init`): `cx.set_global(ConnectionCheck(Rc::new(|proxy, url, then, cx| { let work = cx.global::<Backend>().call(async move { ide_http::client::check_connection(&proxy, &url, &ide_core::CancellationToken::new()).await }); cx.spawn(async move |cx| { let shown = match work.await { Ok(Ok(status)) => Ok(status), Ok(Err(e)) => Err(e.message), Err(e) => Err(e.to_string()) }; cx.update(|cx| then(shown, cx)); }).detach(); })))`. Tested only through a fake checker; never a real request.
- **SDK detection** (`sdkDetect` → `ide_host::ops::sdk::sdk_detect`): `backend_detector` calls it on the backend runtime, naming `ide_sdk::SdkKind` by `serde_json::from_value` inference (ide-sdk is not a dependency). It runs only once a Gradle / Maven field is drawn. Not run by any test.
- **Updates**: the page is listed only when `app.checkForUpdates` is registered; the native app registers no handler and has no `appUpdateStatus` source, so in the app the page stays hidden. `UpdateStatusView` follows `IdeEvent::UpdateStatusChanged` from the backend; tests register a stub action and emit the event.

## 7. Per-page parity

Columns: webview control (label · key · type · default · rule) → native control (selector) → proving test → missing. "e2e" = `workbench::tests::proofs19_settings_misc::…`; "view" = the page file's own tests.

### `settings:general.dateFormats` (`pages/behavior.tsx` DateFormatRows)

| Webview                                                                                                                    | Native                                                                 | Test                                                                                   | Missing                                                                                 |
| -------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Override system date and time format · `general.dateFormat.overrideSystem` · toggle · false                                | `settings-toggle-general.dateFormat.overrideSystem`                    | e2e `date_formats_override_and_pattern_apply`                                          | —                                                                                       |
| Date format · `.pattern` · text (mono) · `dd.MM.yyyy` · disabled unless override; blank not stored; hint "Placeholders: …" | `settings-text-general.dateFormat.pattern` (read-only unless override) | e2e; view `the_pattern_is_editable_once…`, `a_blank_pattern_is_flagged_and_not_staged` | blank input is flagged red and kept, where the webview snaps back to the stored pattern |
| Use 24-hour time format · `.use24Hour` · toggle · true · disabled unless override                                          | `settings-toggle-general.dateFormat.use24Hour`                         | view                                                                                   | —                                                                                       |
| Use pretty formatting · `.pretty` · toggle · true · hint                                                                   | `settings-toggle-general.dateFormat.pretty`                            | e2e                                                                                    | —                                                                                       |
| Preview (2 h and 3 days before opening)                                                                                    | `settings-date-preview` (`format_timestamp`)                           | e2e, view                                                                              | —                                                                                       |

State: done.

### `settings:general.httpProxy` (`pages/httpProxy.tsx`), global only

| Webview                                                                                                                                | Native                                                                                                        | Test                                                                              | Missing                                               |
| -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ----------------------------------------------------- |
| No proxy / Auto-detect (hint) / Manual · `proxy.mode` · radio · `auto`                                                                 | `settings-radio-proxy.mode-{none,auto,manual}`                                                                | e2e `http_proxy_manual_configuration_applies_and_the_password_stays_secret`, view | —                                                     |
| HTTP / SOCKS · `proxy.kind` · radio · `http` · disabled unless manual; a default port moves with the kind                              | `settings-radio-proxy.kind-*`                                                                                 | view `manual_configuration_enables…`                                              | —                                                     |
| Host name · `proxy.host` · text · disabled unless manual                                                                               | `settings-text-proxy.host`                                                                                    | e2e                                                                               | —                                                     |
| Port number · `proxy.port` · integer 1–65535 · 80 · read-only text unless manual                                                       | `settings-number-proxy.port` (invalid flagged, not staged)                                                    | e2e, view `a_port_out_of_range…`                                                  | —                                                     |
| No proxy for · `proxy.noProxy` · text · hint                                                                                           | `settings-text-proxy.noProxy`                                                                                 | lines in e2e                                                                      | —                                                     |
| Proxy authentication · `proxy.authenticate` · checkbox · false · disabled unless manual                                                | `settings-toggle-proxy.authenticate`                                                                          | e2e                                                                               | —                                                     |
| Login · `proxy.login`; Password · `proxy.password` (type=password) · disabled unless manual+auth                                       | `settings-text-proxy.login`; `SecretField` in `settings-secret-proxy.password` (dimmed bullets when disabled) | e2e (not in lines, log, Debug, settings.toml; kept in `proxy-password.json`)      | —                                                     |
| Problem line (`proxyProblem`)                                                                                                          | `settings-proxy-problem` (same four texts)                                                                    | view                                                                              | —                                                     |
| Check connection… → URL popup, Check, "Connecting…", "Connection successful: …", "Problem with connection: …" (`proxyCheckConnection`) | `settings-proxy-check`, `-url`, `-run`, `-result` (panel under the button)                                    | view (fake checker), e2e (fake checker through `ConnectionCheck`)                 | real checker not wired (§6); popup is an inline panel |

State: partial (Check connection needs the `ide-http` wiring of §6; `SettingsDialog` Debug, §4).

### `settings:general.updates` (`pages/updates.tsx`)

| Webview                                                                                                                                       | Native                                                                                 | Test                                                               | Missing                                                                                   |
| --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| Current version (`appBuildInfo().version`)                                                                                                    | `UpdateStatusView` row; `CARGO_PKG_VERSION` (**assumption**: same version)             | e2e `updates_status_check_now_and_the_automatic_check_apply`, view | —                                                                                         |
| Last checked: Loading… / Never / date+clock; Check Now / Checking for updates…, disabled without the action, without status or while checking | `settings-updates-last-checked`, `settings-updates-check` (runs `app.checkForUpdates`) | e2e (event emitted, stub action)                                   | no native `appUpdateStatus` source: stays "Loading…" until an `UpdateStatusChanged` event |
| Check for updates automatically · `general.autoCheckUpdates` · toggle · false · hint                                                          | standard control                                                                       | e2e                                                                | —                                                                                         |
| Update source · `general.updateSource` · select · auto · hint                                                                                 | standard control                                                                       | e2e                                                                | —                                                                                         |

State: partial (the page is hidden in the app: no native `app.checkForUpdates` handler and no status source).

### `settings:editor.files` (`pages/builtins.tsx`, `FileTypes.tsx`), project scope

| Webview                                                                                              | Native                                              | Test                                                                         | Missing |
| ---------------------------------------------------------------------------------------------------- | --------------------------------------------------- | ---------------------------------------------------------------------------- | ------- |
| Filter file types · text                                                                             | `settings-file-types-filter`                        | view `the_filter_matches_names_and_patterns…`                                | —       |
| Recognized file types list (icon, name, "modified") sorted by name; first shown selected             | `settings-file-type-<id>` (`collate::compare`)      | e2e `file_types_patterns_and_ignored_names_are_written_to_the_project`, view | —       |
| File name patterns of the selected (`files.fileTypes` additions/removals, `applyPatternEdit`) · hint | `RecordTable` `settings-table-file-type-patterns-*` | e2e (project file), view                                                     | —       |
| Hashbang patterns (`setHashbangs`) · hint                                                            | `settings-table-file-type-hashbangs-*`              | view                                                                         | —       |
| "Also recognized by its first line:"                                                                 | note line                                           | view                                                                         | —       |
| Ignore files and folders · `files.ignoredNames` · glob list · defaults · hint                        | `ListEditor` `settings-list-files.ignoredNames-*`   | e2e                                                                          | —       |
| Exclude patterns · `files.exclude` · glob list · [] · hint                                           | `ListEditor` `settings-list-files.exclude-*`        | e2e (lines)                                                                  | —       |

Languages come from `host.languages.all()` (**assumption**: the same list `commands.languages()` returns). State: done.

### `settings:build.cmake` (`pages/cmake.tsx`), project first

| Webview                                                                                                                                                       | Native                                                    | Test                                                                                                | Missing                                              |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| CMake profiles list + / − (`cmake.profiles`, default Debug profile; new "CMake profile N")                                                                    | `settings-cmake-add/remove`, `settings-cmake-profile-<i>` | view `add_and_remove_profiles…`                                                                     | —                                                    |
| Name, Build type, Generator (placeholder "CMake default"), CMake arguments (`parseArgs`), Build directory (placeholder `cmake-build-<type>`), Build arguments | `settings-text-cmake.<field>`                             | e2e `cmake_profile_settings_are_written_to_the_project`, view `arguments_typed_are_stored_as_words` | the datalist suggestions of Build type and Generator |
| Environment variables table (Name/Value)                                                                                                                      | `RecordTable` `settings-table-cmake-env-*`                | e2e, view                                                                                           | —                                                    |
| Note "Quote arguments…"                                                                                                                                       | note                                                      | — (drawn)                                                                                           | —                                                    |

State: partial (no suggestion lists).

### `settings:build.gradle` (`languages/LanguagePages.tsx` GradlePage), project first

| Webview                                                                                                       | Native                                       | Test                                                                      | Missing                                                  |
| ------------------------------------------------------------------------------------------------------------- | -------------------------------------------- | ------------------------------------------------------------------------- | -------------------------------------------------------- |
| Build and run using · `languages.java.gradle.runUsing` · Gradle/OpenIDEA · gradle · hint                      | `settings-select-…runUsing`                  | e2e `gradle_jvm_distribution_and_home_are_written_to_the_project` (lines) | —                                                        |
| Gradle JVM · `languages.java.gradle.jvm` · SDK combo (jdk, empty "Project JDK"), Detect again, Browse… · hint | `SdkFieldView` `settings-sdk-…jvm`           | e2e (fake detection), view                                                | "Download JDK…" (network; the languages writer's dialog) |
| Distribution · `.distribution` · wrapper/local · wrapper · hint                                               | `settings-select-…distribution`              | e2e                                                                       | —                                                        |
| Gradle home · `.home` · path + Browse · shown only for local · hint                                           | `settings-text-…home` (`ToolPage` condition) | e2e, view                                                                 | value not trimmed as typed                               |
| Gradle user home · `.userHome` · path, placeholder `~/.gradle` · hint                                         | `settings-text-…userHome`                    | e2e (lines)                                                               | value not trimmed                                        |

State: partial (Download JDK…, trimming).

### `settings:build.maven` (MavenPage), project first

| Webview                                                                                               | Native                                                     | Test                                                               | Missing            |
| ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------------------ | ------------------ |
| JRE · `languages.java.maven.runnerJre` · SDK combo (jdk, "Use Project JDK") · hint                    | `settings-sdk-…runnerJre`                                  | e2e `maven_home_and_thread_count_are_written_to_the_project`, view | Download JDK…      |
| Maven home path · `.home` · SDK combo (maven, "Auto" — "Maven wrapper, else mvn on PATH") · hint      | `settings-sdk-…home`                                       | e2e                                                                | —                  |
| User settings file / Toolchains file (file browse) / Local repository (folder) · placeholders · hints | `settings-text-…{userSettings,toolchains,localRepository}` | e2e (lines), view                                                  | values not trimmed |
| Active profiles · `.activeProfiles` · text "dev,integration" · hint                                   | `settings-text-…activeProfiles`                            | e2e (lines)                                                        | —                  |
| Thread count · `.threads` · text "4, 2C" · hint                                                       | `settings-text-…threads`                                   | e2e (typed), view                                                  | not trimmed        |
| Use Maven Daemon (mvnd) · `.daemon` · toggle · false · hint                                           | `settings-toggle-…daemon`                                  | view                                                               | —                  |
| Maven Daemon home path · `.mvndHome` · path · hint                                                    | `settings-text-…mvndHome`                                  | e2e (lines)                                                        | —                  |

State: partial (Download JDK…, trimming).

### `settings:tools.actionsOnSave` (`pages/tools.tsx`), project scope

| Webview                                                                                                                                                         | Native                                   | Test                                                   | Missing                         |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- | ------------------------------------------------------ | ------------------------------- |
| Reformat code · `editor.formatOnSave`; Optimize imports · `actionsOnSave.optimizeImports`; Run code cleanup · `actionsOnSave.cleanup` · toggles · false · hints | `FormView` toggles                       | e2e `actions_on_save_are_written_to_the_project`, view | —                               |
| Note with link "Editor › General"                                                                                                                               | `Part::Link` → `OpenPage editor.general` | e2e (lines), view `the_note_opens_editor_general`      | the trailing "." after the link |

State: done.

### `settings:tools.externalTools` (`pages/tools.tsx`), global

| Webview                                                                                                                                                | Native                                                                                       | Test                                                                         | Missing |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------- |
| Tools list (enabled checkbox, name, group); + / − / Copy / Move Up / Move Down; empty text                                                             | `settings-tools-{add,remove,copy,up,down}`, `settings-tool-<i>`, `settings-tool-<i>-enabled` | e2e `external_tools_a_tool_added_applies`, view `copy_move_and_remove_tools` | —       |
| Name, Group ("External Tools"), Description, Program ("e.g. /usr/bin/env", macros), Arguments (macros), Working directory ("$ProjectFileDir$", macros) | `settings-text-tool.<field>`, `settings-tool-macro-<field>`, `…-<n>`                         | e2e, view                                                                    | —       |
| Synchronize files after execution / Open console for tool output                                                                                       | `settings-toggle-tool.{synchronizeAfter,openConsole}`                                        | view                                                                         | —       |
| Note                                                                                                                                                   | note                                                                                         | lines                                                                        | —       |

New ids `tool-<base36 ms>-<n>` as the webview makes them. State: done.

### `settings:vcs.mappings` (`pages/vcs.tsx`)

| Webview                                                                           | Native                                   | Test                                                 | Missing                                                              |
| --------------------------------------------------------------------------------- | ---------------------------------------- | ---------------------------------------------------- | -------------------------------------------------------------------- |
| Directory / VCS table: "No project is open", or `<Project> root` · Git / `<none>` | `MappingsView`, `settings-vcs-mapping-0` | e2e `directory_mappings_show_the_project_root`, view | the Git case is proven by the view test only (no git run in the e2e) |

State: done.

### `settings:vcs.issueNavigation` (`pages/vcs.tsx`), project scope

| Webview                                                                                                                            | Native                                                            | Test                                                          | Missing                                                                                                                                     |
| ---------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Issue navigation table (Issue ID / Issue Link, placeholders) · `vcs.issueNavigation` · records · [] · hint; invalid patterns named | `RecordTable` `settings-table-issues-*`, `settings-issue-invalid` | e2e `issue_navigation_links_are_written_to_the_project`, view | patterns are checked with the `regex` crate, not JS `RegExp` (**assumption**: close enough; look-around is refused here and accepted there) |
| Add JIRA / YouTrack / GitHub Pattern                                                                                               | `settings-issue-preset-{jira,youtrack,github}`                    | e2e, view                                                     | —                                                                                                                                           |

State: done.

Reset and Cancel: e2e `reset_and_cancel_discard_an_edit_on_each_kind_of_page` (Updates: standard; Date Formats, Actions on Save, Issue Navigation: own controls; CMake: page view).

## 8. IPC commands each page stands for

`appBuildInfo`, `appUpdateStatus` + event `updateStatusChanged`, action `app.checkForUpdates` (Updates); `proxyCheckConnection` (HTTP Proxy, seam); `languages` (File Types → `host.languages.all()`); `sdkDetect` (Gradle/Maven → `ide_host::ops::sdk::sdk_detect`); `chooseFile`/dialog → `ControlEvent::Browse`; every value through the existing Apply (`settings_set` → `ops::project::set_settings`, which takes `proxy.password` out into the saved passwords).

## 9. Assumptions to check first

1. `Context<V>` derefs to `App::defer(FnOnce(&mut App))` (used in `SdkFieldView::detect`), and updating a child `TextField` from an observer is allowed (pattern of `FormView::sync`).
2. `TextField::set_text` does not emit `Changed` (as `SecretField` documents); if it does, `ExternalToolsView::insert_macro` stages twice (harmless).
3. `ide_core::settings::{CMakeProfile, ExternalTool, ProxySettings}` (de)serialize camelCase with `#[serde(default)]` (read in `ide-core`); `ProxySettings` deserializes `mode`/`kind` from `"manual"`/`"socks"`.
4. `host.project(label).repos.read().is_empty()` (as `ops::project::info`).
5. A click on a control below the dialog's visible area still lands (the e2e tests click the first controls of each page; Maven's thread count is typed through its field handle).
6. Serialized JSON key order: assertions compare parsed `Value`s, not strings.

## 10. Tests most likely to fail first

- `http_proxy_…_stays_secret`: needs §4; the final "only `proxy-password.json` holds it" check fails if the backend's `SecretFile` writes elsewhere under the temp home; typing into the `SecretField` relies on `simulate_input` reaching its input handler after `window.focus`.
- `file_types_…`: assumes the backend lists Python with id `python` and that `settings-scope-project` is drawn on the page.
- `gradle_…`/`maven_…`: depend on §2 (`page_view`) and the page view filling the pane (clicks inside `ToolPage`).
- `updates_…`: depends on `ActionRegistry::handle` after `register_actions` (the manifest has `app.checkForUpdates`) and the backend pump delivering an event emitted with `window: None` to listeners.
- clippy: `general_http_proxy::HttpProxyView::{radio, text_row}` have 7 parameters (at the limit).
