# Integrating the Languages & Frameworks settings pages

New files, not compiled yet:

- `app-gpui/src/settings_view/pages/languages_sdk.rs`: the toolchain catalog (global, per kind, detected when a page is first shown), `LanguageAsk` / `LanguageEvent`, `answer()` (backend calls; test builds answer from the `TestToolchains` global instead), `SdkFieldView` (SDK combo), `PathFieldView` (trimmed path + Browse…). 3 view tests.
- `app-gpui/src/settings_view/pages/languages_controls.rs`: `LanguagePageView` (the page view: sections of `Control`s, forwards `ControlEvent`s and `LanguageAsk`s, asks for detection on first render), `FormatterLanguagesView`, `PreviewFontSizeView`, `JdtlsSharedView`, `ServerTargetView`, `PluginFormattersView`, `PhpInterpreterView`, `ProjectOnlyView`. 5 view tests.
- `app-gpui/src/settings_view/pages/languages_pages.rs`: the 16 page builders, `search_labels`, `page_view`, `install`. 3 plain tests.
- `app-gpui/src/workbench/tests/proofs19_settings_languages.rs`: 19 end-to-end tests.

Design (why page views, not spec sections): the language specs in `settings_model/language_pages.rs` stay without sections, as the module doc of `builtin_pages.rs` says, and each page is mounted with `set_page_view` + `set_page_labels` (as Keymap is). Putting the controls into spec sections would break `builtin_pages::tests::every_option_writes_a_setting_the_backend_takes` (the Go module keys `languages.go.modules.env.*` have no default) and the smoke test's "nothing to change" branch for pages made only of filled custom slots; a page view shows as `<page view>`, which the smoke test already accepts. No existing file under `settings_model/` is edited.

Every anchor below is quoted from the file as it is now. Run `cargo fmt` after pasting: several lines of the new files run past 120 columns.

## 1. `app-gpui/src/settings_view/pages/mod.rs`

After the line `pub mod general;` insert:

```rust
pub mod languages_controls;
pub mod languages_pages;
pub mod languages_sdk;
```

(The other writer may insert their own modules after another anchor; these three only need to follow `pub mod general;`.)

## 2. `app-gpui/src/settings_view/dialog.rs`

After the method

```rust
    pub fn custom_control(&self, page: &str, label: &str) -> Option<&AnyView> {
        self.custom_views.get(&(page.to_owned(), label.to_owned()))
    }
```

insert:

```rust

    /// The view drawn for `page` in place of its controls.
    pub fn page_view(&self, page: &str) -> Option<&AnyView> {
        self.page_views.get(page)
    }
```

The end-to-end tests read the page's view through it (`<page view>` is all `control_lines` says).

## 3. `app-gpui/src/settings_view/custom_controls.rs`, in `pub fn install`

After the line

```rust
    slots.put(pages::code_style::PAGE, "Scheme", &scheme, cx);
```

insert (before `slots.subscriptions`):

```rust

    // Languages & Frameworks and Python Interpreter: pages drawn by views of their own.
    let (language_pages, asks) =
        pages::languages_pages::install(dialog, &model, scope.clone(), install.window_label, install.has_project, cx);
    for page in &language_pages {
        slots.listen(page, cx);
    }
    slots.subscriptions.extend(asks);
```

`slots.listen` answers the pages' `ControlEvent`s (OK, Cancel, Browse… through `prompt_for_paths`); the subscriptions `install` returns answer their `LanguageAsk`s. If the other writer also inserts after the same anchor, both blocks can stand one after the other.

## 4. `app-gpui/src/workbench/mod.rs`, in `mod tests`

After the line `    mod proofs18_e2e_search;` insert:

```rust
    mod proofs19_settings_languages;
```

Nothing else in `mod.rs` changes: no field, no action.

## IPC commands / backend calls the pages stand for

