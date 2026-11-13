//! The welcome screen's pages beside Projects: the navigation that lists them (Plugins only while
//! the plugins settings page exists, embedded as it is), and Customize, the quick settings: colour
//! theme with Sync with OS, accent colour, IDE font size, keymap, reopening the last project, and
//! the links to Import Settings…, the keymap and all settings.
//!
//! Customize writes nothing itself: it shows the effective settings it is given and reports the
//! global settings writes as a [`CustomizeEvent`]. Until the settings written come back, the values
//! just chosen show.

use crate::platform::i18n::Localization;
use crate::platform::keymap::{PRESET_TITLES, default_preset, is_mac_host};
use crate::platform::settings::SettingsWrite;
use crate::settings_model::schema::get_at;
use crate::theme;
use crate::theme::appearance::ThemeState;
use crate::theme::themes::{
    ACCENT_PRESETS, DEFAULT_ACCENT, SelectOption, is_theme_choice_id, theme_choices, theme_select_options,
};
use crate::theme::tokens::parse_color;
use crate::ui::tooltip::WithTooltip;
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    MouseButton, Render, Rgba, SharedString, Window, deferred, div, prelude::*, px,
};
use serde_json::{Value, json};

/// The IDE font size without "Use custom font".
pub const DEFAULT_UI_FONT_SIZE: f64 = 13.0;
/// The IDE font family without "Use custom font".
pub const DEFAULT_UI_FONT: &str = "Inter";
/// The sizes Customize offers.
pub const UI_FONT_SIZES: [u32; 9] = [10, 11, 12, 13, 14, 15, 16, 17, 18];

/// The action Import Settings… runs.
pub const IMPORT_SETTINGS: &str = "file.importSettings";
/// The action Configure… beside the keymap runs.
pub const CONFIGURE_KEYMAP: &str = "settings.keymap";
/// The action All settings… runs.
pub const ALL_SETTINGS: &str = "file.settings";

const SELECT_WIDTH: f32 = 240.0;
const FIELD_HEIGHT: f32 = 28.0;
const LABEL_WIDTH: f32 = 120.0;

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

// --- the navigation -------------------------------------------------------------------------

/// A page of the welcome screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WelcomePage {
    Projects,
    Customize,
    Plugins,
}

impl WelcomePage {
    /// Its name in the navigation, in English.
    pub fn label(self) -> &'static str {
        match self {
            WelcomePage::Projects => "Projects",
            WelcomePage::Customize => "Customize",
            WelcomePage::Plugins => "Plugins",
        }
    }

    /// What tests click.
    pub fn selector(self) -> &'static str {
        match self {
            WelcomePage::Projects => "welcome-nav-projects",
            WelcomePage::Customize => "welcome-nav-customize",
            WelcomePage::Plugins => "welcome-nav-plugins",
        }
    }
}

/// Which page the welcome screen shows, and the Plugins page while the plugins settings page exists.
#[derive(Default)]
pub struct WelcomeNav {
    selected: Option<WelcomePage>,
    plugins: Option<AnyView>,
}

impl WelcomeNav {
    /// The plugins settings page to embed (`None`: there is none, and Plugins is not listed).
    pub fn set_plugins_page(&mut self, page: Option<AnyView>) {
        self.plugins = page;
    }

    /// The pages listed, in order.
    pub fn pages(&self) -> Vec<WelcomePage> {
        let mut pages = vec![WelcomePage::Projects, WelcomePage::Customize];
        if self.plugins.is_some() {
            pages.push(WelcomePage::Plugins);
        }
        pages
    }

    /// The page shown: the one selected, Projects while Plugins is selected without a plugins page.
    pub fn shown(&self) -> WelcomePage {
        match self.selected {
            Some(WelcomePage::Plugins) if self.plugins.is_none() => WelcomePage::Projects,
            Some(page) => page,
            None => WelcomePage::Projects,
        }
    }

    /// Selects `page`; false when it is not listed.
    pub fn select(&mut self, page: WelcomePage) -> bool {
        if !self.pages().contains(&page) {
            return false;
        }
        self.selected = Some(page);
        true
    }

