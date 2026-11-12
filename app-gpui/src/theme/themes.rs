//! The themes a user can select and how a choice becomes what the windows draw with: the built-in
//! themes, the ones live plugins contribute, how the appearance settings resolve to one of them
//! ("Sync with OS" included), and the root attributes and token overrides that apply it.
//!
//! Nothing here reads settings, storage or the OS: callers pass that state in and persist what
//! the functions hand back.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::tokens::{Color, Theme, ThemeAttributes};

/// Root attribute naming the light / dark base the stylesheet cascades from.
pub const THEME_ATTRIBUTE: &str = "data-theme";
/// Root attribute set to `high` by the high-contrast theme.
pub const CONTRAST_ATTRIBUTE: &str = "data-contrast";
/// Root attribute naming a palette theme (Solarized, One) over its light / dark base.
pub const COLOR_THEME_ATTRIBUTE: &str = "data-color-theme";
/// Root attribute naming the active plugin theme as `<pluginId>/<themeId>`.
pub const PLUGIN_THEME_ATTRIBUTE: &str = "data-plugin-theme";
/// Root attribute naming the light / dark base of the active plugin theme.
pub const PLUGIN_THEME_KIND_ATTRIBUTE: &str = "data-plugin-theme-kind";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeKind {
    Light,
    Dark,
}

impl ThemeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// Stable built-in ids shared by settings, quick switch and editor schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuiltinThemeId {
    Light,
    Dark,
    DarkHc,
    SolarizedLight,
    SolarizedDark,
    OneLight,
    OneDark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTheme {
    pub id: BuiltinThemeId,
    pub label: &'static str,
    pub kind: ThemeKind,
}

/// The built-in themes, in the order of [`BuiltinThemeId`]'s variants.
pub const BUILTIN_THEMES: [BuiltinTheme; 7] = [
    BuiltinTheme { id: BuiltinThemeId::Light, label: "Islands Light", kind: ThemeKind::Light },
    BuiltinTheme { id: BuiltinThemeId::Dark, label: "Islands Dark", kind: ThemeKind::Dark },
    BuiltinTheme { id: BuiltinThemeId::DarkHc, label: "High Contrast", kind: ThemeKind::Dark },
    BuiltinTheme { id: BuiltinThemeId::SolarizedLight, label: "Solarized Light", kind: ThemeKind::Light },
    BuiltinTheme { id: BuiltinThemeId::SolarizedDark, label: "Solarized Dark", kind: ThemeKind::Dark },
    BuiltinTheme { id: BuiltinThemeId::OneLight, label: "One Light", kind: ThemeKind::Light },
    BuiltinTheme { id: BuiltinThemeId::OneDark, label: "One Dark", kind: ThemeKind::Dark },
];

pub const BUILTIN_THEME_IDS: [BuiltinThemeId; 7] = [
    BuiltinThemeId::Light,
    BuiltinThemeId::Dark,
    BuiltinThemeId::DarkHc,
    BuiltinThemeId::SolarizedLight,
    BuiltinThemeId::SolarizedDark,
    BuiltinThemeId::OneLight,
    BuiltinThemeId::OneDark,
];

impl BuiltinThemeId {
    /// The id as settings and stylesheet selectors write it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::DarkHc => "dark-hc",
            Self::SolarizedLight => "solarized-light",
            Self::SolarizedDark => "solarized-dark",
            Self::OneLight => "one-light",
            Self::OneDark => "one-dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        BUILTIN_THEME_IDS.into_iter().find(|id| id.as_str() == value)
    }

    pub fn kind(self) -> ThemeKind {
        BUILTIN_THEMES[self as usize].kind
    }

    pub fn label(self) -> &'static str {
        BUILTIN_THEMES[self as usize].label
    }
}

impl fmt::Display for BuiltinThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BuiltinTheme {
    /// The theme as the pickers list it.
    pub fn choice(&self) -> ThemeChoice {
        ThemeChoice { id: ThemeChoiceId::Builtin(self.id), label: self.label.to_owned(), kind: self.kind, origin: None }
    }
}

pub fn is_builtin_theme(value: &str) -> bool {
    BuiltinThemeId::parse(value).is_some()
}

pub fn theme_kind(id: BuiltinThemeId) -> ThemeKind {
    id.kind()
}

/// A plugin theme as a theme setting value: `plugin:<pluginId>/<themeId>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginThemeRef {
    plugin_id: String,
    theme_id: String,
}

impl PluginThemeRef {
    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn theme_id(&self) -> &str {
        &self.theme_id
    }
}

impl fmt::Display for PluginThemeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin:{}/{}", self.plugin_id, self.theme_id)
    }
}

/// The setting value naming theme `theme_id` of plugin `plugin_id`.
pub fn plugin_theme_ref(plugin_id: &str, theme_id: &str) -> String {
    format!("plugin:{plugin_id}/{theme_id}")
}

/// A plugin id may contain `/`, a theme id may not: the last one separates them.
pub fn parse_plugin_theme_ref(value: &str) -> Option<PluginThemeRef> {
    let (plugin_id, theme_id) = value.strip_prefix("plugin:")?.rsplit_once('/')?;
    (!plugin_id.is_empty() && !theme_id.is_empty())
        .then(|| PluginThemeRef { plugin_id: plugin_id.to_owned(), theme_id: theme_id.to_owned() })
}

