//! Settings › Plugins: the Installed and Available tabs with their search, the list of plugins
//! (icon, name, producer, version and state), the selected plugin's details (description,
//! permissions with their risk, what it contributes, What's New, the homepage, and the buttons
//! that enable, restart, reload, roll back, uninstall, install or update it), the gear menu
//! (install from a folder or a package file, the repository, the IntelliJ theme imports, enable
//! or disable every downloaded plugin, rescan) and the secrets a plugin declares.
//!
//! [`ConsentDialog`] is the install and permission page: every capability with its risk, the
//! optional ones to accept, and the warning for those that are like running arbitrary code.
//! [`IntellijThemesDialog`] lists the theme plugins of the JetBrains IDEs on this machine.
//!
//! None of them calls the plugin host. The page shows what it is given (the listing, the local
//! catalog, the marketplace, the secrets' state) and reports through [`PluginsPageEvent`] what
//! the workbench runs; the answers come back through the page's methods. An install is an
//! [`InstallFlow`] the page drives one step at a time.

use super::consent::{
    Capability, Risk, capabilities_of, capability_description, initially_accepted, is_high_risk, sort_by_risk,
};
use super::contribution::Contribution;
use super::icon_themes::{adopt_icon_theme, icon_theme_key};
use super::plugin_icon::{IconMarkup, PluginIcons};
use super::store::{
    CatalogState, ConsentRequest, InstallFlow, InstallStep, catalog_state, consent_without_dialog, filter_catalog,
    filter_plugins, grant_request, installed_to_toggle, is_web_homepage,
};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::plugins::plugin_action_id;
use crate::theme;
use crate::theme::themes::plugin_theme_ref;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::WithTooltip;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    Keystroke, MouseButton, Render, SharedString, Stateful, Subscription, Window, deferred, div, prelude::*, px,
};
use ide_host::plugins::intellij::InstalledIntellijTheme;
use ide_host::plugins::{CatalogPlugin, Consent, MarketPlugin, PluginInfo, PluginSource, PluginStatus};
use std::collections::{HashMap, HashSet, VecDeque};

/// Shown on Available when no marketplace index is configured.
pub const NO_REPOSITORY: &str = "There is no online plugin repository";

const SIDE_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 44.0;
const BUTTON_HEIGHT: f32 = 24.0;
const ICON_SIZE: f32 = 20.0;
const LARGE_ICON_SIZE: f32 = 32.0;
const MENU_WIDTH: f32 = 300.0;
const TOP_BAR_HEIGHT: f32 = 36.0;
const FIELD_HEIGHT: f32 = 28.0;
const DIALOG_WIDTH: f32 = 520.0;
const INTELLIJ_WIDTH: f32 = 640.0;

/// The marketplace categories this IDE knows, in the order the filter lists them, with their names.
const CATEGORIES: [(&str, &str); 9] = [
    ("languages", "Languages"),
    ("themes", "Themes"),
    ("icon-themes", "Icon Themes"),
    ("linters", "Linters & Formatters"),
    ("vcs", "Version Control"),
    ("ai", "AI"),
    ("frameworks", "Frameworks"),
    ("tools", "Tools"),
    ("other", "Other"),
];

/// The names of the built-in contribution points; others show their id.
fn point_title(point: &str) -> &str {
    match point {
        "commands" => "Commands",
        "themes" => "Themes",
        "iconThemes" => "File Icon Themes",
        "languages" => "Languages",
        "providers" => "Editor Providers",
        "locales" => "Translations",
        "secrets" => "Secrets",
        other => other,
    }
}

/// `text` in the active locale.
fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn i18n(cx: &mut App) -> &mut I18n {
    &mut cx.default_global::<Localization>().0
}

fn text(value: &str) -> Param {
    Param::Text(value.to_owned())
}

/// The first font of the theme's `--font-mono` list that is installed: ids and paths are code.
fn code_font(cx: &App) -> SharedString {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|active| active.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family = families.into_iter().find(|family| installed.contains(family)).unwrap_or_else(|| "Menlo".to_owned());
    SharedString::from(family)
}

/// The capabilities a catalog entry or an install preview carries, as the consent model reads them.
fn listed_capabilities<T: serde::Serialize>(capabilities: &T) -> Vec<Capability> {
    serde_json::to_value(capabilities).ok().and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default()
}

/// A plugin's own icon as markup, from the `icon` field of a listing, a catalog or a marketplace entry.
macro_rules! markup {
    ($icon:expr) => {
        $icon.as_ref().map(|icon| IconMarkup { light: icon.light.as_str(), dark: icon.dark.as_deref() })
    };
}

/// The marketplace categories of `entry` this IDE knows; `other` without any.
fn categories_of(entry: &MarketPlugin) -> Vec<&'static str> {
    let known: Vec<&'static str> = CATEGORIES
        .iter()
        .filter(|(id, _)| entry.entry.categories.iter().any(|category| category == id))
        .map(|(id, _)| *id)
        .collect();
    if known.is_empty() { vec!["other"] } else { known }
}

fn category_name(id: &str) -> &'static str {
    CATEGORIES.iter().find(|(known, _)| *known == id).map_or("Other", |(_, name)| name)
}

fn matches(texts: &[&str], query: &str) -> bool {
    texts.iter().any(|text| text.to_lowercase().contains(query))
}

/// The repository a typed marketplace URL names: an empty one restores the default (`None`).
pub fn repository_url(typed: &str) -> Option<String> {
    let url = typed.trim();
    (!url.is_empty()).then(|| url.to_owned())
}

/// Which tab of the page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginsTab {
    /// "Available": the marketplace and the local catalog.
    Marketplace,
    Installed,
}

/// Which tab of the details shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTab {
    Overview,
    WhatsNew,
}

/// The colour of a badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    fn token(self) -> &'static str {
        match self {
            Tone::Neutral => "fg-muted",
            Tone::Accent => "accent",
            Tone::Success => "success",
            Tone::Warning => "warning",
            Tone::Danger => "danger",
        }
    }
}

fn risk_badge(risk: Risk, i18n: &I18n) -> Badge {
    let (label, tone) = match risk {
        Risk::Low => ("Low", Tone::Neutral),
        Risk::Medium => ("Medium", Tone::Warning),
        Risk::High => ("High", Tone::Danger),
    };
    Badge { label: i18n.tx(label), tone }
}

/// A small label with a colour, translated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub label: String,
    pub tone: Tone,
}

/// What a list row stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Installed,
    /// A package of the local catalog.
    Catalog,
    Market,
}

/// A plugin in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub kind: ItemKind,
    pub id: String,
    pub name: String,
    /// The authors, else the publisher or the id.
    pub vendor: String,
    pub version: String,
    /// The icon name: the plugin's own icon, else the generic one.
    pub icon: String,
    pub tooltip: String,
    pub tags: Vec<Badge>,
    /// An installed plugin's checkbox: checked when enabled, and whether it can be clicked now.
    pub enabled: Option<(bool, bool)>,
    /// A catalog package's Install or Update button, and whether it can be pressed.
    pub install: Option<(String, bool)>,
    pub selected: bool,
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListRow {
    /// The category filter at the top of Available.
    Category,
    Header {
        title: String,
        count: usize,
    },
    Item(ListItem),
    /// A line in place of plugins: nothing to list, or why a list could not be read.
    Message {
        text: String,
        error: bool,
    },
}

/// A button of the details' header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DetailButton {
    /// Enable, Disable, or Restart for a failed plugin.
    Toggle,
    Reload,
    RollBack,
    Uninstall,
    ConfirmUninstall,
    CancelUninstall,
    /// Install or Update a catalog package or a marketplace plugin.
    Install,
    /// Review Permissions… of a plugin waiting for consent.
    ReviewPermissions,
}

impl DetailButton {
    fn selector(self) -> &'static str {
        match self {
            DetailButton::Toggle => "plugins-details-toggle",
            DetailButton::Reload => "plugins-details-reload",
            DetailButton::RollBack => "plugins-details-roll-back",
            DetailButton::Uninstall => "plugins-details-uninstall",
            DetailButton::ConfirmUninstall => "plugins-details-confirm-uninstall",
            DetailButton::CancelUninstall => "plugins-details-cancel-uninstall",
            DetailButton::Install => "plugins-details-install",
            DetailButton::ReviewPermissions => "plugins-details-review",
        }
    }
}

/// A button as shown: its label (translated) and whether it can be pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownButton {
    pub id: DetailButton,
    pub label: String,
    pub primary: bool,
    pub enabled: bool,
}

/// A line of the details' facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub label: String,
    pub value: String,
    pub mono: bool,
    /// A web address that opens on a click.
    pub link: bool,
}

/// What a row's button or checkbox does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowCommand {
    RunCommand(String),
    /// Writes the Appearance theme setting.
    SetTheme(String),
    SelectIconTheme(Option<String>),
    SetCapability {
        capability: String,
        granted: bool,
    },
}

/// A row's button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowAction {
    pub label: String,
    pub primary: bool,
    pub enabled: bool,
    pub command: RowCommand,
    /// What tests click.
    pub selector: String,
}

/// A line of a section of the details.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SectionRow {
    pub main: String,
    pub mono: Option<String>,
    pub detail: Option<String>,
    pub badge: Option<Badge>,
    pub description: Option<String>,
    /// A capability's checkbox: checked, enabled; it runs [`SectionRow::action`].
    pub check: Option<(bool, bool)>,
    pub action: Option<RowAction>,
}

/// A declared secret and whether a value is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretRow {
    pub name: String,
    pub label: String,
    pub description: Option<String>,
    pub set: bool,
    pub busy: bool,
    pub error: Option<String>,
}

/// A section of the details: Permissions, one per contribution point, Statistics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub rows: Vec<SectionRow>,
    pub secrets: Vec<SecretRow>,
    /// Muted lines under the rows.
    pub notes: Vec<String>,
}

/// The selected plugin's details, translated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Details {
    pub kind: ItemKind,
    pub id: String,
    pub name: String,
    pub authors: Option<String>,
    pub version: String,
    pub icon: String,
    pub badges: Vec<Badge>,
    pub buttons: Vec<ShownButton>,
    /// An operation is running for the plugin.
    pub busy: bool,
    pub error: Option<String>,
    /// A notice above the description (waiting for consent, a version this IDE cannot load).
    pub notice: Option<String>,
    pub description: Option<String>,
    pub facts: Vec<Fact>,
    pub warnings: Vec<String>,
    pub sections: Vec<Section>,
    /// The change notes of What's New; `None` for a marketplace plugin, which has no tabs.
    pub notes: Option<Option<String>>,
}

/// An item of the gear menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GearCommand {
    UpdateAll,
    InstallFromDisk,
    InstallPackageFile,
    EditRepository,
    ImportInstalledIntellij,
    ImportIntellijFile,
    DisableAll,
    EnableAll,
    Rescan,
}

/// An item of the gear menu as shown; `None` is a separator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GearItem {
    pub command: Option<GearCommand>,
    pub label: String,
    pub enabled: bool,
}

/// What the page asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginsPageEvent {
    /// List again: the plugins (`Plugins::refresh`, answered with [`PluginsPage::set_listing`]), the local
    /// catalog ([`PluginsPage::set_catalog`]) and the marketplace, its index read again with `Some(true)`
    /// ([`PluginsPage::set_market`]).
    Refresh {
        plugins: bool,
        catalog: bool,
        market: Option<bool>,
    },
    /// `set_enabled`; enabling an enabled plugin restarts it. Answer with [`PluginsPage::finished`].
    SetEnabled {
        id: String,
        enabled: bool,
    },
    /// `reload`; answer with [`PluginsPage::finished`].
    Reload(String),
    /// `uninstall`; answer with [`PluginsPage::finished`].
    Uninstall(String),
    /// `grant` with the accepted optional capabilities; answer with [`PluginsPage::finished`].
    Grant {
        id: String,
        optional: Vec<String>,
        signer_change: bool,
    },
    /// `set_capability`; answer with [`PluginsPage::finished`].
    SetCapability {
        id: String,
        capability: String,
        granted: bool,
    },
    /// `roll_back_update`; answer with [`PluginsPage::rolled_back`].
    RollBack(String),
    /// Show [`ConsentDialog`] for the request; answer with [`PluginsPage::consented`].
    AskConsent(ConsentRequest),
    /// An install step: [`InstallStep::Pick`] (a folder or a package file picker, answered with
    /// [`PluginsPage::picked`]), [`InstallStep::Unpack`] (`unpack_package`, [`PluginsPage::unpacked`]),
    /// [`InstallStep::Preview`] (`preview_install`, [`PluginsPage::previewed`]) or
    /// [`InstallStep::Install`] (`install_from_dir`, [`PluginsPage::installed`]).
    Install(InstallStep),
    /// `download_market_plugin`; answer with [`PluginsPage::downloaded`].
    DownloadMarket(String),
    /// Ask for the marketplace index URL (prefilled with `current`), then `set_market_url` with
    /// [`repository_url`] of the answer and list the marketplace again.
    EditRepository {
        current: Option<String>,
    },
    /// Import Theme from JetBrains IDE: show [`IntellijThemesDialog`].
    ImportInstalledIntellij,
    /// Import IntelliJ Theme from File.
    ImportIntellijFile,
    OpenUrl(String),
    /// Write `appearance.theme`.
    SetTheme(String),
    /// Select the file icon theme (`None`: the default icons).
    SelectIconTheme(Option<String>),
    /// Run the plugin command's action ([`plugin_action_id`]).
    RunCommand {
        plugin_id: String,
        command_id: String,
    },
    /// `secrets`; answer with [`PluginsPage::set_secrets`].
    LoadSecrets(String),
    /// `set_secret` (`None` removes it); answer with [`PluginsPage::secret_saved`].
    SetSecret {
        plugin_id: String,
        name: String,
        value: Option<String>,
    },
    Notify(NotificationRequest),
}

/// An operation in flight for a plugin.
#[derive(Clone, Debug)]
enum Operation {
    Toggle { enabled: bool },
    Uninstall,
    Other,
}

#[derive(Clone, Debug)]
struct Busy {
    operation: Operation,
    /// The title of the notification when it fails.
    failure: String,
    name: String,
}

/// The install in progress.
#[derive(Debug)]
struct Install {
    flow: InstallFlow,
    /// Show the installed plugin on Installed afterwards.
    show: bool,
    /// From the marketplace: list it again afterwards.
    market: bool,
}

/// Update All in progress: the marketplace is read again (`queue` is `None` until it answers),
/// then each update installs in turn.
#[derive(Debug, Default)]
struct UpdateRun {
    queue: Option<VecDeque<String>>,
    updated: usize,
}

/// Who the consent dialog answers.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asking {
    Grant(String),
    Install,
}

type SecretKey = (String, String);

pub struct PluginsPage {
    plugins: Vec<PluginInfo>,
    contributions: Vec<Contribution>,
    loaded: bool,
    loading: bool,
    error: Option<String>,
    busy: HashMap<String, Busy>,
    catalog: Option<Vec<CatalogPlugin>>,
    catalog_error: Option<String>,
    market_url: Option<String>,
    market: Option<Vec<MarketPlugin>>,
    market_error: Option<String>,
    can_import: bool,
    theme_choice: Option<String>,
    icon_theme: Option<String>,
    /// The declared secrets' state per plugin, as last read.
    secrets: HashMap<String, Vec<(String, bool)>>,
    /// The plugins whose secrets were asked for, so that each is asked once.
    secrets_requested: HashSet<String>,
    secret_fields: HashMap<(String, String, bool), Entity<TextField>>,
    secret_busy: HashSet<SecretKey>,
    secret_errors: HashMap<SecretKey, String>,
    tab: PluginsTab,
    detail_tab: DetailTab,
    query: String,
    /// The search field of each tab, with its own placeholder; both hold the same query.
    search: [Entity<TextField>; 2],
    selected_id: Option<String>,
    selected_catalog: Option<String>,
    selected_market: Option<String>,
    category: Option<&'static str>,
    category_open: bool,
    gear_open: bool,
    confirm_uninstall: bool,
    /// The details the per-plugin state (the uninstall confirmation, the details tab) belongs to.
    detail_key: Option<(ItemKind, String)>,
    install: Option<Install>,
    downloading: Option<(String, bool, bool)>,
    updates: Option<UpdateRun>,
    asking: Option<Asking>,
    /// Plugins whose first icon theme is adopted when the next listing arrives.
    adopt: HashSet<String>,
    icons: PluginIcons,
    code_font: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PluginsPageEvent> for PluginsPage {}

impl PluginsPage {
    /// An empty page; the workbench calls [`PluginsPage::opened`] when it shows it.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let installed = tx(cx, "Search installed plugins");
        let available = tx(cx, "Search plugins");
        let search = [
            cx.new(|cx| TextField::new(cx).with_placeholder(available)),
            cx.new(|cx| TextField::new(cx).with_placeholder(installed)),
        ];
        let subscriptions = search
            .iter()
            .map(|field| {
                cx.subscribe_in(field, window, |page: &mut Self, field, event: &TextFieldEvent, _, cx| {
                    if *event == TextFieldEvent::Changed {
                        page.query = field.read(cx).text().to_owned();
                        page.request_secrets(cx);
                        cx.notify();
                    }
                })
            })
            .collect();
        Self {
            plugins: Vec::new(),
            contributions: Vec::new(),
            loaded: false,
            loading: false,
            error: None,
            busy: HashMap::new(),
            catalog: None,
            catalog_error: None,
            market_url: None,
            market: None,
            market_error: None,
            can_import: false,
            theme_choice: None,
            icon_theme: None,
            secrets: HashMap::new(),
            secrets_requested: HashSet::new(),
            secret_fields: HashMap::new(),
            secret_busy: HashSet::new(),
            secret_errors: HashMap::new(),
            tab: PluginsTab::Installed,
            detail_tab: DetailTab::Overview,
            query: String::new(),
            search,
            selected_id: None,
            selected_catalog: None,
            selected_market: None,
            category: None,
            category_open: false,
            gear_open: false,
            confirm_uninstall: false,
            detail_key: None,
            install: None,
            downloading: None,
            updates: None,
            asking: None,
            adopt: HashSet::new(),
            icons: PluginIcons::default(),
            code_font: code_font(cx),
            _subscriptions: subscriptions,
        }
    }

    // --- what the workbench gives ---------------------------------------------------------

    /// The page is shown: list the plugins, the catalog and the marketplace (its last index).
    pub fn opened(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        cx.emit(PluginsPageEvent::Refresh { plugins: true, catalog: true, market: Some(false) });
        cx.notify();
    }