    /// The plugins settings page, while Plugins shows.
    pub fn plugins_page(&self) -> Option<&AnyView> {
        (self.shown() == WelcomePage::Plugins).then_some(self.plugins.as_ref()).flatten()
    }
}

/// The navigation rows for the welcome screen's sidebar: the shown page highlighted, a click
/// calling `on_select` on the view that draws them.
pub fn nav_rows<V: 'static>(
    nav: &WelcomeNav,
    on_select: impl Fn(&mut V, WelcomePage, &mut Window, &mut Context<V>) + Clone + 'static,
    cx: &mut Context<V>,
) -> Div {
    let shown = nav.shown();
    let (strong, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-secondary"));
    let (selected, hover) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
    let rows: Vec<AnyElement> = nav
        .pages()
        .into_iter()
        .map(|page| {
            let label = tx(cx, page.label());
            let on_select = on_select.clone();
            div()
                .id(page.selector())
                .debug_selector(move || page.selector().to_owned())
                .h(px(34.0))
                .px(px(24.0))
                .flex()
                .items_center()
                .rounded(theme::length(cx, "radius-selection"))
                .when(page == shown, |el| el.bg(selected).text_color(strong))
                .when(page != shown, |el| el.text_color(muted).hover(|el| el.bg(hover)))
                .child(SharedString::from(label))
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| on_select(view, page, window, cx)))
                .into_any_element()
        })
        .collect();
    div().flex().flex_col().gap(px(2.0)).children(rows)
}

// --- Customize ------------------------------------------------------------------------------

/// What Customize shows.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomizeView {
    /// A theme setting value (`light`, `dark`, `plugin:<plugin>/<theme>` …).
    pub theme: String,
    pub sync_with_os: bool,
    /// Lower case.
    pub accent: String,
    pub font_size: f64,
    pub keymap: String,
    pub reopen: bool,
}

/// The values Customize shows from the effective settings, partial or written by an older version:
/// an unknown theme (the legacy `system` among them) is `light`, `system` means Sync with OS, the
/// font size counts only with "Use custom font", and the keymap defaults to the OS's preset.
/// Reopening follows `general.reopenLastProject`, else the preference kept on this machine.
pub fn customize_view(settings: Option<&Value>, mac: bool, reopen_preference: bool) -> CustomizeView {
    let at = |key: &str| settings.and_then(|settings| get_at(settings, key));
    let theme = at("appearance.theme").and_then(Value::as_str);
    let custom = at("appearance.useCustomFont").and_then(Value::as_bool) == Some(true);
    let size = at("appearance.uiFontSize").and_then(Value::as_f64).unwrap_or(DEFAULT_UI_FONT_SIZE);
    CustomizeView {
        theme: theme.filter(|theme| is_theme_choice_id(theme)).unwrap_or("light").to_owned(),
        sync_with_os: at("appearance.syncWithOs").and_then(Value::as_bool) == Some(true) || theme == Some("system"),
        accent: at("appearance.accent")
            .and_then(Value::as_str)
            .map_or_else(|| DEFAULT_ACCENT.to_owned(), str::to_lowercase),
        font_size: if custom { size } else { DEFAULT_UI_FONT_SIZE },
        keymap: at("keymap")
            .and_then(Value::as_str)
            .filter(|keymap| !keymap.is_empty())
            .unwrap_or(default_preset(mac))
            .to_owned(),
        reopen: at("general.reopenLastProject").and_then(Value::as_bool).unwrap_or(reopen_preference),
    }
}

/// The keymaps offered: the presets, the custom keymaps by name, and the current one when it is
/// neither (as its id).
pub fn keymap_options(settings: Option<&Value>, current: &str) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> =
        PRESET_TITLES.iter().map(|(id, title)| ((*id).to_owned(), (*title).to_owned())).collect();
    if let Some(custom) = settings.and_then(|settings| get_at(settings, "customKeymaps")).and_then(Value::as_object) {
        for (id, keymap) in custom {
            let name = keymap.get("name").and_then(Value::as_str).filter(|name| !name.is_empty());
            options.push((id.clone(), name.unwrap_or(id).to_owned()));
        }
    }
    if !options.iter().any(|(id, _)| id == current) {
        options.push((current.to_owned(), current.to_owned()));
    }
    options
}