/// Any theme the settings can name: built-in or contributed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ThemeChoiceId {
    Builtin(BuiltinThemeId),
    Plugin(PluginThemeRef),
}

impl ThemeChoiceId {
    pub fn parse(value: &str) -> Option<Self> {
        BuiltinThemeId::parse(value).map(Self::Builtin).or_else(|| parse_plugin_theme_ref(value).map(Self::Plugin))
    }
}

impl fmt::Display for ThemeChoiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(id) => f.write_str(id.as_str()),
            Self::Plugin(reference) => write!(f, "{reference}"),
        }
    }
}

impl From<BuiltinThemeId> for ThemeChoiceId {
    fn from(id: BuiltinThemeId) -> Self {
        Self::Builtin(id)
    }
}

impl From<ThemeChoiceId> for String {
    fn from(id: ThemeChoiceId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for ThemeChoiceId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| format!("not a theme: {value}"))
    }
}

pub fn is_theme_choice_id(value: &str) -> bool {
    ThemeChoiceId::parse(value).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeChoice {
    pub id: ThemeChoiceId,
    pub label: String,
    pub kind: ThemeKind,
    /// Who contributes it (a plugin's name); built-in themes have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// Storage key of the kinds contributed themes had: `{ "<theme setting value>": "light" | "dark" }`.
pub const KINDS_KEY: &str = "openidea.themeKinds";

/// Themes of live plugins, offered next to the built-in ones, and the kind each one had when
/// it was last offered: a disabled plugin's theme falls back to the built-in of that kind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContributedThemes {
    themes: Vec<ThemeChoice>,
    /// As stored under [`KINDS_KEY`]; entries this code did not write are kept as they are.
    kinds: Map<String, Value>,
}

impl ContributedThemes {
    /// `stored_kinds` is the value stored under [`KINDS_KEY`], if any.
    pub fn new(stored_kinds: Option<&str>) -> Self {
        let kinds = match stored_kinds.map(serde_json::from_str::<Value>) {
            Some(Ok(Value::Object(kinds))) => kinds,
            _ => Map::new(),
        };
        Self { themes: Vec::new(), kinds }
    }

    pub fn themes(&self) -> &[ThemeChoice] {
        &self.themes
    }

    /// Offers `theme` (in place of one with the same id) until it is removed. Returns the value
    /// to store under [`KINDS_KEY`] when its kind was not the one remembered.
    pub fn add(&mut self, theme: ThemeChoice) -> Option<String> {
        let id = theme.id.to_string();
        let kind = Value::from(theme.kind.as_str());
        self.themes.retain(|t| t.id != theme.id);
        self.themes.push(theme);
        if self.kinds.get(&id) == Some(&kind) {
            return None;
        }
        self.kinds.insert(id, kind);
        serde_json::to_string(&self.kinds).ok()
    }

    pub fn remove(&mut self, theme: &ThemeChoice) {
        self.themes.retain(|t| t != theme);
    }

    /// Whether `id` can be applied now (a contributed theme's plugin may be off).
    pub fn is_available(&self, id: &ThemeChoiceId) -> bool {
        matches!(id, ThemeChoiceId::Builtin(_)) || self.themes.iter().any(|t| &t.id == id)
    }

    /// The built-in theme under `id`: itself, or for a contributed theme the light / dark
    /// default of its kind (the base its tokens apply over, and what shows while its plugin is
    /// off). `fallback` is used when its kind was never seen.
    pub fn base_theme(&self, id: &ThemeChoiceId, fallback: BuiltinThemeId) -> BuiltinThemeId {
        let reference = match id {
            ThemeChoiceId::Builtin(id) => return *id,
            ThemeChoiceId::Plugin(reference) => reference.to_string(),
        };
        let live = self.themes.iter().find(|t| &t.id == id).map(|t| t.kind.as_str());
        match live.or_else(|| self.kinds.get(&reference).and_then(Value::as_str)) {
            Some("dark") => BuiltinThemeId::Dark,
            Some("light") => BuiltinThemeId::Light,
            _ => fallback,
        }
    }

    pub fn groups(&self) -> Vec<ThemeGroup> {
        theme_groups(&self.themes)
    }

    pub fn choices(&self) -> Vec<ThemeChoice> {
        theme_choices(&self.themes)
    }
}

/// Themes of one origin: the built-in ones, or one plugin's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeGroup {
    /// The contributing plugin; none for the built-in themes.
    pub plugin_id: Option<String>,
    /// The plugin's name.
    pub origin: Option<String>,
    pub themes: Vec<ThemeChoice>,
}

/// Orders names for display the way a locale-aware comparison does for plain names: case is
/// ignored unless the names are otherwise equal (then lowercase first), and spaces sort before
/// punctuation, digits and letters. Accents and scripts are compared by code point, not by a
/// collation table.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    fn class(c: char) -> u8 {
        match c {
            c if c.is_whitespace() => 0,
            c if c.is_ascii_punctuation() => 1,
            c if c.is_numeric() => 2,
            _ => 3,
        }
    }
    let primary = |text: &str| text.chars().flat_map(char::to_lowercase).map(|c| (class(c), c)).collect::<Vec<_>>();
    let case = |text: &str| text.chars().map(|c| (c.is_uppercase(), c)).collect::<Vec<_>>();
    primary(a).cmp(&primary(b)).then_with(|| case(a).cmp(&case(b)))
}