    /// A listing: the plugins and the (translated) contributions of the live ones. Icon themes of
    /// plugins just enabled or installed are adopted, and the selected plugin's secrets are read.
    pub fn set_listing(&mut self, plugins: Vec<PluginInfo>, contributions: Vec<Contribution>, cx: &mut Context<Self>) {
        self.plugins = plugins;
        self.contributions = contributions;
        self.loaded = true;
        self.loading = false;
        self.error = None;
        for id in std::mem::take(&mut self.adopt) {
            if let Some(key) = adopt_icon_theme(self.icon_theme.as_deref(), &self.contributions, &id) {
                self.icon_theme = Some(key.clone());
                cx.emit(PluginsPageEvent::SelectIconTheme(Some(key)));
            }
        }
        self.request_secrets(cx);
        cx.notify();
    }

    /// The listing failed.
    pub fn listing_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.loaded = true;
        self.loading = false;
        self.error = Some(error);
        cx.notify();
    }

    /// The local catalog, or why it could not be read.
    pub fn set_catalog(&mut self, catalog: Result<Vec<CatalogPlugin>, String>, cx: &mut Context<Self>) {
        match catalog {
            Ok(catalog) => (self.catalog, self.catalog_error) = (Some(catalog), None),
            Err(error) => (self.catalog, self.catalog_error) = (Some(Vec::new()), Some(error)),
        }
        cx.notify();
    }

    /// The marketplace: its index URL (none configured: no marketplace) and its plugins, or why they
    /// could not be read. Update All waiting for it goes on.
    pub fn set_market(
        &mut self,
        url: Option<String>,
        market: Result<Vec<MarketPlugin>, String>,
        cx: &mut Context<Self>,
    ) {
        match (url.is_some(), market) {
            (false, Ok(_)) => (self.market, self.market_error) = (None, None),
            (_, Ok(market)) => (self.market, self.market_error) = (Some(market), None),
            (_, Err(error)) => self.market_error = Some(error),
        }
        self.market_url = url;
        if let Some(run) = self.updates.as_mut().filter(|run| run.queue.is_none()) {
            let offered = self.market.iter().flatten().filter(|m| m.is_update()).map(|m| m.entry.id.clone());
            run.queue = Some(offered.collect());
            self.next_update(cx);
        }
        cx.notify();
    }

    /// Whether the window's project is trusted: installing (the IntelliJ imports) needs one.
    pub fn set_trusted_project(&mut self, trusted: bool, cx: &mut Context<Self>) {
        self.can_import = trusted;
        cx.notify();
    }

    /// The Appearance theme setting and the selected file icon theme.
    pub fn set_appearance(&mut self, theme: Option<String>, icon_theme: Option<String>, cx: &mut Context<Self>) {
        self.theme_choice = theme;
        self.icon_theme = icon_theme;
        cx.notify();
    }

    /// The declared secrets of `plugin_id` and whether each is stored.
    pub fn set_secrets(&mut self, plugin_id: &str, secrets: Vec<(String, bool)>, cx: &mut Context<Self>) {
        self.secrets.insert(plugin_id.to_owned(), secrets);
        cx.notify();
    }

    /// A secret was stored or removed (or not): its field empties, its state is read again.
    pub fn secret_saved(&mut self, plugin_id: &str, name: &str, result: Result<(), String>, cx: &mut Context<Self>) {
        let key = (plugin_id.to_owned(), name.to_owned());
        self.secret_busy.remove(&key);
        match result {
            Ok(()) => {
                self.secret_errors.remove(&key);
                for set in [false, true] {
                    if let Some(field) = self.secret_fields.get(&(key.0.clone(), key.1.clone(), set)) {
                        field.update(cx, |field, cx| field.set_text("", cx));
                    }
                }
                cx.emit(PluginsPageEvent::LoadSecrets(plugin_id.to_owned()));
            }
            Err(error) => {
                self.secret_errors.insert(key, error);
            }
        }
        cx.notify();
    }

    /// An operation on `id` ended: a failure is reported, an uninstall announced, an enabled
    /// plugin's icon theme adopted, and the plugins are listed again.
    pub fn finished(&mut self, id: &str, result: Result<(), String>, cx: &mut Context<Self>) {
        let Some(busy) = self.busy.remove(id) else { return };
        match result {
            Err(error) => self.notify(NotificationLevel::Error, busy.failure, Some(error), cx),
            Ok(()) => match busy.operation {
                Operation::Uninstall => {
                    let title = i18n(cx).tx_built(&["Uninstalled ", ""], &[text(&busy.name)]);
                    self.notify(NotificationLevel::Info, title, None, cx);
                }
                Operation::Toggle { enabled: true } => {
                    self.adopt.insert(id.to_owned());
                }
                Operation::Toggle { enabled: false } | Operation::Other => {}
            },
        }
        self.refresh_plugins(cx);
    }

    /// Roll Back ended: the version gone back to, or why not.
    pub fn rolled_back(&mut self, id: &str, result: Result<PluginInfo, String>, cx: &mut Context<Self>) {
        self.busy.remove(id);
        match result {
            Ok(info) => {
                let title = i18n(cx).tx_built(&["Rolled back ", " to ", ""], &[text(&info.name), text(&info.version)]);
                self.notify(NotificationLevel::Info, title, None, cx);
            }
            Err(error) => {
                let title = tx(cx, "Could not roll back");
                self.notify(NotificationLevel::Error, title, Some(error), cx);
            }
        }
        self.refresh_plugins(cx);
    }

    /// The consent dialog's answer (`None`: cancelled), for the grant or the install that asked.
    pub fn consented(&mut self, consent: Option<Consent>, cx: &mut Context<Self>) {
        match self.asking.take() {
            Some(Asking::Grant(id)) => {
                if let Some(consent) = consent {
                    self.send_grant(&id, consent, cx);
                }
            }
            Some(Asking::Install) => {
                if let Some(install) = &mut self.install {
                    let step = install.flow.consented(consent);
                    self.step(step, cx);
                }
            }
            None => {}
        }
        cx.notify();
    }

    /// The picker's answer (`Ok(None)`: cancelled).
    pub fn picked(&mut self, picked: Result<Option<String>, String>, cx: &mut Context<Self>) {
        let Some(install) = &mut self.install else { return };
        let step = install.flow.picked(picked, i18n(cx));
        self.step(step, cx);
    }

    /// The folder a package file was unpacked to.
    pub fn unpacked(&mut self, dir: Result<String, String>, cx: &mut Context<Self>) {
        let Some(install) = &mut self.install else { return };
        let step = install.flow.unpacked(dir, i18n(cx));
        self.step(step, cx);
    }

    /// The install page's content.
    pub fn previewed(&mut self, preview: Result<ide_host::plugins::InstallPreview, String>, cx: &mut Context<Self>) {
        let Some(install) = &mut self.install else { return };
        let step = install.flow.previewed(preview, i18n(cx));
        self.step(step, cx);
    }

    /// The installed plugin.
    pub fn installed(&mut self, info: Result<PluginInfo, String>, cx: &mut Context<Self>) {
        let Some(install) = &mut self.install else { return };
        let step = install.flow.installed(info, i18n(cx));
        self.step(step, cx);
    }

    /// The folder a marketplace plugin was downloaded to: its install page follows.
    pub fn downloaded(&mut self, id: &str, dir: Result<String, String>, cx: &mut Context<Self>) {
        let Some((downloading, quiet, show)) = self.downloading.take() else { return };
        if downloading != id {
            self.downloading = Some((downloading, quiet, show));
            return;
        }
        match dir {
            Ok(dir) => self.start_install(Some(&dir), false, quiet, show, true, cx),
            Err(error) => {
                let title = tx(cx, "Could not download plugin");
                self.notify(NotificationLevel::Error, title, Some(error), cx);
                self.next_update(cx);
            }
        }
        cx.notify();
    }

    // --- what the user does ---------------------------------------------------------------

    pub fn select_tab(&mut self, tab: PluginsTab, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        let field = &self.search[self.search_index()];
        let query = self.query.clone();
        field.update(cx, |field, cx| field.set_text(&query, cx));
        self.category_open = false;
        self.request_secrets(cx);
        cx.notify();
    }

    pub fn select_detail_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        self.detail_tab = tab;
        cx.notify();
    }

    /// Selects a row of the list.
    pub fn select(&mut self, kind: ItemKind, id: &str, cx: &mut Context<Self>) {
        match kind {
            ItemKind::Installed => self.selected_id = Some(id.to_owned()),
            ItemKind::Catalog => {
                self.selected_market = None;
                self.selected_catalog = Some(id.to_owned());
            }
            ItemKind::Market => self.selected_market = Some(id.to_owned()),
        }
        self.sync_detail();
        self.request_secrets(cx);
        cx.notify();
    }

    /// Shows the marketplace of `category` only (`None`: all categories).
    pub fn select_category(&mut self, category: Option<&str>, cx: &mut Context<Self>) {
        self.category = category.and_then(|id| CATEGORIES.iter().find(|(known, _)| *known == id).map(|(id, _)| *id));
        self.category_open = false;
        cx.notify();
    }

    /// Enables or disables `id`; enabling an enabled plugin restarts it.
    pub fn set_enabled(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        let name = self.name_of(id);
        let failure = if enabled { ["Could not enable ", ""] } else { ["Could not disable ", ""] };
        let failure = i18n(cx).tx_built(&failure, &[text(&name)]);
        if self.begin(id, Operation::Toggle { enabled }, failure, cx) {
            cx.emit(PluginsPageEvent::SetEnabled { id: id.to_owned(), enabled });
        }
    }

    /// Enables or disables every downloaded (not development) plugin.
    pub fn set_all_installed_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        for id in installed_to_toggle(&self.plugins, enabled) {
            self.set_enabled(&id, enabled, cx);
        }
    }

    pub fn reload(&mut self, id: &str, cx: &mut Context<Self>) {
        let failure = i18n(cx).tx_built(&["Could not reload ", ""], &[text(&self.name_of(id))]);
        if self.begin(id, Operation::Other, failure, cx) {
            cx.emit(PluginsPageEvent::Reload(id.to_owned()));
        }
    }

    pub fn uninstall(&mut self, id: &str, cx: &mut Context<Self>) {
        let failure = i18n(cx).tx_built(&["Could not uninstall ", ""], &[text(&self.name_of(id))]);
        if self.begin(id, Operation::Uninstall, failure, cx) {
            cx.emit(PluginsPageEvent::Uninstall(id.to_owned()));
        }
    }

    pub fn roll_back(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.begin(id, Operation::Other, String::new(), cx) {
            cx.emit(PluginsPageEvent::RollBack(id.to_owned()));
        }
    }

    /// Asks for the permissions of a plugin waiting for consent; one asking for nothing is granted.
    pub fn grant(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(plugin) = self.plugins.iter().find(|p| p.id == id) else { return };
        if self.asking.is_some() || self.installing() || self.busy.contains_key(id) {
            return;
        }
        let request = grant_request(plugin);
        match consent_without_dialog(&request) {
            Some(consent) => self.send_grant(id, consent, cx),
            None => {
                self.asking = Some(Asking::Grant(id.to_owned()));
                cx.emit(PluginsPageEvent::AskConsent(request));
            }
        }
        cx.notify();
    }

    pub fn set_capability(&mut self, id: &str, capability: &str, granted: bool, cx: &mut Context<Self>) {
        let failure =
            i18n(cx).tx_built(&["Could not change ", " of ", ""], &[text(capability), text(&self.name_of(id))]);
        if self.begin(id, Operation::Other, failure, cx) {
            cx.emit(PluginsPageEvent::SetCapability { id: id.to_owned(), capability: capability.to_owned(), granted });
        }
    }

    /// Installs from `path` (a folder, its `plugin.toml`, or a package file), or from what a picker
    /// gives; `show` selects the installed plugin on Installed afterwards.
    pub fn install_from_disk(&mut self, path: Option<&str>, package_file: bool, show: bool, cx: &mut Context<Self>) {
        self.start_install(path, package_file, false, show, false, cx);
    }

    /// Downloads marketplace plugin `id`, then shows its install page.
    pub fn install_from_market(&mut self, id: &str, cx: &mut Context<Self>) {
        self.download(id, false, true, cx);
    }

    /// Update All: reads the marketplace again, then installs every update it offers in turn
    /// (those with nothing to confirm without asking) and reports how many were updated.
    pub fn update_all(&mut self, cx: &mut Context<Self>) {
        if self.updates.is_some() {
            return;
        }
        self.updates = Some(UpdateRun::default());
        cx.emit(PluginsPageEvent::Refresh { plugins: false, catalog: false, market: Some(true) });
        cx.notify();
    }

    /// Runs an item of the gear menu.
    pub fn run_gear(&mut self, command: GearCommand, cx: &mut Context<Self>) {
        self.gear_open = false;
        match command {
            GearCommand::UpdateAll => self.update_all(cx),
            GearCommand::InstallFromDisk => self.install_from_disk(None, false, true, cx),
            GearCommand::InstallPackageFile => self.install_from_disk(None, true, true, cx),
            GearCommand::EditRepository => {
                cx.emit(PluginsPageEvent::EditRepository { current: self.market_url.clone() });
            }
            GearCommand::ImportInstalledIntellij => cx.emit(PluginsPageEvent::ImportInstalledIntellij),
            GearCommand::ImportIntellijFile => cx.emit(PluginsPageEvent::ImportIntellijFile),
            GearCommand::DisableAll => self.set_all_installed_enabled(false, cx),
            GearCommand::EnableAll => self.set_all_installed_enabled(true, cx),
            GearCommand::Rescan => {
                self.loading = true;
                cx.emit(PluginsPageEvent::Refresh { plugins: true, catalog: true, market: Some(true) });
            }
        }
        cx.notify();
    }

    /// Selects the installed plugin `id` on Installed (an import or an install finished elsewhere).
    pub fn show_installed(&mut self, id: &str, cx: &mut Context<Self>) {
        self.select_tab(PluginsTab::Installed, cx);
        self.select(ItemKind::Installed, id, cx);
    }

    fn press(&mut self, button: DetailButton, cx: &mut Context<Self>) {
        let Some(details) = self.details(cx) else { return };
        if !details.buttons.iter().any(|shown| shown.id == button && shown.enabled) {
            return;
        }
        let id = details.id.clone();
        match (details.kind, button) {
            (ItemKind::Installed, DetailButton::Toggle) => {
                if let Some(plugin) = self.plugins.iter().find(|p| p.id == id) {
                    let enabled = !plugin.enabled || plugin.status == PluginStatus::Failed;
                    self.set_enabled(&id, enabled, cx);
                }
            }
            (_, DetailButton::Reload) => self.reload(&id, cx),
            (_, DetailButton::RollBack) => self.roll_back(&id, cx),
            (_, DetailButton::Uninstall) => self.confirm_uninstall = true,
            (_, DetailButton::CancelUninstall) => self.confirm_uninstall = false,
            (_, DetailButton::ConfirmUninstall) => self.uninstall(&id, cx),
            (_, DetailButton::ReviewPermissions) => self.grant(&id, cx),
            (ItemKind::Catalog, DetailButton::Install) => self.install_catalog(&id, cx),
            (ItemKind::Market, DetailButton::Install) => self.install_from_market(&id, cx),
            _ => {}
        }
        cx.notify();
    }

    fn run_row(&mut self, plugin_id: &str, command: RowCommand, cx: &mut Context<Self>) {
        match command {
            RowCommand::RunCommand(command_id) => {
                cx.emit(PluginsPageEvent::RunCommand { plugin_id: plugin_id.to_owned(), command_id });
            }
            RowCommand::SetTheme(choice) => {
                self.theme_choice = Some(choice.clone());
                cx.emit(PluginsPageEvent::SetTheme(choice));
            }
            RowCommand::SelectIconTheme(key) => {
                self.icon_theme = key.clone();
                cx.emit(PluginsPageEvent::SelectIconTheme(key));
            }
            RowCommand::SetCapability { capability, granted } => {
                self.set_capability(plugin_id, &capability, granted, cx);
            }
        }
        cx.notify();
    }

    fn install_catalog(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.catalog.iter().flatten().find(|entry| entry.id == id) else { return };
        let path = entry.path.to_string_lossy().into_owned();
        self.start_install(Some(&path), false, false, false, false, cx);
    }

    fn save_secret(&mut self, plugin_id: &str, name: &str, value: Option<String>, cx: &mut Context<Self>) {
        let key = (plugin_id.to_owned(), name.to_owned());
        if self.secret_busy.contains(&key) || value.as_deref() == Some("") {
            return;
        }
        self.secret_errors.remove(&key);
        self.secret_busy.insert(key);
        cx.emit(PluginsPageEvent::SetSecret { plugin_id: plugin_id.to_owned(), name: name.to_owned(), value });
        cx.notify();
    }

    fn open_url(&mut self, url: &str, cx: &mut Context<Self>) {
        if is_web_homepage(url) {
            cx.emit(PluginsPageEvent::OpenUrl(url.to_owned()));
        }
    }

    // --- the steps ------------------------------------------------------------------------

    fn notify(&mut self, level: NotificationLevel, title: String, body: Option<String>, cx: &mut Context<Self>) {
        let request =
            NotificationRequest { group: Some("plugins".to_owned()), body, ..NotificationRequest::new(level, title) };
        cx.emit(PluginsPageEvent::Notify(request));
    }

    fn refresh_plugins(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        cx.emit(PluginsPageEvent::Refresh { plugins: true, catalog: false, market: None });
        cx.notify();
    }

    /// Marks `id` busy for `operation`; false while another operation runs for it.
    fn begin(&mut self, id: &str, operation: Operation, failure: String, cx: &mut Context<Self>) -> bool {
        if self.busy.contains_key(id) {
            return false;
        }
        let name = self.name_of(id);
        self.busy.insert(id.to_owned(), Busy { operation, failure, name });
        cx.notify();
        true
    }

    fn send_grant(&mut self, id: &str, consent: Consent, cx: &mut Context<Self>) {
        let failure = i18n(cx).tx_built(&["Could not enable ", ""], &[text(&self.name_of(id))]);
        if self.begin(id, Operation::Other, failure, cx) {
            let Consent { optional, signer_change } = consent;
            cx.emit(PluginsPageEvent::Grant { id: id.to_owned(), optional, signer_change });
        }
    }

    fn download(&mut self, id: &str, quiet: bool, show: bool, cx: &mut Context<Self>) {
        if self.installing() {
            return;
        }
        self.downloading = Some((id.to_owned(), quiet, show));
        cx.emit(PluginsPageEvent::DownloadMarket(id.to_owned()));
        cx.notify();
    }

    fn start_install(
        &mut self,
        path: Option<&str>,
        package_file: bool,
        quiet: bool,
        show: bool,
        market: bool,
        cx: &mut Context<Self>,
    ) {
        if self.install.is_some() {
            return;
        }
        let (flow, step) = InstallFlow::start(path, package_file, quiet);
        self.install = Some(Install { flow, show, market });
        self.step(step, cx);
    }

    /// Carries an install on: the consent dialog, a backend step, or the end.
    fn step(&mut self, step: InstallStep, cx: &mut Context<Self>) {
        match step {
            InstallStep::AskConsent(request) => {
                self.asking = Some(Asking::Install);
                cx.emit(PluginsPageEvent::AskConsent(request));
            }
            InstallStep::Done { installed, notification } => {
                let install = self.install.take();
                if let Some(notification) = notification {
                    cx.emit(PluginsPageEvent::Notify(notification));
                }
                let market = install.as_ref().is_some_and(|install| install.market);
                self.loading = true;
                cx.emit(PluginsPageEvent::Refresh { plugins: true, catalog: false, market: market.then_some(false) });
                if let Some(id) = installed {
                    self.adopt.insert(id.clone());
                    if let Some(run) = &mut self.updates {
                        run.updated += 1;
                    }
                    if install.is_some_and(|install| install.show) {
                        self.show_installed(&id, cx);
                    }
                }
                self.next_update(cx);
            }
            step => cx.emit(PluginsPageEvent::Install(step)),
        }
        cx.notify();
    }

    /// Update All goes on with the next update, or reports how many were updated.
    fn next_update(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &mut self.updates else { return };
        let Some(queue) = &mut run.queue else { return };
        if let Some(id) = queue.pop_front() {
            self.download(&id, true, false, cx);
            return;
        }
        let updated = run.updated;
        self.updates = None;
        if updated > 0 {
            let title = i18n(cx).tx_built(&["Updated ", " plugins"], &[Param::Number(updated as f64)]);
            self.notify(NotificationLevel::Info, title, None, cx);
        }
    }

    /// Reads the secrets of the selected plugin when it declares some and they are not known yet.
    fn request_secrets(&mut self, cx: &mut Context<Self>) {
        if self.tab != PluginsTab::Installed {
            return;
        }
        let Some(plugin) = self.selected_plugin() else { return };
        let id = plugin.id.clone();
        let declares = self.contributions.iter().any(|c| c.plugin_id == id && c.point == "secrets");
        if declares && !self.secrets.contains_key(&id) && self.secrets_requested.insert(id.clone()) {
            cx.emit(PluginsPageEvent::LoadSecrets(id));
        }
    }

    // --- what is shown --------------------------------------------------------------------

    fn search_index(&self) -> usize {
        match self.tab {
            PluginsTab::Marketplace => 0,
            PluginsTab::Installed => 1,
        }
    }

    fn name_of(&self, id: &str) -> String {
        self.plugins.iter().find(|p| p.id == id).map_or_else(|| id.to_owned(), |p| p.name.clone())
    }

    /// An install, or the download before it, is running.
    pub fn installing(&self) -> bool {
        self.install.is_some() || self.downloading.is_some()
    }

    pub fn tab(&self) -> PluginsTab {
        self.tab
    }

    pub fn detail_tab(&self) -> DetailTab {
        self.detail_tab
    }

    /// The English placeholder of the shown search field.
    pub fn placeholder(&self) -> &'static str {
        match self.tab {
            PluginsTab::Marketplace => "Search plugins",
            PluginsTab::Installed => "Search installed plugins",
        }
    }

    /// The search field of the shown tab.
    pub fn search_field(&self) -> &Entity<TextField> {
        &self.search[self.search_index()]
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The marketplace updates of installed plugins: what Update All installs.
    pub fn updates_offered(&self) -> Vec<String> {
        self.market.iter().flatten().filter(|entry| entry.is_update()).map(|entry| entry.entry.id.clone()).collect()
    }

    fn shown_installed(&self) -> Vec<&PluginInfo> {
        filter_plugins(&self.plugins, &self.query)
    }

    fn selected_plugin(&self) -> Option<&PluginInfo> {
        let shown = self.shown_installed();
        shown.iter().find(|p| Some(&p.id) == self.selected_id.as_ref()).or(shown.first()).copied()
    }

    fn shown_market(&self) -> Vec<&MarketPlugin> {
        let query = self.query.trim().to_lowercase();
        self.market
            .iter()
            .flatten()
            .filter(|m| self.category.is_none_or(|category| categories_of(m).contains(&category)))
            .filter(|m| {
                let e = &m.entry;
                let mut texts = vec![e.name.as_str(), e.id.as_str(), e.description.as_str(), e.publisher.as_str()];
                texts.extend(e.authors.iter().map(String::as_str));
                query.is_empty() || matches(&texts, &query)
            })
            .collect()
    }

    /// Local packages have no categories: they show only without a category filter.
    fn shown_catalog(&self) -> Vec<&CatalogPlugin> {
        match (&self.catalog, self.category) {
            (Some(catalog), None) => filter_catalog(catalog, &self.query),
            _ => Vec::new(),
        }
    }

    fn selected_market_entry(&self) -> Option<&MarketPlugin> {
        let id = self.selected_market.as_ref()?;
        self.shown_market().into_iter().find(|m| &m.entry.id == id)
    }

    fn selected_catalog_entry(&self) -> Option<&CatalogPlugin> {
        let shown = self.shown_catalog();
        shown.iter().find(|c| Some(&c.id) == self.selected_catalog.as_ref()).or(shown.first()).copied()
    }

    /// What the details show now.
    fn detail_target(&self) -> Option<(ItemKind, String)> {
        match self.tab {
            PluginsTab::Installed => self.selected_plugin().map(|p| (ItemKind::Installed, p.id.clone())),
            PluginsTab::Marketplace => match self.selected_market_entry() {
                Some(entry) => Some((ItemKind::Market, entry.entry.id.clone())),
                None => self.selected_catalog_entry().map(|entry| (ItemKind::Catalog, entry.id.clone())),
            },
        }
    }

    /// Another plugin's details start on Overview, without the uninstall confirmation; the same
    /// plugin listed again keeps them.
    fn sync_detail(&mut self) {
        let target = self.detail_target();
        if target != self.detail_key {
            self.detail_key = target;
            self.detail_tab = DetailTab::Overview;
            self.confirm_uninstall = false;
        }
    }

    fn icon_name(&mut self, id: &str, icon: Option<IconMarkup<'_>>) -> String {
        let (name, registrations) = self.icons.name(id, icon);
        if !registrations.is_empty() {
            theme::icons::register_external_icons(
                registrations.into_iter().map(|(name, svg)| (name, svg.into_bytes())),
            );
        }
        name
    }

    /// The list of the shown tab.
    pub fn rows(&mut self, cx: &mut App) -> Vec<ListRow> {
        self.sync_detail();
        match self.tab {
            PluginsTab::Installed => self.installed_rows(cx),
            PluginsTab::Marketplace => self.available_rows(cx),
        }
    }

    fn installed_rows(&mut self, cx: &mut App) -> Vec<ListRow> {
        let selected = self.selected_plugin().map(|p| p.id.clone());
        let shown: Vec<PluginInfo> = self.shown_installed().into_iter().cloned().collect();
        let groups = [
            (PluginSource::Installed, "Downloaded"),
            (PluginSource::Linked, "Linked for development"),
            (PluginSource::Dev, "Project (.openidea/plugins)"),
        ];
        let mut rows = Vec::new();
        for (source, title) in groups {
            let members: Vec<&PluginInfo> = shown.iter().filter(|p| p.source == source).collect();
            if members.is_empty() {
                continue;
            }
            rows.push(ListRow::Header { title: tx(cx, title), count: members.len() });
            for plugin in members {
                let icon = self.icon_name(&plugin.id, markup!(plugin.icon));
                let i18n = i18n(cx);
                let mut tags = Vec::new();
                match plugin.source {
                    PluginSource::Linked => tags.push(Badge { label: i18n.tx("linked"), tone: Tone::Neutral }),
                    PluginSource::Dev => tags.push(Badge { label: i18n.tx("dev"), tone: Tone::Neutral }),
                    PluginSource::Installed => {}
                }
                match plugin.status {
                    PluginStatus::Failed => tags.push(Badge { label: i18n.tx("Failed"), tone: Tone::Danger }),
                    PluginStatus::NeedsConsent => {
                        tags.push(Badge { label: i18n.tx("Needs permission"), tone: Tone::Warning });
                    }
                    _ => {}
                }
                let vendor = if plugin.authors.is_empty() { plugin.id.clone() } else { plugin.authors.join(", ") };
                let tooltip = if plugin.description.is_empty() { &plugin.name } else { &plugin.description };
                rows.push(ListRow::Item(ListItem {
                    kind: ItemKind::Installed,
                    id: plugin.id.clone(),
                    name: plugin.name.clone(),
                    vendor,
                    version: plugin.version.clone(),
                    icon,
                    tooltip: tooltip.clone(),
                    tags,
                    enabled: Some((plugin.enabled, !self.busy.contains_key(&plugin.id))),
                    install: None,
                    selected: selected.as_ref() == Some(&plugin.id),
                }));
            }
        }
        if self.loaded && shown.is_empty() {
            let message = if self.plugins.is_empty() { "No plugins installed." } else { "No plugins match." };
            rows.push(ListRow::Message { text: tx(cx, message), error: false });
        }
        rows
    }

    fn available_rows(&mut self, cx: &mut App) -> Vec<ListRow> {
        let mut rows = Vec::new();
        if self.market.as_ref().is_some_and(|market| !market.is_empty()) {
            rows.push(ListRow::Category);
        }
        if let Some(error) = &self.market_error {
            rows.push(ListRow::Message { text: error.clone(), error: true });
        }
        let market: Vec<MarketPlugin> = self.shown_market().into_iter().cloned().collect();
        let selected_market = self.selected_market_entry().map(|m| m.entry.id.clone());
        if !market.is_empty() {
            rows.push(ListRow::Header { title: tx(cx, "Marketplace"), count: market.len() });
        }
        for entry in &market {
            let icon = self.icon_name(&entry.entry.id, markup!(entry.entry.icon));
            let i18n = i18n(cx);
            let tag = if entry.is_update() {
                Some(Badge { label: i18n.tx("Update"), tone: Tone::Accent })
            } else if entry.installed.is_some() {
                Some(Badge { label: i18n.tx("Installed"), tone: Tone::Neutral })
            } else if !entry.compatible {
                Some(Badge { label: i18n.tx("Incompatible"), tone: Tone::Warning })
            } else {
                None
            };
            let e = &entry.entry;
            rows.push(ListRow::Item(ListItem {
                kind: ItemKind::Market,
                id: e.id.clone(),
                name: e.name.clone(),
                vendor: e.publisher.clone(),
                version: e.version.clone(),
                icon,
                tooltip: if e.description.is_empty() { e.name.clone() } else { e.description.clone() },
                tags: tag.into_iter().collect(),
                enabled: None,
                install: None,
                selected: selected_market.as_ref() == Some(&e.id),
            }));
        }
        let catalog: Vec<CatalogPlugin> = self.shown_catalog().into_iter().cloned().collect();
        let selected_catalog =
            if selected_market.is_some() { None } else { self.selected_catalog_entry().map(|c| c.id.clone()) };
        if !catalog.is_empty() {
            rows.push(ListRow::Header { title: tx(cx, "Local"), count: catalog.len() });
        }
        for entry in &catalog {
            let icon = self.icon_name(&entry.id, markup!(entry.icon));
            let (tags, install) = self.catalog_button(entry, cx);
            let vendor = if entry.authors.is_empty() { entry.id.clone() } else { entry.authors.join(", ") };
            rows.push(ListRow::Item(ListItem {
                kind: ItemKind::Catalog,
                id: entry.id.clone(),
                name: entry.name.clone(),
                vendor,
                version: entry.version.clone(),
                icon,
                tooltip: if entry.description.is_empty() { entry.name.clone() } else { entry.description.clone() },
                tags,
                enabled: None,
                install,
                selected: selected_catalog.as_ref() == Some(&entry.id),
            }));
        }
        if let Some(all) = &self.catalog
            && self.category.is_none()
            && catalog.is_empty()
        {
            let message = if all.is_empty() { "No local plugin packages." } else { "No plugins match." };
            rows.push(ListRow::Message { text: tx(cx, message), error: false });
        }
        if let Some(error) = &self.catalog_error {
            rows.push(ListRow::Message { text: error.clone(), error: true });
        }
        rows
    }

    /// A catalog package's tag (installed, or a development copy loaded) or its Install / Update button.
    fn catalog_button(&self, entry: &CatalogPlugin, cx: &mut App) -> (Vec<Badge>, Option<(String, bool)>) {
        let i18n = i18n(cx);
        match catalog_state(entry, &self.plugins) {
            CatalogState::Installed => (vec![Badge { label: i18n.tx("Installed"), tone: Tone::Neutral }], None),
            CatalogState::Dev => (vec![Badge { label: i18n.tx("Dev plugin loaded"), tone: Tone::Neutral }], None),
            state => {
                let label = if state == CatalogState::Update { "Update" } else { "Install" };
                let enabled = !self.installing() && entry.problem.is_none();
                (Vec::new(), Some((i18n.tx(label), enabled)))
            }
        }
    }

    /// The marketplace categories with plugins, with their counts; the first is all of them.
    pub fn category_options(&self, cx: &mut App) -> Vec<(Option<&'static str>, String, usize)> {
        let market = self.market.as_deref().unwrap_or_default();
        let mut options = vec![(None, tx(cx, "All Categories"), market.len())];
        for (id, name) in CATEGORIES {
            let count = market.iter().filter(|m| categories_of(m).contains(&id)).count();
            if count > 0 {
                options.push((Some(id), tx(cx, name), count));
            }
        }
        options
    }

    /// The gear menu's items.
    pub fn gear_items(&self, cx: &mut App) -> Vec<GearItem> {
        let i18n = i18n(cx);
        let updates = self.updates_offered().len();
        let installed: Vec<&PluginInfo> = self.plugins.iter().filter(|p| p.source == PluginSource::Installed).collect();
        let item = |command, label: String, enabled| GearItem { command: Some(command), label, enabled };
        let separator = || GearItem { command: None, label: String::new(), enabled: false };
        let update_label = if updates > 0 {
            i18n.tx_built(&["Update All Plugins (", ")"], &[Param::Number(updates as f64)])
        } else {
            i18n.tx("Update All Plugins")
        };
        let mut items = vec![
            item(GearCommand::UpdateAll, update_label, updates > 0 && self.updates.is_none()),
            separator(),
            item(GearCommand::InstallFromDisk, i18n.tx("Install Plugin from Disk…"), true),
            item(GearCommand::InstallPackageFile, i18n.tx("Install Plugin from Package File…"), true),
            item(GearCommand::EditRepository, i18n.tx("Plugin Repository…"), true),
        ];
        if self.can_import {
            items.push(separator());
            items.push(item(GearCommand::ImportInstalledIntellij, i18n.tx("Import Theme from JetBrains IDE…"), true));
            items.push(item(GearCommand::ImportIntellijFile, i18n.tx("Import IntelliJ Theme from File…"), true));
        }
        items.extend([
            separator(),
            item(
                GearCommand::DisableAll,
                i18n.tx("Disable All Downloaded Plugins"),
                installed.iter().any(|p| p.enabled),
            ),
            item(
                GearCommand::EnableAll,
                i18n.tx("Enable All Downloaded Plugins"),
                installed.iter().any(|p| !p.enabled),
            ),
            separator(),
            item(GearCommand::Rescan, i18n.tx("Rescan Plugins"), true),
        ]);
        items
    }

    /// The details of the selected plugin, package or marketplace entry.
    pub fn details(&mut self, cx: &mut App) -> Option<Details> {
        self.sync_detail();
        let (kind, id) = self.detail_key.clone()?;
        match kind {
            ItemKind::Installed => {
                let plugin = self.plugins.iter().find(|p| p.id == id)?.clone();
                Some(self.installed_details(&plugin, cx))
            }
            ItemKind::Catalog => {
                let entry = self.catalog.iter().flatten().find(|c| c.id == id)?.clone();
                Some(self.catalog_details(&entry, cx))
            }
            ItemKind::Market => {
                let entry = self.market.iter().flatten().find(|m| m.entry.id == id)?.clone();
                Some(self.market_details(&entry, cx))
            }
        }
    }

    fn fact(i18n: &I18n, label: &str, value: String, mono: bool) -> Fact {
        Fact { label: i18n.tx(label), value, mono, link: false }
    }

    fn code_fact(i18n: &I18n, has_code: bool) -> Fact {
        let code = if has_code { "WebAssembly component (sandboxed)" } else { "None — contributions only" };
        Self::fact(i18n, "Code", i18n.tx(code), false)
    }

    fn homepage_fact(i18n: &I18n, homepage: Option<&str>) -> Option<Fact> {
        let value = homepage.filter(|value| !value.is_empty())?;
        Some(Fact { label: i18n.tx("Homepage"), value: value.to_owned(), mono: true, link: is_web_homepage(value) })
    }

    fn installed_details(&mut self, plugin: &PluginInfo, cx: &mut App) -> Details {
        let icon = self.icon_name(&plugin.id, markup!(plugin.icon));
        let busy = self.busy.contains_key(&plugin.id);
        let confirmed = plugin.status != PluginStatus::NeedsConsent;
        let i18n = i18n(cx);
        let (status, tone) = match plugin.status {
            PluginStatus::Active => ("Running", Tone::Success),
            PluginStatus::Inactive | PluginStatus::Static => ("Enabled", Tone::Accent),
            PluginStatus::Disabled => ("Disabled", Tone::Neutral),
            PluginStatus::NeedsConsent => ("Needs permission", Tone::Warning),
            PluginStatus::Failed => ("Failed", Tone::Danger),
        };
        let mut badges = vec![Badge { label: i18n.tx(status), tone }];
        match (plugin.source, &plugin.publisher) {
            (PluginSource::Linked, _) => badges.push(Badge { label: i18n.tx("Linked plugin"), tone: Tone::Warning }),
            (PluginSource::Dev, _) => badges.push(Badge { label: i18n.tx("Project plugin"), tone: Tone::Warning }),
            (PluginSource::Installed, Some(publisher)) => badges
                .push(Badge { label: i18n.tx_built(&["Signed by ", ""], &[text(publisher)]), tone: Tone::Success }),
            (PluginSource::Installed, None) => badges.push(Badge { label: i18n.tx("Unsigned"), tone: Tone::Neutral }),
        }
        let failed = plugin.status == PluginStatus::Failed;
        let toggle = match (plugin.enabled, failed) {
            (true, true) => "Restart",
            (true, false) => "Disable",
            (false, _) => "Enable",
        };
        let mut buttons = vec![
            ShownButton { id: DetailButton::Toggle, label: i18n.tx(toggle), primary: !plugin.enabled, enabled: !busy },
            ShownButton {
                id: DetailButton::Reload,
                label: i18n.tx("Reload from disk"),
                primary: false,
                enabled: !busy,
            },
        ];
        if let Some(previous) = &plugin.previous_version {
            let label = i18n.tx_built(&["Roll Back to ", ""], &[text(previous)]);
            buttons.push(ShownButton { id: DetailButton::RollBack, label, primary: false, enabled: !busy });
        }
        if plugin.source == PluginSource::Installed {
            if self.confirm_uninstall {
                let (uninstall, cancel) = (i18n.tx("Uninstall"), i18n.tx("Cancel"));
                let confirm = DetailButton::ConfirmUninstall;
                buttons.push(ShownButton { id: confirm, label: uninstall, primary: true, enabled: !busy });
                let cancel_id = DetailButton::CancelUninstall;
                buttons.push(ShownButton { id: cancel_id, label: cancel, primary: false, enabled: true });
            } else {
                let label = i18n.tx("Uninstall…");
                buttons.push(ShownButton { id: DetailButton::Uninstall, label, primary: false, enabled: !busy });
            }
        }
        let notice = (plugin.status == PluginStatus::NeedsConsent).then(|| {
            buttons.push(ShownButton {
                id: DetailButton::ReviewPermissions,
                label: i18n.tx("Review Permissions…"),
                primary: true,
                enabled: !busy && self.asking.is_none(),
            });
            i18n.tx(if plugin.source == PluginSource::Installed {
                "This version asks for new permissions. It stays off until you allow them."
            } else {
                "Plugins from a project folder run only after you allow them."
            })
        });
        let mut facts = vec![
            Self::fact(i18n, "Id", plugin.id.clone(), true),
            Self::fact(i18n, "Location", plugin.path.to_string_lossy().into_owned(), true),
            Self::code_fact(i18n, plugin.has_code),
        ];
        facts.extend(Self::homepage_fact(i18n, plugin.homepage.as_deref()));

        let mut sections = vec![self.permissions(plugin, busy || !confirmed, i18n)];
        let mut points: Vec<&str> = Vec::new();
        for contribution in &plugin.contributions {
            if !points.contains(&contribution.point.as_str()) {
                points.push(&contribution.point);
            }
        }
        for point in points {
            sections.push(self.point_section(plugin, point, i18n));
        }
        let stats = &plugin.stats;
        if plugin.has_code && stats.activations > 0 {
            let ms = |value: f64| Param::Text(format!("{value:.1}"));
            let count = |value: u32| Param::Number(f64::from(value));
            let first = i18n.tx_built(
                &["Activated ", "× (last in ", " ms), ", " calls, ", " errors, slowest ", " ms"],
                &[
                    count(stats.activations),
                    ms(stats.activation_ms),
                    count(stats.calls),
                    count(stats.errors),
                    ms(stats.max_call_ms),
                ],
            );
            let second = i18n.tx_built(
                &["Recent calls p95 ", " ms; ", " crashes, ", " editor results too late, reclaimed ", "× when idle"],
                &[ms(stats.p95_call_ms), count(stats.crashes), count(stats.slow_calls), count(stats.reclaims)],
            );
            sections.push(Section { title: i18n.tx("Statistics"), notes: vec![first, second], ..Section::default() });
        }
        Details {
            kind: ItemKind::Installed,
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            authors: (!plugin.authors.is_empty()).then(|| plugin.authors.join(", ")),
            version: plugin.version.clone(),
            icon,
            badges,
            buttons,
            busy,
            error: plugin.error.clone(),
            notice,
            description: (!plugin.description.is_empty()).then(|| plugin.description.clone()),
            facts,
            warnings: plugin.warnings.clone(),
            sections,
            notes: Some(plugin.change_notes.as_ref().map(|notes| notes.trim().to_owned()).filter(|n| !n.is_empty())),
        }
    }

    /// Permissions: each capability with its risk, what it allows and a checkbox to grant or
    /// revoke it (not while the plugin waits for consent or is busy).
    fn permissions(&self, plugin: &PluginInfo, locked: bool, i18n: &mut I18n) -> Section {
        let capabilities = sort_by_risk(&capabilities_of(plugin));
        let title = i18n.tx("Permissions");
        if capabilities.is_empty() {
            return Section { title, notes: vec![i18n.tx("No special permissions.")], ..Section::default() };
        }
        let rows = capabilities
            .iter()
            .map(|capability| {
                let label = if capability.granted { ["Revoke ", ""] } else { ["Allow ", ""] };
                SectionRow {
                    main: capability.id.clone(),
                    mono: None,
                    badge: Some(risk_badge(capability.risk, i18n)),
                    description: Some(capability_description(capability, i18n)),
                    check: Some((capability.granted, !locked)),
                    action: Some(RowAction {
                        label: i18n.tx_built(&label, &[text(&capability.id)]),
                        primary: false,
                        enabled: !locked,
                        command: RowCommand::SetCapability {
                            capability: capability.id.clone(),
                            granted: !capability.granted,
                        },
                        selector: format!("plugins-capability-{}", capability.id),
                    }),
                    ..SectionRow::default()
                }
            })
            .collect();
        Section { title, rows, ..Section::default() }
    }

    /// A contribution point's section: its own rows for commands, themes, icon themes and secrets
    /// while the plugin is live, the labels the plugin lists otherwise.
    fn point_section(&self, plugin: &PluginInfo, point: &str, i18n: &mut I18n) -> Section {
        let title = i18n.tx(point_title(point));
        let items: Vec<&Contribution> =
            self.contributions.iter().filter(|c| c.plugin_id == plugin.id && c.point == point).collect();
        let labels = || -> Vec<SectionRow> {
            plugin
                .contributions
                .iter()
                .filter(|c| c.point == point)
                .map(|c| SectionRow { main: c.label.clone(), ..SectionRow::default() })
                .collect()
        };
        if items.is_empty() {
            return Section { title, rows: labels(), ..Section::default() };
        }
        let live = plugin.status.is_live();
        match point {
            "commands" => {
                let runnable = matches!(plugin.status, PluginStatus::Active | PluginStatus::Inactive);
                let rows = items
                    .iter()
                    .filter_map(|item| {
                        let id = item.text("id")?;
                        let title = item.text("title").unwrap_or(id);
                        Some(SectionRow {
                            main: title.to_owned(),
                            mono: Some(plugin_action_id(&plugin.id, id)),
                            action: Some(RowAction {
                                label: i18n.tx("Run"),
                                primary: false,
                                enabled: runnable,
                                command: RowCommand::RunCommand(id.to_owned()),
                                selector: format!("plugins-run-{id}"),
                            }),
                            ..SectionRow::default()
                        })
                    })
                    .collect();
                Section { title, rows, ..Section::default() }
            }
            "themes" => {
                let rows = items
                    .iter()
                    .filter_map(|item| {
                        let id = item.text("id")?;
                        let name = item.text("name").unwrap_or(id);
                        let light = item.text("kind") == Some("light");
                        let choice = plugin_theme_ref(&plugin.id, id);
                        let action = if self.theme_choice.as_deref() == Some(choice.as_str()) {
                            RowAction {
                                label: i18n.tx("Use Default Theme"),
                                primary: false,
                                enabled: true,
                                command: RowCommand::SetTheme(if light { "light" } else { "dark" }.to_owned()),
                                selector: format!("plugins-default-theme-{id}"),
                            }
                        } else {
                            RowAction {
                                label: i18n.tx("Apply"),
                                primary: false,
                                enabled: live,
                                command: RowCommand::SetTheme(choice),
                                selector: format!("plugins-apply-theme-{id}"),
                            }
                        };
                        Some(SectionRow {
                            main: name.to_owned(),
                            detail: Some(i18n.tx(if light { "Light" } else { "Dark" })),
                            action: Some(action),
                            ..SectionRow::default()
                        })
                    })
                    .collect();
                Section { title, rows, ..Section::default() }
            }
            "iconThemes" => {
                let rows = items
                    .iter()
                    .filter_map(|item| {
                        let id = item.text("id")?;
                        let label = item.text("label").unwrap_or(id);
                        let key = icon_theme_key(&plugin.id, id);
                        let count = item.decl.get("icons").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                        let action = if self.icon_theme.as_deref() == Some(key.as_str()) {
                            RowAction {
                                label: i18n.tx("Use Default Icons"),
                                primary: false,
                                enabled: true,
                                command: RowCommand::SelectIconTheme(None),
                                selector: format!("plugins-default-icons-{id}"),
                            }
                        } else {
                            RowAction {
                                label: i18n.tx("Apply"),
                                primary: false,
                                enabled: true,
                                command: RowCommand::SelectIconTheme(Some(key)),
                                selector: format!("plugins-apply-icons-{id}"),
                            }
                        };
                        Some(SectionRow {
                            main: label.to_owned(),
                            detail: Some(i18n.tx_built(&["", " icons"], &[Param::Number(count)])),
                            action: Some(action),
                            ..SectionRow::default()
                        })
                    })
                    .collect();
                Section { title, rows, ..Section::default() }
            }
            "secrets" => {
                // A secret's row, with the field its value is typed in, shows only once its state has
                // been read: until then the page cannot say whether a value is stored.
                let state = self.secrets.get(&plugin.id);
                let secrets = items
                    .iter()
                    .filter(|_| state.is_some())
                    .filter_map(|item| {
                        let name = item.text("name")?.to_owned();
                        let key = (plugin.id.clone(), name.clone());
                        Some(SecretRow {
                            label: item.text("label").unwrap_or(&name).to_owned(),
                            description: item.text("description").map(str::to_owned),
                            set: state.is_some_and(|state| state.iter().any(|(n, set)| *n == name && *set)),
                            busy: self.secret_busy.contains(&key),
                            error: self.secret_errors.get(&key).cloned(),
                            name,
                        })
                    })
                    .collect();
                let note = i18n.tx("The plugin can only send these with its network requests; it cannot read them.");
                Section { title, secrets, notes: vec![note], ..Section::default() }
            }
            _ => Section { title, rows: labels(), ..Section::default() },
        }
    }

    fn catalog_details(&mut self, entry: &CatalogPlugin, cx: &mut App) -> Details {
        let icon = self.icon_name(&entry.id, markup!(entry.icon));
        let (badges, install) = self.catalog_button(entry, cx);
        let i18n = i18n(cx);
        let mut facts = vec![
            Self::fact(i18n, "Id", entry.id.clone(), true),
            Self::fact(i18n, "Source", entry.path.to_string_lossy().into_owned(), true),
            Self::code_fact(i18n, entry.has_code),
        ];
        facts.extend(Self::homepage_fact(i18n, entry.homepage.as_deref()));
        if !entry.contributions.is_empty() {
            let contributes: Vec<String> = entry
                .contributions
                .iter()
                .map(|c| format!("{}: {}", i18n.tx(point_title(&c.point)), c.count))
                .collect();
            facts.push(Self::fact(i18n, "Contributes", contributes.join(", "), false));
        }
        let capabilities = sort_by_risk(&listed_capabilities(&entry.capabilities));
        let mut sections = Vec::new();
        if !capabilities.is_empty() {
            let rows = capabilities
                .iter()
                .map(|capability| SectionRow {
                    main: capability.id.clone(),
                    badge: Some(risk_badge(capability.risk, i18n)),
                    description: Some(capability_description(capability, i18n)),
                    ..SectionRow::default()
                })
                .collect();
            sections.push(Section { title: i18n.tx("Permissions"), rows, ..Section::default() });
        }
        let buttons = install
            .map(|(label, enabled)| ShownButton { id: DetailButton::Install, label, primary: true, enabled })
            .into_iter()
            .collect();
        Details {
            kind: ItemKind::Catalog,
            id: entry.id.clone(),
            name: entry.name.clone(),
            authors: (!entry.authors.is_empty()).then(|| entry.authors.join(", ")),
            version: entry.version.clone(),
            icon,
            badges,
            buttons,
            busy: false,
            error: entry.problem.clone(),
            notice: None,
            description: (!entry.description.is_empty()).then(|| entry.description.clone()),
            facts,
            warnings: Vec::new(),
            sections,
            notes: Some(entry.change_notes.as_ref().map(|notes| notes.trim().to_owned()).filter(|n| !n.is_empty())),
        }
    }

    fn market_details(&mut self, entry: &MarketPlugin, cx: &mut App) -> Details {
        let e = &entry.entry;
        let icon = self.icon_name(&e.id, markup!(e.icon));
        let installing = self.installing();
        let i18n = i18n(cx);
        let update = entry.is_update();
        let mut buttons = Vec::new();
        if entry.compatible && (entry.installed.is_none() || update) {
            let label =
                if update { i18n.tx_built(&["Update to ", ""], &[text(&e.version)]) } else { i18n.tx("Install") };
            buttons.push(ShownButton { id: DetailButton::Install, label, primary: true, enabled: !installing });
        }
        let notice = (!entry.compatible)
            .then(|| i18n.tx_built(&["Needs plugin API ", ". Update OpenIDEA to install it."], &[text(&e.engine)]));
        let categories: Vec<String> = categories_of(entry).into_iter().map(|c| i18n.tx(category_name(c))).collect();
        let mut facts = vec![
            Self::fact(i18n, "Id", e.id.clone(), true),
            Self::fact(i18n, "Category", categories.join(", "), false),
        ];
        if let Some(installed) = &entry.installed {
            facts.push(Self::fact(i18n, "Installed", installed.clone(), false));
        }
        let mut sections = Vec::new();
        if !e.capabilities.is_empty() {
            let rows =
                e.capabilities.iter().map(|id| SectionRow { main: id.clone(), ..SectionRow::default() }).collect();
            sections.push(Section { title: i18n.tx("Permissions"), rows, ..Section::default() });
        }
        Details {
            kind: ItemKind::Market,
            id: e.id.clone(),
            name: e.name.clone(),
            authors: Some(if e.authors.is_empty() { e.publisher.clone() } else { e.authors.join(", ") }),
            version: e.version.clone(),
            icon,
            badges: vec![Badge {
                label: i18n.tx_built(&["Signed by ", ""], &[text(&e.publisher)]),
                tone: Tone::Success,
            }],
            buttons,
            busy: false,
            error: None,
            notice,
            description: (!e.description.is_empty()).then(|| e.description.clone()),
            facts,
            warnings: Vec::new(),
            sections,
            notes: None,
        }
    }

    /// The empty details: their title and the text under it, translated.
    pub fn empty_details(&self, cx: &mut App) -> (String, Option<String>) {
        match self.tab {
            PluginsTab::Installed => {
                let title = tx(cx, if self.loaded { "No plugin selected" } else { "Loading…" });
                let body = tx(
                    cx,
                    "Plugins are folders with a plugin.toml. Use the gear menu to install one from disk, or put dev \
                     plugins in .openidea/plugins/ inside your project.",
                );
                (title, Some(body))
            }
            PluginsTab::Marketplace if self.market_url.is_some() => (tx(cx, "No plugin selected"), None),
            PluginsTab::Marketplace => {
                let body =
                    tx(cx, "Install a plugin folder with plugin.toml via the gear menu › Install Plugin from Disk….");
                (tx(cx, NO_REPOSITORY), Some(body))
            }
        }
    }
}