| Ask / control                              | Native call                                                                                                                                                                                        | IPC command                                            |
| ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Detect (on first show, Detect again)       | `ide_host::ops::sdk::sdk_detect(&host, label, kind)` (kind deserialized from its wire name; the result read back through JSON into `Toolchain`, since `ide-sdk` is not a dependency of `app-gpui`) | `sdk_detect`                                           |
| PHP on the PATH (Default interpreter)      | `ide_host::ops::sdk::sdk_locate_tools(&host, label, ["php"], None)`                                                                                                                                | `sdk_locate_tools`                                     |
| Xdebug of the selected PHP                 | `ide_host::ops::sdk::sdk_php_xdebug(&host, label, php, None)`                                                                                                                                      | `sdk_php_xdebug`                                       |
| Python's empty choice                      | `ide_host::ops::sdk::sdk_python_effective(&host, label, None)`                                                                                                                                     | `sdk_python_effective`                                 |
| Language list (targets, plugin formatters) | `backend.host.languages.all()`                                                                                                                                                                     | `languages`                                            |
| Plugin formatters                          | `ide_host::ops::lsp::plugin_formatters()` on the background executor                                                                                                                               | `plugin_formatters`                                    |
| Browse…                                    | `ControlEvent::Browse` → `cx.prompt_for_paths` (existing `answer`)                                                                                                                                 | dialog plugin `open`                                   |
| Shared JDT LS rule (workspace)             | `ide_workspace::workspace::Workspace::load(&project.root)` on the background executor                                                                                                              | (the webview reads `project.workspace` from its store) |
| Apply / Cancel / Reset                     | unchanged: the dialog's staging                                                                                                                                                                    | `settings_set`                                         |

## Pages: webview control → native control → test → missing

Tests: E = `workbench::tests::proofs19_settings_languages::…`, V = `settings_view::pages::languages_*::tests::…`.

### `languages.go`

| Webview (label, key, type, default, rule)                                                                           | Native                   | Proof                                                                                                                                                                                     | Missing                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| GOROOT, `languages.go.goroot`, SDK combo (Default / go on PATH, detected Go, custom), "" , browse dir, Detect again | `SdkFieldView`           | E `go_goroot_and_gopath_are_written_to_the_project`; V `the_combo_offers_the_detected_installations_and_stages_a_choice`, `detect_again_asks_anew_and_browse_stages_the_directory_chosen` | Browse… has no title "Select GOROOT" nor start path (shared `ControlEvent::Browse` has none); the spinner is the text "Detecting…" |
| Project GOPATH, `languages.go.gopath`, path, "", placeholder = detected GOPATH else "go env GOPATH", trims          | `PathFieldView` (GoPath) | same E; V `a_path_is_staged_trimmed_and_the_placeholder_follows_the_detected_go`                                                                                                          | Browse title                                                                                                                       |

### `languages.go.buildTags`

| Operating System / Architecture / Compiler / CGO (selects, "" = Automatic), Go Version / Custom Tags / Experiments (text with placeholders), keys `languages.go.buildTags.*`, always project scope; no project or new-projects mode → "Open a project to configure Go build tags." | `ProjectOnlyView` over a `FormView` bound to the project scope | E `go_build_tags_are_written_to_the_project`, `go_build_tags_and_modules_say_a_project_is_needed_without_one` | none |

### `languages.go.modules`

| Enable vendoring support automatically (`languages.go.modules.autoVendor`, true); GOPROXY … GOWORK (`languages.go.modules.env.<NAME>`, text, placeholder "Inherit from environment"); project only | `ProjectOnlyView` + `FormView` | E `go_modules_variables_are_written_to_the_project_and_cancel_discards`, `go_build_tags_and_modules_say_a_project_is_needed_without_one` | none. Assumption: an emptied field stores `""` (as the webview's text option does) |

### `languages.java`

| SDK (`languages.java.jdk`, JDK combo, Auto, dir, hint); Language level (`languages.java.languageLevel`, select "" = SDK default / 8 / 11 / 17 / 21 / 25, hint); Share JDT LS across builds (`languages.java.jdtlsShared`, false, disabled in project scope when the project is a workspace or `languageServerTargets.java.target` is set, three hints); JDK (`languages.java.jdtlsJdk`, JDK combo, Auto); JVM options (`languages.java.jdtlsVmArgs`, text, placeholder -Xmx4g, hint) | `SdkFieldView` ×2, `FormView` select and field, `JdtlsSharedView` | E `java_level_and_shared_jdt_ls_apply_and_reset_discards_jvm_options`; V `shared_jdt_ls_is_not_editable_for_a_workspace_project` | "Download JDK…" entry and its dialog (`DownloadJdkDialog.tsx`); the select's detail "from build / JDK"; Browse title; mono fonts. The registry keyword "jdk for importer" is missing from `language_pages.rs` (added to the page's search labels here instead) |

