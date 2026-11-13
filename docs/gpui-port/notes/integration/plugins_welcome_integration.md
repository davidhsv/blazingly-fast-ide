# Integrating the Plugins page, the masked secret field and the Welcome screen's pages

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Paths below are relative to `app-gpui/src/`. Nothing here was compiled.

New, not compiled: `ui/secret_field.rs` (4 tests), `workbench/plugins_window.rs` (4 tests plus `test_plugin_host` and `test_support` for other test modules). Already written, not compiled: `workbench/welcome_pages.rs` (unchanged by this draft).

Order: apply §1 to §6 in one go, then build. `plugins_window.rs` uses symbols that only exist after the edits: `Workbench::plugin_ui` (§3.2), `FieldRule::Any` (§5), `Welcome::set_plugins_page` (§4), `PluginsPage::secret_input` and the hand-written `Debug` of `PluginsPageEvent` (§2), `sync_customize` (§3.8). `settings_window.rs` (§6) calls `plugin_host`, `new_plugins_page` and `follow_settings_on_plugin_pages` from `plugins_window.rs`.

## 1. The `mod` lines

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

## 2. `plugin_models/plugins_page.rs`: the masked field, secret rows drawn at once, redacted `Debug`

### 2.1 Import

After `use crate::ui::text_field::{TextField, TextFieldEvent};` add:

```rust
use crate::ui::secret_field::{SecretField, SecretFieldEvent};
```

(`TextField` stays: the two search fields use it.)

### 2.2 The event's `Debug` never writes a secret

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

### 2.3 The field type

Replace

```rust
    secret_fields: HashMap<(String, String, bool), Entity<TextField>>,
```

with

```rust
    secret_fields: HashMap<(String, String, bool), Entity<SecretField>>,
```

`secret_saved`'s `field.update(cx, |field, cx| field.set_text("", cx));` compiles unchanged (`SecretField::set_text(&mut self, &str, &mut Context<Self>)`).

### 2.4 Secret rows are drawn as soon as the plugin is listed

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

### 2.5 The field itself

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

### 2.6 `render_secret` reads the field's text through the app

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

### 2.7 An accessor for tests

After `pub fn search_field(&self) -> &Entity<TextField> { … }` add:

```rust
    /// The masked field of secret `name` of `plugin_id`, while it is drawn for the state `set`.
    pub fn secret_input(&self, plugin_id: &str, name: &str, set: bool) -> Option<&Entity<SecretField>> {
        self.secret_fields.get(&(plugin_id.to_owned(), name.to_owned(), set))
    }
```

### 2.8 Tests in `plugins_page.rs`

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

## 3. `workbench/mod.rs`

### 3.1 A constant

After `const WINDOW_BUTTONS_WIDTH: f32 = 82.0;` add:

```rust

/// Where the reopen-last-project preference of this machine is kept (`"false"`: off).
const REOPEN_PREFERENCE: &str = "openidea.welcome.reopenLastProject";
```

### 3.2 The field

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

### 3.3 Its initialiser, the listener and its release (in `Workbench::new`)

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

### 3.4 The actions

Replace

```rust
    settings_window::register_actions(registry);
```

with

```rust
    settings_window::register_actions(registry);
    plugins_window::register_actions(registry);
```

### 3.5 The layer, over Settings

Replace

```rust
            .children(self.settings_layer(cx))
```

with

```rust
            .children(self.settings_layer(cx))
            .children(self.plugins_dialog_layer())
```

### 3.6 `sync_welcome`: the Welcome screen's Plugins page and Customize settings

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

### 3.7 `welcome_event`: the Customize arm

In `fn welcome_event`, replace

```rust
            WelcomeEvent::Reveal(path) => cx.reveal_path(&path),
```

with

```rust
            WelcomeEvent::Reveal(path) => cx.reveal_path(&path),
            WelcomeEvent::Customize(event) => self.customize_event(event, window, cx),
```

### 3.8 Two methods, right after `fn welcome_event` (before `pub fn open_project`)

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

### 3.9 Tests to paste into `mod tests` of `workbench/mod.rs` (after `settings_for_new_projects_and_plugins_open_on_their_pages`)

They use the module's `start`, `settle`, `click_on`, `settings_dialog_of` and `global_settings_file`.