fn write(key: &'static str, value: Value) -> SettingsWrite {
    SettingsWrite { key, value }
}

/// What Customize asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomizeEvent {
    /// Write these global settings keys, in order, then give the effective settings back
    /// ([`CustomizePage::set_settings`]); a failure is reported as "Cannot save setting".
    Write(Vec<SettingsWrite>),
    /// Keep the reopen preference on this machine (it applies before the settings load).
    ReopenPreference(bool),
    /// Run this action: Import Settings…, Configure… (the keymap), All settings….
    RunAction(&'static str),
}

/// A drop-down of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomizeSelect {
    Theme,
    FontSize,
    Keymap,
}

impl CustomizeSelect {
    fn selector(self) -> &'static str {
        match self {
            CustomizeSelect::Theme => "customize-theme",
            CustomizeSelect::FontSize => "customize-font-size",
            CustomizeSelect::Keymap => "customize-keymap",
        }
    }
}

/// The values chosen whose writes have not come back yet.
#[derive(Clone, Debug, Default)]
struct Pending {
    theme: Option<String>,
    sync_with_os: Option<bool>,
    accent: Option<String>,
    font_size: Option<f64>,
    keymap: Option<String>,
    reopen: Option<bool>,
}

/// An option of a drop-down: its value, label, detail, and whether it is a heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
    pub heading: bool,
}

/// Welcome › Customize.
pub struct CustomizePage {
    settings: Value,
    reopen_preference: bool,
    mac: bool,
    pending: Pending,
    open: Option<CustomizeSelect>,
    focus: FocusHandle,
}

impl EventEmitter<CustomizeEvent> for CustomizePage {}

impl CustomizePage {
    /// The page over the effective `settings` (the JSON of the settings in effect) and the reopen
    /// preference kept on this machine.
    pub fn new(settings: Value, reopen_preference: bool, cx: &mut Context<Self>) -> Self {
        Self {
            settings,
            reopen_preference,
            mac: is_mac_host(),
            pending: Pending::default(),
            open: None,
            focus: cx.focus_handle(),
        }
    }

    /// The effective settings changed (a write came back, or another window wrote them).
    pub fn set_settings(&mut self, settings: Value, reopen_preference: bool, cx: &mut Context<Self>) {
        self.settings = settings;
        self.reopen_preference = reopen_preference;
        self.pending = Pending::default();
        cx.notify();
    }

    /// What the page shows: the settings, with the values just chosen over them.
    pub fn view(&self) -> CustomizeView {
        let mut view = customize_view(Some(&self.settings), self.mac, self.reopen_preference);
        let pending = self.pending.clone();
        view.theme = pending.theme.unwrap_or(view.theme);
        view.sync_with_os = pending.sync_with_os.unwrap_or(view.sync_with_os);
        view.accent = pending.accent.unwrap_or(view.accent);
        view.font_size = pending.font_size.unwrap_or(view.font_size);
        view.keymap = pending.keymap.unwrap_or(view.keymap);
        view.reopen = pending.reopen.unwrap_or(view.reopen);
        view
    }

    /// The drop-down open now.
    pub fn open_select(&self) -> Option<CustomizeSelect> {
        self.open
    }