### `languages.markdown` (global only)

| Preview font size (`languages.markdown.previewFontSize`, number 8..72 px, shows `editor.fontSize` while unset, Up/Down step, hint) | `PreviewFontSizeView` | E `markdown_preview_size_follows_the_editor_until_set_and_applies` (incl. Reset and Cancel, keyboard Up); V `the_preview_size_follows_the_editor_until_set_and_keeps_its_range` | none |

### `languages.node`

| Node interpreter (`languages.node.interpreter`, Node combo, Default / node on PATH, file, hint); Run in target (`languages.node.runTarget`, "" Default run target / local / plugin targets / stored); Language server targets (Language select over javascript, javascriptreact, typescript, typescriptreact + Language server runs on + Language server, project scope, disabled without project); Package manager (`languages.node.packageManager`, "" Auto + npm/pnpm/yarn/bun with detected version and path or "not found", hint) | `SdkFieldView`, `FormView` selects, `ServerTargetView(Picker)` | E `node_package_manager_is_written_to_the_project` | plugin run targets are not offered (no native source of `usePluginRunTargets` reaches the dialog; a stored one is shown); option details are folded into the label ("pnpm — 9.1.0 /path"); targets' disabled options with "Requires a container root" detail |

### `python.interpreter`

| Python Interpreter (`languages.python.interpreter`, combo whose empty choice is "Project virtualenv (auto-detect)" with its path, else "<No interpreter>"), the path line `name — path`; Activate virtualenv (`languages.python.activateVirtualenv`, true, hint) | `SdkFieldView(Python)`, `FormView` | E `python_interpreter_and_activation_are_written_to_the_project` | **partial**: Add Interpreter… (`AddInterpreterDialog.tsx`: venv / uv / poetry / conda creation), the packages table (list, search, Install, Uninstall, Upgrade, Reload, context menu, pip log, Cancel) are not built: they run pip and interpreters (`sdk_python_packages`, `sdk_python_pip`, `sdk_python_create_venv`) |

### `languages.php`

| PHP language level (`languages.php.languageLevel`, "" Auto … 5.6, hint); Include paths, Additional stubs (`languages.php.includePaths` / `.stubs`, string lists, empty texts, hints); PHPStan › Configuration file (`languages.php.phpstanConfig`, path file, placeholder, hint, trims); CLI Interpreter › Run in target (`languages.php.runTarget`), Language server runs on / Language server (php), PHP interpreter (`languages.php.interpreter`, combo Default / php on PATH, hint), Version, Xdebug (or "Not installed") of the selected PHP (the one set, else the one on PATH) | `FormView`, `ListEditor` ×2, `PathFieldView`, `ServerTargetView(Fixed)`, `SdkFieldView`, `PhpInterpreterView` | E `php_include_path_and_level_are_written_to_the_project`; V `a_stored_target_shows_as_unavailable_and_local_machine_is_staged`, `the_selected_php_shows_its_version_and_xdebug` | plugin run targets (as Node); Auto's detail folded into the label; list item labels (aria only) |

### `languages.prettier`, `languages.oxfmt`, `languages.ruff`

