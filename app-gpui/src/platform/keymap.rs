//! Keymaps: binding syntax, presets, user overrides and display.
//!
//! Binding syntax: modifiers `Meta Ctrl Alt Shift` joined with `+` to a key, e.g.
//! `Meta+Shift+O`, `Alt+F7`, `F8`. Space separates chord strokes (`Meta+K Meta+C`).
//! A modifier pressed twice is `Shift Shift` (double Shift, `Ctrl Ctrl`…); pressed
//! twice and held, then a key, `Alt Alt+Up`. Mouse shortcuts name the button 1-based
//! (`Button1` = left, `Button4` = back side button, `Button5` = forward), optionally
//! with modifiers (`Alt+Button4`); they are single strokes. `SwipeLeft` / `SwipeRight`
//! are the trackpad page swipes.
//!
//! Dispatch lives in [`super::dispatcher`].

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

/// Action id → its bindings.
pub type Keymap = BTreeMap<String, Vec<String>>;

/// Display names of the presets, in menu order.
pub const PRESET_TITLES: [(&str, &str); 6] = [
    ("intellij-macos", "macOS"),
    ("intellij-windows", "Windows"),
    ("vscode-macos", "VS Code (macOS)"),
    ("vscode-windows", "VS Code (Windows)"),
    ("eclipse-macos", "Eclipse (macOS)"),
    ("eclipse-windows", "Eclipse"),
];

/// The built-in preset `id`. The presets are generated from `src/platform/keymaps/*.ts`
/// (`docs/gpui-port/gen-data.mjs`) and parsed once.
pub fn preset(id: &str) -> Option<&'static Keymap> {
    static PRESETS: OnceLock<BTreeMap<String, Keymap>> = OnceLock::new();
    PRESETS
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/gen/keymaps.json"))
                .expect("assets/gen/keymaps.json is generated and valid")
        })
        .get(id)
}

/// macOS spells the primary modifier ⌘ (`Meta`); Windows and Linux use Ctrl.
pub fn is_mac_host() -> bool {
    cfg!(target_os = "macos")
}

/// The preset used when settings name none (or an unknown one): IDEA's keymap for this OS.
pub fn default_preset(mac: bool) -> &'static str {
    if mac { "intellij-macos" } else { "intellij-windows" }
}

/// A binding written for macOS (`Meta+C`) as this OS spells it (`Ctrl+C` on Windows/Linux).
pub fn host_binding(binding: &str, mac: bool) -> String {
    if mac { binding.to_owned() } else { binding.replace("Meta+", "Ctrl+") }
}

/// A canonical stroke in macOS spelling, so tables keyed `Meta+C` also match Ctrl+C on Windows/Linux.
pub fn mac_stroke(stroke: &str, mac: bool) -> String {
    if mac { stroke.to_owned() } else { normalize_binding(&stroke.replace("Ctrl+", "Meta+")) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

impl Modifier {
    /// Canonical order of modifiers in a stroke.
    pub const ALL: [Modifier; 4] = [Modifier::Ctrl, Modifier::Alt, Modifier::Shift, Modifier::Meta];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Meta => "Meta",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(name))
    }
}

/// `ButtonN` → N.
fn mouse_button(key: &str) -> Option<&str> {
    let digits = key.get(..6).filter(|p| p.eq_ignore_ascii_case("button")).map(|_| &key[6..])?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

fn swipe(key: &str) -> Option<&'static str> {
    if key.eq_ignore_ascii_case("swipeleft") {
        Some("SwipeLeft")
    } else if key.eq_ignore_ascii_case("swiperight") {
        Some("SwipeRight")
    } else {
        None
    }
}

/// The key of a stroke: what follows the last `+` separator (`+` itself for `Meta++`).
fn split_stroke(stroke: &str) -> (Vec<&str>, &str) {
    if let Some(mods) = stroke.strip_suffix("++") {
        return (mods.split('+').collect(), "+");
    }
    if stroke == "+" {
        return (Vec::new(), "+");
    }
    let mut parts: Vec<&str> = stroke.split('+').collect();
    let key = parts.pop().unwrap_or("");
    (parts, key)
}