```rust
    /// The Welcome screen lists Projects and Customize, and Plugins only once the window has a plugin
    /// host; Plugins then shows the Plugins page in place of the projects, and Projects brings them back.
    #[gpui_kit::test]
    async fn the_welcome_screen_lists_projects_and_customize_and_plugins_only_with_a_plugin_host(
        cx: &mut TestAppContext,
    ) {
        use welcome_pages::WelcomePage;
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let welcome = workbench.read_with(cx, |w, _| w.welcome().cloned()).expect("the Welcome screen shows");
        let pages = |cx: &mut VisualTestContext| welcome.read_with(cx, |view, _| view.nav().pages());
        assert_eq!(pages(cx), [WelcomePage::Projects, WelcomePage::Customize]);
        assert!(cx.debug_bounds("welcome-nav-projects").is_some());
        assert!(cx.debug_bounds("welcome-nav-customize").is_some());
        assert!(cx.debug_bounds("welcome-nav-plugins").is_none(), "no plugin host, no Plugins");

        let plugins = tempfile::tempdir().expect("a folder for the plugins");
        let (host, _) = plugins_window::test_plugin_host(plugins.path());
        workbench.update_in(cx, |w, window, cx| w.use_plugin_host(host, window, cx));
        assert_eq!(pages(cx), [WelcomePage::Projects, WelcomePage::Customize, WelcomePage::Plugins]);
        click_on(cx, "welcome-nav-plugins");
        assert_eq!(welcome.read_with(cx, |view, _| view.nav().shown()), WelcomePage::Plugins);
        assert!(cx.debug_bounds("plugins-tab-installed").is_some(), "the Plugins page is embedded");
        assert!(cx.debug_bounds("welcome-brand").is_some(), "beside the sidebar");
        click_on(cx, "welcome-nav-projects");
        assert_eq!(welcome.read_with(cx, |view, _| view.nav().shown()), WelcomePage::Projects);
        assert!(cx.debug_bounds("plugins-tab-installed").is_none());
    }

    /// Customize › Theme and IDE font write the global settings: the theme chosen is the one the
    /// settings name and the windows are drawn with, and the font size is stored with "Use custom
    /// font" on. The page then shows what is stored.
    #[gpui_kit::test]
    async fn customize_changes_the_theme_and_the_font_size_through_the_global_settings(cx: &mut TestAppContext) {
        use welcome_pages::CustomizeSelect;
        let home = start(cx);
        cx.update(crate::theme::appearance::init);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        click_on(cx, "welcome-nav-customize");
        let welcome = workbench.read_with(cx, |w, _| w.welcome().cloned()).expect("the Welcome screen shows");
        let customize = welcome.read_with(cx, |view, _| view.customize().clone());
        assert!(!cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));

        click_on(cx, "customize-theme");
        let choices = customize.update(cx, |page, cx| page.choices(CustomizeSelect::Theme, cx));
        let dark = choices.iter().position(|choice| !choice.heading && choice.value == "dark").expect("Dark is offered");
        let option: &'static str = Box::leak(format!("customize-option-{dark}").into_boxed_str());
        click_on(cx, option);
        assert_eq!(cx.update(|_, cx| crate::platform::settings::global(cx).appearance.theme), "dark");
        settle(cx, |cx| cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));

        click_on(cx, "customize-font-size");
        // The sizes start at 10: the sixth is 15.
        click_on(cx, "customize-option-5");
        let appearance = cx.update(|_, cx| crate::platform::settings::global(cx).appearance);
        assert_eq!((appearance.ui_font_size, appearance.use_custom_font), (15.0, true));
        let stored = global_settings_file(&home);
        assert!(stored.contains("uiFontSize") && stored.contains("useCustomFont = true"), "{stored}");
        let shown = customize.read_with(cx, |page, _| page.view());
        assert_eq!((shown.theme.as_str(), shown.font_size), ("dark", 15.0), "the page shows what is stored");
    }

    /// Customize › All settings… opens the Settings dialog over the Welcome screen.
    #[gpui_kit::test]
    async fn all_settings_on_customize_opens_the_settings_dialog(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        click_on(cx, "welcome-nav-customize");
        assert!(settings_dialog_of(&workbench, cx).is_none());
        click_on(cx, "customize-all-settings");
        cx.run_until_parked();
        assert!(settings_dialog_of(&workbench, cx).is_some(), "All settings… opens Settings");
        assert!(workbench.read_with(cx, |w, _| w.welcome().is_some()), "over the Welcome screen");
    }
```

## 4. `workbench/welcome.rs`: the navigation, Customize and Plugins

### 4.1 Imports

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

### 4.2 The event

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

### 4.3 The fields

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

### 4.4 `Welcome::new`

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

### 4.5 Methods, after `pub fn with_version`

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

### 4.6 The keyboard follows only the Projects page

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

### 4.7 The sidebar lists the pages

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

### 4.8 `render` shows the page selected

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