| `<tool>` path (`languages.<tool>.path`, text, placeholder Auto, hint); Ruff only: Configuration directory (`languages.ruff.configurationDirectory`, text + Browse… dir, placeholder, hint); Formatting provider (writes `languages.<tool>.prefer`: false = Automatic (language server first), true = the tool; oxfmt/Ruff hints); Run on save (`languages.<tool>.runOnSave`, false, hint); Enabled languages (`languages.<tool>.languages`, a checkbox per language: 14 for Prettier/oxfmt, Python for Ruff; on appends, off removes) | `FormView` (provider as a select over boolean values), `FormatterLanguagesView` | E `prettier_provider_and_languages_are_written_to_the_project`, `oxfmt_run_on_save_is_written_to_the_project`, `ruff_configuration_directory_is_chosen_with_browse_and_written_to_the_project`; V `a_language_checkbox_removes_and_adds_the_language`; plain `offers_the_languages_the_backend_enables_by_default` | Browse title and start path for Ruff; mono font |

### `languages.ruby`

| Ruby interpreter (`languages.ruby.interpreter`, combo Auto / ".ruby-version / mise / asdf, else ruby on PATH", file, hint) | `SdkFieldView` | E `ruby_interpreter_is_written_to_the_project` | Browse title |

### `languages.rust`