/// A mouse shortcut (`Button4`, `Shift+Button5`) rather than a keystroke.
pub fn is_mouse_binding(binding: &str) -> bool {
    let b = binding.trim();
    !b.contains(char::is_whitespace) && mouse_button(split_stroke(b).1).is_some()
}

/// A trackpad page swipe (`SwipeLeft`, `SwipeRight`).
pub fn is_page_swipe(binding: &str) -> bool {
    matches!(binding, "SwipeLeft" | "SwipeRight")
}

/// A double-modifier gesture: `Shift Shift` (key `None`) or `Alt Alt+Up`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoubleModifier {
    pub modifier: Modifier,
    pub key: Option<String>,
}

pub fn parse_double_modifier(binding: &str) -> Option<DoubleModifier> {
    let mut strokes = binding.split_whitespace();
    let (first, second) = (strokes.next()?, strokes.next()?);
    if strokes.next().is_some() {
        return None;
    }
    let modifier = Modifier::named(first)?;
    let (second_modifier, key) = match second.split_once('+') {
        None => (second, None),
        Some((m, key)) if !key.is_empty() && (key == "+" || !key.contains('+')) => (m, Some(key)),
        Some(_) => return None,
    };
    if Modifier::named(second_modifier)? != modifier {
        return None;
    }
    let key = key.map(|k| if k.chars().count() == 1 { k.to_uppercase() } else { k.to_owned() });
    Some(DoubleModifier { modifier, key })
}

fn is_stroke(stroke: &str) -> bool {
    let (mods, key) = split_stroke(stroke);
    !key.is_empty() && !key.contains(char::is_whitespace) && mods.iter().all(|m| Modifier::named(m).is_some())
}

/// Syntactic check: one or two strokes of `Mod+…+Key`, or a double-modifier gesture.
pub fn is_valid_binding(binding: &str) -> bool {
    let b = binding.trim();
    if parse_double_modifier(b).is_some() {
        return true;
    }
    let strokes: Vec<&str> = b.split_whitespace().collect();
    if strokes.iter().any(|s| {
        let lower = s.to_ascii_lowercase();
        lower.ends_with("swipeleft") || lower.ends_with("swiperight")
    }) {
        return strokes.len() == 1 && swipe(b).is_some();
    }
    (1..=2).contains(&strokes.len()) && strokes.iter().all(|s| is_stroke(s))
}

/// Canonical form: modifiers in fixed order, so `Shift+Meta+o` == `Meta+Shift+O`.
pub fn normalize_binding(binding: &str) -> String {
    if let Some(double) = parse_double_modifier(binding) {
        let m = double.modifier.name();
        return match double.key {
            Some(key) => format!("{m} {m}+{key}"),
            None => format!("{m} {m}"),
        };
    }
    binding
        .split_whitespace()
        .map(|stroke| {
            let (parts, key) = split_stroke(stroke);
            if let Some(swipe) = swipe(key) {
                return swipe.to_owned();
            }
            let mut out: Vec<String> = Modifier::ALL
                .into_iter()
                .filter(|m| parts.iter().any(|p| p.eq_ignore_ascii_case(m.name())))
                .map(|m| m.name().to_owned())
                .collect();
            out.push(match mouse_button(key) {
                Some(n) => format!("Button{n}"),
                None if key.chars().count() == 1 => key.to_uppercase(),
                None => key.to_owned(),
            });
            out.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "Meta" => "⌘",
        "Ctrl" => "⌃",
        "Alt" => "⌥",
        "Shift" => "⇧",
        "Enter" => "↵",
        "Escape" => "⎋",
        "Tab" => "⇥",
        "Backspace" => "⌫",
        "Delete" => "⌦",
        "Up" => "↑",
        "Down" => "↓",
        "Left" => "←",
        "Right" => "→",
        "Space" => "Space",
        "PageUp" => "⇞",
        "PageDown" => "⇟",
        _ => return None,
    })
}

fn key_label(key: &str) -> Option<&'static str> {
    Some(match key {
        "NumpadAdd" => "NumPad +",
        "NumpadSubtract" => "NumPad -",
        "NumpadMultiply" => "NumPad *",
        "NumpadDivide" => "NumPad /",
        _ => return None,
    })
}