## 5. `vcs/prompt_dialogs.rs`: a prompt that accepts an empty value

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

## 6. `workbench/settings_window.rs`: hand the Plugins page to `plugins_window.rs`

### 6.1 Imports

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

### 6.2 Constants

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

### 6.3 Functions that moved to `plugins_window.rs`

Delete the free functions `catalog_dirs` (with its doc comment), `plugin_work` (with its doc comment) and `event_name` (with its doc comment). Keep `store_ssh_secret`.

### 6.4 `open_settings` makes the page through `plugins_window.rs`

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

### 6.5 `settings_changed`

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

and update its doc comment's last sentence to: "the keymap follows the keymap settings, the Plugins pages the theme and the file icon theme, and the Welcome screen's Customize page every setting."

### 6.6 The Plugins page section

Delete everything from

```rust
    // ---------------------------------------------------------------------------------------------
    // The Plugins page
```

through the end of `fn answer_plugins_page` (its closing `    }`), leaving the `impl Workbench` block's closing `}` and `#[cfg(test)] mod tests` in place. The deleted methods (`follow_plugins_settings`, `plugins_page_event`, `list_plugins`, `read_plugin_catalog`, `read_plugin_market`, `answer_plugins_page`) are in `plugins_window.rs` now, with the host passed in instead of read from the global.

## 7. `plugin_models/consent.rs`: high risk first

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

## 8. What `plugins_window.rs` carries out

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

Actions (none has a `when` in `assets/gen/actions.json`): `plugins.installFromDisk` (string argument: a folder, its `plugin.toml` or a package; none: the picker) and `plugins.updateAll` drive Settings' page while it is open, else the Welcome screen's, else a page made for them and kept (never drawn: its consent dialog and notifications still show). `plugins.reloadAll` reloads every listed plugin on the host, reports each failure as "Could not reload <name>" and lists the pages again.

Types that carry a secret and have a hand-written `Debug`: `PluginsPageEvent` (§2.2: `SetSecret.value` shows as `…`) and `SecretField` (writes only its placeholder). Nothing in `plugins_window.rs` derives `Debug` over a secret; its log lines name the event's kind (`event_name`) or a plugin id, never a field of `SetSecret`. `SettingsWrite`/`staging::Secret` are unchanged.

## 9. How `database/source_dialog.rs` would use `SecretField` (not done; that file is not compiled)

1. Delete the `// ---- password field ----` section (`PasswordField`, `BULLET`, its `utf16_len`, its `Focusable`, `EntityInputHandler` and `Render` impls).
2. `use crate::ui::secret_field::{SecretField, SecretFieldEvent};`; drop `ElementInputHandler`, `EntityInputHandler`, `UTF16Selection`, `Bounds`, `Pixels`, `Point`, `canvas` and `std::ops::Range` from its imports if nothing else uses them.
3. `password: Entity<PasswordField>` / `ssh_secret: Entity<PasswordField>` become `Entity<SecretField>`; `cx.new(PasswordField::new)` becomes `cx.new(SecretField::new)`; `set_placeholder` exists with the same signature.
4. Its subscriptions match `SecretFieldEvent::{Changed, Submitted, Cancelled}` instead of `TextFieldEvent`.
5. `password_field().read(cx).text(cx)` keeps its form (same signature); `display(cx)` becomes `shown(cx)`, also in `shows_the_password_as_one_bullet_per_character`.

## 10. Not wired