// --- drawing --------------------------------------------------------------------------------

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(BUTTON_HEIGHT))
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .whitespace_nowrap()
        .child(SharedString::from(label));
    let face = if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    };
    if enabled { face } else { face.opacity(0.5) }
}

/// A check box.
fn check_box(cx: &App, on: bool, enabled: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    let face = div()
        .flex_none()
        .size(px(16.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .border_1()
        .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
        .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
        .children(mark);
    if enabled { face } else { face.opacity(0.5) }
}

fn badge(cx: &App, badge: &Badge) -> Div {
    let tone = theme::color(cx, badge.tone.token());
    div()
        .flex_none()
        .px(px(6.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(tone)
        .text_color(tone)
        .text_size(theme::length(cx, "fs-tiny"))
        .whitespace_nowrap()
        .child(SharedString::from(badge.label.clone()))
}

/// A tab of a tab strip, underlined when selected.
fn tab(cx: &App, label: String, selected: bool, trailing: Option<String>) -> Div {
    let strong = theme::color(cx, "fg-strong");
    div()
        .flex_none()
        .h_full()
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .when(selected, |el| el.border_b_1().border_color(theme::color(cx, "accent")))
        .text_color(if selected { strong } else { theme::color(cx, "fg-muted") })
        .child(SharedString::from(label))
        .children(trailing.map(|count| {
            div().text_color(theme::color(cx, "fg-muted")).text_size(theme::length(cx, "fs-small")).child(count)
        }))
}

impl PluginsPage {
    fn render_top_bar(&self, cx: &mut Context<Self>) -> Div {
        let (available, installed) = (tx(cx, "Available"), tx(cx, "Installed"));
        let preview = Badge { label: tx(cx, "Preview"), tone: Tone::Accent };
        let status = if self.installing() {
            Some(tx(cx, "Installing plugin"))
        } else if self.loading {
            Some(tx(cx, "Loading plugins"))
        } else {
            None
        };
        let muted = theme::color(cx, "fg-muted");
        let gear = theme::icon(cx, "settings", 16.0, theme::color(cx, "fg-icon"));
        div()
            .flex_none()
            .h(px(TOP_BAR_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(
                tab(cx, available, self.tab == PluginsTab::Marketplace, None)
                    .id("plugins-tab-marketplace")
                    .debug_selector(|| "plugins-tab-marketplace".to_owned())
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| page.select_tab(PluginsTab::Marketplace, cx))),
            )
            .child(
                tab(cx, installed, self.tab == PluginsTab::Installed, Some(self.plugins.len().to_string()))
                    .id("plugins-tab-installed")
                    .debug_selector(|| "plugins-tab-installed".to_owned())
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| page.select_tab(PluginsTab::Installed, cx))),
            )
            .child(badge(cx, &preview))
            .child(div().flex_1())
            .children(status.map(|status| {
                div().text_color(muted).text_size(theme::length(cx, "fs-small")).child(SharedString::from(status))
            }))
            .child(
                div()
                    .id("plugins-gear")
                    .debug_selector(|| "plugins-gear".to_owned())
                    .flex_none()
                    .size(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(gear)
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                        page.gear_open = !page.gear_open;
                        cx.notify();
                    })),
            )
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> Div {
        let rows = self.rows(cx);
        let category = self.category_options(cx).into_iter().find(|(id, _, _)| *id == self.category);
        let (fg, muted, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-strong"));
        let small = theme::length(cx, "fs-small");
        let elements: Vec<AnyElement> = rows
            .into_iter()
            .map(|row| match row {
                ListRow::Category => {
                    let label = category.as_ref().map(|(_, label, _)| label.clone()).unwrap_or_default();
                    div()
                        .id("plugins-category")
                        .debug_selector(|| "plugins-category".to_owned())
                        .mx(px(8.0))
                        .my(px(4.0))
                        .h(theme::length(cx, "h-control"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded(theme::length(cx, "radius-control"))
                        .border_1()
                        .border_color(theme::color(cx, "border"))
                        .bg(theme::color(cx, "bg-control"))
                        .child(SharedString::from(label))
                        .child(theme::icon(cx, "chevron-down", 12.0, muted))
                        .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                            page.category_open = !page.category_open;
                            cx.notify();
                        }))
                        .into_any_element()
                }
                ListRow::Header { title, count } => div()
                    .flex()
                    .justify_between()
                    .px(px(10.0))
                    .pt(px(8.0))
                    .pb(px(2.0))
                    .text_size(small)
                    .text_color(muted)
                    .child(SharedString::from(title))
                    .child(SharedString::from(count.to_string()))
                    .into_any_element(),
                ListRow::Message { text, error } => div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_color(if error { theme::color(cx, "danger") } else { muted })
                    .child(SharedString::from(text))
                    .into_any_element(),
                ListRow::Item(item) => self.render_item(item, fg, strong, muted, cx).into_any_element(),
            })
            .collect();
        let field = self.search[self.search_index()].clone();
        div()
            .flex_none()
            .w(px(SIDE_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(
                div().flex_none().p(px(8.0)).child(
                    div()
                        .h(theme::length(cx, "h-field"))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, "border"))
                        .bg(theme::color(cx, "bg-input"))
                        .child(theme::icon(cx, "search", 14.0, muted))
                        .child(div().flex_1().min_w_0().debug_selector(|| "plugins-search".to_owned()).child(field)),
                ),
            )
            .child(div().id("plugins-list").flex_1().min_h_0().overflow_y_scroll().pb(px(8.0)).children(elements))
    }

    fn render_item(
        &self,
        item: ListItem,
        fg: gpui_kit::Hsla,
        strong: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
        cx: &mut Context<Self>,
    ) -> Div {
        let prefix = match item.kind {
            ItemKind::Installed => "plugins-row",
            ItemKind::Catalog => "plugins-catalog-row",
            ItemKind::Market => "plugins-market-row",
        };
        let row_selector = format!("{prefix}-{}", item.id);
        let (kind, id) = (item.kind, item.id.clone());
        let dimmed = item.enabled.is_some_and(|(on, _)| !on);
        let main = div()
            .id(SharedString::from(row_selector.clone()))
            .debug_selector(move || row_selector.clone())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .with_tooltip(item.tooltip.clone())
            .child(theme::icon(cx, &item.icon, ICON_SIZE, theme::color(cx, "fg-icon")))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        div()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(strong)
                            .child(SharedString::from(item.name.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(theme::length(cx, "fs-small"))
                            .text_color(muted)
                            .child(SharedString::from(item.vendor.clone()))
                            .child(SharedString::from(item.version.clone()))
                            .children(item.tags.iter().map(|tag| badge(cx, tag))),
                    ),
            )
            .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.select(kind, &id, cx)));
        let check = item.enabled.map(|(on, enabled)| {
            let id = item.id.clone();
            let selector = format!("plugins-check-{}", item.id);
            check_box(cx, on, enabled)
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .when(enabled, |el| {
                    el.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.set_enabled(&id, !on, cx)))
                })
        });
        let install = item.install.clone().map(|(label, enabled)| {
            let id = item.id.clone();
            let selector = format!("plugins-catalog-install-{}", item.id);
            button(cx, label, false, enabled)
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .when(enabled, |el| {
                    el.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.install_catalog(&id, cx)))
                })
        });
        div()
            .h(px(ROW_HEIGHT))
            .mx(px(4.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .rounded(theme::length(cx, "radius-row"))
            .text_color(fg)
            .when(item.selected, |el| el.bg(theme::color(cx, "bg-selected")))
            .when(!item.selected, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .when(dimmed, |el| el.opacity(0.6))
            .child(main)
            .children(check)
            .children(install)
    }

    fn render_details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(details) = self.details(cx) else {
            let (title, body) = self.empty_details(cx);
            let muted = theme::color(cx, "fg-muted");
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .p(px(24.0))
                .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title)))
                .children(body.map(|body| div().text_color(muted).text_center().child(SharedString::from(body))))
                .into_any_element();
        };
        let (fg, muted, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-strong"));
        let danger = theme::color(cx, "danger");
        let buttons: Vec<Stateful<Div>> = details
            .buttons
            .iter()
            .map(|shown| {
                let id = shown.id;
                button(cx, shown.label.clone(), shown.primary, shown.enabled)
                    .id(id.selector())
                    .debug_selector(move || id.selector().to_owned())
                    .when(shown.enabled, |el| {
                        el.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.press(id, cx)))
                    })
            })
            .collect();
        let header = div()
            .flex()
            .items_start()
            .gap(px(12.0))
            .child(theme::icon(cx, &details.icon, LARGE_ICON_SIZE, theme::color(cx, "fg-icon")))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(theme::length(cx, "fs-input-large"))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(strong)
                            .child(SharedString::from(details.name.clone())),
                    )
                    .children(details.authors.clone().map(|authors| div().text_color(muted).child(authors)))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.0))
                            .child(div().text_color(muted).child(SharedString::from(details.version.clone())))
                            .children(details.badges.iter().map(|b| badge(cx, b))),
                    ),
            )
            .child(div().flex().flex_wrap().justify_end().gap(px(6.0)).children(buttons));
        let tabs = details.notes.is_some().then(|| {
            let (overview, whats_new) = (tx(cx, "Overview"), tx(cx, "What's New"));
            div()
                .h(theme::length(cx, "h-tab"))
                .flex()
                .border_b_1()
                .border_color(theme::color(cx, "border"))
                .child(
                    tab(cx, overview, self.detail_tab == DetailTab::Overview, None)
                        .id("plugins-details-overview")
                        .debug_selector(|| "plugins-details-overview".to_owned())
                        .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                            page.select_detail_tab(DetailTab::Overview, cx);
                        })),
                )
                .child(
                    tab(cx, whats_new, self.detail_tab == DetailTab::WhatsNew, None)
                        .id("plugins-details-whats-new")
                        .debug_selector(|| "plugins-details-whats-new".to_owned())
                        .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                            page.select_detail_tab(DetailTab::WhatsNew, cx);
                        })),
                )
        });
        let body = match (&details.notes, self.detail_tab) {
            (Some(notes), DetailTab::WhatsNew) => {
                let shown = match notes {
                    Some(notes) => div().font_family(self.code_font.clone()).text_color(fg).children(
                        notes.lines().map(|line| div().min_h(px(16.0)).child(SharedString::from(line.to_owned()))),
                    ),
                    None => {
                        let none = tx(cx, "This version has no release notes.");
                        div().text_color(muted).child(SharedString::from(none))
                    }
                };
                shown.into_any_element()
            }
            _ => self.render_overview(&details, cx).into_any_element(),
        };
        div()
            .id("plugins-details")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p(px(16.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .text_color(fg)
            .child(header)
            .children(details.error.clone().map(|error| div().text_color(danger).child(SharedString::from(error))))
            .children(tabs)
            .child(body)
            .into_any_element()
    }

    fn render_overview(&mut self, details: &Details, cx: &mut Context<Self>) -> Div {
        let (muted, strong) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-strong"));
        let link = theme::color(cx, "link");
        let notice = details.notice.clone().map(|notice| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .p(px(8.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-feedback-warning"))
                .bg(theme::color(cx, "bg-feedback-warning"))
                .child(theme::icon(cx, "info", 14.0, theme::color(cx, "warning")))
                .child(SharedString::from(notice))
        });
        let facts: Vec<Div> = details
            .facts
            .iter()
            .map(|fact| {
                let value = div()
                    .flex_1()
                    .min_w_0()
                    .when(fact.mono, |el| el.font_family(self.code_font.clone()))
                    .child(SharedString::from(fact.value.clone()));
                let value = if fact.link {
                    let url = fact.value.clone();
                    value
                        .id("plugins-homepage")
                        .debug_selector(|| "plugins-homepage".to_owned())
                        .text_color(link)
                        .cursor_pointer()
                        .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.open_url(&url, cx)))
                        .into_any_element()
                } else {
                    value.into_any_element()
                };
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(
                        div().flex_none().w(px(96.0)).text_color(muted).child(SharedString::from(fact.label.clone())),
                    )
                    .child(value)
            })
            .collect();
        let sections: Vec<Div> =
            details.sections.iter().map(|section| self.render_section(&details.id, section, cx)).collect();
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .children(notice)
            .children(details.description.clone().map(|d| div().text_color(strong).child(SharedString::from(d))))
            .child(div().flex().flex_col().gap(px(4.0)).children(facts))
            .children(details.warnings.iter().map(|w| div().text_color(muted).child(SharedString::from(w.clone()))))
            .children(sections)
    }

    fn render_section(&mut self, plugin_id: &str, section: &Section, cx: &mut Context<Self>) -> Div {
        let (muted, strong) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-strong"));
        let small = theme::length(cx, "fs-small");
        let rows: Vec<Div> = section
            .rows
            .iter()
            .map(|row| {
                let action = row.action.clone();
                let check = row.check.map(|(on, enabled)| {
                    let selector = action.as_ref().map_or_else(String::new, |action| action.selector.clone());
                    let face = check_box(cx, on, enabled)
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone());
                    match action.clone().filter(|_| enabled) {
                        Some(action) => {
                            let plugin = plugin_id.to_owned();
                            face.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                                page.run_row(&plugin, action.command.clone(), cx);
                            }))
                        }
                        None => face,
                    }
                });
                let action_button = action.filter(|_| row.check.is_none()).map(|action| {
                    let plugin = plugin_id.to_owned();
                    let selector = action.selector.clone();
                    let enabled = action.enabled;
                    button(cx, action.label.clone(), action.primary, enabled)
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .when(enabled, |el| {
                            el.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                                page.run_row(&plugin, action.command.clone(), cx);
                            }))
                        })
                });
                let mono_main = row.check.is_some() || row.badge.is_some();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .py(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .children(check)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .when(mono_main, |el| el.font_family(self.code_font.clone()))
                                    .child(SharedString::from(row.main.clone())),
                            )
                            .children(row.mono.clone().map(|mono| {
                                div().font_family(self.code_font.clone()).text_color(muted).text_size(small).child(mono)
                            }))
                            .children(row.detail.clone().map(|detail| div().text_color(muted).child(detail)))
                            .children(row.badge.as_ref().map(|b| badge(cx, b)))
                            .children(action_button),
                    )
                    .children(row.description.clone().map(|d| div().text_color(muted).text_size(small).child(d)))
            })
            .collect();
        let secrets: Vec<Div> =
            section.secrets.iter().map(|secret| self.render_secret(plugin_id, secret, cx)).collect();
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .text_color(strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(section.title.clone())),
            )
            .children(rows)
            .children(secrets)
            .children(section.notes.iter().map(|note| div().text_color(muted).child(SharedString::from(note.clone()))))
    }

    /// The field a secret's value is typed in; a new one when the secret's state changes, with its
    /// placeholder for that state.
    fn secret_field(&mut self, plugin_id: &str, secret: &SecretRow, cx: &mut Context<Self>) -> Entity<TextField> {
        let key = (plugin_id.to_owned(), secret.name.clone(), secret.set);
        if let Some(field) = self.secret_fields.get(&key) {
            return field.clone();
        }
        let placeholder = tx(cx, if secret.set { "Replace…" } else { "Value" });
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let (plugin, name) = (plugin_id.to_owned(), secret.name.clone());
        let subscription =
            cx.subscribe(&field, move |page: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Submitted => {
                    let value = field.read(cx).text().to_owned();
                    page.save_secret(&plugin, &name, Some(value), cx);
                }
                TextFieldEvent::Changed => cx.notify(),
                TextFieldEvent::Cancelled => {}
            });
        self._subscriptions.push(subscription);
        self.secret_fields.insert(key, field.clone());
        field
    }

    fn render_secret(&mut self, plugin_id: &str, secret: &SecretRow, cx: &mut Context<Self>) -> Div {
        let field = self.secret_field(plugin_id, secret, cx);
        let typed = field.read(cx).text().to_owned();
        let state = if secret.set {
            Badge { label: tx(cx, "Stored"), tone: Tone::Success }
        } else {
            Badge { label: tx(cx, "Not set"), tone: Tone::Neutral }
        };
        let can_save = !secret.busy && !typed.is_empty();
        let (plugin, name) = (plugin_id.to_owned(), secret.name.clone());
        let save_selector = format!("plugins-secret-save-{}", secret.name);
        let (save_label, remove_label) = (tx(cx, "Save"), tx(cx, "Remove"));
        let save = button(cx, save_label, false, can_save)
            .id(SharedString::from(save_selector.clone()))
            .debug_selector(move || save_selector.clone())
            .when(can_save, |el| {
                let field = field.clone();
                el.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                    let value = field.read(cx).text().to_owned();
                    page.save_secret(&plugin, &name, Some(value), cx);
                }))
            });
        let remove = secret.set.then(|| {
            let (plugin, name) = (plugin_id.to_owned(), secret.name.clone());
            let selector = format!("plugins-secret-remove-{}", secret.name);
            button(cx, remove_label, false, !secret.busy)
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .when(!secret.busy, |el| {
                    el.on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                        page.save_secret(&plugin, &name, None, cx);
                    }))
                })
        });
        let field_selector = format!("plugins-secret-field-{}", secret.name);
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .py(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_1().min_w_0().child(SharedString::from(secret.label.clone())))
                    .child(badge(cx, &state))
                    .child(
                        div()
                            .w(px(180.0))
                            .h(theme::length(cx, "h-field"))
                            .flex()
                            .items_center()
                            .px(px(6.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .border_1()
                            .border_color(theme::color(cx, "border"))
                            .bg(theme::color(cx, "bg-input"))
                            .debug_selector(move || field_selector.clone())
                            .child(field),
                    )
                    .child(save)
                    .children(remove),
            )
            .children(secret.description.clone().map(|d| div().text_color(theme::color(cx, "fg-muted")).child(d)))
            .children(secret.error.clone().map(|e| div().text_color(theme::color(cx, "danger")).child(e)))
    }

    fn render_gear(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.gear_open {
            return None;
        }
        let items = self.gear_items(cx);
        let (fg, faint) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-faint"));
        let hover = theme::color(cx, "bg-hover");
        let elements: Vec<AnyElement> = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| match item.command {
                None => div().my(px(4.0)).h(px(1.0)).bg(theme::color(cx, "border")).into_any_element(),
                Some(command) => {
                    let line = div()
                        .id(index)
                        .debug_selector(move || format!("plugins-gear-item-{index}"))
                        .h(theme::length(cx, "h-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .whitespace_nowrap()
                        .child(SharedString::from(item.label));
                    if item.enabled {
                        line.text_color(fg)
                            .hover(|el| el.bg(hover))
                            .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.run_gear(command, cx)))
                            .into_any_element()
                    } else {
                        line.text_color(faint).into_any_element()
                    }
                }
            })
            .collect();
        let menu = div()
            .w(px(MENU_WIDTH))
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(elements);
        let backdrop = div()
            .id("plugins-gear-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .justify_end()
            .pt(px(TOP_BAR_HEIGHT))
            .pr(px(8.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|page, _, _, cx| {
                    page.gear_open = false;
                    cx.notify();
                }),
            )
            .child(div().flex_none().child(menu));
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }

    fn render_categories(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.category_open || self.tab != PluginsTab::Marketplace {
            return None;
        }
        let options = self.category_options(cx);
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let hover = theme::color(cx, "bg-hover");
        let elements: Vec<AnyElement> = options
            .into_iter()
            .enumerate()
            .map(|(index, (category, label, count))| {
                div()
                    .id(index)
                    .debug_selector(move || format!("plugins-category-option-{index}"))
                    .h(theme::length(cx, "h-row"))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(4.0))
                    .text_color(fg)
                    .hover(|el| el.bg(hover))
                    .child(SharedString::from(label))
                    .child(div().text_color(muted).child(SharedString::from(count.to_string())))
                    .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| page.select_category(category, cx)))
                    .into_any_element()
            })
            .collect();
        let list = div()
            .w(px(SIDE_WIDTH - 16.0))
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(elements);
        // Under the search field and the category button.
        let top = TOP_BAR_HEIGHT + 2.0 * (8.0 + FIELD_HEIGHT);
        let backdrop = div()
            .id("plugins-category-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .pt(px(top))
            .pl(px(8.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|page, _, _, cx| {
                    page.category_open = false;
                    cx.notify();
                }),
            )
            .child(div().flex_none().child(list));
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }
}