| Toolchain location (`languages.rust.toolchain`, combo Default / rustup default / PATH, dir, hint); Standard library (`languages.rust.stdlib`, path dir, placeholder = the toolchain's library else "Detected from the toolchain", hint); External tool (`languages.rust.checkCommand`, check / clippy); Run external linter on save (`languages.rust.checkOnSave`, true, hint); Additional arguments (`languages.rust.rustfmtArgs`, text, hint) | `SdkFieldView`, `PathFieldView(RustStdlib)`, `FormView` | E `rust_linter_and_rustfmt_arguments_apply_and_reset_and_cancel_discard` (Reset and Cancel of a project-scope page; the project file is unchanged after Cancel) | Browse title |

### `languages.typescript`

| TypeScript (`languages.typescript.version`, select Bundled / workspace (detected name and path, or node_modules/typescript "not installed in this project") / custom path, Browse… dir, hint) | `SdkFieldView(TypeScript)`: no Detect again, as the webview's page | E `typescript_version_is_chosen_with_browse_and_written_to_the_project` | Browse title |

### `languages.pluginFormatters`

| A select per language (`languages.pluginFormatters.<id>`, "" Automatic (language server first), the plugin formatters covering it, a stored unknown id), languages = registry ∪ stored ∪ plugin-named, only those a plugin covers or one was chosen for; Format on save (`editor.formatOnSave`) | `PluginFormattersView`, `FormView` | E `plugin_formatters_format_on_save_applies` | no test installs a plugin formatter, so the per-language select is proven only by reading (assumption: `ide_host::ops::lsp::plugin_formatters()` returns nothing in tests) |

### `languages.serverTargets`

| Language (every registry language with a language server, starting on php), Language server runs on (Local machine, stored target "Target unavailable; language server runs on the host", plugin targets with container roots), Language server (shown when a target is set: First configured server + the language's servers); project scope only, disabled without project or for new projects | `ServerTargetView(Picker(None))` | E `language_server_targets_local_machine_is_written_to_the_project`, `language_server_targets_are_not_editable_without_a_project` | plugin run targets (no native list reaches the dialog) |

### `projectStructure` — not built

The 2,023-line `ProjectStructurePage.tsx` (content roots and folder marks, modules with SDK / language level / output paths / dependencies, libraries, facets, artifacts, SDKs) is not drawn; the page still says "This page is not available yet". Nothing is mounted for it.

## Seams and test isolation

- `TestToolchains` (`#[cfg(test)]` global in `languages_sdk.rs`) answers detection, the PATH lookup, Xdebug and the Python virtualenv in test builds; `answer` never reaches `ide_host::ops::sdk` there. It is a new seam, shaped after `theme::installed_fonts` (test builds read what the test set). It counts detections, so the tests prove that Detect again _asks_ anew, not that the backend detects.
- Not run in any test: `sdk_detect`, `sdk_locate_tools`, `sdk_php_xdebug`, `sdk_python_effective`. Their callers (`detect_on`, `run_detection`, `locate_tool`, `php_xdebug`, `python_effective`) are `#[cfg(not(test))]`, so a test build never compiles them: **run `cargo build -p openidea-gpui` (or `cargo clippy -p openidea-gpui` without `--tests`) as well as the tests**.
- Browse… is tested through `did_prompt_for_paths` / `simulate_path_prompt_response` (GOPATH, Ruff configuration directory, TypeScript).
- The catalog is emptied for each Settings opened (`install`), so a project opened since, or another window's project, is detected again. Divergence: the webview's cache lives for its window; two Settings dialogs open at once in two windows share one catalog (the second opening empties it, and the first window's pages, which already asked, show nothing found until Detect again).

## Expected states after integration

- done: `languages.go.buildTags`, `languages.go.modules`, `languages.markdown`, `languages.prettier`, `languages.oxfmt`.
- partial (Browse… opens the system chooser without the page's title or start folder, as `settings:general`): `languages.go`, `languages.ruby`, `languages.rust`, `languages.typescript`, `languages.ruff`.
- partial for more: `languages.java` (also no Download JDK…), `languages.node` and `languages.php` (also no plugin run targets in Run in target / Language server runs on; option details folded into labels), `languages.serverTargets` (no plugin targets), `languages.pluginFormatters` (the per-language select is not exercised by a test: no plugin formatter in tests; only tested in the global scope), `python.interpreter` (no Add Interpreter…, no packages table).
- not built: `projectStructure`.

## Assumptions (check first)

1. `Global` is implemented by deriving nothing: `impl Global for T {}` as `InstalledFontNames` does; `cx.update_global::<G, _>` notifies `observe_global` observers (used by `SdkFieldView`, `PathFieldView`, `PhpInterpreterView`).
2. `LanguageConfig` fields: `id.0: String`, `name: String`, `language_servers: Vec<LanguageServerConfig { name, .. }>` (read from `crates/ide-language/src/lib.rs`).
3. `ide_host::ops::lsp::plugin_formatters` and `FormatterChoice { id, name, languages }` are reachable from `app-gpui` (the function is `pub` in a `pub mod`).
4. `IpcPath` derefs to `Path` (`to_string_lossy` on `LocatedTool::path`); `IdeError::message` is a `String` field.
5. Emitting from `render` is allowed (the font picker does it); a view with two `EventEmitter` impls (`LanguagePageView`) is subscribed to with the event type annotated.
6. The registry has a JavaScript/TypeScript language with a server, and `php` ("PHP") with servers (`languages/php.toml`), which the Node and target tests rely on.
7. `staged_value(Scope::Project, …)` returns the project layer the dialog was opened over; with no project the target rows are drawn disabled, so nothing is staged there.

## Compile risks

- `serde_json::from_value(Value::from(kind.wire()))` relies on inference from `sdk_detect`'s `kind` parameter type.
- The `cx.subscribe` closures in `LanguagePageView::listen` infer the event type per arm.
- `levels(&JAVA_LANGUAGE_LEVELS, …)` needs the const array promoted to `'static`.
- The three-level `Option` in `PhpInterpreterView::xdebug` (asked path, answer pending, check failed, Xdebug or none).

## Tests most likely to fail on first run

- Exact-line assertions in `proofs19` (`lines` / `labels`), wherever a label or a choice's text is translated differently, or where a FormView field line ends in a space (`"Go Version: "`).
- `node_package_manager_is_written_to_the_project`: depends on the registry's JS/TS languages having servers (row count 6).
- `go_goroot_and_gopath_are_written_to_the_project`: the detection count assumes the page view renders once before the assertion and the catalog starts empty.
- `the_search_finds_a_language_page_by_a_label_of_its_controls`: assumes the search matches `set_page_labels` texts by substring, as for the Keymap's.
- Rows low on a long page may lie outside the scrolled page when clicked (Java's JVM options and server JDK, Rust's rustfmt arguments, Node's package manager, PHP's CLI Interpreter rows): `click_on` then misses. Remedy: scroll the page first as `custom_controls::tests::scroll_page` does (a `ScrollEvent` over `settings-page`), or open the test window larger.
- All tests are unrun: nothing here was compiled.