/// The selectable themes by origin: the built-in ones, then each plugin's by its name, themes
/// by name.
pub fn theme_groups(contributed: &[ThemeChoice]) -> Vec<ThemeGroup> {
    fn name(group: &ThemeGroup) -> &str {
        group.origin.as_deref().or(group.plugin_id.as_deref()).unwrap_or("")
    }
    let mut plugins: Vec<ThemeGroup> = Vec::new();
    for theme in contributed {
        let plugin_id = match &theme.id {
            ThemeChoiceId::Plugin(reference) => reference.plugin_id(),
            ThemeChoiceId::Builtin(_) => "",
        };
        match plugins.iter_mut().find(|g| g.plugin_id.as_deref() == Some(plugin_id)) {
            Some(group) => group.themes.push(theme.clone()),
            None => plugins.push(ThemeGroup {
                plugin_id: Some(plugin_id.to_owned()),
                origin: theme.origin.clone(),
                themes: vec![theme.clone()],
            }),
        }
    }
    plugins.sort_by(|a, b| {
        locale_compare(name(a), name(b))
            .then_with(|| locale_compare(a.plugin_id.as_deref().unwrap_or(""), b.plugin_id.as_deref().unwrap_or("")))
    });
    for group in &mut plugins {
        group.themes.sort_by(|a, b| locale_compare(&a.label, &b.label));
    }
    let builtin =
        ThemeGroup { plugin_id: None, origin: None, themes: BUILTIN_THEMES.iter().map(BuiltinTheme::choice).collect() };
    std::iter::once(builtin).chain(plugins).collect()
}

/// Every theme that can be selected, in the order of [`theme_groups`].
pub fn theme_choices(contributed: &[ThemeChoice]) -> Vec<ThemeChoice> {
    theme_groups(contributed).into_iter().flat_map(|group| group.themes).collect()
}

// --- file icon themes (plugins) ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconThemePack {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconThemeVariantOption {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconThemeVariant {
    pub id: String,
    pub name: String,
    pub default: String,
    pub options: Vec<IconThemeVariantOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IconThemeSettingOption {
    HideFiles,
    HideFolders,
    HideUiIcons,
    HidePsiIcons,
    HollowFolders,
    Groups,
    Choice,
    Accent,
    Monochrome,
    Saturation,
    Opacity,
    IconSize,
    RowHeight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IconThemeSettingDefault {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconThemeSettingItem {
    pub option: IconThemeSettingOption,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<IconThemeSettingDefault>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconThemeSettingsSection {
    pub title: String,
    pub items: Vec<IconThemeSettingItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconThemeDetails {
    pub groups: Vec<IconThemePack>,
    pub choices: Vec<IconThemeVariant>,
    pub accent: bool,
    pub settings: Vec<IconThemeSettingsSection>,
}

/// A file icon theme of a live plugin, for Settings › Appearance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconThemeChoice {
    /// `<pluginId>/<themeId>`: the `appearance.fileIconTheme` value.
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_page: Option<String>,
}

/// Offers `theme` in place of one with the same id.
pub fn add_contributed_icon_theme(themes: &mut Vec<IconThemeChoice>, theme: IconThemeChoice) {
    themes.retain(|t| t.id != theme.id);
    themes.push(theme);
}

pub fn remove_contributed_icon_theme(themes: &mut Vec<IconThemeChoice>, theme: &IconThemeChoice) {
    themes.retain(|t| t != theme);
}

// --- resolving and applying the appearance settings ---

/// The `appearance.theme` setting: a theme, or the legacy `"system"` for "Sync with OS".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ThemeSetting {
    System,
    Choice(ThemeChoiceId),
}

impl ThemeSetting {
    pub fn parse(value: &str) -> Option<Self> {
        if value == "system" { Some(Self::System) } else { ThemeChoiceId::parse(value).map(Self::Choice) }
    }
}

impl fmt::Display for ThemeSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => f.write_str("system"),
            Self::Choice(id) => write!(f, "{id}"),
        }
    }
}

impl From<ThemeSetting> for String {
    fn from(setting: ThemeSetting) -> Self {
        setting.to_string()
    }
}

impl TryFrom<String> for ThemeSetting {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| format!("not a theme setting: {value}"))
    }
}

/// PyCharm's brand blue: the accent both Islands themes ship with.
pub const DEFAULT_ACCENT: &str = "#3871e1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentPreset {
    pub value: &'static str,
    pub label: &'static str,
}

/// Accent presets (Settings › Appearance, Welcome › Customize); the first is the Islands default.
pub const ACCENT_PRESETS: [AccentPreset; 4] = [
    AccentPreset { value: DEFAULT_ACCENT, label: "Blue" },
    AccentPreset { value: "#24a394", label: "Teal" },
    AccentPreset { value: "#955ae0", label: "Purple" },
    AccentPreset { value: "#e08855", label: "Orange" },
];

/// Accents that mean "use the theme's own" (the current default and the pre-Islands one).
const THEME_ACCENTS: [&str; 2] = [DEFAULT_ACCENT, "#8fa5d6"];

/// The part of the `appearance` settings that selects the theme. Values are kept as written:
/// one that names no theme resolves to a default instead of failing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub sync_with_os: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_light_theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_dark_theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
}

impl Appearance {
    fn follows_os(&self) -> bool {
        self.sync_with_os || self.theme.as_deref() == Some("system")
    }
}