/// Human display. macOS: `Meta+Shift+O` → `⇧⌘O`, `Shift Shift` → `⇧⇧`, `Alt Alt+Up` → `⌥⌥↑`.
/// Windows/Linux: `Ctrl+Shift+N`, chords as `Ctrl+K, Ctrl+I`, `Shift, Shift`, `Ctrl, Ctrl+Up`.
pub fn format_binding(binding: &str, mac: bool) -> String {
    let symbols = |parts: &[&str]| parts.iter().map(|p| symbol(p).unwrap_or(p)).collect::<String>();
    if is_mouse_binding(binding) {
        let normalized = normalize_binding(binding);
        let (parts, key) = split_stroke(&normalized);
        let button = if key == "Button1" { "Left Click".to_owned() } else { format!("{key} Click") };
        return if mac {
            symbols(&parts) + &button
        } else {
            parts.into_iter().map(str::to_owned).chain([button]).collect::<Vec<_>>().join("+")
        };
    }
    if !mac {
        return normalize_binding(binding)
            .split(' ')
            .map(|stroke| {
                let (parts, key) = split_stroke(stroke);
                parts.into_iter().chain([key_label(key).unwrap_or(key)]).collect::<Vec<_>>().join("+")
            })
            .collect::<Vec<_>>()
            .join(", ");
    }
    if let Some(double) = parse_double_modifier(binding) {
        let m = symbol(double.modifier.name()).unwrap_or_default();
        let key = double.key.as_deref().map(|k| symbol(k).unwrap_or(k)).unwrap_or_default();
        return format!("{m}{m}{key}");
    }
    normalize_binding(binding)
        .split(' ')
        .map(|stroke| {
            let (parts, key) = split_stroke(stroke);
            // macOS menu order: ⌃⌥⇧⌘
            symbols(&parts) + symbol(key).or_else(|| key_label(key)).unwrap_or(key)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A user keymap in settings: its parent preset plus its own overrides.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct CustomKeymap {
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub overrides: Option<serde_json::Value>,
}

/// The effective keymap: a preset, the user's overrides and extensions' defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct KeymapState {
    preset: String,
    /// User overrides: action id → bindings (an empty list removes the defaults).
    overrides: Keymap,
    /// Extensions' default bindings (plugins), below the preset.
    extensions: Keymap,
    /// `extensions` minus what the preset and the user already use.
    usable_extensions: Keymap,
}

impl Default for KeymapState {
    fn default() -> Self {
        Self::new(default_preset(is_mac_host()))
    }
}

impl KeymapState {
    pub fn new(preset_id: &str) -> Self {
        let preset = if preset(preset_id).is_some() { preset_id } else { default_preset(is_mac_host()) };
        Self {
            preset: preset.to_owned(),
            overrides: Keymap::new(),
            extensions: Keymap::new(),
            usable_extensions: Keymap::new(),
        }
    }

    pub fn preset_id(&self) -> &str {
        &self.preset
    }

    pub fn overrides(&self) -> &Keymap {
        &self.overrides
    }

    fn preset_map(&self) -> &'static Keymap {
        preset(&self.preset).expect("the state only ever names a built-in preset")
    }

    /// Replaces the user's overrides as given (bindings are normalized).
    pub fn set_overrides(&mut self, overrides: Keymap) {
        self.overrides = overrides
            .into_iter()
            .map(|(id, keys)| (id, dedup(keys.iter().filter(|k| is_valid_binding(k)).map(|k| normalize_binding(k)))))
            .collect();
        self.refresh_extensions();
    }

    /// Applies keymap settings (preset id + per-action overrides). Input comes from settings
    /// files, so it is validated: an unknown preset falls back to the default, malformed
    /// bindings are dropped. Returns whether anything changed.
    pub fn apply_settings(
        &mut self,
        preset_setting: Option<&serde_json::Value>,
        overrides: Option<&serde_json::Value>,
        custom_keymaps: Option<&serde_json::Value>,
    ) -> bool {
        let mut preset_name = preset_setting.and_then(|v| v.as_str()).map(str::to_owned);
        let mut overrides = overrides.cloned();
        // A user keymap (`customKeymaps[id]`) is its parent preset plus its own overrides.
        if let Some(name) = &preset_name
            && preset(name).is_none()
            && let Some(custom) = custom_keymaps.and_then(|c| c.get(name)).filter(|c| c.is_object())
            && let Ok(custom) = serde_json::from_value::<CustomKeymap>(custom.clone())
        {
            preset_name = custom.parent;
            overrides = custom.overrides;
        }
        let id = preset_name.as_deref().filter(|p| preset(p).is_some()).unwrap_or(default_preset(is_mac_host()));
        let mut clean = Keymap::new();
        if let Some(serde_json::Value::Object(entries)) = &overrides {
            for (action, keys) in entries {
                let Some(keys) = keys.as_array() else { continue };
                let valid =
                    keys.iter().filter_map(|k| k.as_str()).filter(|k| is_valid_binding(k)).map(normalize_binding);
                clean.insert(action.clone(), dedup(valid));
            }
        }
        if self.preset == id && self.overrides == clean {
            return false;
        }
        self.preset = id.to_owned();
        self.overrides = clean;
        self.refresh_extensions();
        true
    }

    /// Default bindings contributed by an extension (a plugin's `keymaps`), below the preset
    /// and the user's: dropped when the keystroke is already taken.
    pub fn register_extension_bindings(&mut self, action_id: &str, bindings: &[String]) {
        let valid = bindings.iter().filter(|b| is_valid_binding(b)).cloned().collect();
        self.extensions.insert(action_id.to_owned(), valid);
        self.refresh_extensions();
    }

    pub fn unregister_extension_bindings(&mut self, action_id: &str) {
        if self.extensions.remove(action_id).is_some() {
            self.refresh_extensions();
        }
    }

    /// Plugin default bindings that stay: those of actions the preset and the user leave
    /// unbound, on keystrokes nothing else uses (the IDE's own shortcuts always win).
    fn refresh_extensions(&mut self) {
        let preset = self.preset_map();
        let mut taken: HashSet<String> = preset
            .iter()
            .filter(|(id, _)| !self.overrides.contains_key(*id))
            .chain(&self.overrides)
            .flat_map(|(_, keys)| keys)
            .map(|k| normalize_binding(k))
            .collect();
        let mut usable = Keymap::new();
        for (id, keys) in &self.extensions {
            if preset.contains_key(id) || self.overrides.contains_key(id) {
                continue;
            }
            let free: Vec<String> = keys.iter().filter(|k| taken.insert(normalize_binding(k))).cloned().collect();
            if !free.is_empty() {
                usable.insert(id.clone(), free);
            }
        }
        self.usable_extensions = usable;
    }

    pub fn bindings_for(&self, action_id: &str) -> &[String] {
        self.overrides
            .get(action_id)
            .or_else(|| self.preset_map().get(action_id))
            .or_else(|| self.usable_extensions.get(action_id))
            .map_or(&[], Vec::as_slice)
    }

    /// Every (binding, action id) pair of the effective keymap, in precedence order: a later
    /// pair wins over an earlier one with the same binding.
    pub fn effective_bindings(&self) -> Vec<(String, &str)> {
        let preset = self.preset_map();
        let layers = self
            .usable_extensions
            .iter()
            .filter(|(id, _)| !preset.contains_key(*id) && !self.overrides.contains_key(*id))
            .chain(preset.iter().filter(|(id, _)| !self.overrides.contains_key(*id)))
            .chain(&self.overrides);
        layers.flat_map(|(id, keys)| keys.iter().map(move |k| (normalize_binding(k), id.as_str()))).collect()
    }

    /// The first keyboard binding of an action: what menus and tooltips show.
    pub fn primary_key_binding(&self, action_id: &str) -> Option<&str> {
        self.bindings_for(action_id).iter().map(String::as_str).find(|b| !is_mouse_binding(b) && !is_page_swipe(b))
    }

    pub fn shortcut_label(&self, action_id: &str, mac: bool) -> Option<String> {
        self.primary_key_binding(action_id).map(|b| format_binding(b, mac))
    }
}

fn dedup(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items.filter(|k| seen.insert(k.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // src/platform/keymap.test.ts › normalizes modifier order and case
    #[test]
    fn normalizes_modifier_order_and_case() {
        assert_eq!(normalize_binding("Shift+Meta+o"), "Shift+Meta+O");
        assert_eq!(normalize_binding("meta+K  meta+c"), "Meta+K Meta+C");
        assert_eq!(normalize_binding("meta+alt+ctrl+f7"), "Ctrl+Alt+Meta+f7");
        assert_eq!(normalize_binding("shift+button5"), "Shift+Button5");
        assert_eq!(normalize_binding("swipeLEFT"), "SwipeLeft");
        assert_eq!(normalize_binding("Meta++"), "Meta++");
    }

    // src/platform/keymap.test.ts › formats macOS style
    #[test]
    fn formats_macos_style() {
        assert_eq!(format_binding("Meta+Shift+O", true), "⇧⌘O");
        assert_eq!(format_binding("Alt+Enter", true), "⌥↵");
        assert_eq!(format_binding("Shift Shift", true), "⇧⇧");
        assert_eq!(format_binding("Meta+K Meta+C", true), "⌘K ⌘C");
        assert_eq!(format_binding("Alt+Button4", true), "⌥Button4 Click");
        assert_eq!(format_binding("Ctrl+Button1", false), "Ctrl+Left Click");
        assert_eq!(format_binding("Ctrl+K Ctrl+I", false), "Ctrl+K, Ctrl+I");
    }

    // src/platform/keymap.test.ts › labels numeric-keypad operators
    #[test]
    fn labels_numeric_keypad_operators() {
        assert_eq!(format_binding("Meta+Shift+NumpadDivide", true), "⇧⌘NumPad /");
        assert_eq!(format_binding("Ctrl+NumpadSubtract", false), "Ctrl+NumPad -");
    }

    // src/platform/keymap.test.ts › double modifier with a key › parses, validates and formats the gesture
    #[test]
    fn parses_validates_and_formats_the_double_modifier_gesture() {
        assert_eq!(
            parse_double_modifier("alt alt+up"),
            Some(DoubleModifier { modifier: Modifier::Alt, key: Some("up".into()) })
        );
        assert_eq!(parse_double_modifier("Ctrl Ctrl"), Some(DoubleModifier { modifier: Modifier::Ctrl, key: None }));
        assert_eq!(parse_double_modifier("Alt Ctrl+Up"), None);
        assert!(is_valid_binding("Alt Alt+Up"));
        assert_eq!(normalize_binding("alt ALT+Down"), "Alt Alt+Down");
        assert_eq!(format_binding("Alt Alt+Up", true), "⌥⌥↑");
        assert_eq!(format_binding("Ctrl Ctrl+Up", false), "Ctrl, Ctrl+Up");
    }

    #[test]
    fn validates_bindings() {
        for valid in ["F8", "Meta+Shift+O", "Meta+K Meta+C", "Shift Shift", "Alt+Button4", "SwipeLeft", "Meta++", "+"] {
            assert!(is_valid_binding(valid), "{valid}");
        }
        for invalid in ["", "Meta+K Meta+C Meta+D", "Hyper+K", "Meta+SwipeLeft", "SwipeLeft F8", "Meta+"] {
            assert!(!is_valid_binding(invalid), "{invalid}");
        }
    }

    #[test]
    fn recognizes_mouse_bindings() {
        assert!(is_mouse_binding("Button4"));
        assert!(is_mouse_binding(" Shift+button5 "));
        assert!(!is_mouse_binding("Meta+B"));
        assert!(!is_mouse_binding("Button"));
        assert!(!is_mouse_binding("Meta+K Button4"));
    }

    #[test]
    fn every_preset_holds_valid_normalized_bindings() {
        for (name, _) in PRESET_TITLES {
            let map = preset(name).unwrap_or_else(|| panic!("preset {name} is built in"));
            assert!(map.len() > 100, "{name} has {} actions", map.len());
            for (action, keys) in map {
                for key in keys {
                    assert!(is_valid_binding(key), "{name}: {action} has the malformed binding {key:?}");
                }
            }
        }
        assert_eq!(preset("intellij-macos").unwrap()["navigate.searchEverywhere"], ["Shift Shift"]);
    }

    #[test]
    fn overrides_replace_preset_bindings_and_later_layers_win() {
        let mut state = KeymapState::new("intellij-macos");
        assert_eq!(state.bindings_for("navigate.searchEverywhere"), ["Shift Shift"]);
        state.set_overrides(Keymap::from([
            (
                "navigate.searchEverywhere".to_owned(),
                vec!["meta+p".to_owned(), "Meta+P".to_owned(), "nonsense+".to_owned()],
            ),
            ("t.unbound".to_owned(), vec![]),
        ]));
        assert_eq!(state.bindings_for("navigate.searchEverywhere"), ["Meta+P"]);
        assert_eq!(state.primary_key_binding("navigate.searchEverywhere"), Some("Meta+P"));
        assert_eq!(state.shortcut_label("navigate.searchEverywhere", true).as_deref(), Some("⌘P"));
        let effective = state.effective_bindings();
        assert!(!effective.iter().any(|(b, id)| b == "Shift Shift" && *id == "navigate.searchEverywhere"));
        assert_eq!(effective.last().map(|(b, id)| (b.as_str(), *id)), Some(("Meta+P", "navigate.searchEverywhere")));
    }

    #[test]
    fn applies_settings_with_validation_and_custom_keymaps() {
        let mut state = KeymapState::new("intellij-macos");
        assert!(state.apply_settings(
            Some(&json!("vscode-macos")),
            Some(&json!({ "a.b": ["meta+j", 3, "bad+"], "c": "x" })),
            None
        ));
        assert_eq!(state.preset_id(), "vscode-macos");
        assert_eq!(state.bindings_for("a.b"), ["Meta+J"]);
        assert!(!state.overrides().contains_key("c"));
        // Unchanged input is not a change.
        assert!(!state.apply_settings(Some(&json!("vscode-macos")), Some(&json!({ "a.b": ["Meta+J"] })), None));
        // An unknown preset falls back to the default.
        assert!(state.apply_settings(Some(&json!("nope")), None, None));
        assert_eq!(state.preset_id(), default_preset(is_mac_host()));
        // A user keymap is its parent preset plus its own overrides.
        let custom = json!({ "mine": { "parent": "eclipse-macos", "overrides": { "x.y": ["F9"] } } });
        assert!(state.apply_settings(Some(&json!("mine")), Some(&json!({ "ignored": ["F1"] })), Some(&custom)));
        assert_eq!(state.preset_id(), "eclipse-macos");
        assert_eq!(state.bindings_for("x.y"), ["F9"]);
        assert!(state.bindings_for("ignored").is_empty());
    }

    #[test]
    fn extension_bindings_yield_to_the_preset_and_the_user() {
        let mut state = KeymapState::new("intellij-macos");
        // Shift Shift is Search Everywhere's; Meta+Alt+Shift+F12 is free.
        state.register_extension_bindings("plugin.a", &["Shift Shift".to_owned(), "Meta+Alt+Shift+F12".to_owned()]);
        assert_eq!(state.bindings_for("plugin.a"), ["Meta+Alt+Shift+F12"]);
        // The first extension to claim a keystroke keeps it.
        state.register_extension_bindings("plugin.b", &["meta+alt+shift+F12".to_owned()]);
        assert!(state.bindings_for("plugin.b").is_empty());
        // The user's binding takes the keystroke away from the extension.
        state.set_overrides(Keymap::from([("user.action".to_owned(), vec!["Meta+Alt+Shift+F12".to_owned()])]));
        assert!(state.bindings_for("plugin.a").is_empty());
        state.unregister_extension_bindings("plugin.a");
        assert!(state.effective_bindings().iter().all(|(_, id)| *id != "plugin.a"));
    }

    #[test]
    fn host_spelling() {
        assert_eq!(host_binding("Meta+C", true), "Meta+C");
        assert_eq!(host_binding("Meta+Shift+C", false), "Ctrl+Shift+C");
        assert_eq!(mac_stroke("Ctrl+Shift+C", false), "Shift+Meta+C");
        assert_eq!(mac_stroke("Ctrl+C", true), "Ctrl+C");
    }
}