/// The page's focus is the search field of the shown tab.
impl Focusable for PluginsPage {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search_field().focus_handle(cx)
    }
}

impl Render for PluginsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top_bar = self.render_top_bar(cx);
        let list = self.render_list(cx);
        let details = self.render_details(cx);
        let error = self.error.clone().map(|error| {
            div().px(px(12.0)).py(px(6.0)).text_color(theme::color(cx, "danger")).child(SharedString::from(error))
        });
        let gear = self.render_gear(cx);
        let categories = self.render_categories(cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(top_bar)
            .children(error)
            .child(div().flex_1().min_h_0().flex().child(list).child(details))
            .children(gear)
            .children(categories)
    }
}

// --- the consent dialog ---------------------------------------------------------------------

/// How the consent dialog was closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsentDialogEvent {
    /// Install or Allow, with the optional capabilities accepted and, when the dialog warned about
    /// another signer, that change accepted too.
    Accepted {
        optional: Vec<String>,
        signer_change: bool,
    },
    Cancelled,
}

impl ConsentDialogEvent {
    /// The answer for [`PluginsPage::consented`]; `None` when cancelled.
    pub fn consent(&self) -> Option<Consent> {
        match self {
            ConsentDialogEvent::Accepted { optional, signer_change } => {
                Some(Consent { optional: optional.clone(), signer_change: *signer_change })
            }
            ConsentDialogEvent::Cancelled => None,
        }
    }
}