/// The theme in effect as the settings name it: the chosen theme, or with "Sync with OS"
/// (legacy `theme = "system"`) the preferred light / dark theme for the OS mode. A plugin theme
/// (`plugin:<id>/<theme>`) is returned as is.
pub fn resolve_theme_choice(appearance: Option<&Appearance>, system_light: bool) -> ThemeChoiceId {
    let concrete = |value: &Option<String>, default: BuiltinThemeId| {
        value.as_deref().and_then(ThemeChoiceId::parse).unwrap_or(ThemeChoiceId::Builtin(default))
    };
    let Some(a) = appearance else { return ThemeChoiceId::Builtin(BuiltinThemeId::Light) };
    if !a.follows_os() {
        concrete(&a.theme, BuiltinThemeId::Light)
    } else if system_light {
        concrete(&a.preferred_light_theme, BuiltinThemeId::Light)
    } else {
        concrete(&a.preferred_dark_theme, BuiltinThemeId::Dark)
    }
}

/// Concrete built-in theme id: the one chosen, or the base of a chosen plugin theme (its light /
/// dark default, which also shows while its plugin is off).
pub fn resolve_theme_id(
    appearance: Option<&Appearance>,
    system_light: bool,
    contributed: &ContributedThemes,
) -> BuiltinThemeId {
    let sync = appearance.is_some_and(Appearance::follows_os);
    let fallback = if sync && !system_light { BuiltinThemeId::Dark } else { BuiltinThemeId::Light };
    contributed.base_theme(&resolve_theme_choice(appearance, system_light), fallback)
}

/// The light / dark base of a theme setting (`dark-hc` is dark plus `data-contrast="high"`).
pub fn resolve_theme(setting: Option<&str>, system_light: bool, contributed: &ContributedThemes) -> ThemeKind {
    let appearance = Appearance { theme: setting.map(str::to_owned), ..Appearance::default() };
    resolve_theme_id(Some(&appearance), system_light, contributed).kind()
}

/// What the theme is resolved from: the session override (a theme chosen while settings could
/// not be saved) replaces the saved appearance altogether.
fn selection(session_override: Option<&ThemeSetting>, appearance: Option<&Appearance>) -> Option<Appearance> {
    match session_override {
        Some(theme) => Some(Appearance { theme: Some(theme.to_string()), ..Appearance::default() }),
        None => appearance.cloned(),
    }
}

/// The concrete built-in theme in effect (`dark-hc` kept distinct; a plugin theme's base).
pub fn current_theme_id(
    session_override: Option<&ThemeSetting>,
    appearance: Option<&Appearance>,
    system_light: bool,
    contributed: &ContributedThemes,
) -> BuiltinThemeId {
    resolve_theme_id(selection(session_override, appearance).as_ref(), system_light, contributed)
}

/// The theme in effect as selected: built-in or a plugin's.
pub fn current_theme_choice(
    session_override: Option<&ThemeSetting>,
    appearance: Option<&Appearance>,
    system_light: bool,
) -> ThemeChoiceId {
    resolve_theme_choice(selection(session_override, appearance).as_ref(), system_light)
}

pub fn current_theme(
    session_override: Option<&ThemeSetting>,
    appearance: Option<&Appearance>,
    system_light: bool,
    contributed: &ContributedThemes,
) -> ThemeKind {
    current_theme_id(session_override, appearance, system_light, contributed).kind()
}

/// The root attributes for a built-in theme id, starting from the root's current ones: the
/// palette themes are `data-color-theme` over a light / dark base, high contrast is the dark
/// base with `data-contrast="high"`. While a plugin theme is active
/// (`data-plugin-theme-kind`), its light / dark base wins. Other attributes are kept.
pub fn apply_theme_attributes(id: BuiltinThemeId, root: ThemeAttributes) -> ThemeAttributes {
    let plugin_kind = root.get(PLUGIN_THEME_KIND_ATTRIBUTE).and_then(ThemeKind::parse);
    if let Some(kind) = plugin_kind {
        return root.without(COLOR_THEME_ATTRIBUTE).with(THEME_ATTRIBUTE, kind.as_str()).without(CONTRAST_ATTRIBUTE);
    }
    let root = match id {
        BuiltinThemeId::Light | BuiltinThemeId::Dark | BuiltinThemeId::DarkHc => root.without(COLOR_THEME_ATTRIBUTE),
        palette => root.with(COLOR_THEME_ATTRIBUTE, palette.as_str()),
    };
    let root = root.with(THEME_ATTRIBUTE, id.kind().as_str());
    if id == BuiltinThemeId::DarkHc { root.with(CONTRAST_ATTRIBUTE, "high") } else { root.without(CONTRAST_ATTRIBUTE) }
}

/// The accent to set as `--accent`: each theme ships its own, only a deliberate choice
/// overrides it.
pub fn custom_accent(accent: Option<&str>) -> Option<&str> {
    let accent = accent?;
    let hex = accent.strip_prefix('#')?;
    let valid = (3..=8).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    (valid && !THEME_ACCENTS.contains(&accent.to_ascii_lowercase().as_str())).then_some(accent)
}

/// What applying the appearance settings puts on the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedTheme {
    /// The attributes to cascade the token stylesheet with.
    pub attributes: ThemeAttributes,
    /// Tokens to set over the stylesheet (`Theme::set`). `--accent` is listed only for a custom
    /// accent: when it is absent the theme's own accent applies and a previous one is unset.
    pub overrides: Vec<(String, String)>,
}

