# Plugins wiring: one integration in three stages

This file replaces `docs/gpui-port/notes/integration/{plugins_welcome,plugin_runtime,plugin_contributions}_integration.md` for the integration. Apply the stages in order: 1 (Plugins page, masked secret field, Welcome pages), 2 (plugin runtime: status items, tasks, notifications, questions, log tabs, workbench events), 3 (plugin contributions: run and debug requests, gutter actions, menus, settings pages, file and activation events, Background Tasks). Build, run clippy and run the stage's tests after each stage; each stage is meant to compile without warnings on its own (nothing in a stage calls a symbol a later stage adds; items only tests use are `#[cfg(test)]` or `pub` on the exported `Workbench`).

Every anchor is quoted from the working tree of 2026-11-13 (after `88677f14`, with the compiling agent's uncommitted edits to `workbench/mod.rs`, `workbench/debug.rs`, `editor/`, `platform/`, `find/`, `run/segments.rs`), or from the file as the previous stage leaves it (said so where it matters). Each was counted once in its target file. No line numbers. Check an anchor with `grep -nF` before pasting; run `cargo fmt` after each stage (a few pasted lines sit at or just over 120 columns before formatting). Within a stage, paths are relative to `app-gpui/src/` unless they start with `app-gpui/` or `crates/`.

Nothing was compiled. Measured in this session (commands run, output read): the anchor counts (each 1 in its file, re-counted for `workbench/mod.rs`, `workbench/debug.rs`, `workbench/project_files.rs` after the compiling agent's latest edits), which compiled files changed since each draft commit, that nothing under `src/` calls `pluginLink`, and that `assets/gen/keymaps.json` binds none of the actions registered here. Read, not measured: the API conformance of the drafted files (three parallel reviews read every call against today's signatures; spot-checked here: `PluginHost` secrets, `debugger/actions.rs`/`driver.rs` plugin paths, `DebugDriverEvent` matches, `test_support` signatures) and what the tests will do. Assumptions that need a build or a run are at the end, in the order to check them.

**The drafted files as reconciled** are in the worktree and, byte-identical (checked with `cmp`), in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/plg/files/app-gpui/src/` (`ui/secret_field.rs`, `workbench/{plugins_window,welcome_pages,plugin_runtime,plugin_contributions,background_tasks_view}.rs`, `plugin_views/{mod,prompt_dialog,log_tab,settings_page}.rs`). During this reconciliation another process restored those worktree files to their committed content once (12:14:14); they were re-applied. If `git status` shows them unmodified before stage 1, copy them from there first.

Rules every stage follows:

- **Secrets never reach the keychain in tests.** Stage 1 §1.8 makes `KeychainSecrets` exist only outside test builds; every unit test of `openidea_gpui` keeps the host's default `FileSecretStore` under its temporary home from `PluginHost::new` on. No test swaps the store after `Plugins::init` (the drafts' `set_secret_store(FileSecretStore …)` lines are gone). See point b at the end.
- **One plugin test harness.** `workbench/plugins_window.rs` holds `#[cfg(test)] pub(crate) mod test_support` (`repo`, `copy_dir`, `plugin_copy`, `start_with_plugins`, `settle_app`, `link`, `trusted_project`, plus `program`/`programs_in` from stage 2); the tests of all three stages use it and the existing `crate::workbench::tests::{start, wait_long, wait_for_notice, run_action, click_on, notification_titles, notice_titled, git_in, init_repo}` (already `pub(super)`). No stage adds a hunk to the `mod tests` region of `workbench/mod.rs`.
- **Fixtures are copies.** Every plugin a test installs or links is first copied with `plugin_copy` into `<temp home>/fixtures/<name>` (from `crates/ide-plugin/tests/fixtures/<name>` or `plugins/official/<name>`, read only). No test names the Bazel plugin folder, the network (debug builds set no market URL) or anything under `$HOME`; programs a plugin is granted resolve in a temp `bin` (or `/bin` for `bash`).
- **Layers.** Stage 1 puts the plugins dialogs (consent, Plugin Repository prompt; priority 2) right after the Settings layer; stage 2 puts the plugin questions (priority 1) right after that, so a question asked from a plugin's Settings page draws over Settings.

---

## Stage 1: the Plugins page, the masked secret field and the Welcome pages

Files to declare: `ui/secret_field.rs` (4 tests), `workbench/plugins_window.rs` (10 end-to-end tests, `test_plugin_host`, `test_support`), `workbench/welcome_pages.rs` (4 view tests).

Hunks: 56 edit sites in `ui/mod.rs`, `workbench/mod.rs`, `plugin_models/plugins_page.rs`, `workbench/welcome.rs`, `vcs/prompt_dialogs.rs`, `workbench/settings_window.rs`, `plugin_models/consent.rs`, `plugins.rs`. 52 apply as the draft wrote them (their files are unchanged since the draft commit `d7b69c6b`, or changed away from the anchors: `vcs/prompt_dialogs.rs` gained `pub(super)` helpers and `Message`, no new exhaustive `match` on `FieldRule`; in `workbench/mod.rs` every enclosing function was read in full). 1 is made exact (1.6.5's doc comment). 3 are new (1.8, `plugins.rs`, point b). The draft's old §3.9 (three tests pasted into `mod tests` of `workbench/mod.rs`) is dropped from the hunks: the three tests live in `plugins_window.rs` now.

`plugins_window.rs` uses symbols that exist only after this stage's edits: `Workbench::plugin_ui` (1.3.2), `FieldRule::Any` (1.5), `Welcome::set_plugins_page` (1.4), `PluginsPage::secret_input` and the hand-written `Debug` of `PluginsPageEvent` (1.2), `sync_customize` (1.3.8). `settings_window.rs` (1.6) calls `plugin_host`, `new_plugins_page` and `follow_settings_on_plugin_pages` from `plugins_window.rs`. Without the `Plugins` global (every existing test) `listen_to_plugins` returns at once, `stop_listening_to_plugins` does nothing, `sync_welcome_plugins` sets no Plugins page, `sync_customize` only hands the Customize page the effective settings.

Needed by: every hunk of this stage is needed by compilation of `plugins_window.rs`/`welcome_pages.rs` or by its tests; 1.8 by `the_app_plugin_host_keeps_secrets_in_a_file_under_the_temporary_home` and by every later test that runs `Plugins::init`.

### 1.1 The `mod` lines

`ui/mod.rs`, after `pub mod popup_menu;`:

```rust
pub mod secret_field;
```

`workbench/mod.rs`, after the line `mod services_window;`:

```rust
mod plugins_window;
```

and after `pub mod welcome_model;`:

```rust
pub mod welcome_pages;
```

### 1.2 `plugin_models/plugins_page.rs`: the masked field, secret rows drawn at once, redacted `Debug`

#### 1.2.1 Import

After `use crate::ui::text_field::{TextField, TextFieldEvent};` add:

```rust
use crate::ui::secret_field::{SecretField, SecretFieldEvent};
```

(`TextField` stays: the two search fields use it.)

#### 1.2.2 The event's `Debug` never writes a secret

Replace

```rust
/// What the page asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginsPageEvent {
```

with

```rust
/// What the page asks of the workbench. Its `Debug` output never holds a secret's value.
#[derive(Clone, PartialEq)]
pub enum PluginsPageEvent {
```

and add right after the enum's closing brace (after `    Notify(NotificationRequest),\n}`):

```rust
/// As a derived `Debug` would write it, except that a secret's value shows as `…`.
impl std::fmt::Debug for PluginsPageEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refresh { plugins, catalog, market } => f
                .debug_struct("Refresh")
                .field("plugins", plugins)
                .field("catalog", catalog)
                .field("market", market)
                .finish(),
            Self::SetEnabled { id, enabled } => {
                f.debug_struct("SetEnabled").field("id", id).field("enabled", enabled).finish()
            }
            Self::Reload(id) => f.debug_tuple("Reload").field(id).finish(),
            Self::Uninstall(id) => f.debug_tuple("Uninstall").field(id).finish(),
            Self::Grant { id, optional, signer_change } => f
                .debug_struct("Grant")
                .field("id", id)
                .field("optional", optional)
                .field("signer_change", signer_change)
                .finish(),
            Self::SetCapability { id, capability, granted } => f
                .debug_struct("SetCapability")
                .field("id", id)
                .field("capability", capability)
                .field("granted", granted)
                .finish(),
            Self::RollBack(id) => f.debug_tuple("RollBack").field(id).finish(),
            Self::AskConsent(request) => f.debug_tuple("AskConsent").field(request).finish(),
            Self::Install(step) => f.debug_tuple("Install").field(step).finish(),
            Self::DownloadMarket(id) => f.debug_tuple("DownloadMarket").field(id).finish(),
            Self::EditRepository { current } => f.debug_struct("EditRepository").field("current", current).finish(),
            Self::ImportInstalledIntellij => f.write_str("ImportInstalledIntellij"),
            Self::ImportIntellijFile => f.write_str("ImportIntellijFile"),
            Self::OpenUrl(url) => f.debug_tuple("OpenUrl").field(url).finish(),
            Self::SetTheme(theme) => f.debug_tuple("SetTheme").field(theme).finish(),
            Self::SelectIconTheme(key) => f.debug_tuple("SelectIconTheme").field(key).finish(),
            Self::RunCommand { plugin_id, command_id } => {
                f.debug_struct("RunCommand").field("plugin_id", plugin_id).field("command_id", command_id).finish()
            }
            Self::LoadSecrets(id) => f.debug_tuple("LoadSecrets").field(id).finish(),
            Self::SetSecret { plugin_id, name, value } => f
                .debug_struct("SetSecret")
                .field("plugin_id", plugin_id)
                .field("name", name)
                .field("value", &value.as_ref().map(|_| "…"))
                .finish(),
            Self::Notify(request) => f.debug_tuple("Notify").field(request).finish(),
        }
    }
}
```

#### 1.2.3 The field type

Replace

```rust
    secret_fields: HashMap<(String, String, bool), Entity<TextField>>,
```

with

```rust
    secret_fields: HashMap<(String, String, bool), Entity<SecretField>>,
```

`secret_saved`'s `field.update(cx, |field, cx| field.set_text("", cx));` compiles unchanged (`SecretField::set_text(&mut self, &str, &mut Context<Self>)`).

#### 1.2.4 Secret rows are drawn as soon as the plugin is listed

In the `"secrets" =>` arm of the section builder, replace

```rust
                // A secret's row, with the field its value is typed in, shows only once its state has
                // been read: until then the page cannot say whether a value is stored.
                let state = self.secrets.get(&plugin.id);
                let secrets = items
                    .iter()
                    .filter(|_| state.is_some())
                    .filter_map(|item| {
```

with

```rust
                // Every declared secret has its row and its masked field at once; it says "Not set"
                // until its state has been read.
                let state = self.secrets.get(&plugin.id);
                let secrets = items
                    .iter()
                    .filter_map(|item| {
```

#### 1.2.5 The field itself

Replace the whole `fn secret_field` (from `    /// The field a secret's value is typed in; a new one when the secret's state changes, with its` to its closing `    }`) with:

```rust
    /// The masked field a secret's value is typed in; a new one when the secret's state changes,
    /// with its placeholder for that state.
    fn secret_field(&mut self, plugin_id: &str, secret: &SecretRow, cx: &mut Context<Self>) -> Entity<SecretField> {
        let key = (plugin_id.to_owned(), secret.name.clone(), secret.set);
        if let Some(field) = self.secret_fields.get(&key) {
            return field.clone();
        }
        let placeholder = tx(cx, if secret.set { "Replace…" } else { "Value" });
        let field = cx.new(|cx| SecretField::new(cx).with_placeholder(placeholder));
        let (plugin, name) = (plugin_id.to_owned(), secret.name.clone());
        let subscription =
            cx.subscribe(&field, move |page: &mut Self, field, event: &SecretFieldEvent, cx| match event {
                SecretFieldEvent::Submitted => {
                    let value = field.read(cx).text(cx).to_owned();
                    page.save_secret(&plugin, &name, Some(value), cx);
                }
                SecretFieldEvent::Changed => cx.notify(),
                SecretFieldEvent::Cancelled => {}
            });
        self._subscriptions.push(subscription);
        self.secret_fields.insert(key, field.clone());
        field
    }
```

#### 1.2.6 `render_secret` reads the field's text through the app

In `fn render_secret`, replace

```rust
        let typed = field.read(cx).text().to_owned();
```

with

```rust
        let typed = field.read(cx).text(cx).to_owned();
```

and in the Save button's listener replace

```rust
                    let value = field.read(cx).text().to_owned();
                    page.save_secret(&plugin, &name, Some(value), cx);
                }))
            });
```

with

```rust
                    let value = field.read(cx).text(cx).to_owned();
                    page.save_secret(&plugin, &name, Some(value), cx);
                }))
            });
```

#### 1.2.7 An accessor for tests

After `pub fn search_field(&self) -> &Entity<TextField> { … }` add:

```rust
    /// The masked field of secret `name` of `plugin_id`, while it is drawn for the state `set`.
    pub fn secret_input(&self, plugin_id: &str, name: &str, set: bool) -> Option<&Entity<SecretField>> {
        self.secret_fields.get(&(plugin_id.to_owned(), name.to_owned(), set))
    }
```

#### 1.2.8 Tests in `plugins_page.rs`

In `stores_and_removes_a_declared_secret_without_showing_its_value`, replace

```rust
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "", "the value is not kept");
```

with

```rust
        assert_eq!(field.read_with(cx, |field, cx| field.text(cx).to_owned()), "", "the value is not kept");
```

Replace the whole test `shows_no_secret_field_while_the_secrets_are_not_read` (from its `// Not from a spec: while the state of a plugin's secrets has not been read, …` comment to its closing brace) with these two:

```rust
    // src/features/plugins/secrets.tsx › SecretsDetails: a password field per declared secret, "Not set"
    // until the state is read; the state is asked for once per plugin.
    #[gpui_kit::test]
    async fn draws_a_masked_field_for_each_declared_secret_before_its_state_is_read(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let ai = plugin("ai", json!({ "contributions": [{ "point": "secrets", "label": "API key" }] }));
        let ui = vec![Contribution::new("ai", "secrets", "api-key", json!({ "name": "api-key", "label": "API key" }))];
        list(&page, cx, vec![ai], ui);
        assert_eq!(taken(&events), [PluginsPageEvent::LoadSecrets("ai".to_owned())]);
        cx.run_until_parked();
        let rows = section(&details(&page, cx), "Secrets").secrets.clone();
        assert_eq!(rows.iter().map(|row| (row.name.as_str(), row.set)).collect::<Vec<_>>(), [("api-key", false)]);
        assert!(cx.debug_bounds("plugins-secret-field-api-key").is_some(), "the masked field is drawn");
        page.update(cx, |page, cx| page.select(ItemKind::Installed, "ai", cx));
        assert!(taken(&events).is_empty(), "the state is asked for once");
        page.update(cx, |page, cx| page.set_secrets("ai", vec![("api-key".to_owned(), true)], cx));
        assert!(section(&details(&page, cx), "Secrets").secrets[0].set);
    }

    // Not from a spec: the debug output of the event that stores a secret names the plugin and the
    // secret, never its value.
    #[test]
    fn the_event_that_stores_a_secret_does_not_write_its_value_in_debug_output() {
        let event = PluginsPageEvent::SetSecret {
            plugin_id: "ai".to_owned(),
            name: "api-key".to_owned(),
            value: Some("sk1secret".to_owned()),
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("SetSecret") && debug.contains("api-key"), "{debug}");
        assert!(!debug.contains("sk1secret"), "{debug}");
        let removed = PluginsPageEvent::SetSecret { plugin_id: "ai".to_owned(), name: "api-key".to_owned(), value: None };
        assert!(format!("{removed:?}").contains("None"));
    }
```

### 1.3 `workbench/mod.rs`

#### 1.3.1 A constant

After `const WINDOW_BUTTONS_WIDTH: f32 = 82.0;` add:

```rust

/// Where the reopen-last-project preference of this machine is kept (`"false"`: off).
const REOPEN_PREFERENCE: &str = "openidea.welcome.reopenLastProject";
```

#### 1.3.2 The field

Replace

```rust
    /// The Settings dialog, while it is open.
    settings: Option<settings_window::SettingsWindow>,
```

with

```rust
    /// The Settings dialog, while it is open.
    settings: Option<settings_window::SettingsWindow>,
    /// The window's plugin host, the Welcome screen's Plugins page, the page of the Plugins actions
    /// and the dialog they show.
    plugin_ui: plugins_window::PluginsSlot,
```

#### 1.3.3 Its initialiser, the listener and its release (in `Workbench::new`)

Replace

```rust
            settings: None,
            vcs: None,
```

with

```rust
            settings: None,
            plugin_ui: Default::default(),
            vcs: None,
```

Replace

```rust
        workbench.sync_welcome(window, cx);
        workbench.start_code_insight(cx);
        workbench
    }
```

with

```rust
        workbench.listen_to_plugins(window, cx);
        workbench.sync_welcome(window, cx);
        workbench.start_code_insight(cx);
        workbench
    }
```

(`listen_to_plugins` comes first so that the Welcome screen created by `sync_welcome` finds the host.)

Replace

```rust
            if cx.has_global::<Backend>() {
                cx.global_mut::<Backend>().unlisten(workbench.listener);
            }
            cx.default_global::<Workbenches>().0.remove(&id);
```

with

```rust
            if cx.has_global::<Backend>() {
                cx.global_mut::<Backend>().unlisten(workbench.listener);
            }
            workbench.stop_listening_to_plugins(cx);
            cx.default_global::<Workbenches>().0.remove(&id);
```

#### 1.3.4 The actions

Replace

```rust
    settings_window::register_actions(registry);
```

with

```rust
    settings_window::register_actions(registry);
    plugins_window::register_actions(registry);
```

#### 1.3.5 The layer, over Settings

Replace

```rust
            .children(self.settings_layer(cx))
```

with

```rust
            .children(self.settings_layer(cx))
            .children(self.plugins_dialog_layer())
```

#### 1.3.6 `sync_welcome`: the Welcome screen's Plugins page and Customize settings

Replace

```rust
        if !self.is_empty() {
            self.welcome = None;
            return;
        }
```

with

```rust
        if !self.is_empty() {
            self.welcome = None;
            self.drop_welcome_plugins();
            return;
        }
```

and replace

```rust
        if let Some((welcome, _)) = &self.welcome {
            welcome.update(cx, |view, cx| view.set_recent(&list, &groups, cx));
            return;
        }
```

with

```rust
        if let Some((welcome, _)) = &self.welcome {
            welcome.update(cx, |view, cx| view.set_recent(&list, &groups, cx));
            // A plugin host that started after the window, or a Welcome screen first made behind a
            // failed open and shown now, gets its listener, its Plugins page and its settings here.
            self.listen_to_plugins(window, cx);
            self.sync_customize(cx);
            self.sync_welcome_plugins(window, cx);
            return;
        }
```

(This branch runs on every window activation and after `close_project`; all three return early when there is nothing to do. Today `lib.rs::init` runs `Plugins::init` before `main.rs` opens the first window, so `Workbench::new` already finds the host; this covers the other orders.)

and replace

```rust
        self.welcome = Some((welcome, events));
    }
```

with

```rust
        self.welcome = Some((welcome, events));
        self.sync_customize(cx);
        self.sync_welcome_plugins(window, cx);
    }
```

#### 1.3.7 `welcome_event`: the Customize arm

In `fn welcome_event`, replace

```rust
            WelcomeEvent::Reveal(path) => cx.reveal_path(&path),
```

with

```rust
            WelcomeEvent::Reveal(path) => cx.reveal_path(&path),
            WelcomeEvent::Customize(event) => self.customize_event(event, window, cx),
```

#### 1.3.8 Two methods, right after `fn welcome_event` (before `pub fn open_project`)

```rust
    /// Carries out what the Welcome screen's Customize page asks: the global settings writes (a
    /// failure is reported; either way the page then shows the settings in effect), the reopen
    /// preference kept on this machine, and its links' actions.
    fn customize_event(&mut self, event: welcome_pages::CustomizeEvent, window: &mut Window, cx: &mut Context<Self>) {
        use welcome_pages::CustomizeEvent;
        match event {
            CustomizeEvent::Write(writes) => {
                if let Err(error) = crate::platform::settings::set_global(&self.label, &writes, cx) {
                    let key = writes.first().map_or("", |write| write.key);
                    let request = NotificationRequest {
                        body: Some(error.message),
                        key: Some(format!("welcome.setting:{key}")),
                        ..NotificationRequest::new(NotificationLevel::Error, "Cannot save setting")
                    };
                    self.notify(request, cx);
                }
                self.sync_customize(cx);
            }
            CustomizeEvent::ReopenPreference(on) => {
                cx.default_global::<UiStorage>().set(REOPEN_PREFERENCE, on.to_string());
            }
            CustomizeEvent::RunAction(id) => window.defer(cx, move |window, cx| {
                execute_action_from(ActionSource::Other, id, None, window, cx);
            }),
        }
    }

    /// Gives the Welcome screen's Customize page the settings in effect for the window and the
    /// reopen preference of this machine (on unless it was turned off).
    pub(super) fn sync_customize(&self, cx: &mut Context<Self>) {
        use crate::settings_model::schema::settings_value;
        let Some(welcome) = self.welcome().cloned() else { return };
        let settings = settings_value(&crate::platform::settings::effective(&self.label, cx));
        let preference =
            cx.try_global::<UiStorage>().and_then(|storage| storage.get(REOPEN_PREFERENCE)) != Some("false");
        let customize = welcome.read(cx).customize().clone();
        customize.update(cx, |page, cx| page.set_settings(settings, preference, cx));
    }
```

#### 1.3.9 (dropped) Welcome tests

Nothing is pasted into `mod tests` of `workbench/mod.rs`. The three tests the draft put there (`the_welcome_screen_lists_projects_and_customize_and_plugins_only_with_a_plugin_host`, `customize_changes_the_theme_and_the_font_size_through_the_global_settings`, `all_settings_on_customize_opens_the_settings_dialog`) are in `plugins_window.rs` `mod tests`, with `settings_dialog_of`/`global_settings_file` (private one-liners of `workbench::tests`) inlined and `settle` replaced by `wait_long`.

### 1.4 `workbench/welcome.rs`: the navigation, Customize and Plugins

#### 1.4.1 Imports

Replace

```rust
use super::project_color::{project_colors, project_initials};
```

with

```rust
use super::project_color::{project_colors, project_initials};
use super::welcome_pages::{CustomizeEvent, CustomizePage, WelcomeNav, WelcomePage, nav_rows};
```

and in the `gpui_kit` import replace `App, ClickEvent, ClipboardItem,` with `AnyView, App, ClickEvent, ClipboardItem,`.

#### 1.4.2 The event

Replace

```rust
    /// Show the menu of `target` (see [`Welcome::menu`]) with its corner at `position`.
    ShowMenu { target: MenuTarget, position: Point<Pixels> },
}
```

with

```rust
    /// Show the menu of `target` (see [`Welcome::menu`]) with its corner at `position`.
    ShowMenu { target: MenuTarget, position: Point<Pixels> },
    /// What the Customize page asks: write settings, keep the reopen preference, run an action.
    Customize(CustomizeEvent),
}
```

#### 1.4.3 The fields

Replace

```rust
    /// The claim on the keys of the list, ahead of the keymap.
    key_claim: u64,
    _field_events: Subscription,
}
```

with

```rust
    /// The claim on the keys of the list, ahead of the keymap.
    key_claim: u64,
    _field_events: Subscription,
    /// The pages beside Projects, and the one shown.
    nav: WelcomeNav,
    customize: Entity<CustomizePage>,
    _customize_events: Subscription,
}
```

#### 1.4.4 `Welcome::new`

Replace

```rust
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        let mut view = Self {
```

with

```rust
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        // Customize shows the defaults until the window gives it the settings in effect.
        let customize = cx.new(|cx| CustomizePage::new(serde_json::Value::Null, true, cx));
        let customize_events = cx.subscribe(&customize, |_, _, event: &CustomizeEvent, cx| {
            cx.emit(WelcomeEvent::Customize(event.clone()));
        });
        let mut view = Self {
```

and replace

```rust
            key_claim,
            _field_events: events,
        };
        view.rebuild(cx);
```

with

```rust
            key_claim,
            _field_events: events,
            nav: WelcomeNav::default(),
            customize,
            _customize_events: customize_events,
        };
        view.rebuild(cx);
```

#### 1.4.5 Methods, after `pub fn with_version`

```rust
    /// The pages listed and the one shown.
    pub fn nav(&self) -> &WelcomeNav {
        &self.nav
    }

    /// The Customize page.
    pub fn customize(&self) -> &Entity<CustomizePage> {
        &self.customize
    }

    /// The Plugins page to list and show (`None`: Plugins is not listed, and Projects shows in its
    /// place).
    pub fn set_plugins_page(&mut self, page: Option<AnyView>, cx: &mut Context<Self>) {
        self.nav.set_plugins_page(page);
        cx.notify();
    }

    /// Shows `page`; false when it is not listed. Projects takes the keyboard back to its search,
    /// Customize takes it, Plugins leaves it to a click.
    pub fn select_page(&mut self, page: WelcomePage, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.nav.select(page) {
            return false;
        }
        match self.nav.shown() {
            WelcomePage::Projects => self.focus_search(window, cx),
            WelcomePage::Customize => window.focus(&self.customize.focus_handle(cx), cx),
            WelcomePage::Plugins => window.focus(&self.focus, cx),
        }
        cx.notify();
        true
    }
```

#### 1.4.6 The keyboard follows only the Projects page

In `fn keep_keyboard`, replace

```rust
    fn keep_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = self.field.focus_handle(cx);
```

with

```rust
    fn keep_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav.shown() != WelcomePage::Projects {
            return;
        }
        let field = self.field.focus_handle(cx);
```

#### 1.4.7 The sidebar lists the pages

In `fn sidebar`, replace

```rust
        let (name, projects) = (tx(cx, "OpenIDEA"), tx(cx, "Projects"));
```

with

```rust
        let name = tx(cx, "OpenIDEA");
```

and replace

```rust
            .child(
                div()
                    .h(px(34.0))
                    .px(px(24.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-selection"))
                    .bg(theme::color(cx, "bg-tree-selected"))
                    .text_color(strong)
                    .child(projects),
            )
            .child(div().flex_1())
```

with

```rust
            .child(nav_rows(
                &self.nav,
                |view: &mut Welcome, page, window, cx| {
                    view.select_page(page, window, cx);
                },
                cx,
            ))
            .child(div().flex_1())
```

#### 1.4.8 `render` shows the page selected

Replace the body of `impl Render for Welcome`'s `render`:

```rust
        self.keep_keyboard(window, cx);
        let landing = self.projects.is_empty();
        let sidebar = self.sidebar(cx);
        let page = if landing { self.landing(cx) } else { self.projects_page(cx) };
        div()
            .size_full()
            .flex()
            .when(landing, |el| el.track_focus(&self.focus))
```

with

```rust
        self.keep_keyboard(window, cx);
        let landing = self.projects.is_empty();
        let shown = self.nav.shown();
        let sidebar = self.sidebar(cx);
        let page = match shown {
            WelcomePage::Projects if landing => self.landing(cx).into_any_element(),
            WelcomePage::Projects => self.projects_page(cx).into_any_element(),
            WelcomePage::Customize => self.customize.clone().into_any_element(),
            WelcomePage::Plugins => match self.nav.plugins_page().cloned() {
                Some(plugins) => plugins.into_any_element(),
                None => self.projects_page(cx).into_any_element(),
            },
        };
        div()
            .size_full()
            .flex()
            // Without a list to search, and on the other pages, the keys come to the screen itself.
            .when(landing || shown != WelcomePage::Projects, |el| el.track_focus(&self.focus))
```

(The rest of `render`, from `.text_size(theme::length(cx, "fs-ui"))` to the end, stays.)

### 1.5 `vcs/prompt_dialogs.rs`: a prompt that accepts an empty value

Plugin Repository restores the default with an empty URL; `FieldRule::NonEmpty` refuses that. Replace

```rust
    /// A branch name: cleaned up as it is typed and checked by its prompt, which may offer to
    /// overwrite an existing branch.
    Branch(Box<BranchPrompt>),
}
```

with

```rust
    /// A branch name: cleaned up as it is typed and checked by its prompt, which may offer to
    /// overwrite an existing branch.
    Branch(Box<BranchPrompt>),
    /// Anything, the empty text included.
    Any,
}
```

and in `fn problem` replace

```rust
            FieldRule::Branch(prompt) => prompt.validate(&field.value, self.offers_overwrite() && self.overwrite, i18n),
        }
```

with

```rust
            FieldRule::Branch(prompt) => prompt.validate(&field.value, self.offers_overwrite() && self.overwrite, i18n),
            FieldRule::Any => None,
        }
```

(`offers_overwrite` and `field_changed` use `matches!`/`if let` on `Branch`; no other exhaustive match exists in `app-gpui/src`.)

### 1.6 `workbench/settings_window.rs`: hand the Plugins page to `plugins_window.rs`

#### 1.6.1 Imports

Replace everything from `use super::Workbench;` through `use std::time::Duration;` with:

```rust
use super::Workbench;
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::platform::actions::ActionRegistry;
use crate::platform::input;
use crate::platform::keymap::Keymap as Bindings;
use crate::platform::settings::{effective, global, set};
use crate::plugin_models::plugins_page::PluginsPage;
use crate::plugin_models::store::SETTINGS_PAGE_ID;
use crate::settings_model::builtin_pages::{code_style_language_pages, settings_registry};
use crate::settings_model::schema::settings_value;
use crate::settings_model::staging::{SecretWrite, SettingsDialog, StagedChange};
use crate::settings_view::dialog::{SettingsDialogEvent, SettingsDialogView, SettingsPageSet};
use crate::settings_view::keymap::{KEYMAP_PAGE, KeymapView, registered_actions};
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Subscription, Window, div,
    prelude::*,
};
use serde_json::Value;
```

#### 1.6.2 Constants

Delete

```rust
/// How long a listing of the plugins waits for the installed ones to have loaded.
const INITIAL_LOAD_WAIT: Duration = Duration::from_secs(5);
```

and replace

```rust
/// Why an operation of the Plugins page that this window does not carry out failed.
const NOT_CARRIED_OUT: &str = "This window cannot do this";
```

with

```rust
/// Why a value kept outside the settings was not saved.
const NOT_CARRIED_OUT: &str = "This window cannot do this";
```

#### 1.6.3 Functions that moved to `plugins_window.rs`

Delete the free functions `catalog_dirs` (with its doc comment), `plugin_work` (with its doc comment) and `event_name` (with its doc comment). Keep `store_ssh_secret`.

#### 1.6.4 `open_settings` makes the page through `plugins_window.rs`

Replace

```rust
        let plugins = if cx.has_global::<Plugins>() {
            let page = cx.new(|cx| PluginsPage::new(window, cx));
            subscriptions.push(cx.subscribe_in(&page, window, Self::plugins_page_event));
            self.follow_plugins_settings(&page, cx);
            dialog.update(cx, |dialog, cx| dialog.set_page_view(SETTINGS_PAGE_ID, page.clone().into(), cx));
            Some(page)
        } else {
            None
        };
```

with

```rust
        let plugins = match self.plugin_host(cx) {
            Some(host) => {
                let (page, events) = self.new_plugins_page(host, window, cx);
                subscriptions.push(events);
                dialog.update(cx, |dialog, cx| dialog.set_page_view(SETTINGS_PAGE_ID, page.clone().into(), cx));
                Some(page)
            }
            None => None,
        };
```

#### 1.6.5 `settings_changed`

Replace

```rust
        if (key.is_empty() || is_under(key, "appearance"))
            && let Some(page) = self.settings_plugins_page().cloned()
        {
            self.follow_plugins_settings(&page, cx);
        }
    }
```

with

```rust
        if key.is_empty() || is_under(key, "appearance") {
            self.follow_settings_on_plugin_pages(cx);
        }
        self.sync_customize(cx);
    }
```

and its doc comment: replace

```rust
    /// A setting changed, in this window or another: the keymap follows the keymap settings, and the
    /// Plugins page the theme and the file icon theme.
```

with

```rust
    /// A setting changed, in this window or another: the keymap follows the keymap settings, the Plugins
    /// pages the theme and the file icon theme, and the Welcome screen's Customize page every setting.
```

#### 1.6.6 The Plugins page section

Delete everything from

```rust
    // ---------------------------------------------------------------------------------------------
    // The Plugins page
```

through the end of `fn answer_plugins_page` (its closing `    }`), leaving the `impl Workbench` block's closing `}` and `#[cfg(test)] mod tests` in place. The deleted methods (`follow_plugins_settings`, `plugins_page_event`, `list_plugins`, `read_plugin_catalog`, `read_plugin_market`, `answer_plugins_page`) are in `plugins_window.rs` now, with the host passed in instead of read from the global.

### 1.7 `plugin_models/consent.rs`: high risk first

The webview orders by `RISK_ORDER = { high: 0, medium: 1, low: 2 }` then `id.localeCompare` (`src/features/plugins/consent.tsx:27-31`). The host's own `preview_install` also sorts high risk first (`crates/ide-host/src/plugins/mod.rs`, `b.risk.cmp(&a.risk)`); `ConsentDialog::new` re-sorted it low first. All three callers (`plugins_page.rs` Permissions of an installed plugin, of a catalog entry, and `ConsentDialog::new`) want high first.

Replace the module doc's first line

```rust
//! What the consent dialog shows of a plugin's capabilities: each with its risk and a description
//! whose parameter (a host, a socket) stays out of the translated text, ordered from the lowest risk,
//! and which optional ones start accepted.
```

with

```rust
//! What the consent dialog shows of a plugin's capabilities: each with its risk and a description
//! whose parameter (a host, a socket) stays out of the translated text, ordered from the highest risk,
//! and which optional ones start accepted.
```

Replace

```rust
/// The capabilities from the lowest risk to the highest, by id within a risk.
pub fn sort_by_risk(capabilities: &[Capability]) -> Vec<Capability> {
    let mut sorted = capabilities.to_vec();
    sorted.sort_by(|a, b| match a.risk.cmp(&b.risk) {
```

with

```rust
/// The capabilities from the highest risk to the lowest, by id within a risk.
pub fn sort_by_risk(capabilities: &[Capability]) -> Vec<Capability> {
    let mut sorted = capabilities.to_vec();
    sorted.sort_by(|a, b| match b.risk.cmp(&a.risk) {
```

In the test `keeps_the_host_out_of_the_translated_text_and_orders_by_risk`, replace

```rust
        assert_eq!(ids, ["project.read", "auth:github", "net.http:example.com", "process:bash"]);
```

with

```rust
        assert_eq!(ids, ["process:bash", "auth:github", "net.http:example.com", "project.read"]);
```

and add this test after it:

```rust
    // src/features/plugins/consent.tsx › sortByRisk: high risk first, then by id within a risk.
    #[test]
    fn lists_high_risk_capabilities_first_and_by_id_within_a_risk() {
        let all = [
            capability("storage", Risk::Low, false, false),
            capability("process:zsh", Risk::High, false, false),
            capability("clipboard", Risk::Medium, true, false),
            capability("ide.commands", Risk::High, false, false),
            capability("events", Risk::Low, false, false),
        ];
        let ids: Vec<String> = sort_by_risk(&all).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["ide.commands", "process:zsh", "clipboard", "events", "storage"]);
    }
```

The end-to-end check is `plugins_window::tests::installing_from_disk_asks_for_consent_with_high_risk_first_and_installs_only_when_accepted` (the install page of `plugins/official/checkstyle` starts with `process:checkstyle`, High). Plugins page tests that index capabilities by position (`plugins_page.rs`, `reviews_the_permissions_of_a_plugin_waiting_for_consent`: `permissions.rows[0]`) list a single capability, so they do not change; `lists_plugins_shows_details_toggles_and_applies_themes` finds rows by id.

### 1.8 `plugins.rs`: the login keychain only outside test builds (point b; needed by every test that runs `Plugins::init`)

The secret store is chosen in `app-gpui/src/plugins.rs` (`Plugins::init`), not in `crates/ide-host` or `crates/ide-plugin`, which only define `SecretStore`, the default `FileSecretStore` and `PluginHost::set_secret_store`. No crate file changes.

Replace

```rust
use crate::backend::{APP_IDENTIFIER, Backend};
```

with

```rust
use crate::backend::Backend;
```

Replace

```rust
/// Plugin secrets in the login keychain, one service per plugin under this app's identifier.
#[cfg(target_os = "macos")]
struct KeychainSecrets;

#[cfg(target_os = "macos")]
impl KeychainSecrets {
    fn service(plugin: &str) -> String {
        format!("{APP_IDENTIFIER}.plugin.{plugin}")
    }
}

#[cfg(target_os = "macos")]
impl ide_host::plugins::SecretStore for KeychainSecrets {
```

with

```rust
/// Plugin secrets in the login keychain, one service per plugin under this app's identifier. Test
/// builds never use it: their hosts keep the default file store under the temporary home.
#[cfg(all(target_os = "macos", not(test)))]
struct KeychainSecrets;

#[cfg(all(target_os = "macos", not(test)))]
impl KeychainSecrets {
    fn service(plugin: &str) -> String {
        format!("{}.plugin.{plugin}", crate::backend::APP_IDENTIFIER)
    }
}

#[cfg(all(target_os = "macos", not(test)))]
impl ide_host::plugins::SecretStore for KeychainSecrets {
```

Replace

```rust
        #[cfg(target_os = "macos")]
        host.set_secret_store(Arc::new(KeychainSecrets));
```

with

```rust
        // Without the keychain (other systems, test builds) the host keeps them in files under its
        // data directory.
        #[cfg(all(target_os = "macos", not(test)))]
        host.set_secret_store(Arc::new(KeychainSecrets));
```

Mechanism: `PluginHost::new(plugins_dir, …)` builds its shared state with `FileSecretStore { dir: plugins_dir.join(".data") }` (`crates/ide-host/src/plugins/mod.rs` `PluginHost::new` → `api::Shared::new`, `crates/ide-host/src/plugins/api.rs`), and `Plugins::init` passes `plugins_dir = <AppDirs.data>/plugins`, which in every test is `<temp home>/data/plugins` (`AppDirs::under`). With this hunk nothing replaces that store in a test build, so from the host's construction on (before `supervise` spawns the load thread, before any listing, activation or command) a secret lives in `<temp home>/data/plugins/.data/<plugin>/secrets.json`. There is no swap, hence no window. It also covers the three compiled tests in `plugins.rs` that run `Plugins::init` and never swapped. Side effect: the `APP_IDENTIFIER` import no longer goes unused on systems other than macOS. Proof: `plugins_window::tests::the_app_plugin_host_keeps_secrets_in_a_file_under_the_temporary_home` (stores a secret of a linked copy of `ai-completion` through the app's host and reads it back from that file).

What the window between `set_secret_store(KeychainSecrets)` and the drafts' swap was, without this hunk (read, for the record): `Plugins::init` installed the keychain, then `supervise` started `plugin-load` (`load_all`, no Wasm, no secret read; `startup_finished` after 1.5 s real time, which can run plugin code), then `refresh` listed in the background; the drafts swapped right after `Plugins::init` in the same `cx.update`, so in practice no plugin code ran first, but nothing enforced it, and the compiled `plugins.rs` tests never swapped.

### 1.9 What `plugins_window.rs` carries out

The level the Plugins tests run at: **workbench level, with a real plugin host, without `Plugins::init`.** `PluginHost::new` sets no process-wide state (that is all in `Plugins::init`: external syntax, run detection, editor providers, the keychain store). `test_plugin_host(dir)` builds one the way `crates/ide-host/tests/plugins.rs::fixture` does (`new`, `load_all`, `startup_finished`) with an in-memory `SecretStore`. `Workbench::use_plugin_host(host, window, cx)` makes it the window's host and gives the Welcome screen its Plugins page over it, so the tests click the real page in a real workbench window. Production goes through the same functions with the app's host (`Plugins::host`).

| Page event                                       | Carried out                                                                                                          | Answer                                 |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `Refresh{plugins}`                               | `wait_initial_load`, `sync_links`, `sync_project(root)`, `list(root)`                                                | `set_listing` / `listing_failed`       |
| `Refresh{catalog}`                               | `catalog(dirs)`                                                                                                      | `set_catalog`                          |
| `Refresh{market}`                                | `market_url`, `market_listing(refresh)`                                                                              | `set_market`                           |
| `SetEnabled` / `Reload` / `Uninstall`            | `set_enabled` / `reload` / `uninstall`                                                                               | `finished`                             |
| `Grant`                                          | `grant(id, &Consent{optional, signer_change})`                                                                       | `finished`                             |
| `SetCapability`                                  | `set_capability`                                                                                                     | `finished`                             |
| `RollBack`                                       | `roll_back_update`                                                                                                   | `rolled_back`                          |
| `AskConsent`                                     | `ConsentDialog` in the plugins layer (priority 2, over Settings)                                                     | `consented`                            |
| `Install(Pick)`                                  | `cx.prompt_for_paths` (files for a package, folders otherwise)                                                       | `picked`                               |
| `Install(Unpack)` / `(Preview)` / `(Install)`    | `unpack_package` / `preview_install` / `install_from_dir`                                                            | `unpacked` / `previewed` / `installed` |
| `DownloadMarket`                                 | `download_market_plugin`                                                                                             | `downloaded`                           |
| `EditRepository`                                 | `PromptDialog` (`FieldRule::Any`, OK "Save"), then `set_market_url(repository_url(text))` and `market_listing(true)` | `set_market`                           |
| `OpenUrl`                                        | `cx.open_url`                                                                                                        | —                                      |
| `SetTheme` / `SelectIconTheme`                   | `set_global` of `appearance.theme` / `appearance.fileIconTheme` (`""` for none); `plugins.rs` follows                | —                                      |
| `RunCommand`                                     | `execute_action_from(Other, plugin_action_id(..))`, deferred                                                         | —                                      |
| `LoadSecrets` / `SetSecret`                      | `secrets` / `set_secret`                                                                                             | `set_secrets` / `secret_saved`         |
| `Notify`                                         | `Workbench::notify`                                                                                                  | —                                      |
| `ImportInstalledIntellij` / `ImportIntellijFile` | logged by kind only                                                                                                  | —                                      |

From the host (the window's listener on `Plugins`, registered in `Workbench::new`): `CommandsChanged` lists every page of the window again; `ConsentRequested{plugin_id, capabilities}` for this window (or every window) shows `ConsentDialog` with the plugin's `grant_request` and the event's capabilities; Allow runs `grant` and lists the pages again, a failure is "Could not enable <name>".

Actions (none has a `when` in `assets/gen/actions.json`): `plugins.installFromDisk` (string argument: a folder, its `plugin.toml` or a package; none: the picker) drives Settings' page while it is open, else the Welcome screen's, else a page made for it and kept (never drawn: its consent dialog and notifications still show). `plugins.reloadAll` reloads every listed plugin on the host, reports each failure as "Could not reload <name>" and lists the pages again.

Types that carry a secret and have a hand-written `Debug`: `PluginsPageEvent` (1.2.2: `SetSecret.value` shows as `…`) and `SecretField` (writes only its placeholder). Nothing in `plugins_window.rs` derives `Debug` over a secret; its log lines name the event's kind (`event_name`) or a plugin id, never a field of `SetSecret`. `SettingsWrite`/`staging::Secret` are unchanged.

Changes to the table and the paragraphs above, as built today:

- Tests of the plugin pages run on `test_plugin_host(dir)` (in-memory `MemorySecrets`, no `Plugins::init`) or, where the app's host matters (point b, point d), on `test_support::start_with_plugins`.
- `plugins.updateAll` is **not registered** (no test can run it without a marketplace index; rule: register only what an end-to-end test runs). Menus drop unregistered actions, so the Welcome screen's Manage menu does not list it; the page's own gear menu still runs `PluginsPage::update_all`. `plugins.installFromDisk` and `plugins.reloadAll` are registered, each run by a test.
- No `plugins.link` action: the webview has none (point d).

### 1.10 `database/source_dialog.rs` (not done)

The old §9 said `source_dialog.rs` is not compiled; it is compiled now and has its own `PasswordField` (same design as `SecretField`: `TextField` state, bullets painted, Copy/Cut swallowed, `painted()`). Moving it onto `SecretField` would edit a compiled file and its tests (`display` → `shown`, `TextFieldEvent` → `SecretFieldEvent`); it is left out of this integration.

Note on 1.2.4: it reverses an earlier integrator's secrets fix on purpose. They hid secret rows until `LoadSecrets` answered because the value field was a plain `TextField` that showed what was typed (`shows_no_secret_field_while_the_secrets_are_not_read`). With `SecretField` the field never paints, copies or logs the value, so the rows are drawn at once as the webview draws them (`src/features/plugins/secrets.tsx:55`, a `type="password"` input per declared secret, "Not set" until the state loads), and that test is replaced by `draws_a_masked_field_for_each_declared_secret_before_its_state_is_read` (1.2.8).

### 1.11 Not wired

- **IntelliJ imports.** `theme/icls.rs` imports editor colour schemes (`.icls`); nothing in `app-gpui/src` converts a JetBrains theme into a plugin (the webview's `intellijTheme.ts`), and `ide_host::plugins::intellij::{installed_themes, read_source}` only list and read. `ImportInstalledIntellij` / `ImportIntellijFile` are logged, `IntellijThemesDialog` is never shown, and the actions `plugins.importIntellijTheme` / `plugins.importInstalledIntellijTheme` are not registered.
- The Plugin Repository prompt has no placeholder ("https://…/index.json or a folder path") and no hint ("Leave empty to use the default repository."): `PromptRequest` has no fields for them.
- The Install picker has no title (`prompt: None`), as the webview's own picker title is not known here.
- Pages are not listed again when a project opens or its trust changes (only on `CommandsChanged`, after their own operations and on Rescan); `follow_settings_on_plugin_pages` runs on `appearance` changes only.
- `ProgramPermission` (choose or reset the program of a `process:*` capability) and the rest of the earlier report's "Not built" list are unchanged.
- New user-visible strings that need entries in `src/platform/locales/text/en.json` and `zh.json` (off limits here): "Plugin Repository", "Marketplace index URL:", "Save" (likely present), "Cannot save setting" (used by the earlier Welcome report as well).

---

## Stage 2: the plugin runtime of a window

Files to declare: `app-gpui/src/plugin_views/mod.rs` (`pub mod log_tab; pub mod prompt_dialog;`), `plugin_views/prompt_dialog.rs` (`PluginPrompt`, 8 tests), `plugin_views/log_tab.rs` (`PluginLogTab`, 6 tests), `workbench/plugin_runtime.rs` (`PluginRuntime` slot, `impl Workbench`, `register_actions`, `build_plugin_log_tab`, 11 end-to-end tests).

Hunks: 12 edit sites in `lib.rs`, `workbench/mod.rs` (8), `workbench/chrome.rs`, `workbench/tab_views.rs`, and the stage-1 draft `workbench/plugins_window.rs` (`test_support`). 10 apply as the draft wrote them (`crates/ide-host`, `crates/ide-plugin`, `crates/ide-core`, `plugin_models/`, `status_bar.rs`, `tasks.rs`, `notifications.rs` are unchanged since the draft commit `d8a8fa57`; `tab_views.rs` changed one unrelated line; the anchors in `workbench/mod.rs` were read with their enclosing functions). 1 is rewritten (2.2.8, the question layer: it moves over Settings, anchored on stage 1's line). 1 is new (2.5, `program`/`programs_in` in `test_support`, used first by this stage's tests).

Order with stage 1: stage 1's hunks in `Workbench::new` replace the tail (`sync_welcome` / `start_code_insight`) and the `if cx.has_global::<Backend>()` block of `on_release`; this stage's anchors are other lines (`welcome: None,` + `};`, and `input::window_blurred(id, cx);`). After both, `Workbench::new` ends `…welcome: None, plugin_runtime: Default::default(), }; workbench.start_plugin_runtime(window, cx); workbench.listen_to_plugins(window, cx); workbench.sync_welcome(window, cx); …`.

Without the `Plugins` global (every existing test): `start_plugin_runtime` returns before listening; `stop_plugin_runtime` returns when nothing was started; `plugin_tabs_changed` returns at once when the window follows no plugin host (edit made to the draft for this: it is called on every tab change of every window). `cancel_background_task`, `plugin_prompt`, `plugin_log_tabs` are `pub` methods of `Workbench`, so they are not dead code while only tests call them.

### 2.1 `app-gpui/src/lib.rs`

After `pub mod plugin_models;` add:

```rust
pub mod plugin_views;
```

### 2.2 `app-gpui/src/workbench/mod.rs`

#### 2.2.1 Module line

Before `mod project_files;` add:

```rust
mod plugin_runtime;
```

#### 2.2.2 The field

Replace

```rust
    chrome: chrome::Chrome,
}
```

(the end of `pub struct Workbench`) with

```rust
    chrome: chrome::Chrome,
    /// What running plugin code shows in the window, and what the window tells the plugins.
    plugin_runtime: plugin_runtime::PluginRuntime,
}
```

#### 2.2.3 Its initialiser and start (in `Workbench::new`)

Replace

```rust
            welcome: None,
        };
```

with

```rust
            welcome: None,
            plugin_runtime: Default::default(),
        };
        workbench.start_plugin_runtime(window, cx);
```

#### 2.2.4 Its end (in the `cx.on_release` of `Workbench::new`)

Before `            input::window_blurred(id, cx);` add:

```rust
            workbench.stop_plugin_runtime(cx);
```

#### 2.2.5 Actions

After `    tab_views::register_actions(registry);` (in `register_actions`) add:

```rust
    plugin_runtime::register_actions(registry);
```

#### 2.2.6 Tabs changed (in `fn tabs_changed`)

After `        self.note_active_tab(cx);` add:

```rust
        self.plugin_tabs_changed(cx);
```

#### 2.2.7 Notification buttons (in `fn run_notification_action`)

Before `            EntryAction::Own(own) if own.command.starts_with("agents.openTask:") => {` add:

```rust
            // A plugin's notification: its button runs the publishing plugin's command.
            EntryAction::Own(own)
                if own.command.starts_with(crate::plugin_models::runtime_ui::NOTIFICATION_COMMAND_PREFIX) =>
            {
                self.run_plugin_notification_button(&own.command, cx);
            }
```

#### 2.2.8 The questions' layer (in `Render for Workbench`), rewritten: over Settings

After the line stage 1 adds (1.3.5)

```rust
            .children(self.plugins_dialog_layer())
```

add

```rust
            .children(self.plugin_prompt_layer())
```

Why it moved from after `.children(self.vcs_dialog_layer())`: `plugin_prompt_layer` is `deferred(..).with_priority(1)`, as is `settings_layer`; equal priorities paint in child order, so after `vcs_dialog_layer` the question drew under the Settings dialog, and a command run from a plugin's Settings page (stage 3) asks while Settings is open. Order now: Settings (1) < plugin question (1, later) < consent dialog, Plugin Repository prompt and close/open question (2). The popup slot (priority 1, after it) still draws above; a question is modal, so no popup opens while it waits. Webview: the question dialog is a modal over everything (`src/features/plugins/runtimeUi.tsx`, `PromptDialog`).

### 2.3 `app-gpui/src/workbench/chrome.rs` (in `fn status_bar_event`)

Before `            StatusBarEvent::RunAction { id, arg, .. } => run_later(id, arg, window, cx),` add:

```rust
            // A plugin's item runs its plugin's command, declared or not, as the plugin set it.
            StatusBarEvent::RunAction { item, .. } if item.starts_with(super::plugin_runtime::PLUGIN_STATUS_PREFIX) => {
                self.plugin_status_item_clicked(&item, cx)
            }
```

### 2.4 `app-gpui/src/workbench/tab_views.rs` (`TAB_KINDS`)

After the `BINARY_KIND` entry (its last line is `        build: build_hex,` then `    },`), add a third entry:

```rust
    TabKind {
        id: crate::plugin_views::log_tab::PLUGIN_LOG_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: crate::plugin_views::log_tab::log_tab_title,
        icon: super::plugin_runtime::log_tab_icon,
        build: super::plugin_runtime::build_plugin_log_tab,
    },
```

`every_kind_is_registered_once` (tab_views tests) keeps holding.

### 2.5 `workbench/plugins_window.rs` (stage-1 draft): `program` and `programs_in` join `test_support` (needed by: compilation of this stage's tests)

They are not in stage 1: as `pub fn` of a `pub(crate)` module no stage-1 test calls, they would be dead code under `-D warnings`. Inside `pub(crate) mod test_support`, before

```rust
    /// A trusted project of `files` (path, text), open in an active window known as `label`.
```

add

```rust
    /// An executable `name` in `bin` running `script`.
    #[cfg(unix)]
    pub fn program(bin: &Path, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(bin).expect("the folder is made");
        let path = bin.join(name);
        std::fs::write(&path, script).expect("the script is written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
    }

    /// Programs a plugin may be granted are looked up in `bin` only.
    pub fn programs_in(bin: &Path, cx: &mut TestAppContext) {
        let bin = bin.to_path_buf();
        let path = move || Some(bin.clone().into_os_string());
        cx.update(|cx| cx.global::<Plugins>().host.set_program_path(std::sync::Arc::new(path)));
    }
```

(`plugin_runtime.rs`'s tests import them under `#[cfg(unix)]`; every caller is a `#[cfg(unix)]` test.)

### 2.8 What each runtime event kind does (complete list of `PluginEvent`)

Built: `StatusItemChanged`, `WindowStatusItemChanged` (status bar; declared `[[contributes.statusBar]]` items shown until code sets its own), `TaskStarted`/`TaskProgress`/`TaskEnded` (task list, Cancel through `cancel_background_task`), `ContextChanged` (refetch, `set_context`), `Notification` (buttons run the plugin command), `CopyToClipboard`, `OpenUrl` (http/https only, the model's check), `ExecuteCommand` (action or plugin command, answered), `EditorOpen` and `OpenLocation` (caret at the position), `EditorRanges` (selections / scroll in open editors), `OpenProject` (`file.openProjectPath` / `file.openInNewWindow`), `PromptRequested`/`PromptClosed`, `OpenLogTab`, `OutputChanged`/`RevealOutput` (an output tab), `SubscriptionsChanged` and `CommandsChanged` (event kinds asked again; the listing is `Plugins::pump`'s), `UpdatesAvailable` (notification with Update All / Show Plugins, commands `plugins.updateAll` / `plugins.settings`, run when registered).

Answered as a window without the service answers (so no plugin call hangs): `WindowRequest::Run` (`finish_operation(op, None, false)`, then `Failed("Run service unavailable")` when it has a request id), `WindowRequest::Terminal` / `DatabaseConnect` / `ContentTab` (`Failed(...)`, the webview's texts), `CommitRequested` (`CommitAnswer::NoRepository`: the native Commit view does not hand out its selection yet).

Logged and not shown, with why: `OpenTerminal` (no native `terminal.runCommand` action yet), `DecorationsChanged`, `ViewChanged`, `ViewMessage`, `RevealView`, `ContentTabReply` (plugin views, web views and content tabs: later draft), `RunTargetsChanged` (run targets: later draft), `DocumentIndexesChanged`, `EndpointsChanged` (navigation, endpoints: later draft), `ConsentRequested` (handled by `plugins_window.rs`, `plugin_host_event`), `ThemeChanged` and `LanguagesChanged` (already handled by `Plugins::pump` and the sink in `plugins.rs`).

Window events posted (500 ms batches, only kinds in `event_kinds`): `editor-focused` (through `set_active_file`, deduplicated, reported again when the project opens), `file-opened`, `file-closed` (from the file tabs), `vcs-changed` (backend `VcsChanged`), `settings-changed` (backend `SettingsChanged`, and once when the kinds include it), `action-performed` (registry `on_action_executed`, deferred). Not posted: `file-saved` (no single save-completed point in the workbench; `Editor::save` callers are many) and `input-mode-changed` (Vim mode changes are not events natively). `files-changed` is the backend's own (`project.rs`). Activation events `on-language:` / `on-file:` (bridge.ts `trackActivationEvents`) are not sent: `ActivationEvent` is in `ide-extension`, which `app-gpui` does not depend on (add `ide-extension = { path = "../crates/ide-extension" }`, already in the graph, and call `host.activation_event` from `plugin_tabs_changed`).

### 2.9 Safety

- Grants: the window calls only `PluginHost`'s public API; every host function checks the plugin's grant itself (log tab spawn: `pty_options` checks `process:<program>`; clipboard, `project.read`, `events` subscriptions are checked by the host). Every call first passes `Host::authorize_command(label, "<ipc name>")` (`plugin_call_allowed`), as Tauri's invoke handler does (`src-tauri/src/lib.rs:170`, `crates/ide-host/src/trust.rs:429`): without an open, trusted project only the restricted-allowed commands go through. Nothing here grants anything.
- Secrets: no code here reads, paints or logs a secret; `PluginRuntime` has no `Debug`. The tests run on the host's default file store under the temporary home (stage 1, 1.8); the draft's swap to `FileSecretStore` after `Plugins::init` is deleted, and no test stores a secret.
- Folders: every test uses the workbench test home (`AppDirs::under(tempdir)`), links a copy of each fixture made in that home (`hello`, `api14`, `jobs`, `web-log` from `crates/ide-plugin/tests/fixtures`, `checkstyle`, `git-toolbox` from `plugins/official`; the repository is only read), and points `set_program_path` at a temp `bin` (or `/bin` for `bash`). The repository of the branch test is made by `init_repo`/`git_in` (the Git tests' isolation: no global config, no hooks). URLs (`https://example.test/*`) only reach the test platform's `opened_url`.

### 2.10 Not built, with why

- Find in Logs (`plugins.log.find`, the log tab's search button): the native find bar works on `Editor` entities only.
- Plugin text catalogs (`usePluginT`) for running status item texts: shown as the plugin wrote them.
- A log tab's soft wrap starts on; the webview also reads `editor.softWrap` and the soft-wrap file patterns for `stream.log`.
- A log tab whose input has no program closes (tab_views closes tabs it cannot build) instead of showing "Invalid log stream configuration". The webview's UI-side `logTabData` validation is left to the host's `pty_options`, which refuses the same programs.
- The folder question (`pickDirectory`) opens the system dialog without its title or initial folder (`PathPromptOptions` has neither).
- `EditorRanges` with `reveal` scrolls the first range to the top; the webview centres it.
- **Cancel is not reachable by a user yet.** The tasks status item's click runs `view.backgroundTasks` (`status_items.rs:193`), which nothing registers natively, so `reachable()` drops the click; there is no Background Tasks popup. `Workbench::cancel_background_task(&TaskId, cx)` is the carrier its Cancel button needs; the task test calls it directly.
- `file-saved` is not posted (see 2.8); `checkstyle` and `license-header` activate on it, so it is the first gap to close.
- Pick lists have no filter field: the webview's `PickDialog` has none either (the assignment's "filtering" is not webview behaviour).

- The `UpdatesAvailable` notification's Update All button runs `plugins.updateAll`, which stage 1 does not register (no test can run it without a marketplace index), so that button does nothing until it is registered; Show Plugins works.

---

## Stage 3: plugin contributions

Files to declare: `workbench/plugin_contributions.rs` (`PluginContributions` slot, `PluginMenu`, `SlotItem`, `plugin_tool_items`, `plugin_slot_items`, `plugin_menu_items`, `plugin_settings_pages`, `register_actions` with `view.backgroundTasks` and `debug.pluginStart`, `impl Workbench`, 14 end-to-end tests), `workbench/background_tasks_view.rs` (`BackgroundTasksPopup`, 3 view tests), `plugin_views/settings_page.rs` (`PluginSettingsPage`, `SettingsDecl`, `settings_pages`, 2 model tests + 7 view tests).

Hunks: 44 edit sites in `app-gpui/Cargo.toml` (the only new dependency, `ide-extension`), the stage-2 drafts `plugin_views/mod.rs` and `workbench/plugin_runtime.rs`, `workbench/mod.rs`, `workbench/menu.rs`, `workbench/project_files.rs`, `workbench/settings_window.rs`, `debugger/actions.rs`, `debugger/driver.rs`, `workbench/debug.rs`. 17 apply as the draft wrote them. 1 is re-anchored (3.4.5: `if still_open {` now occurs twice in the file-opening task). 5 are rewritten: 3.3.2 (passes `window`, the debug request needs it), 3.5 (menu.rs: no import edit, which would push the import list past 120 columns), 3.6 (project_files.rs: a wrapper instead of a signature change, which would break the New popup's caller and the undeclared `project_view_actions.rs` draft; the draft's 3 sites become 3 others). 21 are new (3.8: the debugger carries a plugin's operation and hands its session events to the window; point c). The draft's `menu.rs` import edit and the New-popup caller edit are dropped.

Stage 3 needs stages 1 and 2: `plugins_window::test_support` (incl. 2.5's `program`/`programs_in`), and anchors 3.3 / 3.4.2–3.4.4 are lines stage 2 adds. Nothing in stages 1 and 2 calls a stage-3 symbol (the stage-3 changes to `plugin_runtime.rs` are hunks 3.3, not edits of the draft). The `pub mod` lines (3.4.1) export `plugin_menu_items`, `plugin_slot_items`, `PluginMenu`, `SlotItem`, `plugin_input_mode_changed`, `plugin_gutter_markers`, `background_tasks_popup`, which have no caller outside tests until the editor's context menu, the toolbar slots and Vim exist.

### 3.1 `app-gpui/Cargo.toml`

After `ide-docker = { path = "../crates/ide-docker" }` add:

```toml
ide-extension = { path = "../crates/ide-extension" }
```

(`ActivationEvent` for `PluginHost::activation_event`; `ide_host` uses it privately and does not re-export it. The crate is already in the graph through `ide-host`.)

### 3.2 `app-gpui/src/plugin_views/mod.rs` (draft)

After `pub mod prompt_dialog;` add:

```rust
pub mod settings_page;
```

and replace its doc

```rust
//! Views of what running plugin code shows: the questions a waiting command asks, and the log tabs
//! plugin code opens.
```

with

```rust
//! Views of what plugins show: the questions a waiting command asks, the log tabs plugin code opens
//! and their pages in Settings.
```

### 3.3 `app-gpui/src/workbench/plugin_runtime.rs` (draft)

#### 3.3.1 Visibility (four helpers the sibling calls)

- Replace `fn plugin_host(cx: &App) -> Option<Arc<PluginHost>> {` with `pub(super) fn plugin_host(cx: &App) -> Option<Arc<PluginHost>> {`
- Replace `    fn plugin_call_allowed(&self, command: &str, cx: &App) -> Result<(), String> {` with `    pub(super) fn plugin_call_allowed(&self, command: &str, cx: &App) -> Result<(), String> {`
- Replace `    fn post_plugin_event(&mut self, event: WorkbenchEvent, cx: &mut Context<Self>) {` with `    pub(super) fn post_plugin_event(&mut self, event: WorkbenchEvent, cx: &mut Context<Self>) {`
- Replace `fn answer_plugin_command(label: &str, request_id: &str, answer: CommandAnswer, cx: &App) {` with `pub(super) fn answer_plugin_command(label: &str, request_id: &str, answer: CommandAnswer, cx: &App) {`

#### 3.3.2 Run and debug requests go to the Run tool window and the debugger (in `fn plugin_runtime_event`), rewritten

Replace

```rust
            PluginEvent::WindowRequest { request_id, request, .. } => {
                self.refuse_window_request(request_id, request, cx);
            }
```

with

```rust
            PluginEvent::WindowRequest { request_id, request: WindowRequest::Run { operation, config, debug }, .. } => {
                self.plugin_run_request(request_id, operation, config, *debug, window, cx);
            }
            PluginEvent::WindowRequest { request_id, request, .. } => {
                self.refuse_window_request(request_id, request, cx);
            }
```

(`plugin_runtime_event` has `window: &mut Window`.) `refuse_window_request`'s `WindowRequest::Run` arm stays (its match stays complete). In the draft's module doc and in 2.8, `WindowRequest::Run` moves from "answered as a window without the service" to "built": a run goes to the run driver, a debug request runs `debug.pluginStart` (point c).

#### 3.3.3 `file-saved` (in `fn plugin_backend_event`)

Before `            IdeEvent::TerminalExited { terminal, code } => self.plugin_terminal_exited(*terminal, *code, cx),` add:

```rust
            // The backend clears a document's dirty flag only when it saved it.
            IdeEvent::DocDirty { doc, dirty: false } => self.plugin_document_saved(*doc, cx),
```

#### 3.3.4 Activation events and the gutter of the focused editor (end of `fn plugin_tabs_changed`)

Replace

```rust
        if let Some(report) = self.plugin_runtime.active_file.focused(active.as_deref()) {
            self.report_active_file(report, cx);
        }
    }
```

with

```rust
        if let Some(report) = self.plugin_runtime.active_file.focused(active.as_deref()) {
            self.report_active_file(report, cx);
        }
        self.plugin_activation_events(cx);
        self.plugin_gutter_shown(cx);
    }
```

### 3.4 `app-gpui/src/workbench/mod.rs`

#### 3.4.1 Module lines

Before `mod bookmarks_window;` add `pub mod background_tasks_view;`. Before `mod plugin_runtime;` (added by the runtime note) add:

```rust
pub mod plugin_contributions;
```

(`pub`: its `pub` items — `plugin_menu_items`, `plugin_slot_items`, `PluginMenu`, `SlotItem`, `Workbench::plugin_input_mode_changed` — have no caller outside tests until the editor's context menu, the toolbar slots and Vim exist; exported, they are not dead code.)

#### 3.4.2 Field

Replace

```rust
    plugin_runtime: plugin_runtime::PluginRuntime,
}
```

with

```rust
    plugin_runtime: plugin_runtime::PluginRuntime,
    /// Plugins' gutter actions, activation events and what else they contribute to the window.
    plugin_contributions: plugin_contributions::PluginContributions,
}
```

#### 3.4.3 Initialiser (in `Workbench::new`)

Replace `            plugin_runtime: Default::default(),` with

```rust
            plugin_runtime: Default::default(),
            plugin_contributions: Default::default(),
```

#### 3.4.4 Actions

After `    plugin_runtime::register_actions(registry);` add:

```rust
    plugin_contributions::register_actions(registry);
```

#### 3.4.5 Every project editor's gutter is followed (in the file-opening task), re-anchored

`if still_open {` now occurs twice in that task (the compiling agent added another block with `workbench.install_intelligence(&editor, window, cx);`). After the unique line

```rust
                            workbench.watch_debug_editor(&editor, window, cx);
```

add

```rust
                            workbench.watch_plugin_gutter(&editor, window, cx);
```

(`watch_plugin_gutter` returns at once without the `Plugins` global.)

#### 3.4.6 The gutter icons (in `Render for Workbench`)

Before `            .child(notification_balloons::layer(&self.balloons, cx))` add:

```rust
            .children(self.plugin_gutter_layer(cx))
```

### 3.5 `workbench/menu.rs` (Tools › plugin entries), rewritten

In `fn state`, replace

```rust
        (DYNAMIC_STATUS_BAR_WIDGETS.to_owned(), status_widgets),
    ]);
```

with

```rust
        (DYNAMIC_STATUS_BAR_WIDGETS.to_owned(), status_widgets),
        (super::menu_spec::DYNAMIC_PLUGIN_TOOLS.to_owned(), super::plugin_contributions::plugin_tool_items(cx)),
    ]);
```

The `use super::menu_spec::{…}` list is not touched (adding `DYNAMIC_PLUGIN_TOOLS` there pushes it past 120 columns). `plugin_tool_items(cx: &App)` borrows `cx` shared while the shared `i18n` is alive. The disabled set below already covers the dynamic items' actions, so an entry is greyed out while its command's `when` fails. `menu_spec.rs` needs nothing: Tools already ends with `Entry::Dynamic(DYNAMIC_PLUGIN_TOOLS)`.

### 3.6 `workbench/project_files.rs` (Project view › plugin group), rewritten

Changing `with_menu_context`'s signature (the draft) breaks its other callers: the New popup (compiled, `new_menu_items`) and the undeclared draft `workbench/project_view_actions.rs` (`select_in_targets`). A wrapper keeps all of them.

#### 3.6.1 Replace

```rust
    fn with_menu_context<R>(&self, cx: &App, build: impl FnOnce(&MenuContext<'_>, &App) -> R) -> R {
```

with

```rust
    fn with_menu_context<R>(&self, cx: &App, build: impl FnOnce(&MenuContext<'_>, &App) -> R) -> R {
        self.with_contributed_menu_context(Contributed::default(), cx, build)
    }

    /// [`Self::with_menu_context`], with the groups other features contribute to the menus.
    fn with_contributed_menu_context<R>(
        &self,
        contributed: Contributed,
        cx: &App,
        build: impl FnOnce(&MenuContext<'_>, &App) -> R,
    ) -> R {
```

and delete the line (once in the file, now in the body of `with_contributed_menu_context`)

```rust
        let contributed = Contributed::default();
```

#### 3.6.2 In `tree_context_menu`, replace

```rust
        let items = cx.update_global::<Localization, _>(|localization, cx| {
            self.with_menu_context(cx, |ctx, _| {
```

with

```rust
        let selected = crate::project_view::context_menu::menu_group_context(&node_refs(&nodes));
        let project_view = self.plugin_project_view_items(&selected.paths, window, cx);
        let contributed = Contributed { project_view, ..Contributed::default() };
        let items = cx.update_global::<Localization, _>(|localization, cx| {
            self.with_contributed_menu_context(contributed, cx, |ctx, _| {
```

(`node_refs` is imported in that file; `menu_group_context(&[&ProjectNode])` takes what `node_refs(&[Arc<ProjectNode>])` returns; `tree_context_menu` has `window`.) The New-popup caller is not touched. The undeclared `context_menus_integration.md` §3 assumed the draft's signature change; with this wrapper its hunks call `with_contributed_menu_context(contributed, cx, …)` instead (not resolved here).

### 3.7 `app-gpui/src/workbench/settings_window.rs` (a page per plugin)

These anchors are not touched by stage 1 (1.6).

Replace

```rust
        let set = SettingsPageSet { specs: settings_registry(updates), pages: code_style_language_pages(&languages) };
```

with

```rust
        let mut pages = code_style_language_pages(&languages);
        pages.extend(super::plugin_contributions::plugin_settings_pages(cx));
        let set = SettingsPageSet { specs: settings_registry(updates), pages };
```

After `        let mut subscriptions = vec![cx.subscribe_in(&dialog, window, Self::settings_dialog_event)];` add:

```rust
        subscriptions.extend(self.mount_plugin_settings_pages(&dialog, has_project, window, cx));
```

### 3.8 The debugger: a plugin's debug request runs under it (point c; needed by `a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end`)

`debugger/actions.rs` already runs `debug.pluginStart` (prepare through the plugin, launch config, save, session, `PluginEvent` on the end), but its events do not say which plugin start they belong to, `driver.rs` refuses `PluginPrepareDebugRun` ("cannot be debugged in this window") and drops the plugin events. The smallest change: the operation travels with the start, the driver asks the host's `prepare_debug_run` (IPC `plugin_prepare_debug_run`, `src-tauri/src/commands/plugin.rs:1036`) and emits the events, and `workbench/debug.rs` hands them to `plugin_contributions.rs`. `workbench/debug.rs` is being edited by the compiling agent: its anchors are single distinctive lines.

### 3.8.1 `app-gpui/src/debugger/actions.rs`: the plugin's operation travels with its debug session

The model already runs `debug.pluginStart` (prepare → launch → session → `PluginEvent`), but its events do not say which plugin start they belong to, so a window could not tell two plugins' sessions apart. Each start now carries the operation from the action's argument (`{ config, operation }`).

1. Replace

```rust
/// What a plugin that started a debug session is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEvent {
    /// The session could not start.
    Failed { message: String },
    /// The session ended; `stopped`: because the user stopped it.
    Exited { session: DebugSessionId, code: Option<i32>, stopped: bool },
}
```

with

```rust
/// What a plugin that started a debug session is told, under the operation it started it for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEvent {
    /// The session could not start.
    Failed { operation: String, message: String },
    /// The session ended; `stopped`: because the user stopped it.
    Exited { operation: String, session: DebugSessionId, code: Option<i32>, stopped: bool },
}
```

2. Replace

```rust
    /// A plugin's configuration is prepared, then saved before its session starts.
    PluginPrepare,
    PluginSave {
        config: Box<RunConfig>,
        launch: Box<LaunchConfig>,
    },
```

with

```rust
    /// A plugin's configuration for its operation is prepared, then saved before its session starts.
    PluginPrepare {
        operation: String,
    },
    PluginSave {
        operation: String,
        config: Box<RunConfig>,
        launch: Box<LaunchConfig>,
    },
```

3. Replace

```rust
    /// A plugin started the session and is told how it went.
    Plugin,
```

with

```rust
    /// A plugin started the session for this operation and is told how it went.
    Plugin(String),
```

4. Replace

```rust
    /// Sessions plugins started, with whether a stop of each was seen.
    plugin_sessions: HashMap<DebugSessionId, bool>,
```

with

```rust
    /// Sessions plugins started, with the operation each is for and whether a stop of it was seen.
    plugin_sessions: HashMap<DebugSessionId, (String, bool)>,
```

5. Replace `            Some(Waiting::PluginPrepare) => self.plugin_prepared(result, host),` with `            Some(Waiting::PluginPrepare { operation }) => self.plugin_prepared(operation, result, host),`

6. Replace

```rust
            Some(Waiting::PluginSave { config, launch }) => match result {
                Err(message) => self.plugin_events.push(PluginEvent::Failed { message }),
```

with

```rust
            Some(Waiting::PluginSave { operation, config, launch }) => match result {
                Err(message) => self.plugin_events.push(PluginEvent::Failed { operation, message }),
```

and, a few lines below in the same arm, replace

```rust
                        Some(start) => {
                            self.starts.insert(start, AfterStart::Plugin);
                        }
                        None => self.plugin_failed("Debug session did not start"),
```

with

```rust
                        Some(start) => {
                            self.starts.insert(start, AfterStart::Plugin(operation));
                        }
                        None => self.plugin_failed(operation, "Debug session did not start"),
```

7. In `fn start_ended`, replace

```rust
            (Some(AfterStart::Plugin), Some(session)) => {
                self.plugin_sessions.insert(session, false);
                self.observe_plugin_sessions();
            }
            (Some(AfterStart::Plugin), None) => self.plugin_failed("Debug session did not start"),
```

with

```rust
            (Some(AfterStart::Plugin(operation)), Some(session)) => {
                self.plugin_sessions.insert(session, (operation, false));
                self.observe_plugin_sessions();
            }
            (Some(AfterStart::Plugin(operation)), None) => self.plugin_failed(operation, "Debug session did not start"),
```

8. Replace (the three functions, quoted whole)

```rust
    fn plugin_failed(&mut self, message: &str) {
        self.plugin_events.push(PluginEvent::Failed { message: message.to_owned() });
    }

    /// Debug Plugin Configuration (arg: `{ config }`): the plugin's backend prepares the
    /// configuration, then it is debugged; the plugin hears how it went through
    /// [`Debugger::take_plugin_events`].
    fn plugin_start(&mut self, arg: Option<&Value>, host: &mut Host) {
        let config = arg.and_then(|arg| arg.get("config")).and_then(run_config);
        match config {
            Some(config) => self.ask(BackendCall::PluginPrepareDebugRun(Box::new(config)), Waiting::PluginPrepare),
            None => self.plugin_failed(&host.i18n.tx("This configuration has no debugger")),
        }
    }

    fn plugin_prepared(&mut self, result: Result<RunConfig, IdeError>, host: &mut Host) {
        let config = match result {
            Ok(config) => config,
            Err(error) => return self.plugin_failed(error_text(&error)),
        };
        let Some(root) = host.root else { return self.plugin_failed("No project") };
        let env = LaunchEnv { debugger: host.env.debugger, windows: host.windows };
        let Some(launch) = launch_config_for(&config, root, None, &env) else {
            return self.plugin_failed("This configuration has no debugger");
        };
        let ticket = self.wait(Waiting::PluginSave { config: Box::new(config), launch: Box::new(launch) });
        self.effects.push(Effect::SaveAllThen { ticket });
    }
```

with

```rust
    fn plugin_failed(&mut self, operation: String, message: &str) {
        self.plugin_events.push(PluginEvent::Failed { operation, message: message.to_owned() });
    }

    /// Debug Plugin Configuration (arg: `{ config, operation }`): the plugin's backend prepares the
    /// configuration, then it is debugged; the plugin of `operation` hears how it went through
    /// [`Debugger::take_plugin_events`].
    fn plugin_start(&mut self, arg: Option<&Value>, host: &mut Host) {
        let operation = arg.and_then(|arg| arg.get("operation")).and_then(Value::as_str).unwrap_or_default();
        let operation = operation.to_owned();
        match arg.and_then(|arg| arg.get("config")).and_then(run_config) {
            Some(config) => {
                self.ask(BackendCall::PluginPrepareDebugRun(Box::new(config)), Waiting::PluginPrepare { operation });
            }
            None => self.plugin_failed(operation, &host.i18n.tx("This configuration has no debugger")),
        }
    }

    fn plugin_prepared(&mut self, operation: String, result: Result<RunConfig, IdeError>, host: &mut Host) {
        let config = match result {
            Ok(config) => config,
            Err(error) => return self.plugin_failed(operation, error_text(&error)),
        };
        let Some(root) = host.root else { return self.plugin_failed(operation, "No project") };
        let env = LaunchEnv { debugger: host.env.debugger, windows: host.windows };
        let Some(launch) = launch_config_for(&config, root, None, &env) else {
            return self.plugin_failed(operation, "This configuration has no debugger");
        };
        let waiting = Waiting::PluginSave { operation, config: Box::new(config), launch: Box::new(launch) };
        let ticket = self.wait(waiting);
        self.effects.push(Effect::SaveAllThen { ticket });
    }
```

9. In `fn observe_plugin_sessions`, replace

```rust
        for (id, stopped) in &mut self.plugin_sessions {
```

with

```rust
        for (id, (operation, stopped)) in &mut self.plugin_sessions {
```

and replace

```rust
                ended.push(PluginEvent::Exited { session: *id, code, stopped: *stopped });
```

with

```rust
                ended.push(PluginEvent::Exited { operation: operation.clone(), session: *id, code, stopped: *stopped });
```

10. In the test `tells_a_plugin_how_its_debug_session_ended`, replace

```rust
        bench.run("debug.pluginStart", Some(json!({ "config": as_arg(&server()) })), None);
```

with

```rust
        bench.run("debug.pluginStart", Some(json!({ "config": as_arg(&server()), "operation": "op-1" })), None);
```

and replace

```rust
        let exited = PluginEvent::Exited { session: DebugSessionId(42), code: Some(3), stopped: true };
```

with

```rust
        let exited = PluginEvent::Exited {
            operation: "op-1".to_owned(),
            session: DebugSessionId(42),
            code: Some(3),
            stopped: true,
        };
```

(The final `matches!(…, [PluginEvent::Failed { .. }])` compiles unchanged.) No other code names `PluginEvent`, `AfterStart::Plugin`, `Waiting::PluginPrepare`/`PluginSave` (grep of `app-gpui/src`).

### 3.8.2 `app-gpui/src/debugger/driver.rs`

1. Replace `use crate::debugger::actions::{ActiveEditor, Debugger, Dispatch, Host};` with `use crate::debugger::actions::{ActiveEditor, Debugger, Dispatch, Host, PluginEvent};`
2. In `pub enum DebugDriverEvent`, replace

```rust
    /// Bring the application window to the front.
    FocusAppWindow,
```

with

```rust
    /// Bring the application window to the front.
    FocusAppWindow,
    /// What plugins that started debug sessions are told, in order.
    PluginSessions(Vec<PluginEvent>),
```

3. In `fn pump`, replace

```rust
        // Only plugins hear these; no plugin starts sessions in this window.
        let dropped = self.debugger.update(cx, |debugger, _| debugger.take_plugin_events());
        if !dropped.is_empty() {
            tracing::debug!("debug session events for plugins: {dropped:?}");
        }
```

with

```rust
        // The plugins that started sessions hear how each went, through the window.
        let plugin_events = self.debugger.update(cx, |debugger, _| debugger.take_plugin_events());
        if !plugin_events.is_empty() {
            cx.emit(DebugDriverEvent::PluginSessions(plugin_events));
        }
```

4. In `fn call`, replace

```rust
            // A plugin's backend is not reachable from this window: the start says so.
            BackendCall::PluginPrepareDebugRun(_) => self.with_host(cx, |debugger, host| {
                let error = IdeError::unavailable("Plugin run configurations cannot be debugged in this window");
                debugger.config_prepared(ticket, Err(error), host);
            }),
```

with

```rust
            // The plugin checks its configuration's program again before it is debugged.
            BackendCall::PluginPrepareDebugRun(config) => {
                let plugins = cx.try_global::<crate::plugins::Plugins>().map(|plugins| plugins.host.clone());
                self.ask(
                    cx,
                    move |host, label| async move {
                        host.authorize_command(&label, "plugin_prepare_debug_run")?;
                        let plugins = plugins.ok_or_else(|| IdeError::unavailable("Debugger unavailable"))?;
                        plugins.prepare_debug_run(&label, *config)
                    },
                    Self::on_debugger(move |debugger, result, host| debugger.config_prepared(ticket, result, host)),
                )
            }
```

(IPC equivalent: `plugin_prepare_debug_run`, `src-tauri/src/commands/plugin.rs:1036`. This also makes Run › Debug of a plugin configuration from the chooser work: `start_with_config` asks the same call for `kind == "plugin"`, `debugger/actions.rs:859-862`.)

### 3.8.3 `app-gpui/src/workbench/debug.rs` (being edited by the compiling agent: anchors are single distinctive lines)

1. In `fn debug_driver_event`, before `            DebugDriverEvent::FocusAppWindow => window.activate_window(),` add

```rust
            DebugDriverEvent::PluginSessions(events) => self.plugin_debug_sessions(events, cx),
```

2. Test module visibility, so the plugin test reuses the fake adapter: replace

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
```

with

```rust
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
```

and replace `    const FAKE_ADAPTER: &str = r#"import json, os, sys, time` with `    pub(in crate::workbench) const FAKE_ADAPTER: &str = r#"import json, os, sys, time`.

The new `DebugDriverEvent::PluginSessions` arm is the only addition to `debug_driver_event`'s match, which is exhaustive and the only matcher of `DebugDriverEvent` outside `driver.rs`. No other code names `PluginEvent` (debugger), `AfterStart::Plugin` or `Waiting::PluginPrepare`/`PluginSave`. `DebugDriverEvent` derives `Clone`; `PluginEvent` derives `Clone`. 3.8.2.4 also makes Run › Debug of a plugin configuration from the chooser work: `start_with_config` asks the same call for `kind == "plugin"` (`debugger/actions.rs`).

### 3.9 Not built, with why

- **The JDWP branch of a plugin debug request.** `run.pluginStart` in the webview sends detectors with `debugger = "java-debug"` and `debugArgs` through `jdwpLaunch` + `debug.attach` (`src/features/run/index.ts:636-656`); `debug.attach` is not registered natively and `run/java_debug.rs`'s `AgentDebug` has no caller, so such a request ends with the model's "This configuration has no debugger". Every other debug request runs under the debugger (point c).
- **`run.pluginStart` as an action.** Not registered: the run request goes straight to the run driver (`plugin_run_request`), as the action's only caller in the webview is the run request itself (`runtimeUi.tsx:677-678`).
- **The editor's context menu** does not exist natively, so `editor.context` entries have no host. `plugin_menu_items(PluginMenu::Editor, …)` builds them; the test asserts the submenu.
- **Editor tab, Problems and Search Results context menus, and the `editor.title`, `mainToolbar`, `commit.message` slots**: no native slot takes plugin items yet; `plugin_menu_items` (`EditorTab`, `Problems`, `SearchResults`) and `plugin_slot_items` give them with their `when` (the undeclared `context_menus.rs` draft is their host).
- **Gutter icons in split panes**: only the focused editor's icons are drawn; they are drawn by the workbench over the editor (the editor's one overlay slot is code insight's), so they lag one frame behind a scroll.
- **A gutter action's failure** is only logged (`languageVisuals.ts:84`, `console.warn`).
- **Run Anything** is not registered natively, so detectors cannot appear there.
- **Multiline string settings** use the one-line field (the webview draws a textarea). A settings page opened before a plugin changes keeps the old schema until the dialog is opened again.
- **`file-saved` of a clean document**: the backend emits `DocDirty { dirty: false }` only when a dirty document was written; the webview posts it on every save.
- **`input-mode-changed`** has no native producer (Vim is not wired); `Workbench::plugin_input_mode_changed` is the carrier, the test calls it directly.

---

## The open points, resolved

**a. `ui/secret_field.rs` never paints, copies, logs or `Debug`s the secret.** Webview: `src/features/plugins/secrets.tsx:55` (`<Input type="password" …>`). The field keeps its text in an inner `TextField` that is never rendered (not a child element), so the inner field's own key handler (which writes the selection to the clipboard on `c`/`x`, `ui/text_field.rs` `key_down`) is never in the dispatch path. `SecretField::key_down` takes secondary+`c`/`x` and stops propagation; in the keymap, `Meta+C`/`Meta+X` are `project.copy`/`project.cut` with `when: projectViewFocus` (`assets/gen/keymaps.json`, `actions.json`), off in a field, and `edit.copy`/`edit.cut` have no default binding. `render` draws exactly the new `painted()` (bullets before/inside/after the selection) plus the placeholder; input methods read bullets (`text_for_range`); `Debug` is hand-written (placeholder only); `SecretFieldEvent` carries no text; `PluginsPageEvent`'s `Debug` writes `SetSecret.value` as `…` (1.2.2). Tests: `secret_field::tests::copy_and_cut_put_nothing_on_the_clipboard_and_paste_inserts` (painted parts are bullets while typed and while selected; a clipboard sentinel survives `cmd-c` and `cmd-a cmd-x`; the text survives Cut; `format!("{field:?}")` has no secret; paste works), and the workbench test `a_secret_typed_into_the_page_is_stored_through_the_host_and_never_shown_emitted_or_logged` (the same sentinel check in a real window with keymap interception, plus events' debug output and a captured `tracing` log). Pattern: `database/source_dialog.rs` `PasswordField` and its tests.

**b. No test reaches the keychain.** 1.8: `KeychainSecrets` and its `set_secret_store` call are `#[cfg(all(target_os = "macos", not(test)))]` in `app-gpui/src/plugins.rs`, the only place the store is chosen. In test builds the host keeps `FileSecretStore { dir: <temp home>/data/plugins/.data }` from `PluginHost::new` on: before `supervise` starts the load thread and before any plugin code can run. No swap, so no window; the drafts' swaps are deleted. Test: `plugins_window::tests::the_app_plugin_host_keeps_secrets_in_a_file_under_the_temporary_home`. Tests on `test_plugin_host` use `MemorySecrets` (no `Plugins::init`).

**c. A plugin's debug request goes through the real debugger.** Webview: `runtimeUi.tsx:677-678` runs `run.pluginStart { config, debug, onExit }`; `src/features/run/index.ts:652-656` hands a debug request to `debug.pluginStart` ("Debugger unavailable" when it is not registered); `src/features/debugger/actions.ts:403-424` registers `debug.pluginStart` (hidden, `when: projectOpen`): `pluginPrepareDebugRun`, `launchConfigFor`, start, `onExit(exitCode, stopped)` when the session ends. Here: `plugin_contributions.rs` registers `debug.pluginStart` (manifest entry exists in `assets/gen/actions.json`; handler `Workbench::run_debug_action`, which the model already routes to `plugin_start`), with `{ config, operation }` as its argument (the operation stands for `onExit`); a `WindowRequest::Run { debug: true }` runs it deferred (3.3.2); a failure ends the plugin's run without a status and answers a waiting request with the reason; `DebugDriverEvent::PluginSessions` (3.8) brings `Failed`/`Exited { code, stopped }` back and `plugin_debug_sessions` calls `finish_operation(op, code, stopped)`. Test: `plugin_contributions::tests::a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end` (`#[cfg(unix)]`, needs `python3`): the `execution` fixture copied into the temp home, its manifest pointed at `main.fake`; the fake adapter of `workbench/debug.rs`'s tests (`FAKE_ADAPTER`, made visible by 3.8.3) is installed as language `fakedap` under `<home>/config/languages` before `Backend::init`; the plugin's command makes the run request with `debug: true`; the test asserts the session shows in the Debug tool window and the plugin hears its run end with status 0.

**d. Linking a plugin folder.** The webview offers no link action: `pluginLink` exists only in the generated `src/bindings.ts:1093` (IPC `plugin_link`, `src-tauri/src/commands/plugin.rs:241`) and nothing under `src/` calls it (grep, this session); linked plugins come from the `openidea-plugin link` CLI and every listing runs `sync_links`, so they appear under "Linked for development" (`PluginsPage.tsx:1012`). What the webview offers for a local plugin folder is Install Plugin from Disk… (`plugins.installFromDisk`, `src/features/plugins/index.ts:93`, no `when`), which takes a folder or its `plugin.toml` (`store.ts:298-303` `packageFolder`, `:318` `installFromDisk`). Stage 1 registers it; test `install_from_disk_takes_a_plugin_folder_by_its_manifest_and_makes_its_commands_actions`: fixture `hello` copied to `<home>/fixtures/hello`, the action run with `<copy>/plugin.toml` in a trusted project window, consent accepted, then the plugin is on the host, listed on the page, and `plugin.hello.insert-date` is a registered action. No `plugins.link` action is registered.

**e. Fixtures.** `test_support::plugin_copy` copies `crates/ide-plugin/tests/fixtures/<name>` or `plugins/official/<name>` (skipping `target`, `.git`) into `<temp home>/fixtures/<name>` before any install or link; `install_from_dir` and `plugins.installFromDisk` get copies too (the draft passed the repository's `plugins/official/<name>`). The repository is only read; the catalog listing reads `plugins/official` read-only (debug builds, `catalog_dirs`). No market URL in debug builds, so no network; the stage-2 test that rewrote `hello`'s `when` to `workspaceContains:MODULE.bazel` edits only the copy (helper renamed `word_count_needs_module_file`); no test names the Bazel plugin folder or `$HOME` (the `MODULE.bazel` strings in the tests are file names written into temp projects to drive `workspaceContains:`). The only `thread::sleep` is the 10 ms step inside `settle_app`'s poll loop, like `wait_long`'s. Programs resolve in a temp `bin` (checkstyle's install test uses an empty one, never the user's `PATH`) or `/bin` for `bash`.

**f. Shortcuts.** None of the actions these stages register has a default key: `grep -c "plugins\.\|backgroundTasks\|pluginStart" app-gpui/assets/gen/keymaps.json` is 0 and `src/platform/keymaps/` names none of them (measured). The only keys taken are claims of focused views: `PluginPrompt` (Enter, Escape, arrows, Home/End, PageUp/PageDown, Cmd/Ctrl+Enter in the multi-line field) and the Background Tasks popup (Escape), through `input::claim_keys(focus, …)`, which the interceptor applies only while that focus handle is focused. Test that a key does nothing outside its condition: `prompt_dialog::tests::the_prompt_keys_do_nothing_while_the_prompt_has_no_keyboard`. Keybindings plugins contribute in their manifests are registered by the compiled `plugins.rs`, not by these stages.

## Edits made to the drafted files during this reconciliation

They are in the files on disk already; nothing to apply.

- `ui/secret_field.rs`: `pub fn painted()` added and `render` draws exactly it; the clipboard test seeds a sentinel, checks bullets while typed and selected and the `Debug` output (point a).
- `workbench/welcome_pages.rs`: `theme_select_options(&choices, Some(&view.theme))` → `Some(view.theme.as_str())` (`Option<&String>` is not `Option<&str>`: compile error); a test comment wrapped to 120 columns.
- `workbench/plugins_window.rs`: `plugins.updateAll` no longer registered (no test can run it); `test_support` is the shared harness; tests use `crate::workbench::tests::{start, wait_long, click_on, run_action, wait_for_notice}` and copies of fixtures; the three Welcome tests moved in from the old §3.9; new tests `the_app_plugin_host_keeps_secrets_in_a_file_under_the_temporary_home` (b), `install_from_disk_takes_a_plugin_folder_by_its_manifest_and_makes_its_commands_actions` (d), `reload_plugins_reports_a_plugin_that_no_longer_loads` (`plugins.reloadAll` had no test); the secret test also checks the clipboard in the real window.
- `workbench/plugin_runtime.rs`: `plugin_tabs_changed` returns at once when the window follows no plugin host; the test harness replaced by `test_support` and the workbench helpers; the keychain swap and its imports deleted; the branch test builds its repository with `init_repo`/`git_in`; `bazel_hello` renamed `word_count_needs_module_file`. The lines stage 3 anchors on are unchanged.
- `plugin_views/prompt_dialog.rs`: new test `the_prompt_keys_do_nothing_while_the_prompt_has_no_keyboard` (point f).
- `workbench/plugin_contributions.rs`: the "can't be debugged" notification replaced by `debug.pluginStart` (registered here), `plugin_debug_request`, `plugin_debug_sessions` and the `debug_requests` map; `plugin_run_request` takes `window`; unused notification/i18n imports dropped; harness replaced, swap deleted, fixtures copied; test `a_plugins_debug_request_fails_visibly_and_the_plugin_hears_it_ended` replaced by `a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end`; long lines split; a comment no longer names the Bazel plugin.
- `plugin_views/settings_page.rs`: two test lines wrapped to 120 columns.
- `plugin_views/{mod,log_tab}.rs`, `workbench/background_tasks_view.rs`: no change (every API they call matches today's code).

API check (read against the working tree): every `PluginHost`, `ide_plugin`, `ide_core`, `Host::authorize_command`, `ActionRegistry`, `Workbench`, `Editor`, `RunDriver`/`RunStore`/`RunView`, `SettingsDialog`, `menu_spec`, `Contributed`, `MenuItem` and GPUI 0.3.12 call in the drafts matches today's signature; the only mismatch found was `welcome_pages.rs`'s `Option<&String>`. The test helpers the drafts use exist with these signatures and are `pub(super)` in `workbench::tests` (reachable from any `workbench::*::tests`): `start(cx) -> TempDir`, `wait_long(cx, what, done)`, `click_on(cx, &'static str)`, `run_action(cx, id, Option<Value>)`, `wait_for_notice(workbench, title, cx)`, `notification_titles`, `notice_titled`, `git_in`, `init_repo`. `settle`, `settings_dialog_of`, `global_settings_file` are private and not used (inlined or replaced by `wait_long`), so no hunk exposes them.

## Anchors in regions the compiling agent may still move

Re-check these with `grep -nF` right before pasting: `workbench/mod.rs`: `            welcome: None,` + `        };` (end of the `Self { … }` literal, 2.2.3), `            input::window_blurred(id, cx);` (`on_release`, 2.2.4), `        self.note_active_tab(cx);` (`tabs_changed`, 2.2.6), `            EntryAction::Own(own) if own.command.starts_with("agents.openTask:") => {` (2.2.7), `                            workbench.watch_debug_editor(&editor, window, cx);` (the file-opening task, which the compiling agent just changed: 3.4.5), `            .child(notification_balloons::layer(&self.balloons, cx))` and `            .children(self.settings_layer(cx))` (the Render layer list, 1.3.5, 2.2.8, 3.4.6). `workbench/debug.rs` (uncommitted edits): `            DebugDriverEvent::FocusAppWindow => window.activate_window(),`, the `#[cfg(test)]\nmod tests {` header and `    const FAKE_ADAPTER: &str = r#"import json, os, sys, time` (3.8.3; the test region is the most likely to move). The other anchors sit in files nobody is editing (`plugins_page.rs`, `welcome.rs`, `settings_window.rs`, `consent.rs`, `plugins.rs`, `menu.rs`, `project_files.rs`, `tab_views.rs`, `chrome.rs`, `lib.rs`, `debugger/`).

## Assumptions to check, in order

1. `test_support` reaching `crate::workbench::tests::{start, wait_long}` from `plugins_window::test_support` (a `pub(super)` item of a private module, used from a descendant of its parent): first compile of stage 1.
2. `Entity<CustomizePage>::into_any_element()` in `welcome.rs` 1.4.8 (needs `IntoElement` for `Entity<V: Render>`).
3. `Plugins::init` in a test builds the host on `<temp home>/data/plugins` and, with 1.8, `secrets.json` appears under `.data/<id>/` (stage-1 test b).
4. A linked, granted plugin's commands are projected as actions while it is Inactive (`link` waits for it; every later test depends on it).
5. `preview_install` of a new plugin with required capabilities asks for consent (the install tests); `install_from_dir` of `checkstyle` succeeds with `process:checkstyle` unresolved; `reload` of a linked plugin whose manifest no longer parses returns an error.
6. In a workbench window `cmd-a`/`cmd-c`/`cmd-x` reach the focused `SecretField`, and Enter reaches it.
7. `cx.entity()`/`observe_in(&cx.entity(), …)` inside `Workbench::new` (stage 2's runtime start) works and does not loop.
8. `IdeEvent::FsChanged`, `VcsChanged`, `SettingsChanged`, `TerminalExited`, `DocDirty` reach `Backend::listen` with the window's label (or none).
9. `plugins.api14.enabled` (and a plugin settings page's `plugins.<id>.<key>`) is accepted by `platform::settings::set`.
10. The `execution` detector of the copied fixture picks the fake adapter (its first argument `main.fake` is a file of a language with a debugger, `crates/ide-host/src/plugins/api.rs` run configuration), the session's tab is titled with the configuration's name, and the adapter's `exitCode: 0` becomes `exit_code: Some(0)`; `prepare_debug_run` (blocking checks) is fine on the backend runtime through `Backend::with_host`.
11. `RunDriver::start` of a `framework = "plugin"` configuration shows the program's raw output in `RunView::lines`.
12. The host lists `[[contributes.menus]]` in declaration order; a deferred, occluding gutter icon gets the press before the editor's mouse-down; `Window::viewport_size` is the content size in the test platform; `cx.defer_in` after `SettingsDialogView::apply` runs after the dialog's `Apply` event.
13. The old notes' remaining assumptions: `Keystroke` parsing of the typed letters, tracing capture on the test thread, `tracing_subscriber::fmt::MakeWriter` path, `CapabilityInfo` serde (camelCase), the details fitting the default test window, `AnyWindowHandle::update` from the pump, nested `track_focus` on the Welcome pages, the Dark theme value `"dark"`.

## Compile risks

- Stage 1: the `selection` `SharedString` moved into the second `.when` closure of `SecretField::render`; closures with an annotated `&mut VisualTestContext` parameter passed as `impl Fn`; `Entity<CustomizePage>` as an element.
- Stage 2: the brace lists of the `test_support` imports (rustfmt may reorder); `Tasks::cancel_task` is `#[must_use]` (used).
- Stage 3: 3.3.2's first replacement line is about 118 columns; `let Err(error) = execute_action_or_throw(…) else { return };` on `Result<(), ActionError>`; `debug::tests` made `pub(super)` and `FAKE_ADAPTER` `pub(in crate::workbench)`; 3.8.1 item 7's second line is 117 columns; 3.8.2.4 moves `config: Box<RunConfig>` into an `async move` and derefs it; `crate::plugins::Plugins` used from `debugger/driver.rs` (new dependency direction inside the crate, no cycle at the type level).
- The undeclared drafts `workbench/context_menus.rs` and `project_view_actions.rs` still swap the secret store / call `with_menu_context`; they are not declared, so they do not compile now, but their notes need 1.8 and 3.6 when they land.

## Tests most likely to fail first

1. `install_from_disk_takes_a_plugin_folder_by_its_manifest_and_makes_its_commands_actions` and `installing_from_disk_asks_for_consent_with_high_risk_first_and_installs_only_when_accepted`: assumption 5, and `CommandsChanged` must reach `Plugins::refresh` (the load thread flushes every 250 ms real time).
2. `a_secret_typed_into_the_page_is_stored_through_the_host_and_never_shown_emitted_or_logged`: assumption 6 and the details fitting the window.
3. `a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end`: needs `python3`, assumption 10, and the languages folder written before `Backend::init`.
4. `the_focused_editor_and_a_checkout_reach_the_plugin_which_shows_the_branch`: the plugin is idle, so events wait the host's idle batch (seconds of real time); the checkout must arrive as `VcsChanged` for `w1`.
5. `a_changed_plugin_setting_reaches_the_plugin_as_settings_changed` and `a_plugins_settings_page_saves_its_value_and_the_plugin_hears_the_change`: assumption 9 and the host's diff of the plugin's settings.
6. `a_plugins_task_is_listed_while_it_runs_and_cancel_stops_it`, `cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it`: the fake `checkstyle` must be approved at grant time and the task addressed to `w1`.
7. `starts_a_tab_opened_by_plugin_code_by_its_key_with_the_toggles_that_are_on`: a real PTY running `/bin/bash`; output with `\r\n` and colour codes.
8. `a_plugins_run_detectors_are_listed_in_the_configuration_chooser`, `focusing_an_editor_fires_the_language_and_file_activation_events`, `reports_actions_to_action_performed_plugins`: activation timing and idle batches.
9. `renders_owned_gutter_commands_and_drops_answers_to_superseded_buffers`, `puts_the_actions_of_one_line_behind_one_icon_whose_click_opens_a_menu`: the icon exists only after the editor painted once.

## Actions: end-to-end test, IPC commands, expected parity

| action id                                                                                    | end-to-end test                                                                                                                                                                                                                                                   | IPC commands it stands for                                                                                                                                                                                                                                            | expected parity                                                                                                                                                                 |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `plugins.installFromDisk`                                                                    | `installing_from_disk_asks_for_consent_with_high_risk_first_and_installs_only_when_accepted`, `install_from_disk_takes_a_plugin_folder_by_its_manifest_and_makes_its_commands_actions`                                                                            | `plugin_preview_install`, `plugin_install_dir`, `plugin_unpack_package`, `plugin_list`                                                                                                                                                                                | partial: the picker has no title; folder, `plugin.toml`, package, consent and install done                                                                                      |
| `plugins.reloadAll`                                                                          | `reload_plugins_reports_a_plugin_that_no_longer_loads`                                                                                                                                                                                                            | `plugin_list`, `plugin_reload`                                                                                                                                                                                                                                        | done                                                                                                                                                                            |
| `plugins.updateAll`                                                                          | none                                                                                                                                                                                                                                                              | `plugin_market`, `plugin_market_download`, `plugin_install_dir`                                                                                                                                                                                                       | missing: not registered (needs a marketplace index in a test)                                                                                                                   |
| `plugins.settings` / `file.settings` (already registered)                                    | `all_settings_on_customize_opens_the_settings_dialog`, existing `settings_for_new_projects_and_plugins_open_on_their_pages`                                                                                                                                       | `settings_get`, then the page operations below                                                                                                                                                                                                                        | done                                                                                                                                                                            |
| Plugins page operations (no id): enable, uninstall, secrets                                  | `the_rows_checkbox_disables_and_enables_the_plugin`, `uninstall_asks_and_removes_the_plugin`, `a_secret_typed_into_the_page_is_stored_through_the_host_and_never_shown_emitted_or_logged`, `the_app_plugin_host_keeps_secrets_in_a_file_under_the_temporary_home` | `plugin_set_enabled`, `plugin_uninstall`, `plugin_secrets`, `plugin_set_secret`                                                                                                                                                                                       | done; grant, capability, roll back, market download, repository prompt carried out but run by no test                                                                           |
| Welcome Plugins / Customize (no id)                                                          | the three Welcome tests                                                                                                                                                                                                                                           | `settings_set` (global)                                                                                                                                                                                                                                               | partial: IntelliJ theme imports not built                                                                                                                                       |
| `plugins.log.clear`                                                                          | `starts_a_tab_opened_by_plugin_code_by_its_key_with_the_toggles_that_are_on`                                                                                                                                                                                      | none                                                                                                                                                                                                                                                                  | done                                                                                                                                                                            |
| `plugins.log.wrap`                                                                           | same                                                                                                                                                                                                                                                              | none                                                                                                                                                                                                                                                                  | partial: soft wrap starts on; `editor.softWrap` and its patterns not read                                                                                                       |
| `plugins.log.toggleArgument`                                                                 | same                                                                                                                                                                                                                                                              | `plugin_log_tab_spawn`                                                                                                                                                                                                                                                | done                                                                                                                                                                            |
| `plugins.log.find`                                                                           | none                                                                                                                                                                                                                                                              | none                                                                                                                                                                                                                                                                  | missing: Find in Logs (the find bar works on `Editor` only)                                                                                                                     |
| `plugin.<id>.<cmd>` from status items, notification buttons, host requests                   | `a_declared_status_item_shows_and_a_click_runs_its_plugin_command`, `runs_notification_buttons_as_commands_of_the_publishing_plugin_with_their_arguments`                                                                                                         | `plugin_run_command`, `plugin_answer_command`                                                                                                                                                                                                                         | done                                                                                                                                                                            |
| runtime (no id): status items, context keys, tasks, questions, files, clipboard, URL, events | the 11 tests of `plugin_runtime.rs`                                                                                                                                                                                                                               | `plugin_subscribe`, `plugin_status_items`, `plugin_context_keys`, `plugin_event_kinds`, `plugin_post_events`, `plugin_set_active_file`, `plugin_cancel_task`, `cancel_task`, `plugin_answer_prompt`, `plugin_answer_commit`, `plugin_finish_operation`, `fs_read_dir` | partial: `OpenTerminal`, plugin views, web views and content tabs not shown                                                                                                     |
| `view.backgroundTasks`                                                                       | `cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it`                                                                                                                                                                                              | `plugin_cancel_task` / `cancel_task`                                                                                                                                                                                                                                  | done                                                                                                                                                                            |
| `debug.pluginStart`                                                                          | `a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end`                                                                                                                                                                                    | `plugin_prepare_debug_run`, `debug_start` and session events, `plugin_finish_operation`, `plugin_answer_command`                                                                                                                                                      | partial: the JDWP branch (`debugger = "java-debug"` with `debugArgs`, `debug.attach`) not built; a waiting request is answered when the session ends rather than when it starts |
| `run.pluginStart`                                                                            | not registered; `a_plugins_run_shows_its_output_in_the_run_tool_window_and_the_plugin_hears_its_exit` covers the run request                                                                                                                                      | `run_start` path (`RunDriver::start`), `plugin_finish_operation`                                                                                                                                                                                                      | partial: no action id                                                                                                                                                           |
| `plugin.<id>.<cmd>` from the Tools menu and the Project view menu                            | `the_tools_menu_lists_plugin_entries_with_their_state_and_runs_the_chosen_one`, `the_context_menus_offer_plugin_entries_where_their_when_holds_and_run_them`                                                                                                      | `plugin_run_command`                                                                                                                                                                                                                                                  | partial: editor, editor tab, Problems, Search Results menus and toolbar slots have no native host yet                                                                           |
| gutter actions (no id)                                                                       | the two gutter tests                                                                                                                                                                                                                                              | `plugin_language_visuals`, `lsp_execute_command`                                                                                                                                                                                                                      | partial: focused editor only                                                                                                                                                    |
| settings pages (no id)                                                                       | `a_plugins_settings_page_saves_its_value_and_the_plugin_hears_the_change`, `applies_pending_values_before_a_command_reads_them_and_shows_command_errors_inline`                                                                                                   | `settings_set`, `plugin_run_command`                                                                                                                                                                                                                                  | partial: multiline strings use one line                                                                                                                                         |
| events (no id): `file-saved`, `input-mode-changed`, activation                               | `a_saved_file_reaches_the_plugin_that_watches_its_saves`, `an_input_mode_change_reaches_the_plugin_that_wants_it`, `focusing_an_editor_fires_the_language_and_file_activation_events`, `reports_actions_to_action_performed_plugins`                              | `plugin_post_events`, `plugin_activation_event`                                                                                                                                                                                                                       | partial: `file-saved` only after a dirty save; no native producer of input-mode changes                                                                                         |