/// The install page of a package, or the permission prompt of a loaded plugin: every capability
/// with its risk and what it allows; optional ones can be refused, required ones are marked.
pub struct ConsentDialog {
    request: ConsentRequest,
    capabilities: Vec<Capability>,
    accepted: Vec<String>,
    focus: FocusHandle,
    code_font: SharedString,
    key_claim: u64,
}

impl EventEmitter<ConsentDialogEvent> for ConsentDialog {}

impl ConsentDialog {
    /// The dialog for `request`; optional capabilities granted before and the low-risk ones start accepted.
    pub fn new(request: ConsentRequest, cx: &mut Context<Self>) -> Self {
        let capabilities = sort_by_risk(&request.capabilities);
        let accepted = initially_accepted(&capabilities);
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { request, capabilities, accepted, focus, code_font: code_font(cx), key_claim }
    }

    pub fn request(&self) -> &ConsentRequest {
        &self.request
    }

    /// The capabilities in the order shown.
    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    /// The optional capabilities accepted now.
    pub fn accepted(&self) -> &[String] {
        &self.accepted
    }

    /// Whether the dialog warns that what it allows is like running arbitrary code.
    pub fn high_risk(&self) -> bool {
        is_high_risk(&self.capabilities, &self.accepted)
    }

    /// The title, translated.
    pub fn title(&self, cx: &mut App) -> String {
        let request = &self.request;
        let i18n = i18n(cx);
        if request.install {
            i18n.tx_built(&["Install ", " ", "?"], &[text(&request.name), text(&request.version)])
        } else {
            i18n.tx_built(&["Allow ", " to run?"], &[text(&request.name)])
        }
    }