/// The theme to apply for the settings. `None` while neither settings nor a session override
/// are known and `root` already carries a theme: the one restored at start-up stays rather
/// than flashing the light default.
pub fn apply_theme(
    session_override: Option<&ThemeSetting>,
    appearance: Option<&Appearance>,
    system_light: bool,
    contributed: &ContributedThemes,
    root: ThemeAttributes,
) -> Option<AppliedTheme> {
    let pending = session_override.is_none() && appearance.is_none();
    if pending && root.get(THEME_ATTRIBUTE).is_some_and(|theme| !theme.is_empty()) {
        return None;
    }
    let id = current_theme_id(session_override, appearance, system_light, contributed);
    let accent = custom_accent(appearance.and_then(|a| a.accent.as_deref()));
    Some(AppliedTheme {
        attributes: apply_theme_attributes(id, root),
        overrides: accent.map(|accent| ("--accent".to_owned(), accent.to_owned())).into_iter().collect(),
    })
}

/// The frame colour as `[r, g, b, a]` (0–255), for native window backgrounds.
pub fn window_background_color(theme: &Theme) -> Option<[u8; 4]> {
    let color = theme.color("bg-frame")?;
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([byte(color.r), byte(color.g), byte(color.b), byte(color.a)])
}

/// The frame colour as `#rrggbb[aa]`, for native window commands that take a string.
pub fn window_background_hex(theme: &Theme) -> Option<String> {
    theme.color("bg-frame").map(Color::to_hex)
}

/// Storage key of the [`StartupTheme`].
pub const STARTUP_THEME_KEY: &str = "openidea.theme";

/// What the next window applies before any stylesheet loads: the light / dark base, and the
/// resolved frame, text and accent colours its splash is painted in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StartupTheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
}

/// The start-up record of `theme`, to store as JSON under [`STARTUP_THEME_KEY`]. A previewed
/// theme is not the next start-up's: skip storing it while [`ThemePreview::previewing`].
pub fn startup_theme(theme: &Theme) -> StartupTheme {
    let attribute = |name: &str| theme.attributes().get(name).map(str::to_owned);
    let hex = |name: &str| theme.color(name).map(Color::to_hex);
    StartupTheme {
        theme: attribute(THEME_ATTRIBUTE),
        contrast: attribute(CONTRAST_ATTRIBUTE),
        color_theme: attribute(COLOR_THEME_ATTRIBUTE),
        frame: hex("bg-frame"),
        fg: hex("fg-muted"),
        accent: hex("accent"),
    }
}

/// One `settings_set` call: a key and its value as JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingWrite {
    pub scope: &'static str,
    pub key: &'static str,
    pub value: String,
}

/// The settings to write when the user picks theme `next`. An explicit choice ends "Sync with
/// OS", as in IDEA: `sync_with_os` is the setting as it stands after the theme is written.
/// Until the writes succeed, `next` is the session override.
pub fn theme_setting_writes(next: &ThemeSetting, sync_with_os: bool) -> Vec<SettingWrite> {
    let write = |key: &'static str, value: Value| SettingWrite { scope: "global", key, value: value.to_string() };
    let theme = write("appearance.theme", Value::from(next.to_string()));
    let sync = sync_with_os.then(|| write("appearance.syncWithOs", Value::Bool(false)));
    std::iter::once(theme).chain(sync).collect()
}

/// The theme the light / dark toggle switches to.
pub fn toggled_theme(current: ThemeKind) -> BuiltinThemeId {
    match current {
        ThemeKind::Dark => BuiltinThemeId::Light,
        ThemeKind::Light => BuiltinThemeId::Dark,
    }
}

/// Which plugin theme a preview shows in this window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginThemePreview {
    /// The plugin theme that is selected, if one is.
    Selected,
    /// None: a built-in theme is previewed.
    Off,
    Theme(PluginThemeRef),
}

/// Showing a theme in this window without selecting it (Quick Switch's instant theme switch).
/// Nothing is saved and other windows keep theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePreview {
    pub plugin: PluginThemePreview,
    /// The built-in theme whose attributes to apply ([`apply_theme_attributes`]); none while a
    /// plugin theme is previewed, which brings its own base.
    pub builtin: Option<BuiltinThemeId>,
    /// Whether a preview is showing: its theme must not be stored as the start-up theme.
    pub previewing: bool,
}

/// Previews `id`; `None` shows the selected theme again, whose built-in id is `current`.
pub fn preview_theme(id: Option<&ThemeChoiceId>, current: BuiltinThemeId) -> ThemePreview {
    let (plugin, builtin) = match id {
        None => (PluginThemePreview::Selected, Some(current)),
        Some(ThemeChoiceId::Builtin(id)) => (PluginThemePreview::Off, Some(*id)),
        Some(ThemeChoiceId::Plugin(reference)) => (PluginThemePreview::Theme(reference.clone()), None),
    };
    ThemePreview { plugin, builtin, previewing: id.is_some() }
}

/// The plugin named beside a contributed theme, unless the theme carries its name already.
pub fn origin_detail<'a>(label: &str, origin: Option<&'a str>) -> Option<&'a str> {
    origin.filter(|origin| !origin.is_empty() && *origin != label)
}

/// One row of a theme picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    /// A theme setting value, or `plugin:<pluginId>` for a section heading.
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
    /// A heading naming the plugin of the rows below it; not selectable.
    pub section: bool,
}