- **IntelliJ imports.** `theme/icls.rs` imports editor colour schemes (`.icls`); nothing in `app-gpui/src` converts a JetBrains theme into a plugin (the webview's `intellijTheme.ts`), and `ide_host::plugins::intellij::{installed_themes, read_source}` only list and read. `ImportInstalledIntellij` / `ImportIntellijFile` are logged, `IntellijThemesDialog` is never shown, and the actions `plugins.importIntellijTheme` / `plugins.importInstalledIntellijTheme` are not registered.
- The Plugin Repository prompt has no placeholder ("https://…/index.json or a folder path") and no hint ("Leave empty to use the default repository."): `PromptRequest` has no fields for them.
- The Install picker has no title (`prompt: None`), as the webview's own picker title is not known here.
- Pages are not listed again when a project opens or its trust changes (only on `CommandsChanged`, after their own operations and on Rescan); `follow_settings_on_plugin_pages` runs on `appearance` changes only.
- `ProgramPermission` (choose or reset the program of a `process:*` capability) and the rest of the earlier report's "Not built" list are unchanged.
- New user-visible strings that need entries in `src/platform/locales/text/en.json` and `zh.json` (off limits here): "Plugin Repository", "Marketplace index URL:", "Save" (likely present), "Cannot save setting" (used by the earlier Welcome report as well).

## 11. Assumptions not confirmed by reading

1. `Keystroke::parse("é")` and the letters of `simulate_input` work as in `source_dialog.rs`'s tests; a `-` in a typed string would read as a separator, so test secrets are letters and digits. Check: `gpui-pre-0.3.12/src/platform/keystroke.rs`.
2. GPUI test background tasks run on the test thread, so `tracing::subscriber::set_default` captures the host's work in `a_secret_typed_into_the_page_is_stored_through_the_host_and_never_shown_emitted_or_logged`. If they run on other threads the log assertion still holds but proves less. Check: `gpui-pre-0.3.12/src/platform/test/dispatcher.rs`.
3. `tracing_subscriber::fmt::MakeWriter` is the re-exported path and `with_ansi` exists with the crate's features (`env-filter`, `fmt`). Check: `~/.cargo/registry/src/*/tracing-subscriber-0.3*/src/fmt/mod.rs`.
4. `CapabilityInfo` serialises as `{id, risk: "low"|"medium"|"high", description, optional, granted}` (camelCase), which `Capability` deserialises: `crates/ide-extension/src/capability.rs:322` says `rename_all = "camelCase"` on both `CapabilityInfo` and `Risk`. Used for `ConsentRequested`.
5. The secret rows and the details fit in the default test window (no scroll needed for `plugins-secret-field-api-key` in `ai-completion`'s details, nor for `plugins-details-uninstall`). Check by running; if clipped, scroll `plugins-details` or enlarge the window.
6. `preview_install` and `install_from_dir` accept `plugins/official/checkstyle` unsigned (no `signature_problem`) without its `checkstyle` program installed: the ide-host test installs it after only setting a program path. Check: `crates/ide-host/src/plugins/mod.rs::signature_problem`.
7. `PluginsPage`'s Uninstall button is offered for an installed (not dev) plugin and its confirm button is drawn after the first click (`DetailButton::{Uninstall, ConfirmUninstall}`).
8. `AnyWindowHandle::update` can be called from inside `Plugins::pump`'s `cx.update` (no window is being updated at that point).
9. `Welcome`'s root `track_focus(&self.focus)` on the Customize and Plugins pages does not take the keyboard away from the fields inside them (nested `track_focus`, as on the landing page).
10. The CustomizePage theme choice value for Islands Dark is `"dark"` (the settings test's `settings-choice-appearance.theme-dark`).
11. `lib.rs::init` runs `Plugins::init` before the first `Workbench::new` (read: `lib.rs:60`, `main.rs:33`); the §3.6 branch covers a host that starts later.
12. Enter reaches the focused `SecretField` inside a workbench window (no keymap binding takes `enter` in the `TextField` key context first). If the secret test stalls after `simulate_keystrokes("enter")`, click `plugins-secret-save-api-key` instead: the same `save_secret` path.

## 12. Notes for the integrator

- **§2.4 reverses the last integrator's secrets fix on purpose.** They hid secret rows until `LoadSecrets` answered because the value field was a plain `TextField` that showed what was typed (`shows_no_secret_field_while_the_secrets_are_not_read`). With `SecretField` the field never paints, copies or logs the value, so the rows are drawn at once as the webview draws them (`src/features/plugins/secrets.tsx`: password fields before the state loads, "Not set" until then), and that test is replaced by `draws_a_masked_field_for_each_declared_secret_before_its_state_is_read`. If you would rather keep the rows hidden, skip §2.4 and keep the old test; nothing else depends on it.
- **Registering `plugins.installFromDisk` makes it appear in the Welcome screen's Manage menu** (`welcome_model::manage_menu` lists it; menus drop unregistered actions). The only test that lists those ids is the model's own `manage_menu` test, which lists them unconditionally, so no expected item changes. No other menu in `app-gpui/src` names the three actions.
- **Carried out but run by no test:** `Grant`, `SetCapability`, `RollBack`, `DownloadMarket`, `EditRepository` (the prompt and `FieldRule::Any`), `Install(Pick)` and `Install(Unpack)`, `RunCommand`, `SetTheme`, `SelectIconTheme`, `OpenUrl`, `plugins.updateAll`, `plugins.reloadAll`, the background page, and the host listener (`CommandsChanged`, `ConsentRequested`). `SetEnabled`, `Uninstall`, `AskConsent`, `Install(Preview)`, `Install(Install)`, `LoadSecrets`, `SetSecret`, `Notify` (cancel path), `Refresh` and `plugins.installFromDisk` are run by the four `plugins_window` tests.