    /// The warnings above the capabilities, in English: an unsigned package, another signer.
    pub fn warnings(&self) -> Vec<&'static str> {
        let mut warnings = Vec::new();
        if self.request.unsigned {
            warnings.push("This package is not signed. Only continue if you trust where it came from.");
        }
        if self.request.signer_changed {
            warnings.push(concat!(
                "This version is signed with a different key than the installed one, and the publisher's earlier ",
                "key doesn't vouch for it. Only continue if you know the publisher changed keys."
            ));
        }
        warnings
    }

    /// Accepts or refuses optional capability `id`.
    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.capabilities.iter().any(|c| c.id == id && c.optional) {
            return;
        }
        match self.accepted.iter().position(|accepted| accepted == id) {
            Some(at) => {
                self.accepted.remove(at);
            }
            None => self.accepted.push(id.to_owned()),
        }
        cx.notify();
    }

    pub fn accept(&mut self, cx: &mut Context<Self>) {
        let optional = self.accepted.clone();
        cx.emit(ConsentDialogEvent::Accepted { optional, signer_change: self.request.signer_changed });
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(ConsentDialogEvent::Cancelled);
    }

    /// Enter accepts, Escape cancels, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.accept(cx),
            "escape" => self.cancel(cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for ConsentDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ConsentDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-strong"));
        let danger = theme::color(cx, "danger");
        let small = theme::length(cx, "fs-small");
        let high = self.high_risk();
        let title = self.title(cx);
        let warnings: Vec<String> = self.warnings().into_iter().map(|warning| tx(cx, warning)).collect();
        let rows: Vec<AnyElement> = if self.capabilities.is_empty() {
            let none = tx(cx, "This plugin needs no special permissions.");
            vec![div().child(SharedString::from(none)).into_any_element()]
        } else {
            let capabilities = self.capabilities.clone();
            capabilities
                .into_iter()
                .map(|capability| {
                    let i18n = i18n(cx);
                    let risk = risk_badge(capability.risk, i18n);
                    let description = capability_description(&capability, i18n);
                    let required = i18n.tx("Required");
                    let marker = if capability.optional {
                        let on = self.accepted.contains(&capability.id);
                        let id = capability.id.clone();
                        let selector = format!("consent-allow-{}", capability.id);
                        check_box(cx, on, true)
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.toggle(&id, cx)))
                            .into_any_element()
                    } else {
                        div()
                            .id(SharedString::from(format!("consent-required-{}", capability.id)))
                            .flex_none()
                            .w(px(16.0))
                            .flex()
                            .justify_center()
                            .text_color(muted)
                            .with_tooltip(required)
                            .child("•")
                            .into_any_element()
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .py(px(4.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(marker)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font_family(self.code_font.clone())
                                        .child(SharedString::from(capability.id.clone())),
                                )
                                .child(badge(cx, &risk)),
                        )
                        .child(div().pl(px(24.0)).text_color(muted).text_size(small).child(description))
                        .into_any_element()
                })
                .collect()
        };
        let high_warning = high.then(|| {
            let warning = tx(cx, "High-risk permissions are equivalent to running arbitrary code on this machine.");
            div().text_color(danger).child(SharedString::from(warning))
        });
        let (cancel, ok) = (tx(cx, "Cancel"), tx(cx, if self.request.install { "Install" } else { "Allow" }));
        let icon = theme::icon(cx, if high { "warning" } else { "info" }, 16.0, theme::color(cx, "warning"));
        div()
            .w(px(DIALOG_WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(icon)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .id("consent-body")
                    .max_h(px(420.0))
                    .overflow_y_scroll()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(warnings.into_iter().map(|w| div().text_color(danger).child(SharedString::from(w))))
                    .children(rows)
                    .children(high_warning)
                    .children(self.request.warnings.iter().map(|w| div().text_color(muted).child(w.clone()))),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .child(
                        button(cx, cancel, false, true)
                            .id("consent-button-cancel")
                            .debug_selector(|| "consent-button-cancel".to_owned())
                            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.cancel(cx))),
                    )
                    .child(
                        button(cx, ok, true, true)
                            .id("consent-button-ok")
                            .debug_selector(|| "consent-button-ok".to_owned())
                            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.accept(cx))),
                    ),
            )
    }
}

// --- Import Theme from JetBrains IDE ---------------------------------------------------------

/// A theme plugin of a JetBrains IDE: importing it installs all its themes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntellijPluginRow {
    pub plugin: String,
    pub ide: String,
    /// What to import: the plugin's folder or jar.
    pub path: String,
    pub themes: Vec<String>,
    /// The IDE uses one of its themes.
    pub in_use: bool,
}

/// One row per plugin of each IDE, in the order found (the plugins of the themes in use first).
pub fn intellij_rows(themes: &[InstalledIntellijTheme]) -> Vec<IntellijPluginRow> {
    let mut rows: Vec<IntellijPluginRow> = Vec::new();
    for theme in themes {
        let path = theme.path.to_string_lossy().into_owned();
        match rows.iter_mut().find(|row| row.ide == theme.ide && row.path == path) {
            Some(row) => {
                row.themes.push(theme.theme_name.clone());
                row.in_use |= theme.active;
            }
            None => rows.push(IntellijPluginRow {
                plugin: theme.plugin.clone(),
                ide: theme.ide.clone(),
                path,
                themes: vec![theme.theme_name.clone()],
                in_use: theme.active,
            }),
        }
    }
    rows
}

/// What the dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntellijThemesEvent {
    /// Import the theme plugin at this path (the IntelliJ theme import, then its install page).
    Import(String),
    /// Choose File…: the theme plugin file picker, then the same import.
    ChooseFile,
    Cancelled,
}

/// Import Theme from JetBrains IDE: the theme plugins installed in the JetBrains IDEs (and Android
/// Studio) on this machine, one row per plugin; a file can be chosen instead.
pub struct IntellijThemesDialog {
    /// `None` while the IDEs are searched.
    rows: Option<Vec<IntellijPluginRow>>,
    error: Option<String>,
    active: usize,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<IntellijThemesEvent> for IntellijThemesDialog {}

impl IntellijThemesDialog {
    /// The dialog while the IDEs are searched (`installed_themes`); answer with [`Self::set_themes`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { rows: None, error: None, active: 0, focus, key_claim }
    }

    /// The themes found, or why they could not be listed.
    pub fn set_themes(&mut self, themes: Result<Vec<InstalledIntellijTheme>, String>, cx: &mut Context<Self>) {
        match themes {
            Ok(themes) => (self.rows, self.error) = (Some(intellij_rows(&themes)), None),
            Err(error) => (self.rows, self.error) = (Some(Vec::new()), Some(error)),
        }
        self.active = 0;
        cx.notify();
    }

    /// The rows; `None` while the IDEs are searched.
    pub fn rows(&self) -> Option<&[IntellijPluginRow]> {
        self.rows.as_deref()
    }

    /// The chosen row.
    pub fn chosen(&self) -> Option<&IntellijPluginRow> {
        self.rows.as_ref()?.get(self.active)
    }

    /// A row's second line: how many themes, and the IDE.
    pub fn row_detail(row: &IntellijPluginRow, cx: &mut App) -> String {
        let i18n = i18n(cx);
        let count = if row.themes.len() == 1 {
            i18n.tx("1 theme")
        } else {
            i18n.tx_built(&["", " themes"], &[Param::Number(row.themes.len() as f64)])
        };
        format!("{count} · {}", row.ide)
    }

    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.rows.as_ref().is_some_and(|rows| index < rows.len()) {
            self.active = index;
            cx.notify();
        }
    }

    pub fn import(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.chosen().map(|row| row.path.clone()) {
            cx.emit(IntellijThemesEvent::Import(path));
        }
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let count = self.rows.as_ref().map_or(0, Vec::len);
        match keystroke.key.as_str() {
            "up" if self.active > 0 => self.choose(self.active - 1, cx),
            "down" if self.active + 1 < count => self.choose(self.active + 1, cx),
            "up" | "down" => {}
            "enter" => self.import(cx),
            "escape" => cx.emit(IntellijThemesEvent::Cancelled),
            _ => return false,
        }
        true
    }
}