/// The themes for a picker: built-in themes, then each plugin's under a heading with its name
/// (`choices` in [`theme_groups`] order). A selected plugin theme whose plugin is off stays
/// listed.
pub fn theme_select_options(choices: &[ThemeChoice], selected: Option<&str>) -> Vec<SelectOption> {
    let heading = |plugin_id: &str, label: &str| SelectOption {
        value: format!("plugin:{plugin_id}"),
        label: label.to_owned(),
        detail: None,
        section: true,
    };
    let mut options = Vec::new();
    let mut group: Option<&str> = None;
    for choice in choices {
        let plugin_id = match &choice.id {
            ThemeChoiceId::Plugin(reference) => Some(reference.plugin_id()),
            ThemeChoiceId::Builtin(_) => None,
        };
        if let Some(plugin_id) = plugin_id.filter(|id| Some(*id) != group) {
            options.push(heading(plugin_id, choice.origin.as_deref().unwrap_or(plugin_id)));
        }
        group = plugin_id;
        // Under its plugin's heading a theme needs no origin.
        options.push(SelectOption {
            value: choice.id.to_string(),
            label: choice.label.clone(),
            detail: None,
            section: false,
        });
    }
    let Some(selected) = selected else { return options };
    let Some(missing) = parse_plugin_theme_ref(selected) else { return options };
    if options.iter().any(|o| o.value == selected) {
        return options;
    }
    let option = SelectOption {
        value: selected.to_owned(),
        label: missing.theme_id().to_owned(),
        detail: Some("Plugin not enabled".to_owned()),
        section: false,
    };
    // Under its own plugin's heading, not the last one listed.
    let own_heading = heading(missing.plugin_id(), missing.plugin_id());
    match options.iter().position(|o| o.section && o.value == own_heading.value) {
        None => options.extend([own_heading, option]),
        Some(at) => {
            let end = options[at + 1..].iter().position(|o| o.section).map_or(options.len(), |next| at + 1 + next);
            options.insert(end, option);
        }
    }
    options
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::super::color_scheme::{
        ColorOverrides, SchemeSource, TextAttributesOverride, effective_scheme, scheme_css, theme_scheme,
    };
    use super::super::tokens::{TOKENS_CSS, Tokens};
    use super::*;

    fn appearance(theme: &str, light: &str, dark: &str) -> Appearance {
        Appearance {
            theme: Some(theme.to_owned()),
            preferred_light_theme: Some(light.to_owned()),
            preferred_dark_theme: Some(dark.to_owned()),
            ..Appearance::default()
        }
    }

    fn plugin_theme(plugin: &str, id: &str, label: &str, origin: &str) -> ThemeChoice {
        ThemeChoice {
            id: ThemeChoiceId::parse(&plugin_theme_ref(plugin, id)).unwrap(),
            label: label.to_owned(),
            kind: ThemeKind::Dark,
            origin: Some(origin.to_owned()),
        }
    }

    fn row(value: &str, label: &str, detail: Option<&str>, section: bool) -> SelectOption {
        SelectOption { value: value.to_owned(), label: label.to_owned(), detail: detail.map(str::to_owned), section }
    }

    /// The rows after the built-in themes, headings in brackets.
    fn plugin_rows(options: &[SelectOption]) -> Vec<String> {
        options[BUILTIN_THEMES.len()..]
            .iter()
            .map(|o| if o.section { format!("[{}]", o.label) } else { o.label.clone() })
            .collect()
    }

    // src/platform/themes.test.ts › built-in palettes › preserves $id through validation and system resolution
    #[test]
    fn preserves_every_builtin_theme_through_system_resolution() {
        let source = SchemeSource::new(TOKENS_CSS);
        let none = ContributedThemes::default();
        let customs = BTreeMap::new();
        for BuiltinTheme { id, kind, .. } in BUILTIN_THEMES {
            let name = id.as_str();
            let a = appearance(name, name, name);
            assert_eq!(resolve_theme_id(Some(&a), false, &none), id);
            assert_eq!(resolve_theme(Some(name), false, &none), kind);
            let synced = Appearance { sync_with_os: true, ..a.clone() };
            assert_eq!(resolve_theme_id(Some(&synced), true, &none), id);
            let system = Appearance { theme: Some("system".to_owned()), ..a };
            assert_eq!(resolve_theme_id(Some(&system), false, &none), id);
            let root = apply_theme_attributes(id, ThemeAttributes::default());
            assert_eq!(root.get(THEME_ATTRIBUTE), Some(kind.as_str()));
            assert_eq!(theme_scheme(Some(&root)), id);
            assert_eq!(effective_scheme(Some(name), Some(&root), &customs), name);
            assert_eq!(scheme_css(&source, Some("theme-default"), None, Some(&root), &customs), "");
            assert!(source.tokens(id).get("--bg-editor").is_some_and(|value| !value.contains("var(")));
        }
    }

    // src/platform/themes.test.ts › built-in palettes › rejects unknown ids and retains legacy fallbacks
    #[test]
    fn rejects_unknown_ids_and_retains_legacy_fallbacks() {
        let none = ContributedThemes::default();
        let themed = |theme: &str| Appearance { theme: Some(theme.to_owned()), ..Appearance::default() };
        assert!(!is_builtin_theme("not-a-theme"));
        assert_eq!(resolve_theme_id(Some(&themed("bad")), true, &none), BuiltinThemeId::Light);
        let system = Appearance { preferred_dark_theme: Some("bad".to_owned()), ..themed("system") };
        assert_eq!(resolve_theme_id(Some(&system), false, &none), BuiltinThemeId::Dark);
        assert_eq!(theme_kind(BuiltinThemeId::SolarizedLight), ThemeKind::Light);
    }

    // src/platform/themes.test.ts › built-in palettes › resolves editor scheme tokens from the palettes
    #[test]
    fn resolves_editor_scheme_tokens_from_the_palettes() {
        let source = SchemeSource::new(TOKENS_CSS);
        let token = |id: BuiltinThemeId, name: &str| source.tokens(id).get(name).map(str::to_owned);
        assert_eq!(token(BuiltinThemeId::SolarizedLight, "--bg-editor").as_deref(), Some("#fdf6e3"));
        assert_eq!(token(BuiltinThemeId::SolarizedDark, "--bg-editor").as_deref(), Some("#002b36"));
        assert_eq!(token(BuiltinThemeId::SolarizedDark, "--syn-string").as_deref(), Some("#2aa198"));
        assert_eq!(token(BuiltinThemeId::SolarizedLight, "--ansi-bright-blue").as_deref(), Some("#839496"));
        assert_eq!(token(BuiltinThemeId::OneLight, "--syn-keyword").as_deref(), Some("hsl(301, 63%, 40%)"));
        assert_eq!(token(BuiltinThemeId::OneDark, "--syn-string").as_deref(), Some("hsl(95, 38%, 62%)"));
    }

    // src/platform/themes.test.ts › built-in palettes › keeps explicit editor schemes and per-scheme edits
    // independent of UI palettes
    #[test]
    fn keeps_explicit_editor_schemes_and_edits_independent_of_ui_palettes() {
        let source = SchemeSource::new(TOKENS_CSS);
        let customs = BTreeMap::new();
        let keyword = TextAttributesOverride { foreground: Some("#123456".to_owned()), ..Default::default() };
        let edits: ColorOverrides =
            BTreeMap::from([("solarized-light".to_owned(), BTreeMap::from([("keyword".to_owned(), keyword)]))]);
        let root = apply_theme_attributes(BuiltinThemeId::SolarizedLight, ThemeAttributes::default());
        assert!(
            scheme_css(&source, Some("one-dark"), None, Some(&root), &customs)
                .contains("--bg-editor: hsl(220, 13%, 18%) !important")
        );
        assert!(
            scheme_css(&source, Some("theme-default"), Some(&edits), Some(&root), &customs)
                .contains("--syn-keyword: #123456 !important")
        );
        let root = apply_theme_attributes(BuiltinThemeId::OneLight, root);
        assert_eq!(scheme_css(&source, Some("theme-default"), Some(&edits), Some(&root), &customs), "");
    }

    // src/platform/themes.test.ts › built-in palettes › clears the palette for plugins and restores it when the
    // plugin is removed
    #[test]
    fn clears_the_palette_for_plugins_and_restores_it_when_the_plugin_is_removed() {
        let root = apply_theme_attributes(BuiltinThemeId::OneDark, ThemeAttributes::default());
        let root = apply_theme_attributes(BuiltinThemeId::OneDark, root.with(PLUGIN_THEME_KIND_ATTRIBUTE, "light"));
        assert_eq!(root.get(COLOR_THEME_ATTRIBUTE), None);
        assert_eq!(root.get(THEME_ATTRIBUTE), Some("light"));
        let root = apply_theme_attributes(BuiltinThemeId::OneDark, root.without(PLUGIN_THEME_KIND_ATTRIBUTE));
        assert_eq!(root.get(COLOR_THEME_ATTRIBUTE), Some("one-dark"));
        let root = apply_theme_attributes(BuiltinThemeId::DarkHc, root);
        assert_eq!(root.get(COLOR_THEME_ATTRIBUTE), None);
        assert_eq!(root.get(CONTRAST_ATTRIBUTE), Some("high"));
        let root = apply_theme_attributes(BuiltinThemeId::SolarizedDark, root);
        assert_eq!(root.get(CONTRAST_ATTRIBUTE), None);
    }

    // src/platform/themes.test.ts › plugin themes › are settings values like the built-in ones, in every picker
    // while their plugin is live
    #[test]
    fn plugin_themes_are_settings_values_in_every_picker_while_their_plugin_is_live() {
        let reference = plugin_theme_ref("acme.night", "night");
        let night = plugin_theme("acme.night", "night", "Night", "Acme");
        let a = appearance(&reference, "light", &reference);
        let synced = Appearance { sync_with_os: true, ..a.clone() };
        assert_eq!(resolve_theme_choice(Some(&a), false), night.id);
        assert_eq!(resolve_theme_choice(Some(&synced), false), night.id);
        assert_eq!(resolve_theme_choice(Some(&synced), true), ThemeChoiceId::Builtin(BuiltinThemeId::Light));

        let mut themes = ContributedThemes::default();
        let stored = themes.add(night.clone());
        assert!(themes.is_available(&night.id));
        assert_eq!(themes.choices().last(), Some(&night));
        let options = theme_select_options(&themes.choices(), Some(reference.as_str()));
        assert_eq!(
            options[options.len() - 2..],
            [row("plugin:acme.night", "Acme", None, true), row(&reference, "Night", None, false)]
        );
        // Its tokens apply over the default of its kind.
        assert_eq!(resolve_theme_id(Some(&a), false, &themes), BuiltinThemeId::Dark);

        themes.remove(&night);
        assert!(!themes.is_available(&night.id));
        // Its plugin off: the base of the kind it had shows, and the pickers keep it listed.
        assert_eq!(resolve_theme_id(Some(&a), false, &themes), BuiltinThemeId::Dark);
        let later = ContributedThemes::new(stored.as_deref());
        assert_eq!(resolve_theme_id(Some(&a), false, &later), BuiltinThemeId::Dark);
        let options = theme_select_options(&themes.choices(), Some(reference.as_str()));
        assert_eq!(
            options[options.len() - 2..],
            [
                row("plugin:acme.night", "acme.night", None, true),
                row(&reference, "night", Some("Plugin not enabled"), false)
            ]
        );
        // Its plugin live but the theme gone (an update dropped it): listed under that plugin's heading.
        let dawn = plugin_theme("acme.night", "dawn", "Dawn", "Acme");
        themes.add(dawn.clone());
        themes.add(plugin_theme("zeta.blue", "blue", "Blue", "Zeta"));
        assert_eq!(
            plugin_rows(&theme_select_options(&themes.choices(), Some(reference.as_str()))),
            ["[Acme]", "Dawn", "night", "[Zeta]", "Blue"]
        );
        themes.remove(&dawn);
    }

    // src/platform/themes.test.ts › plugin themes › are grouped by plugin: the built-in ones first, then each
    // plugin's by name
    #[test]
    fn plugin_themes_are_grouped_by_plugin_after_the_builtin_ones() {
        let mut themes = ContributedThemes::default();
        themes.add(plugin_theme("intellij.dracula", "dracula", "Dracula", "Dracula Theme"));
        themes.add(plugin_theme("acme.night", "night", "Night", "Acme"));
        themes.add(plugin_theme("intellij.dracula", "alucard", "Dracula Alucard", "Dracula Theme"));
        themes.add(plugin_theme("acme.night", "dawn", "Dawn", "Acme"));

        let groups = themes.groups();
        let labels = |group: &ThemeGroup| group.themes.iter().map(|t| t.label.clone()).collect::<Vec<_>>();
        let builtin: Vec<String> = BUILTIN_THEMES.iter().map(|t| t.label.to_owned()).collect();
        assert_eq!(
            groups.iter().map(|g| (g.origin.clone(), labels(g))).collect::<Vec<_>>(),
            [
                (None, builtin),
                (Some("Acme".to_owned()), vec!["Dawn".to_owned(), "Night".to_owned()]),
                (Some("Dracula Theme".to_owned()), vec!["Dracula".to_owned(), "Dracula Alucard".to_owned()]),
            ]
        );
        assert_eq!(
            themes.choices().iter().map(|t| t.label.clone()).collect::<Vec<_>>(),
            groups.iter().flat_map(labels).collect::<Vec<_>>()
        );
        // Selects head each plugin's themes with its name.
        assert_eq!(
            plugin_rows(&theme_select_options(&themes.choices(), None)),
            ["[Acme]", "Dawn", "Night", "[Dracula Theme]", "Dracula", "Dracula Alucard"]
        );
    }

    // src/platform/themes.test.ts › plugin themes › parse only well-formed references
    #[test]
    fn plugin_theme_references_parse_only_when_well_formed() {
        let parsed = parse_plugin_theme_ref(&plugin_theme_ref("acme.night", "night")).unwrap();
        assert_eq!((parsed.plugin_id(), parsed.theme_id()), ("acme.night", "night"));
        for bad in ["plugin:", "plugin:/night", "plugin:acme/", "acme/night"] {
            assert_eq!(parse_plugin_theme_ref(bad), None, "{bad}");
        }
    }

    // src/workbench/theme.test.ts › window background › resolves the frame to Tauri's RGBA tuple instead of
    // passing CSS hsl() or mixes
    #[test]
    fn resolves_the_frame_to_an_rgba_tuple_instead_of_css_functions() {
        let mut theme = Tokens::parse(":root { --bg-frame: #26282c; }").theme(&ThemeAttributes::default());
        assert_eq!(window_background_color(&theme), Some([38, 40, 44, 255]));
        theme.set("--bg-frame", "hsl(230, 1%, 92%)");
        assert_eq!(window_background_color(&theme), Some([234, 234, 235, 255]));
        assert_eq!(window_background_hex(&theme).as_deref(), Some("#eaeaeb"));
        theme.set("--bg-frame", "color-mix(in srgb, #8040bf 80%, transparent)");
        assert_eq!(window_background_color(&theme), Some([128, 64, 191, 204]));
        assert_eq!(window_background_hex(&theme).as_deref(), Some("#8040bfcc"));
        let empty = Tokens::parse("").theme(&ThemeAttributes::default());
        assert_eq!(window_background_color(&empty), None);
        assert_eq!(window_background_hex(&empty), None);
    }

    // src/workbench/theme.test.ts › window background › remembers the base and the resolved splash colours for
    // the next window
    #[test]
    fn remembers_the_base_and_the_resolved_splash_colours_for_the_next_window() {
        let tokens = Tokens::parse(":root { --bg-frame: #eee8d5; --fg-muted: #657b83; --accent: #268bd2; }");
        let stored = serde_json::to_string(&startup_theme(&tokens.theme(&ThemeAttributes::light()))).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&stored).unwrap(),
            json!({ "theme": "light", "frame": "#eee8d5", "fg": "#657b83", "accent": "#268bd2" })
        );
    }
}