    /// The options of a drop-down, translated.
    pub fn choices(&self, select: CustomizeSelect, cx: &mut App) -> Vec<Choice> {
        let view = self.view();
        match select {
            CustomizeSelect::Theme => {
                let contributed = cx.try_global::<ThemeState>().map(|state| state.contributed.themes().to_vec());
                let choices = theme_choices(&contributed.unwrap_or_default());
                theme_select_options(&choices, Some(view.theme.as_str()))
                    .into_iter()
                    .map(|SelectOption { value, label, detail, section }| Choice {
                        value,
                        label: if section { label } else { tx(cx, &label) },
                        detail: detail.map(|detail| tx(cx, &detail)),
                        heading: section,
                    })
                    .collect()
            }
            CustomizeSelect::FontSize => UI_FONT_SIZES
                .iter()
                .map(|size| Choice { value: size.to_string(), label: size.to_string(), detail: None, heading: false })
                .collect(),
            CustomizeSelect::Keymap => keymap_options(Some(&self.settings), &view.keymap)
                .into_iter()
                .map(|(value, label)| Choice { value, label: tx(cx, &label), detail: None, heading: false })
                .collect(),
        }
    }

    fn emit_writes(&mut self, writes: Vec<SettingsWrite>, cx: &mut Context<Self>) {
        cx.emit(CustomizeEvent::Write(writes));
        cx.notify();
    }

    /// Opens or closes a drop-down; the theme cannot be picked while it follows the OS.
    pub fn toggle_select(&mut self, select: CustomizeSelect, cx: &mut Context<Self>) {
        if select == CustomizeSelect::Theme && self.view().sync_with_os {
            return;
        }
        self.open = if self.open == Some(select) { None } else { Some(select) };
        cx.notify();
    }

    /// Picks option `value` of a drop-down.
    pub fn choose(&mut self, select: CustomizeSelect, value: &str, cx: &mut Context<Self>) {
        self.open = None;
        match select {
            CustomizeSelect::Theme => self.select_theme(value, cx),
            CustomizeSelect::FontSize => {
                if let Ok(size) = value.parse::<f64>() {
                    self.select_font_size(size, cx);
                }
            }
            CustomizeSelect::Keymap => self.select_keymap(value, cx),
        }
        cx.notify();
    }

    pub fn select_theme(&mut self, theme: &str, cx: &mut Context<Self>) {
        if !is_theme_choice_id(theme) {
            return;
        }
        self.pending.theme = Some(theme.to_owned());
        self.emit_writes(vec![write("appearance.theme", json!(theme))], cx);
    }

    /// Sync with OS; turning it off makes the legacy `system` theme the explicit `light`.
    pub fn set_sync_with_os(&mut self, on: bool, cx: &mut Context<Self>) {
        self.pending.sync_with_os = Some(on);
        let mut writes = vec![write("appearance.syncWithOs", json!(on))];
        if !on && get_at(&self.settings, "appearance.theme").and_then(Value::as_str) == Some("system") {
            writes.push(write("appearance.theme", json!("light")));
        }
        self.emit_writes(writes, cx);
    }

    pub fn select_accent(&mut self, accent: &str, cx: &mut Context<Self>) {
        self.pending.accent = Some(accent.to_lowercase());
        self.emit_writes(vec![write("appearance.accent", json!(accent))], cx);
    }

    /// The IDE font size, which applies only with "Use custom font": that is on for any other size
    /// than the default, and stays on for a custom family.
    pub fn select_font_size(&mut self, size: f64, cx: &mut Context<Self>) {
        self.pending.font_size = Some(size);
        let custom = get_at(&self.settings, "appearance.useCustomFont").and_then(Value::as_bool) == Some(true);
        let family = get_at(&self.settings, "appearance.uiFontFamily").and_then(Value::as_str).unwrap_or_default();
        let keep_family = custom && !family.is_empty() && family != DEFAULT_UI_FONT;
        let size_json = if size.fract() == 0.0 && size.abs() < 1e15 { json!(size as i64) } else { json!(size) };
        let writes = vec![
            write("appearance.uiFontSize", size_json),
            write("appearance.useCustomFont", json!(size != DEFAULT_UI_FONT_SIZE || keep_family)),
        ];
        self.emit_writes(writes, cx);
    }

    pub fn select_keymap(&mut self, keymap: &str, cx: &mut Context<Self>) {
        self.pending.keymap = Some(keymap.to_owned());
        self.emit_writes(vec![write("keymap", json!(keymap))], cx);
    }