impl Focusable for IntellijThemesDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for IntellijThemesDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-strong"));
        let small = theme::length(cx, "fs-small");
        let title = tx(cx, "Import Theme from JetBrains IDE");
        let intro = tx(
            cx,
            "Theme plugins installed in the JetBrains IDEs on this computer. Importing one installs it with all its \
             themes; choose the theme to use in Settings › Appearance. Colours without an OpenIDEA counterpart keep \
             the default theme colours.",
        );
        let body: AnyElement = match &self.rows {
            None => {
                let looking = tx(cx, "Looking for JetBrains themes…");
                div().text_color(muted).child(SharedString::from(looking)).into_any_element()
            }
            Some(rows) if rows.is_empty() => {
                let message = self.error.clone().unwrap_or_else(|| {
                    tx(
                        cx,
                        "No theme plugins found in JetBrains IDEs on this computer. Choose a theme plugin downloaded \
                         from the JetBrains Marketplace (.jar or .zip) or a .theme.json file instead.",
                    )
                });
                div().text_color(muted).child(SharedString::from(message)).into_any_element()
            }
            Some(rows) => {
                let rows = rows.clone();
                let in_use = Badge { label: tx(cx, "In use"), tone: Tone::Accent };
                let elements: Vec<AnyElement> = rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| {
                        let detail = Self::row_detail(row, cx);
                        div()
                            .id(index)
                            .debug_selector(move || format!("intellij-row-{index}"))
                            .h(px(ROW_HEIGHT))
                            .px(px(8.0))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .rounded(theme::length(cx, "radius-row"))
                            .when(index == self.active, |el| el.bg(theme::color(cx, "bg-selected")))
                            .when(index != self.active, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(div().text_color(strong).child(SharedString::from(row.plugin.clone())))
                                    .children(row.in_use.then(|| badge(cx, &in_use)))
                                    .child(div().flex_1())
                                    .child(div().text_color(muted).text_size(small).child(SharedString::from(detail))),
                            )
                            .child(
                                div()
                                    .text_color(muted)
                                    .text_size(small)
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .child(SharedString::from(row.themes.join(", "))),
                            )
                            .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.choose(index, cx)))
                            .into_any_element()
                    })
                    .collect();
                div().id("intellij-rows").max_h(px(360.0)).overflow_y_scroll().children(elements).into_any_element()
            }
        };
        let can_import = self.chosen().is_some();
        let (choose, cancel, import) = (tx(cx, "Choose File…"), tx(cx, "Cancel"), tx(cx, "Import"));
        div()
            .w(px(INTELLIJ_WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .px(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(div().text_color(muted).child(SharedString::from(intro)))
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .child(
                        button(cx, choose, false, true)
                            .id("intellij-button-choose")
                            .debug_selector(|| "intellij-button-choose".to_owned())
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(IntellijThemesEvent::ChooseFile))),
                    )
                    .child(div().flex_1())
                    .child(
                        button(cx, cancel, false, true)
                            .id("intellij-button-cancel")
                            .debug_selector(|| "intellij-button-cancel".to_owned())
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(IntellijThemesEvent::Cancelled))),
                    )
                    .child(
                        button(cx, import, true, can_import)
                            .id("intellij-button-import")
                            .debug_selector(|| "intellij-button-import".to_owned())
                            .when(can_import, |el| {
                                el.on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.import(cx)))
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_models::test_support::plugin;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_host::plugins::InstallPreview;
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<PluginsPageEvent>>>;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// The page in a window, focused on its search field, with the events it emits.
    fn open(cx: &mut TestAppContext) -> (Entity<PluginsPage>, Events, &mut VisualTestContext) {
        init(cx);
        let (page, cx) = cx.add_window_view(PluginsPage::new);
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&page, move |_, event: &PluginsPageEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            let focus = page.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (page, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        cx.run_until_parked();
        cx.debug_bounds(selector).is_some()
    }

    fn taken(events: &Events) -> Vec<PluginsPageEvent> {
        std::mem::take(&mut *events.borrow_mut())
    }

    fn notes(events: &[PluginsPageEvent]) -> Vec<(NotificationLevel, String, Option<String>)> {
        events
            .iter()
            .filter_map(|event| match event {
                PluginsPageEvent::Notify(note) => Some((note.level, note.title.clone(), note.body.clone())),
                _ => None,
            })
            .collect()
    }

    fn cap(id: &str, risk: &str, description: &str, optional: bool, granted: bool) -> Value {
        json!({ "id": id, "risk": risk, "description": description, "optional": optional, "granted": granted })
    }

    /// The bundled sample plugins: Hello (commands), Harbor Night Theme (a theme), Toy Language.
    fn samples() -> (Vec<PluginInfo>, Vec<Contribution>) {
        let hello = plugin(
            "hello",
            json!({
                "name": "Hello", "version": "0.1.0", "status": "inactive", "authors": ["OpenIDEA contributors"],
                "capabilities": [
                    cap("editor.write", "medium", "Change text in the editor", false, true),
                    cap("project.read", "low", "Read files in the open project", false, true),
                ],
                "contributions": [
                    { "point": "commands", "label": "Hello: Insert Date" },
                    { "point": "commands", "label": "Hello: Word Count of Active File" },
                ],
            }),
        );
        let theme = plugin(
            "theme-harbor-night",
            json!({
                "name": "Harbor Night Theme", "version": "1.0.0", "status": "static", "hasCode": false,
                "contributions": [{ "point": "themes", "label": "Harbor Night" }],
            }),
        );
        let toy = plugin(
            "language-toy",
            json!({
                "name": "Toy Language", "version": "0.1.0", "status": "static", "hasCode": false,
                "contributions": [{ "point": "languages", "label": "Toy (.toy)" }],
            }),
        );
        let contributions = vec![
            Contribution::new(
                "hello",
                "commands",
                "insert-date",
                json!({ "id": "insert-date", "title": "Hello: Insert Date" }),
            ),
            Contribution::new(
                "hello",
                "commands",
                "word-count",
                json!({ "id": "word-count", "title": "Hello: Word Count of Active File" }),
            ),
            Contribution::new(
                "theme-harbor-night",
                "themes",
                "harbor-night",
                json!({ "id": "harbor-night", "name": "Harbor Night", "kind": "dark", "tokens": {} }),
            ),
        ];
        (vec![hello, theme, toy], contributions)
    }

    fn catalog_entry(id: &str, name: &str, version: &str, homepage: Option<&str>) -> CatalogPlugin {
        serde_json::from_value(json!({
            "id": id, "name": name, "version": version, "description": "", "authors": ["OpenIDEA contributors"],
            "homepage": homepage, "changeNotes": format!("{version} — first release."), "icon": null,
            "path": format!("plugins/{id}"), "hasCode": false, "capabilities": [], "contributions": [],
            "problem": null,
        }))
        .expect("a catalog entry")
    }

    fn sample_catalog() -> Vec<CatalogPlugin> {
        vec![
            catalog_entry("hello", "Hello", "0.1.0", None),
            catalog_entry("theme-harbor-night", "Harbor Night Theme", "1.0.0", None),
            catalog_entry("language-toy", "Toy Language", "0.1.0", None),
        ]
    }

    fn market_entry(id: &str, name: &str, version: &str, fields: Value) -> MarketPlugin {
        let mut entry = json!({
            "id": id, "name": name, "version": version, "description": "", "authors": [], "publisher": "acme",
            "download": format!("{id}.oiplugin"), "digest": "x", "capabilities": [], "engine": "^1.0",
            "categories": [], "icon": null, "installed": null, "compatible": true,
        });
        if let (Some(entry), Some(fields)) = (entry.as_object_mut(), fields.as_object()) {
            entry.extend(fields.clone());
        }
        serde_json::from_value(entry).expect("a marketplace entry")
    }

    fn preview(id: &str, version: &str, fields: Value) -> InstallPreview {
        let mut preview = json!({
            "id": id, "name": id.to_uppercase(), "version": version, "description": "", "authors": [], "hasCode": true,
            "capabilities": [], "contributions": [], "warnings": [], "replaces": null, "unsigned": true,
            "publisher": null, "signerChanged": false, "needsConsent": false,
        });
        if let (Some(preview), Some(fields)) = (preview.as_object_mut(), fields.as_object()) {
            preview.extend(fields.clone());
        }
        serde_json::from_value(preview).expect("an install preview")
    }

    fn list(page: &Entity<PluginsPage>, cx: &mut VisualTestContext, plugins: Vec<PluginInfo>, ui: Vec<Contribution>) {
        page.update(cx, |page, cx| page.set_listing(plugins, ui, cx));
        cx.run_until_parked();
    }

    fn items(page: &Entity<PluginsPage>, cx: &mut VisualTestContext) -> Vec<ListItem> {
        let rows = page.update(cx, |page, cx| page.rows(cx));
        rows.into_iter()
            .filter_map(|row| match row {
                ListRow::Item(item) => Some(item),
                _ => None,
            })
            .collect()
    }

    fn details(page: &Entity<PluginsPage>, cx: &mut VisualTestContext) -> Details {
        page.update(cx, |page, cx| page.details(cx)).expect("details are shown")
    }

    fn section<'a>(details: &'a Details, title: &str) -> &'a Section {
        details.sections.iter().find(|s| s.title == title).unwrap_or_else(|| panic!("a {title} section"))
    }

    fn button_label(details: &Details, id: DetailButton) -> Option<String> {
        details.buttons.iter().find(|b| b.id == id).map(|b| b.label.clone())
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › lists plugins, shows details, toggles and
    // applies themes
    #[gpui_kit::test]
    async fn lists_plugins_shows_details_toggles_and_applies_themes(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        page.update(cx, |page, cx| page.opened(cx));
        assert_eq!(taken(&events), [PluginsPageEvent::Refresh { plugins: true, catalog: true, market: Some(false) }]);
        let (plugins, ui) = samples();
        list(&page, cx, plugins.clone(), ui.clone());
        assert_eq!(items(&page, cx).len(), 3);

        // Listed as returned; the first one is selected.
        let shown = details(&page, cx);
        assert_eq!(shown.name, "Hello");
        let commands = section(&shown, "Commands");
        assert_eq!(commands.rows[0].mono.as_deref(), Some("plugin.hello.insert-date"));
        let permissions = section(&shown, "Permissions");
        let write = permissions.rows.iter().find(|row| row.main == "editor.write").expect("editor.write");
        assert_eq!(write.description.as_deref(), Some("Change text in the editor"));
        assert_eq!(write.badge.as_ref().map(|b| b.label.as_str()), Some("Medium"));

        click(cx, "plugins-check-hello");
        assert_eq!(taken(&events), [PluginsPageEvent::SetEnabled { id: "hello".to_owned(), enabled: false }]);
        page.update(cx, |page, cx| page.finished("hello", Ok(()), cx));
        let off = plugin(
            "hello",
            json!({
                "name": "Hello", "enabled": false, "status": "disabled",
                "contributions": [{ "point": "commands", "label": "Hello: Insert Date" }],
            }),
        );
        let disabled: Vec<PluginInfo> =
            plugins.iter().map(|p| if p.id == "hello" { off.clone() } else { p.clone() }).collect();
        let live: Vec<Contribution> = ui.iter().filter(|c| c.plugin_id != "hello").cloned().collect();
        list(&page, cx, disabled, live.clone());
        let hello = items(&page, cx).into_iter().find(|item| item.id == "hello").expect("hello is listed");
        assert_eq!(hello.enabled, Some((false, true)));
        let commands = section(&details(&page, cx), "Commands").clone();
        assert!(commands.rows.iter().all(|row| row.action.is_none()), "no Run without the plugin's commands");

        click(cx, "plugins-row-theme-harbor-night");
        click(cx, "plugins-apply-theme-harbor-night");
        let applied = taken(&events);
        assert!(applied.contains(&PluginsPageEvent::SetTheme("plugin:theme-harbor-night/harbor-night".to_owned())));
        let themes = section(&details(&page, cx), "Themes").clone();
        assert_eq!(themes.rows[0].action.as_ref().map(|a| a.label.as_str()), Some("Use Default Theme"));

        cx.update(|window, cx| {
            let focus = page.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("toy");
        assert_eq!(items(&page, cx).len(), 1);
        let toy = details(&page, cx);
        assert_eq!(toy.name, "Toy Language");
        assert_eq!(section(&toy, "Languages").rows[0].main, "Toy (.toy)");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › offers the IntelliJ theme imports only in a
    // trusted project's window
    #[gpui_kit::test]
    async fn offers_the_intellij_theme_imports_only_in_a_trusted_projects_window(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let imports = |page: &Entity<PluginsPage>, cx: &mut VisualTestContext| -> Vec<String> {
            let items = page.update(cx, |page, cx| page.gear_items(cx));
            items
                .into_iter()
                .filter(|item| {
                    matches!(item.command, Some(GearCommand::ImportInstalledIntellij | GearCommand::ImportIntellijFile))
                })
                .map(|item| item.label)
                .collect()
        };
        assert!(imports(&page, cx).is_empty(), "the welcome screen: no installs without a trusted project");
        page.update(cx, |page, cx| page.set_trusted_project(false, cx));
        assert!(imports(&page, cx).is_empty());
        page.update(cx, |page, cx| page.set_trusted_project(true, cx));
        assert_eq!(imports(&page, cx), ["Import Theme from JetBrains IDE…", "Import IntelliJ Theme from File…"]);
        click(cx, "plugins-gear");
        click(cx, "plugins-gear-item-6");
        assert_eq!(taken(&events), [PluginsPageEvent::ImportInstalledIntellij]);
        assert!(!drawn(cx, "plugins-gear-item-6"), "choosing an item closes the menu");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › shows a plugin's own icon in its row and details
    #[gpui_kit::test]
    async fn shows_a_plugins_own_icon_in_its_row_and_details(cx: &mut TestAppContext) {
        let (page, _, cx) = open(cx);
        let circle = r#"<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="8"/></svg>"#;
        let icon = json!({ "light": circle, "dark": null });
        list(&page, cx, vec![plugin("iconic", json!({ "name": "Iconic", "icon": icon }))], Vec::new());
        let row = items(&page, cx).remove(0);
        assert!(row.icon.starts_with("ext:pluginIcon/iconic/"), "{}", row.icon);
        assert_eq!(details(&page, cx).icon, row.icon);
        list(&page, cx, vec![plugin("plain", json!({}))], Vec::new());
        assert_eq!(items(&page, cx)[0].icon, "plugin");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › installed / catalog homepage › opens a web
    // homepage with a click; keeps an unsupported or malformed homepage as plain text
    #[gpui_kit::test]
    async fn opens_a_web_homepage_and_keeps_an_unsupported_or_malformed_one_as_plain_text(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        for homepage in ["https://example.com/plugin", "not a URL", "javascript:alert(1)"] {
            for catalog in [false, true] {
                let entry = plugin("homepage", json!({ "name": "Homepage Example", "homepage": homepage }));
                let installed = if catalog { Vec::new() } else { vec![entry] };
                list(&page, cx, installed, Vec::new());
                page.update(cx, |page, cx| {
                    let entry = catalog_entry("homepage", "Homepage Example", "1.0.0", Some(homepage));
                    page.set_catalog(Ok(vec![entry]), cx);
                    let tab = if catalog { PluginsTab::Marketplace } else { PluginsTab::Installed };
                    page.select_tab(tab, cx);
                });
                let shown = details(&page, cx);
                assert_eq!(shown.name, "Homepage Example");
                let fact = shown.facts.iter().find(|f| f.label == "Homepage").expect("the homepage");
                let web = is_web_homepage(homepage);
                assert_eq!(fact.link, web, "{homepage}");
                assert_eq!(drawn(cx, "plugins-homepage"), web, "{homepage}");
                if web {
                    taken(&events);
                    click(cx, "plugins-homepage");
                    assert_eq!(taken(&events), [PluginsPageEvent::OpenUrl(homepage.to_owned())]);
                }
            }
        }
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › restarts a failed plugin instead of disabling it
    #[gpui_kit::test]
    async fn restarts_a_failed_plugin_instead_of_disabling_it(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        list(&page, cx, vec![plugin("broken", json!({ "status": "failed", "error": "trap" }))], Vec::new());
        let shown = details(&page, cx);
        assert_eq!(button_label(&shown, DetailButton::Toggle).as_deref(), Some("Restart"));
        assert_eq!(shown.error.as_deref(), Some("trap"));
        click(cx, "plugins-details-toggle");
        assert_eq!(taken(&events), [PluginsPageEvent::SetEnabled { id: "broken".to_owned(), enabled: true }]);
        assert!(!details(&page, cx).buttons.iter().any(|b| b.id == DetailButton::Toggle && b.enabled), "busy");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › uninstalls after confirmation
    #[gpui_kit::test]
    async fn uninstalls_after_confirmation(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let (plugins, ui) = samples();
        list(&page, cx, plugins.clone(), ui.clone());
        click(cx, "plugins-details-uninstall");
        click(cx, "plugins-details-confirm-uninstall");
        assert_eq!(taken(&events), [PluginsPageEvent::Uninstall("hello".to_owned())]);
        page.update(cx, |page, cx| page.finished("hello", Ok(()), cx));
        let done = taken(&events);
        assert_eq!(notes(&done), [(NotificationLevel::Info, "Uninstalled Hello".to_owned(), None)]);
        assert!(done.contains(&PluginsPageEvent::Refresh { plugins: true, catalog: false, market: None }));
        list(&page, cx, plugins[1..].to_vec(), ui[2..].to_vec());
        assert_ne!(details(&page, cx).name, "Hello");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › keeps failed cleanup visible and lets Uninstall
    // retry
    #[gpui_kit::test]
    async fn keeps_failed_cleanup_visible_and_lets_uninstall_retry(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let (plugins, ui) = samples();
        list(&page, cx, plugins.clone(), ui.clone());
        click(cx, "plugins-details-uninstall");
        click(cx, "plugins-details-confirm-uninstall");
        page.update(cx, |page, cx| page.finished("hello", Err("Private data is locked".to_owned()), cx));
        let failed = taken(&events);
        let expected = ("Could not uninstall Hello".to_owned(), Some("Private data is locked".to_owned()));
        assert_eq!(notes(&failed), [(NotificationLevel::Error, expected.0, expected.1)]);
        let mut listed = plugins.clone();
        listed[0] = plugin(
            "hello",
            json!({ "name": "Hello", "version": "0.1.0", "status": "failed", "error": "Private data is locked" }),
        );
        list(&page, cx, listed, ui[2..].to_vec());
        let shown = details(&page, cx);
        assert_eq!((shown.name.as_str(), shown.error.as_deref()), ("Hello", Some("Private data is locked")));
        click(cx, "plugins-details-confirm-uninstall");
        assert_eq!(taken(&events), [PluginsPageEvent::Uninstall("hello".to_owned())]);
        page.update(cx, |page, cx| page.finished("hello", Ok(()), cx));
        assert_eq!(notes(&taken(&events)), [(NotificationLevel::Info, "Uninstalled Hello".to_owned(), None)]);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › shows What's New from the change notes
    #[gpui_kit::test]
    async fn shows_whats_new_from_the_change_notes(cx: &mut TestAppContext) {
        let (page, _, cx) = open(cx);
        let hello = plugin("hello", json!({ "name": "Hello", "changeNotes": "Added word count.\n" }));
        list(&page, cx, vec![hello], Vec::new());
        assert_eq!(page.read_with(cx, |page, _| page.detail_tab()), DetailTab::Overview);
        assert_eq!(details(&page, cx).notes, Some(Some("Added word count.".to_owned())));
        click(cx, "plugins-details-whats-new");
        assert_eq!(page.read_with(cx, |page, _| page.detail_tab()), DetailTab::WhatsNew);
        list(&page, cx, vec![plugin("other", json!({}))], Vec::new());
        assert_eq!(page.update(cx, |page, cx| page.details(cx).map(|d| d.notes)), Some(Some(None)));
        assert_eq!(page.read_with(cx, |page, _| page.detail_tab()), DetailTab::Overview, "another plugin");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › installs from the local catalog on the
    // Available tab
    #[gpui_kit::test]
    async fn installs_from_the_local_catalog_on_the_available_tab(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        list(&page, cx, Vec::new(), Vec::new());
        page.update(cx, |page, cx| page.set_catalog(Ok(sample_catalog()), cx));
        click(cx, "plugins-tab-marketplace");
        assert!(items(&page, cx).iter().any(|item| item.name == "Toy Language"));
        click(cx, "plugins-catalog-row-language-toy");
        assert_eq!(details(&page, cx).name, "Toy Language");
        click(cx, "plugins-catalog-install-language-toy");
        let previewing = InstallStep::Preview("plugins/language-toy".to_owned());
        assert_eq!(taken(&events), [PluginsPageEvent::Install(previewing)]);
        assert!(page.read_with(cx, |page, _| page.installing()));
        page.update(cx, |page, cx| page.previewed(Ok(preview("language-toy", "0.1.0", json!({}))), cx));
        let dir = "plugins/language-toy".to_owned();
        let install = InstallStep::Install { dir, optional: Vec::new(), signer_change: false };
        assert_eq!(taken(&events), [PluginsPageEvent::Install(install)]);
        let toy = plugin("language-toy", json!({ "name": "Toy Language", "version": "0.1.0", "status": "static" }));
        page.update(cx, |page, cx| page.installed(Ok(toy.clone()), cx));
        let done = taken(&events);
        assert_eq!(notes(&done), [(NotificationLevel::Info, "Installed Toy Language 0.1.0".to_owned(), None)]);
        list(&page, cx, vec![toy], Vec::new());
        assert_eq!(page.read_with(cx, |page, _| page.tab()), PluginsTab::Marketplace);
        let toy_row = items(&page, cx).into_iter().find(|item| item.id == "language-toy").expect("the package");
        assert_eq!(toy_row.tags.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Installed"]);
        assert_eq!(toy_row.install, None);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › PluginsPage › shows why the marketplace couldn't be read
    // instead of claiming there is none
    #[gpui_kit::test]
    async fn shows_why_the_marketplace_couldnt_be_read_instead_of_claiming_there_is_none(cx: &mut TestAppContext) {
        let (page, _, cx) = open(cx);
        page.update(cx, |page, cx| {
            page.set_catalog(Ok(sample_catalog()), cx);
            page.set_market(None, Err("Index unreachable".to_owned()), cx);
            page.select_tab(PluginsTab::Marketplace, cx);
        });
        let rows = page.update(cx, |page, cx| page.rows(cx));
        assert!(rows.contains(&ListRow::Message { text: "Index unreachable".to_owned(), error: true }));
        assert!(!format!("{rows:?}").contains(NO_REPOSITORY));
    }

    // src/features/plugins/__tests__/plugins.test.tsx › secrets › stores and removes a declared secret without
    // showing its value
    #[gpui_kit::test]
    async fn stores_and_removes_a_declared_secret_without_showing_its_value(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let ai = plugin("ai", json!({ "contributions": [{ "point": "secrets", "label": "API key" }] }));
        let ui = vec![Contribution::new("ai", "secrets", "api-key", json!({ "name": "api-key", "label": "API key" }))];
        list(&page, cx, vec![ai], ui);
        assert_eq!(taken(&events), [PluginsPageEvent::LoadSecrets("ai".to_owned())]);
        page.update(cx, |page, cx| page.set_secrets("ai", vec![("api-key".to_owned(), false)], cx));
        cx.run_until_parked();
        let secret = |page: &Entity<PluginsPage>, cx: &mut VisualTestContext| {
            section(&details(page, cx), "Secrets").secrets[0].clone()
        };
        assert!(!secret(&page, cx).set);
        let key = ("ai".to_owned(), "api-key".to_owned(), false);
        let field = page.read_with(cx, |page, _| page.secret_fields.get(&key).cloned()).expect("the value field");
        cx.update(|window, cx| {
            let focus = field.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("sk-1");
        cx.simulate_keystrokes("enter");
        let value = Some("sk-1".to_owned());
        let save = PluginsPageEvent::SetSecret { plugin_id: "ai".to_owned(), name: "api-key".to_owned(), value };
        assert_eq!(taken(&events), [save]);
        page.update(cx, |page, cx| page.secret_saved("ai", "api-key", Ok(()), cx));
        assert_eq!(taken(&events), [PluginsPageEvent::LoadSecrets("ai".to_owned())]);
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "", "the value is not kept");
        page.update(cx, |page, cx| page.set_secrets("ai", vec![("api-key".to_owned(), true)], cx));
        assert!(secret(&page, cx).set);
        click(cx, "plugins-secret-remove-api-key");
        let (plugin_id, name) = ("ai".to_owned(), "api-key".to_owned());
        let remove = PluginsPageEvent::SetSecret { plugin_id, name, value: None };
        assert_eq!(taken(&events), [remove]);
    }

    // Not from a spec: while the state of a plugin's secrets has not been read, its Secrets section
    // lists no secret and draws no field a value could be typed in.
    #[gpui_kit::test]
    async fn shows_no_secret_field_while_the_secrets_are_not_read(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let ai = plugin("ai", json!({ "contributions": [{ "point": "secrets", "label": "API key" }] }));
        let ui = vec![Contribution::new("ai", "secrets", "api-key", json!({ "name": "api-key", "label": "API key" }))];
        list(&page, cx, vec![ai], ui);
        assert_eq!(taken(&events), [PluginsPageEvent::LoadSecrets("ai".to_owned())]);
        cx.run_until_parked();
        assert!(section(&details(&page, cx), "Secrets").secrets.is_empty());
        assert!(cx.debug_bounds("plugins-secret-field-api-key").is_none(), "no secret field is drawn");
        assert!(page.read_with(cx, |page, _| page.secret_fields.is_empty()));
        // Selecting the plugin again does not ask a second time.
        page.update(cx, |page, cx| page.select(ItemKind::Installed, "ai", cx));
        assert!(taken(&events).is_empty());
        // Once the state is read, the row and its field are drawn.
        page.update(cx, |page, cx| page.set_secrets("ai", vec![("api-key".to_owned(), false)], cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("plugins-secret-field-api-key").is_some());
    }

    fn market_page(
        cx: &mut TestAppContext,
        market: Vec<MarketPlugin>,
    ) -> (Entity<PluginsPage>, Events, &mut VisualTestContext) {
        let (page, events, cx) = open(cx);
        page.update(cx, |page, cx| {
            page.set_listing(Vec::new(), Vec::new(), cx);
            page.set_catalog(Ok(sample_catalog()), cx);
            page.set_market(Some("https://plugins.example/index.json".to_owned()), Ok(market), cx);
            page.select_tab(PluginsTab::Marketplace, cx);
        });
        cx.run_until_parked();
        taken(&events);
        (page, events, cx)
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › lists the index's plugins and installs one
    // through the install page
    #[gpui_kit::test]
    async fn lists_the_indexs_plugins_and_installs_one_through_the_install_page(cx: &mut TestAppContext) {
        let hello = market_entry("hello", "Hello", "1.0.0", json!({ "capabilities": ["editor.write"] }));
        let (page, events, cx) = market_page(cx, vec![hello]);
        let rows = page.update(cx, |page, cx| page.rows(cx));
        assert!(rows.contains(&ListRow::Header { title: "Marketplace".to_owned(), count: 1 }));
        click(cx, "plugins-market-row-hello");
        let shown = details(&page, cx);
        assert!(shown.badges.iter().any(|b| b.label == "Signed by acme"));
        click(cx, "plugins-details-install");
        assert_eq!(taken(&events), [PluginsPageEvent::DownloadMarket("hello".to_owned())]);
        page.update(cx, |page, cx| page.downloaded("hello", Ok("plugins/hello".to_owned()), cx));
        assert_eq!(taken(&events), [PluginsPageEvent::Install(InstallStep::Preview("plugins/hello".to_owned()))]);
        let needs = preview("hello", "1.0.0", json!({ "needsConsent": true }));
        page.update(cx, |page, cx| page.previewed(Ok(needs), cx));
        let asked = taken(&events);
        let [PluginsPageEvent::AskConsent(request)] = asked.as_slice() else { panic!("the install page: {asked:?}") };
        assert!(request.install && request.plugin_id == "hello");
        page.update(cx, |page, cx| page.consented(Some(Consent::default()), cx));
        let dir = "plugins/hello".to_owned();
        let install = InstallStep::Install { dir, optional: Vec::new(), signer_change: false };
        assert_eq!(taken(&events), [PluginsPageEvent::Install(install)]);
        page.update(cx, |page, cx| page.installed(Ok(plugin("hello", json!({ "name": "Hello" }))), cx));
        let done = taken(&events);
        assert!(done.contains(&PluginsPageEvent::Refresh { plugins: true, catalog: false, market: Some(false) }));
        assert_eq!(page.read_with(cx, |page, _| page.tab()), PluginsTab::Installed, "the installed plugin shows");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › doesn't claim there is no repository while one
    // is configured
    #[gpui_kit::test]
    async fn doesnt_claim_there_is_no_repository_while_one_is_configured(cx: &mut TestAppContext) {
        let (page, _, cx) = open(cx);
        page.update(cx, |page, cx| {
            page.set_catalog(Ok(Vec::new()), cx);
            page.set_market(Some("https://plugins.example/index.json".to_owned()), Ok(Vec::new()), cx);
            page.select_tab(PluginsTab::Marketplace, cx);
        });
        assert!(page.update(cx, |page, cx| page.details(cx)).is_none());
        assert_eq!(page.update(cx, |page, cx| page.empty_details(cx)), ("No plugin selected".to_owned(), None));
        assert_eq!(page.read_with(cx, |page, _| page.placeholder()), "Search plugins");
        page.update(cx, |page, cx| page.set_market(None, Ok(Vec::new()), cx));
        assert_eq!(page.update(cx, |page, cx| page.empty_details(cx)).0, NO_REPOSITORY);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › edits the repository from the gear menu; an
    // empty URL restores the default
    #[gpui_kit::test]
    async fn edits_the_repository_from_the_gear_menu_an_empty_url_restores_the_default(cx: &mut TestAppContext) {
        let (page, events, cx) = market_page(cx, Vec::new());
        click(cx, "plugins-gear");
        click(cx, "plugins-gear-item-4");
        let current = Some("https://plugins.example/index.json".to_owned());
        assert_eq!(taken(&events), [PluginsPageEvent::EditRepository { current }]);
        let mirror = Some("https://mirror.example/index.json".to_owned());
        assert_eq!(repository_url(" https://mirror.example/index.json "), mirror);
        assert_eq!(repository_url("  "), None);
        drop(page);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › filters the marketplace by category
    #[gpui_kit::test]
    async fn filters_the_marketplace_by_category(cx: &mut TestAppContext) {
        let market = vec![
            market_entry("acme.dusk", "Dusk", "1.0.0", json!({ "categories": ["themes"] })),
            market_entry("acme.lint", "Lint", "1.0.0", json!({ "categories": ["linters", "tools"] })),
            market_entry("acme.misc", "Misc", "1.0.0", json!({ "categories": ["robots"] })),
        ];
        let (page, _, cx) = market_page(cx, market);
        let options: Vec<(String, usize)> =
            page.update(cx, |page, cx| page.category_options(cx)).into_iter().map(|(_, l, n)| (l, n)).collect();
        let expected = [("All Categories", 3), ("Themes", 1), ("Linters & Formatters", 1), ("Tools", 1), ("Other", 1)];
        assert_eq!(options, expected.map(|(label, count)| (label.to_owned(), count)));
        click(cx, "plugins-category");
        click(cx, "plugins-category-option-3");
        let names: Vec<String> = items(&page, cx).into_iter().map(|item| item.name).collect();
        assert_eq!(names, ["Lint"]);
        let rows = page.update(cx, |page, cx| page.rows(cx));
        assert!(!rows.iter().any(|row| matches!(row, ListRow::Header { title, .. } if title == "Local")));
        click(cx, "plugins-market-row-acme.lint");
        let category = details(&page, cx).facts.into_iter().find(|f| f.label == "Category").expect("the category");
        assert_eq!(category.value, "Linters & Formatters, Tools");
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › shows a plugin this IDE can't load without
    // installing it
    #[gpui_kit::test]
    async fn shows_a_plugin_this_ide_cant_load_without_installing_it(cx: &mut TestAppContext) {
        let next = market_entry("acme.next", "Next", "2.0.0", json!({ "engine": "^9.0", "compatible": false }));
        let (page, _, cx) = market_page(cx, vec![next]);
        let row = items(&page, cx).into_iter().find(|item| item.id == "acme.next").expect("listed");
        assert_eq!(row.tags[0].label, "Incompatible");
        click(cx, "plugins-market-row-acme.next");
        let shown = details(&page, cx);
        assert_eq!(shown.notice.as_deref(), Some("Needs plugin API ^9.0. Update OpenIDEA to install it."));
        assert!(button_label(&shown, DetailButton::Install).is_none());
    }

    fn updates_page(cx: &mut TestAppContext) -> (Entity<PluginsPage>, Events, &mut VisualTestContext) {
        let market = vec![
            market_entry("a", "A", "1.1.0", json!({ "installed": "1.0.0" })),
            market_entry("b", "B", "2.0.0", json!({ "installed": "1.0.0" })),
            market_entry("c", "C", "1.0.0", json!({ "installed": "1.0.0" })),
        ];
        let (page, events, cx) = market_page(cx, market);
        let installed = vec![plugin("a", json!({ "version": "1.0.0" })), plugin("b", json!({ "version": "1.0.0" }))];
        list(&page, cx, installed, Vec::new());
        taken(&events);
        (page, events, cx)
    }

    fn update_preview(id: &str, version: &str, fields: Value) -> InstallPreview {
        let capabilities = json!([
            cap("editor.read", "low", "Read", false, true),
            cap("net:a.example", "medium", "Net", true, true),
            cap("net:b.example", "medium", "Net", true, false),
        ]);
        let mut all =
            json!({ "capabilities": capabilities, "replaces": "1.0.0", "unsigned": false, "publisher": "acme" });
        if let (Some(all), Some(fields)) = (all.as_object_mut(), fields.as_object()) {
            all.extend(fields.clone());
        }
        preview(id, version, all)
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › updates › installs updates with nothing to
    // confirm without asking, keeping their grants
    #[gpui_kit::test]
    async fn installs_updates_with_nothing_to_confirm_without_asking_keeping_their_grants(cx: &mut TestAppContext) {
        let (page, events, cx) = updates_page(cx);
        page.update(cx, |page, cx| page.update_all(cx));
        assert_eq!(taken(&events), [PluginsPageEvent::Refresh { plugins: false, catalog: false, market: Some(true) }]);
        let market = page.read_with(cx, |page, _| page.market.clone().unwrap_or_default());
        let url = Some("https://plugins.example/index.json".to_owned());
        page.update(cx, |page, cx| page.set_market(url, Ok(market), cx));
        let mut all = taken(&events);
        assert_eq!(all, [PluginsPageEvent::DownloadMarket("a".to_owned())]);
        page.update(cx, |page, cx| {
            page.downloaded("a", Ok("a".to_owned()), cx);
            page.previewed(Ok(update_preview("a", "1.1.0", json!({}))), cx);
        });
        let optional = vec!["net:a.example".to_owned()];
        let kept = InstallStep::Install { dir: "a".to_owned(), optional, signer_change: false };
        let previewing = InstallStep::Preview("a".to_owned());
        assert_eq!(taken(&events), [PluginsPageEvent::Install(previewing), PluginsPageEvent::Install(kept)]);
        page.update(cx, |page, cx| page.installed(Ok(plugin("a", json!({ "version": "1.1.0" }))), cx));
        let after_a = taken(&events);
        assert!(after_a.contains(&PluginsPageEvent::DownloadMarket("b".to_owned())));
        all.extend(after_a);
        page.update(cx, |page, cx| {
            page.downloaded("b", Ok("b".to_owned()), cx);
            let other_signer = update_preview("b", "2.0.0", json!({ "signerChanged": true, "needsConsent": true }));
            page.previewed(Ok(other_signer), cx);
        });
        let asked = taken(&events);
        let Some(PluginsPageEvent::AskConsent(request)) = asked.last() else { panic!("b asks: {asked:?}") };
        assert_eq!((request.plugin_id.as_str(), request.signer_changed), ("b", true));
        page.update(cx, |page, cx| page.consented(Some(Consent { optional: Vec::new(), signer_change: true }), cx));
        let changed = InstallStep::Install { dir: "b".to_owned(), optional: Vec::new(), signer_change: true };
        assert_eq!(taken(&events), [PluginsPageEvent::Install(changed)]);
        page.update(cx, |page, cx| page.installed(Ok(plugin("b", json!({ "version": "2.0.0" }))), cx));
        all.extend(taken(&events));
        assert_eq!(notes(&all), [(NotificationLevel::Info, "Updated 2 plugins".to_owned(), None)]);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › updates › warns about another signer on the
    // install page and confirms it
    #[gpui_kit::test]
    async fn warns_about_another_signer_on_the_install_page_and_confirms_it(cx: &mut TestAppContext) {
        init(cx);
        let request = ConsentRequest {
            plugin_id: "b".to_owned(),
            name: "B".to_owned(),
            version: "2.0.0".to_owned(),
            install: true,
            capabilities: Vec::new(),
            unsigned: false,
            signer_changed: true,
            warnings: Vec::new(),
        };
        let (dialog, cx) = cx.add_window_view(|_, cx| ConsentDialog::new(request, cx));
        let answers: Rc<RefCell<Vec<ConsentDialogEvent>>> = Rc::default();
        let log = Rc::clone(&answers);
        cx.update(|_, cx| {
            let record = move |_, event: &ConsentDialogEvent, _: &mut App| log.borrow_mut().push(event.clone());
            cx.subscribe(&dialog, record).detach();
        });
        let warnings = dialog.read_with(cx, |dialog, _| dialog.warnings());
        assert!(warnings.iter().any(|warning| warning.contains("signed with a different key")));
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "Install B 2.0.0?");
        click(cx, "consent-button-ok");
        let answer = answers.borrow().last().cloned().expect("answered");
        assert_eq!(answer, ConsentDialogEvent::Accepted { optional: Vec::new(), signer_change: true });
        assert!(answer.consent().is_some_and(|consent| consent.signer_change));
    }

    // Not from a spec: optional capabilities start accepted when granted before or low-risk, a click refuses
    // one, required ones have no checkbox, an accepted high-risk one warns, and Escape cancels.
    #[gpui_kit::test]
    async fn accepts_optional_capabilities_and_warns_about_high_risk_ones(cx: &mut TestAppContext) {
        init(cx);
        let capabilities: Vec<Capability> = serde_json::from_value(json!([
            cap("project.read", "low", "Read files", true, false),
            cap("process:bash", "high", "Run bash", true, false),
            cap("editor.write", "medium", "Write", false, false),
        ]))
        .expect("capabilities");
        let request = ConsentRequest {
            plugin_id: "p".to_owned(),
            name: "P".to_owned(),
            version: "1".to_owned(),
            install: false,
            capabilities,
            unsigned: true,
            signer_changed: false,
            warnings: Vec::new(),
        };
        let (dialog, cx) = cx.add_window_view(|_, cx| ConsentDialog::new(request, cx));
        let answers: Rc<RefCell<Vec<ConsentDialogEvent>>> = Rc::default();
        let log = Rc::clone(&answers);
        cx.update(|window, cx| {
            let record = move |_, event: &ConsentDialogEvent, _: &mut App| log.borrow_mut().push(event.clone());
            cx.subscribe(&dialog, record).detach();
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "Allow P to run?");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.accepted().to_vec()), ["project.read"]);
        assert!(!dialog.read_with(cx, |dialog, _| dialog.high_risk()));
        assert!(!drawn(cx, "consent-allow-editor.write"), "a required capability has no checkbox");
        click(cx, "consent-allow-process:bash");
        assert!(dialog.read_with(cx, |dialog, _| dialog.high_risk()));
        click(cx, "consent-allow-project.read");
        cx.simulate_keystrokes("enter");
        let answer = answers.borrow().last().cloned().expect("answered");
        let optional = vec!["process:bash".to_owned()];
        assert_eq!(answer, ConsentDialogEvent::Accepted { optional, signer_change: false });
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().last(), Some(&ConsentDialogEvent::Cancelled));
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › updates › offers Update All in the gear menu
    // while there are updates
    #[gpui_kit::test]
    async fn offers_update_all_in_the_gear_menu_while_there_are_updates(cx: &mut TestAppContext) {
        let (page, events, cx) = updates_page(cx);
        let first = page.update(cx, |page, cx| page.gear_items(cx)).remove(0);
        assert_eq!((first.label.as_str(), first.enabled), ("Update All Plugins (2)", true));
        click(cx, "plugins-gear");
        click(cx, "plugins-gear-item-0");
        assert_eq!(taken(&events), [PluginsPageEvent::Refresh { plugins: false, catalog: false, market: Some(true) }]);
        page.update(cx, |page, cx| page.set_market(None, Ok(Vec::new()), cx));
        let first = page.update(cx, |page, cx| page.gear_items(cx)).remove(0);
        assert_eq!((first.label.as_str(), first.enabled), ("Update All Plugins", false));
    }

    // src/features/plugins/__tests__/plugins.test.tsx › marketplace › offers to roll back an update
    #[gpui_kit::test]
    async fn offers_to_roll_back_an_update(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let updated = plugin("p", json!({ "name": "P", "version": "2.0.0", "previousVersion": "1.0.0" }));
        list(&page, cx, vec![updated], Vec::new());
        let shown = details(&page, cx);
        assert!(shown.badges.iter().any(|b| b.label == "Unsigned"));
        assert_eq!(button_label(&shown, DetailButton::RollBack).as_deref(), Some("Roll Back to 1.0.0"));
        click(cx, "plugins-details-roll-back");
        assert_eq!(taken(&events), [PluginsPageEvent::RollBack("p".to_owned())]);
        let previous = plugin("p", json!({ "name": "P", "version": "1.0.0" }));
        page.update(cx, |page, cx| page.rolled_back("p", Ok(previous), cx));
        assert_eq!(notes(&taken(&events)), [(NotificationLevel::Info, "Rolled back P to 1.0.0".to_owned(), None)]);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › catalog helpers › disables and re-enables all downloaded
    // plugins, leaving dev plugins alone (the gear menu items)
    #[gpui_kit::test]
    async fn disables_and_re_enables_all_downloaded_plugins_from_the_gear_menu(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let plugins = vec![plugin("a", json!({})), plugin("b", json!({})), plugin("d", json!({ "source": "dev" }))];
        list(&page, cx, plugins, Vec::new());
        let items = page.update(cx, |page, cx| page.gear_items(cx));
        let enable_all = items.iter().find(|i| i.command == Some(GearCommand::EnableAll)).expect("Enable All");
        assert!(!enable_all.enabled, "nothing to enable");
        page.update(cx, |page, cx| page.run_gear(GearCommand::DisableAll, cx));
        let ids: Vec<String> = taken(&events)
            .into_iter()
            .filter_map(|event| match event {
                PluginsPageEvent::SetEnabled { id, enabled: false } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["a", "b"]);
    }

    // Not from a spec: Review Permissions of a plugin waiting for consent asks, and grants what was accepted.
    #[gpui_kit::test]
    async fn reviews_the_permissions_of_a_plugin_waiting_for_consent(cx: &mut TestAppContext) {
        let (page, events, cx) = open(cx);
        let waiting = plugin(
            "p",
            json!({ "name": "P", "source": "dev", "status": "needsConsent",
                    "capabilities": [cap("net:x.example", "medium", "Net", true, false)] }),
        );
        list(&page, cx, vec![waiting], Vec::new());
        let shown = details(&page, cx);
        assert_eq!(shown.notice.as_deref(), Some("Plugins from a project folder run only after you allow them."));
        let permissions = section(&shown, "Permissions");
        assert_eq!(permissions.rows[0].check, Some((false, false)), "nothing to change before consent");
        click(cx, "plugins-details-review");
        let asked = taken(&events);
        let [PluginsPageEvent::AskConsent(request)] = asked.as_slice() else { panic!("asks: {asked:?}") };
        assert!(!request.install && request.unsigned);
        let consent = Consent { optional: vec!["net:x.example".to_owned()], signer_change: false };
        page.update(cx, |page, cx| page.consented(Some(consent), cx));
        let optional = vec!["net:x.example".to_owned()];
        let grant = PluginsPageEvent::Grant { id: "p".to_owned(), optional, signer_change: false };
        assert_eq!(taken(&events), [grant]);
        page.update(cx, |page, cx| page.finished("p", Err("denied".to_owned()), cx));
        let failure = (NotificationLevel::Error, "Could not enable P".to_owned(), Some("denied".to_owned()));
        assert_eq!(notes(&taken(&events)), [failure]);
    }

    fn jetbrains(ide: &str, plugin: &str, path: &str, theme: &str, active: bool) -> InstalledIntellijTheme {
        serde_json::from_value(json!({
            "ide": ide, "plugin": plugin, "path": path, "themeId": theme, "themeName": theme, "dark": true,
            "active": active,
        }))
        .expect("an installed theme")
    }

    type IntellijEvents = Rc<RefCell<Vec<IntellijThemesEvent>>>;

    fn intellij(cx: &mut TestAppContext) -> (Entity<IntellijThemesDialog>, IntellijEvents, &mut VisualTestContext) {
        init(cx);
        let (dialog, cx) = cx.add_window_view(|_, cx| IntellijThemesDialog::new(cx));
        let events: IntellijEvents = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            let record = move |_, event: &IntellijThemesEvent, _: &mut App| log.borrow_mut().push(event.clone());
            cx.subscribe(&dialog, record).detach();
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        (dialog, events, cx)
    }

    // src/features/plugins/__tests__/intellijImport.test.tsx › Import Theme from JetBrains IDE › lists one row per
    // theme plugin, the one in use first, and imports the chosen plugin
    #[gpui_kit::test]
    async fn lists_one_row_per_theme_plugin_the_one_in_use_first_and_imports_the_chosen_plugin(
        cx: &mut TestAppContext,
    ) {
        let (dialog, events, cx) = intellij(cx);
        assert!(dialog.read_with(cx, |dialog, _| dialog.rows().is_none()), "still looking");
        let found = vec![
            jetbrains("IntelliJ IDEA 2025.2", "Deep Sea Themes", "/idea/plugins/DeepSea", "Shallow", false),
            jetbrains("IntelliJ IDEA 2025.2", "Deep Sea Themes", "/idea/plugins/DeepSea", "Deep Sea", true),
            jetbrains("DataGrip 2026.2", "Night Owl", "/dg/plugins/NightOwl", "Night Owl", false),
        ];
        dialog.update(cx, |dialog, cx| dialog.set_themes(Ok(found), cx));
        let rows = dialog.read_with(cx, |dialog, _| dialog.rows().map(<[IntellijPluginRow]>::to_vec)).expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].plugin.as_str(), rows[0].in_use), ("Deep Sea Themes", true));
        assert_eq!(rows[0].themes, ["Shallow", "Deep Sea"]);
        assert!(!rows[1].in_use);
        let details: Vec<String> =
            cx.update(|_, cx| rows.iter().map(|row| IntellijThemesDialog::row_detail(row, cx)).collect());
        assert_eq!(details, ["2 themes · IntelliJ IDEA 2025.2", "1 theme · DataGrip 2026.2"]);
        click(cx, "intellij-row-1");
        click(cx, "intellij-button-import");
        assert_eq!(events.borrow().last(), Some(&IntellijThemesEvent::Import("/dg/plugins/NightOwl".to_owned())));
        cx.simulate_keystrokes("up enter");
        assert_eq!(events.borrow().last(), Some(&IntellijThemesEvent::Import("/idea/plugins/DeepSea".to_owned())));
    }

    // src/features/plugins/__tests__/intellijImport.test.tsx › Import Theme from JetBrains IDE › offers a file when
    // no IDE has theme plugins
    #[gpui_kit::test]
    async fn offers_a_file_when_no_ide_has_theme_plugins(cx: &mut TestAppContext) {
        let (dialog, events, cx) = intellij(cx);
        dialog.update(cx, |dialog, cx| dialog.set_themes(Ok(Vec::new()), cx));
        assert!(dialog.read_with(cx, |dialog, _| dialog.chosen().is_none()), "Import is disabled");
        click(cx, "intellij-button-import");
        assert!(events.borrow().is_empty());
        click(cx, "intellij-button-choose");
        assert_eq!(events.borrow().last(), Some(&IntellijThemesEvent::ChooseFile));
    }
}