    /// Reopen last project on startup: kept on this machine and in the settings.
    pub fn set_reopen(&mut self, on: bool, cx: &mut Context<Self>) {
        self.pending.reopen = Some(on);
        cx.emit(CustomizeEvent::ReopenPreference(on));
        self.emit_writes(vec![write("general.reopenLastProject", json!(on))], cx);
    }

    fn run(&mut self, action: &'static str, cx: &mut Context<Self>) {
        cx.emit(CustomizeEvent::RunAction(action));
    }
}

/// A check box with its label.
fn checkbox(cx: &App, label: String, on: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(mark),
        )
        .child(SharedString::from(label))
}

/// A link-styled button.
fn link(cx: &App, label: String) -> Div {
    div()
        .flex_none()
        .text_color(theme::color(cx, "link"))
        .cursor_pointer()
        .hover(|el| el.text_color(theme::color(cx, "link-hover")))
        .child(SharedString::from(label))
}

impl CustomizePage {
    fn field(cx: &mut App, label: &str, control: impl IntoElement) -> Div {
        let label = tx(cx, label);
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .min_h(px(FIELD_HEIGHT))
            .child(div().flex_none().w(px(LABEL_WIDTH)).text_color(theme::color(cx, "fg-muted")).child(label))
            .child(div().flex().items_center().gap(px(12.0)).child(control))
    }

    fn section(cx: &mut App, title: &str, rows: Vec<AnyElement>) -> Div {
        let title = tx(cx, title);
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .children(rows)
    }

    /// A drop-down button showing `label`.
    fn select_button(
        &self,
        select: CustomizeSelect,
        label: String,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = theme::color(cx, "fg-muted");
        div()
            .id(select.selector())
            .debug_selector(move || select.selector().to_owned())
            .w(px(SELECT_WIDTH))
            .h(px(FIELD_HEIGHT))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_between()
            .rounded(theme::length(cx, "radius-control"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-control"))
            .when(!enabled, |el| el.opacity(0.5))
            .child(div().overflow_hidden().whitespace_nowrap().child(SharedString::from(label)))
            .child(theme::icon(cx, "chevron-down", 12.0, muted))
            .when(enabled, |el| {
                el.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.toggle_select(select, cx)))
            })
            .into_any_element()
    }

    /// The open drop-down's options, under the page's top edge.
    fn render_options(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let select = self.open?;
        let choices = self.choices(select, cx);
        let view = self.view();
        let current = match select {
            CustomizeSelect::Theme => view.theme,
            CustomizeSelect::FontSize => view.font_size.to_string(),
            CustomizeSelect::Keymap => view.keymap,
        };
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let faint = theme::color(cx, "fg-subtle");
        let (selected, hover) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let rows: Vec<AnyElement> = choices
            .into_iter()
            .enumerate()
            .map(|(index, choice)| {
                let line = div()
                    .id(index)
                    .debug_selector(move || format!("customize-option-{index}"))
                    .h(theme::length(cx, "h-row"))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap()
                    .child(SharedString::from(choice.label.clone()))
                    .children(choice.detail.clone().map(|detail| div().text_color(muted).child(detail)));
                if choice.heading {
                    return line.text_color(faint).into_any_element();
                }
                let value = choice.value.clone();
                line.text_color(fg)
                    .when(choice.value == current, |el| el.bg(selected))
                    .when(choice.value != current, |el| el.hover(|el| el.bg(hover)))
                    .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.choose(select, &value, cx)))
                    .into_any_element()
            })
            .collect();
        let list = div()
            .id("customize-options")
            .w(px(SELECT_WIDTH))
            .max_h(px(320.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(rows);
        let backdrop = div()
            .id("customize-options-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .justify_center()
            .pt(px(48.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|page, _, _, cx| {
                    page.open = None;
                    cx.notify();
                }),
            )
            .child(div().flex_none().child(list));
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }
}

impl Focusable for CustomizePage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CustomizePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view();
        let theme_label = self
            .choices(CustomizeSelect::Theme, cx)
            .into_iter()
            .find(|choice| !choice.heading && choice.value == view.theme)
            .map_or_else(|| view.theme.clone(), |choice| choice.label);
        let keymap_label = self
            .choices(CustomizeSelect::Keymap, cx)
            .into_iter()
            .find(|choice| choice.value == view.keymap)
            .map_or_else(|| view.keymap.clone(), |choice| choice.label);
        let theme_select = self.select_button(CustomizeSelect::Theme, theme_label, !view.sync_with_os, cx);
        let sync = view.sync_with_os;
        let sync_label = tx(cx, "Sync with OS");
        let sync_box = checkbox(cx, sync_label, sync)
            .id("customize-sync-with-os")
            .debug_selector(|| "customize-sync-with-os".to_owned())
            .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.set_sync_with_os(!sync, cx)));
        let swatches: Vec<AnyElement> = ACCENT_PRESETS
            .iter()
            .map(|preset| {
                let color = parse_color(preset.value).map_or_else(
                    || theme::color(cx, "accent"),
                    |color| Rgba { r: color.r, g: color.g, b: color.b, a: color.a }.into(),
                );
                let checked = view.accent == preset.value;
                let value = preset.value;
                let label = preset.label;
                div()
                    .id(SharedString::from(format!("customize-accent-{}", label.to_lowercase())))
                    .debug_selector(move || format!("customize-accent-{}", label.to_lowercase()))
                    .size(px(20.0))
                    .rounded_full()
                    .bg(color)
                    .border_1()
                    .border_color(if checked { theme::color(cx, "fg-strong") } else { color })
                    .cursor_pointer()
                    .with_tooltip(label)
                    .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.select_accent(value, cx)))
                    .into_any_element()
            })
            .collect();
        let font = self.select_button(CustomizeSelect::FontSize, view.font_size.to_string(), true, cx);
        let keymap = self.select_button(CustomizeSelect::Keymap, keymap_label, true, cx);
        let configure_label = tx(cx, "Configure…");
        let configure = link(cx, configure_label)
            .id("customize-configure-keymap")
            .debug_selector(|| "customize-configure-keymap".to_owned())
            .on_click(cx.listener(|page, _: &ClickEvent, _, cx| page.run(CONFIGURE_KEYMAP, cx)));
        let reopen = view.reopen;
        let reopen_label = tx(cx, "Reopen last project on startup");
        let reopen_box = checkbox(cx, reopen_label, reopen)
            .id("customize-reopen")
            .debug_selector(|| "customize-reopen".to_owned())
            .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.set_reopen(!reopen, cx)));
        let import_label = tx(cx, "Import Settings…");
        let import = link(cx, import_label)
            .id("customize-import-settings")
            .debug_selector(|| "customize-import-settings".to_owned())
            .on_click(cx.listener(|page, _: &ClickEvent, _, cx| page.run(IMPORT_SETTINGS, cx)));
        let all_label = tx(cx, "All settings…");
        let all = link(cx, all_label)
            .id("customize-all-settings")
            .debug_selector(|| "customize-all-settings".to_owned())
            .on_click(cx.listener(|page, _: &ClickEvent, _, cx| page.run(ALL_SETTINGS, cx)));

        let theme_control = div().flex().items_center().gap(px(12.0)).child(theme_select).child(sync_box);
        let theme_row = Self::field(cx, "Theme", theme_control);
        let accent_row = Self::field(cx, "Accent color", div().flex().gap(px(8.0)).children(swatches));
        let color = Self::section(cx, "Color theme", vec![theme_row.into_any_element(), accent_row.into_any_element()]);
        let font_row = Self::field(cx, "IDE font", font);
        let accessibility = Self::section(cx, "Accessibility", vec![font_row.into_any_element()]);
        let keymap_control = div().flex().items_center().gap(px(12.0)).child(keymap).child(configure);
        let keymap_row = Self::field(cx, "Keymap", keymap_control);
        let keymap_section = Self::section(cx, "Keymap", vec![keymap_row.into_any_element()]);
        let startup = Self::section(cx, "Startup", vec![reopen_box.into_any_element()]);
        let options = self.render_options(cx);
        div()
            .id("customize")
            .relative()
            .size_full()
            .overflow_y_scroll()
            .track_focus(&self.focus)
            .p(px(24.0))
            .flex()
            .flex_col()
            .gap(px(20.0))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(color)
            .child(accessibility)
            .child(keymap_section)
            .child(startup)
            .child(div().flex().gap(px(16.0)).child(import).child(all))
            .children(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// A stand-in for the plugins settings page.
    struct Probe;

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().debug_selector(|| "plugin-page".to_owned()).size(px(40.0)).child("plugin page")
        }
    }

    /// The welcome screen's sidebar and content as an integrator draws them.
    struct Host {
        nav: WelcomeNav,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let rows = nav_rows(
                &self.nav,
                |host: &mut Host, page, _, cx| {
                    host.nav.select(page);
                    cx.notify();
                },
                cx,
            );
            let content = self.nav.plugins_page().cloned().map(|page| div().size_full().child(page));
            div().size_full().flex().child(div().w(px(240.0)).child(rows)).child(div().flex_1().children(content))
        }
    }

    // src/features/welcome/welcome.test.tsx › Welcome › navigation › lists Plugins only when the plugins
    // settings page is contributed, and embeds it
    #[gpui_kit::test]
    async fn lists_plugins_only_when_the_plugins_settings_page_is_contributed_and_embeds_it(cx: &mut TestAppContext) {
        init(cx);
        let (host, cx) = cx.add_window_view(|_, _| Host { nav: WelcomeNav::default() });
        assert!(cx.debug_bounds("welcome-nav-plugins").is_none());
        assert_eq!(host.read_with(cx, |host, _| host.nav.pages()), [WelcomePage::Projects, WelcomePage::Customize]);
        let probe = cx.update(|_, cx| AnyView::from(cx.new(|_| Probe)));
        host.update(cx, |host, cx| {
            host.nav.set_plugins_page(Some(probe));
            cx.notify();
        });
        click(cx, "welcome-nav-plugins");
        assert_eq!(host.read_with(cx, |host, _| host.nav.shown()), WelcomePage::Plugins);
        assert!(cx.debug_bounds("plugin-page").is_some(), "the plugins page is embedded");
        host.update(cx, |host, cx| {
            host.nav.set_plugins_page(None);
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(host.read_with(cx, |host, _| host.nav.shown()), WelcomePage::Projects);
        assert!(cx.debug_bounds("plugin-page").is_none());
        assert!(!host.update(cx, |host, _| host.nav.select(WelcomePage::Plugins)), "not listed without a page");
    }

    type Events = Rc<RefCell<Vec<CustomizeEvent>>>;

    fn customize(cx: &mut TestAppContext, settings: Value) -> (Entity<CustomizePage>, Events, &mut VisualTestContext) {
        init(cx);
        let (page, cx) = cx.add_window_view(move |_, cx| CustomizePage::new(settings, false, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|_, cx| {
            let record = move |_, event: &CustomizeEvent, _: &mut App| log.borrow_mut().push(event.clone());
            cx.subscribe(&page, record).detach();
        });
        cx.run_until_parked();
        (page, events, cx)
    }

    fn writes(events: &Events) -> Vec<(&'static str, Value)> {
        events
            .borrow()
            .iter()
            .flat_map(|event| match event {
                CustomizeEvent::Write(writes) => writes.iter().map(|w| (w.key, w.value.clone())).collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    // src/features/welcome/welcome.test.tsx › Welcome › navigation › Customize writes the global settings keys
    #[gpui_kit::test]
    async fn customize_writes_the_global_settings_keys(cx: &mut TestAppContext) {
        let (page, events, cx) = customize(cx, json!({ "appearance": { "theme": "light" } }));
        click(cx, "customize-accent-purple");
        click(cx, "customize-sync-with-os");
        assert_eq!(writes(&events), [("appearance.accent", json!("#955ae0")), ("appearance.syncWithOs", json!(true))]);
        let shown = page.read_with(cx, |page, _| page.view());
        assert_eq!((shown.accent.as_str(), shown.sync_with_os), ("#955ae0", true), "chosen values show at once");
        assert!(cx.debug_bounds("customize-option-0").is_none());
        click(cx, "customize-theme");
        assert_eq!(page.read_with(cx, |page, _| page.open_select()), None, "no theme while it follows the OS");

        events.borrow_mut().clear();
        page.update(cx, |page, cx| page.set_settings(json!({ "appearance": { "theme": "system" } }), false, cx));
        click(cx, "customize-sync-with-os");
        assert_eq!(
            writes(&events),
            [("appearance.syncWithOs", json!(false)), ("appearance.theme", json!("light"))],
            "the legacy system theme becomes an explicit one"
        );

        events.borrow_mut().clear();
        page.update(cx, |page, cx| page.set_settings(json!({}), false, cx));
        click(cx, "customize-font-size");
        let sizes: Vec<String> = page
            .update(cx, |page, cx| page.choices(CustomizeSelect::FontSize, cx))
            .into_iter()
            .map(|choice| choice.value)
            .collect();
        assert_eq!(sizes.first().map(String::as_str), Some("10"));
        click(cx, "customize-option-5");
        assert_eq!(writes(&events), [("appearance.uiFontSize", json!(15)), ("appearance.useCustomFont", json!(true))]);

        events.borrow_mut().clear();
        click(cx, "customize-reopen");
        click(cx, "customize-all-settings");
        let all = events.borrow().clone();
        assert_eq!(
            all,
            [
                CustomizeEvent::ReopenPreference(true),
                CustomizeEvent::Write(vec![write("general.reopenLastProject", json!(true))]),
                CustomizeEvent::RunAction(ALL_SETTINGS),
            ]
        );
    }

    // src/features/welcome/welcome.test.tsx › helpers › derives the Customize view from partial settings
    #[test]
    fn derives_the_customize_view_from_partial_settings() {
        let none = customize_view(None, true, false);
        assert_eq!(
            (none.theme.as_str(), none.sync_with_os, none.font_size, none.keymap.as_str()),
            ("light", false, 13.0, "intellij-macos")
        );
        assert_eq!(none.accent, DEFAULT_ACCENT);
        let legacy = json!({
            "appearance": { "theme": "system", "useCustomFont": true, "uiFontSize": 15 },
            "keymap": "vscode-macos",
        });
        let legacy = customize_view(Some(&legacy), true, false);
        assert_eq!(
            (legacy.theme.as_str(), legacy.sync_with_os, legacy.font_size, legacy.keymap.as_str()),
            ("light", true, 15.0, "vscode-macos")
        );
        let size_only = json!({ "appearance": { "uiFontSize": 15 } });
        assert_eq!(customize_view(Some(&size_only), true, false).font_size, 13.0);
        assert_eq!(customize_view(None, false, false).keymap, "intellij-windows");
        let reopen = json!({ "general": { "reopenLastProject": false } });
        assert!(!customize_view(Some(&reopen), true, true).reopen, "the setting wins over the preference");
        assert!(customize_view(None, true, true).reopen);
        let accent = json!({ "appearance": { "accent": "#955AE0" } });
        assert_eq!(customize_view(Some(&accent), true, false).accent, "#955ae0");
    }

    // Not from a spec: the keymaps offered are the presets, the custom ones by name, and the current one.
    #[test]
    fn offers_the_presets_the_custom_keymaps_and_the_current_one() {
        let settings = json!({ "customKeymaps": { "custom.mine": { "name": "Mine" }, "custom.blank": {} } });
        let options = keymap_options(Some(&settings), "legacy");
        let ids: Vec<&str> = options.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids.len(), PRESET_TITLES.len() + 3);
        assert!(options.contains(&("custom.mine".to_owned(), "Mine".to_owned())));
        assert!(options.contains(&("custom.blank".to_owned(), "custom.blank".to_owned())));
        assert_eq!(options.last(), Some(&("legacy".to_owned(), "legacy".to_owned())));
        assert_eq!(keymap_options(None, "intellij-macos").len(), PRESET_TITLES.len());
    }
}
