//! Font families: the family lists the editor, terminal and interface fonts resolve to, and the
//! catalogue of families the font pickers suggest.
//!
//! "JetBrains Mono" is the bundled default, registered as "JetBrains Mono Variable" as well; the
//! bundled variable face stays in every list so the family renders without a system install, but
//! the installed family comes first: its hinted static outlines render like IntelliJ IDEA's.
//!
//! Measuring text and asking the native font system are the caller's: the functions take a
//! measuring closure or the answer, and return what to load or ask next.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::themes::locale_compare;
use super::tokens::Theme;

pub const BUNDLED_MONO_FAMILY: &str = "JetBrains Mono";
pub const BUNDLED_UI_FAMILY: &str = "Inter";

/// The `--font-mono` stack when the stylesheet defines none.
pub const MONO_STACK_FALLBACK: &str = r#""JetBrains Mono", "JetBrains Mono Variable", ui-monospace, Menlo, monospace"#;

const MONO_FACES: &[&str] = &[r#""JetBrains Mono""#, r#""JetBrains Mono Variable""#];
const MONO_VARIABLE_FACES: &[&str] = &[r#""JetBrains Mono Variable""#];
const UI_VARIABLE_FACES: &[&str] = &[r#""Inter Variable""#];
const UI_FACES: &[&str] = &[r#""Inter""#, r#""Inter Variable""#];

/// The faces a bundled family is registered under, whatever the case of `name`.
fn bundled_faces(name: &str) -> Option<&'static [&'static str]> {
    match name.to_lowercase().as_str() {
        "jetbrains mono" => Some(MONO_FACES),
        "jetbrains mono variable" => Some(MONO_VARIABLE_FACES),
        "inter variable" => Some(UI_VARIABLE_FACES),
        "inter" => Some(UI_FACES),
        _ => None,
    }
}

/// Quote one CSS family safely (including quotes, backslashes and control characters).
pub fn quote_font_family(family: &str) -> String {
    let mut quoted = String::from('"');
    for c in family.trim().chars() {
        if matches!(c, '"' | '\\' | '\0'..='\u{1f}' | '\u{7f}') {
            // Writing to a `String` cannot fail.
            let _ = write!(quoted, "\\{:x} ", u32::from(c));
        } else {
            quoted.push(c);
        }
    }
    quoted.push('"');
    quoted
}

/// The family as named, without the CSS quotes previously saved names may be wrapped in.
fn family_name(family: &str) -> &str {
    let name = family.trim();
    let unquoted = name.strip_prefix('"').and_then(|rest| rest.strip_suffix('"'));
    unquoted.filter(|inner| !inner.contains(['\n', '\r', '\u{2028}', '\u{2029}'])).unwrap_or(name)
}

/// A family the host cannot resolve by name: its faces are loaded and registered under `alias`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFont {
    pub family: String,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyFaces {
    /// The quoted faces to list for the family, in order.
    pub faces: Vec<String>,
    /// The font to read (`user_font_data`) and register under its alias: set the first time a
    /// family needs it.
    pub load: Option<UserFont>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontList {
    /// The CSS `font-family` list.
    pub family: String,
    /// The fonts to read and register before the list renders as meant.
    pub loads: Vec<UserFont>,
}

/// The families loaded under an alias because the host cannot resolve them by name (WKWebView
/// on macOS does not see user-installed fonts). One resolver for primary, fallback and UI
/// families, so each family is loaded once and keeps its alias.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserFonts {
    aliasing: bool,
    fonts: Vec<UserFont>,
}

impl UserFonts {
    /// `aliasing`: whether unresolvable families get an alias (the macOS desktop webview);
    /// without it every family is listed by its name alone.
    pub fn new(aliasing: bool) -> Self {
        Self { aliasing, fonts: Vec::new() }
    }

    /// The faces of `family`, matching the bundled registrations. `available` tells whether
    /// the host resolves a family by name ([`font_available`]); it is asked once per family.
    pub fn family_faces(&mut self, family: &str, available: &dyn Fn(&str) -> bool) -> FamilyFaces {
        let name = family_name(family);
        if let Some(bundled) = bundled_faces(name) {
            return FamilyFaces { faces: bundled.iter().map(|face| (*face).to_owned()).collect(), load: None };
        }
        let by_name = || FamilyFaces { faces: vec![quote_font_family(name)], load: None };
        if name.is_empty() || !self.aliasing {
            return by_name();
        }
        let key = name.to_lowercase();
        let (font, load) = match self.fonts.iter().find(|font| font.family.to_lowercase() == key) {
            Some(font) => (font.clone(), None),
            None if available(name) => return by_name(),
            None => {
                let alias = format!("OpenIDEA User Font {}", self.fonts.len());
                let font = UserFont { family: name.to_owned(), alias };
                self.fonts.push(font.clone());
                (font.clone(), Some(font))
            }
        };
        // The local name stays listed: the alias only renders once its faces are loaded.
        FamilyFaces { faces: vec![quote_font_family(&font.alias), quote_font_family(name)], load }
    }

    fn font_list(&mut self, family: &str, fallback: &str, tail: &[&str], available: &dyn Fn(&str) -> bool) -> FontList {
        let fallback = family_name(fallback);
        let (mut faces, mut loads) = (Vec::new(), Vec::new());
        for family in std::iter::once(family).chain((!fallback.is_empty()).then_some(fallback)) {
            let resolved = self.family_faces(family, available);
            faces.extend(resolved.faces);
            loads.extend(resolved.load);
        }
        faces.extend(tail.iter().map(|face| (*face).to_owned()));
        FontList { family: faces.join(", "), loads }
    }

    /// `family`, then `fallback`, then `base` (the `--font-mono` stack, [`mono_stack`]). The
    /// bundled family maps to its bundled face, so it renders without a system install.
    pub fn mono_font_family(
        &mut self,
        family: Option<&str>,
        fallback: &str,
        base: &str,
        available: &dyn Fn(&str) -> bool,
    ) -> FontList {
        let name = family_name(family.unwrap_or(""));
        let head = if name.is_empty() { BUNDLED_MONO_FAMILY } else { name };
        self.font_list(head, fallback, &[base], available)
    }

    /// Bundled aliases also apply when selected for the interface; `fallback` covers glyphs
    /// `family` lacks (empty = none).
    pub fn ui_font_family(&mut self, family: &str, fallback: &str, available: &dyn Fn(&str) -> bool) -> FontList {
        self.font_list(family, fallback, &["system-ui", "sans-serif"], available)
    }

    /// The user fonts the list `family` names: code that measures glyphs waits for their faces,
    /// so it measures the real font rather than the fallback.
    pub fn pending_in(&self, family: &str) -> Vec<&UserFont> {
        self.fonts.iter().filter(|font| family.contains(&quote_font_family(&font.alias))).collect()
    }
}

/// The faces of `family` where every family resolves by name.
pub fn family_faces(family: &str) -> Vec<String> {
    UserFonts::default().family_faces(family, &|_| true).faces
}

/// [`UserFonts::mono_font_family`] where every family resolves by name.
pub fn mono_font_family(family: Option<&str>, fallback: &str, base: &str) -> String {
    UserFonts::default().mono_font_family(family, fallback, base, &|_| true).family
}

/// [`UserFonts::ui_font_family`] where every family resolves by name.
pub fn ui_font_family(family: &str, fallback: &str) -> String {
    UserFonts::default().ui_font_family(family, fallback, &|_| true).family
}

/// The `--font-mono` stack of `theme`, resolved.
pub fn mono_stack(theme: &Theme) -> String {
    let stack = theme.resolved("font-mono").map(|stack| stack.trim().to_owned());
    stack.filter(|stack| !stack.is_empty()).unwrap_or_else(|| MONO_STACK_FALLBACK.to_owned())
}

const GENERIC_FAMILIES: [&str; 3] = ["monospace", "serif", "sans-serif"];
const LATIN_SAMPLE: &str = "mmmmmmmmmmlli1WQ@";

/// Whether the host resolves `name` to a font of its own: some sample text is as wide in it as
/// in no generic family. `measure(font, text)` is the width of `text` in the CSS font shorthand
/// `font`, `None` when text cannot be measured; then the family counts as available.
pub fn font_available(name: &str, mut measure: impl FnMut(&str, &str) -> Option<f64>) -> bool {
    let quoted = quote_font_family(name);
    // Symbol-only Nerd fonts and CJK faces may have no Latin glyphs.
    for sample in [LATIN_SAMPLE, "\u{f418}\u{e725}\u{f0001}", "汉字かな한글"] {
        for base in GENERIC_FAMILIES {
            let Some(baseline) = measure(&format!("32px {base}"), sample) else { return true };
            let Some(width) = measure(&format!("32px {quoted}, {base}"), sample) else { return true };
            if width.is_finite() && baseline.is_finite() && width != baseline {
                return true;
            }
        }
    }
    false
}

/// One font family of a picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFamily {
    pub name: String,
    pub monospaced: bool,
}

/// The families probed when the native catalogue is unavailable: a bounded recovery list, not
/// a whitelist of installed fonts.
pub const CANDIDATE_FONTS: [&str; 31] = [
    "JetBrains Mono",
    "SF Mono",
    "Menlo",
    "Monaco",
    "Fira Code",
    "Fira Mono",
    "Source Code Pro",
    "Cascadia Code",
    "Cascadia Mono",
    "IBM Plex Mono",
    "Hack",
    "Inconsolata",
    "Roboto Mono",
    "Ubuntu Mono",
    "DejaVu Sans Mono",
    "Consolas",
    "Courier New",
    "PT Mono",
    "Andale Mono",
    "Victor Mono",
    "Iosevka",
    "Sarasa Mono SC",
    "Noto Sans Mono",
    "Noto Sans Mono CJK SC",
    "PingFang SC",
    "Hiragino Sans GB",
    "Microsoft YaHei",
    "Helvetica Neue",
    "Arial",
    "Inter",
    "Georgia",
];

/// The candidate families that measure differently from the generic ones, so are probably
/// installed. Width comparisons can miss installed families and are only hints; they say
/// nothing about coverage. `measure` is as for [`font_available`]; when it cannot measure,
/// nothing is inferred: unmeasurable candidates are never marked as installed.
pub fn detect_candidate_fonts(mut measure: impl FnMut(&str, &str) -> Option<f64>) -> Vec<FontFamily> {
    let mut width = |font: &str, text: &str| measure(&format!("32px {font}"), text);
    let mut out = Vec::new();
    for name in CANDIDATE_FONTS {
        let quoted = quote_font_family(name);
        let mut present = false;
        for base in GENERIC_FAMILIES {
            let Some(measured) = width(&format!("{quoted}, {base}"), LATIN_SAMPLE) else { return Vec::new() };
            let Some(baseline) = width(base, LATIN_SAMPLE) else { return Vec::new() };
            if measured.is_finite() && baseline.is_finite() && measured != baseline {
                present = true;
                break;
            }
        }
        if !present {
            continue;
        }
        let mono = format!("{quoted}, monospace");
        let (Some(narrow), Some(wide)) = (width(&mono, "iiiiiiiiii"), width(&mono, "WWWWWWWWWW")) else {
            return Vec::new();
        };
        let monospaced = narrow.is_finite() && wide.is_finite() && (narrow - wide).abs() < 0.5;
        out.push(FontFamily { name: name.to_owned(), monospaced });
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontStatus {
    Idle,
    Loading,
    Ready,
    Empty,
    Error,
    Browser,
}

/// Where the suggested families come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontSource {
    /// Only the bundled families.
    Bundled,
    /// The probed candidates: necessarily incomplete.
    Limited,
    /// The native font system's catalogue.
    Native,
}

pub fn bundled_fonts() -> [FontFamily; 2] {
    [
        FontFamily { name: BUNDLED_MONO_FAMILY.to_owned(), monospaced: true },
        FontFamily { name: BUNDLED_UI_FAMILY.to_owned(), monospaced: false },
    ]
}

/// `fonts` and the bundled families, one entry per name whatever its case (the last one
/// wins), sorted by name.
pub fn merge_font_families(fonts: &[FontFamily]) -> Vec<FontFamily> {
    let mut unique: Vec<FontFamily> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for font in fonts.iter().cloned().chain(bundled_fonts()) {
        if font.name.trim().is_empty() {
            continue;
        }
        match index.entry(font.name.to_lowercase()) {
            Entry::Occupied(at) => unique[*at.get()] = font,
            Entry::Vacant(slot) => {
                slot.insert(unique.len());
                unique.push(font);
            }
        }
    }
    unique.sort_by(|a, b| locale_compare(&a.name, &b.name));
    unique
}

/// What a call to [`SystemFonts::load`] leaves the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontLoad {
    /// Nothing: the families are known, or were just taken from the candidates.
    Done,
    /// Nothing: a query is in flight and its answer serves this call too.
    Pending,
    /// Ask the native font system for its families (re-reading them when `refresh`), then
    /// call [`SystemFonts::finish_load`].
    Query { refresh: bool },
}

/// The families the font pickers suggest. Shared suggestions only: editor, UI and terminal
/// preferences remain independent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFonts {
    pub fonts: Vec<FontFamily>,
    pub status: FontStatus,
    pub source: FontSource,
    pending: bool,
}

impl Default for SystemFonts {
    fn default() -> Self {
        Self { fonts: bundled_fonts().to_vec(), status: FontStatus::Idle, source: FontSource::Bundled, pending: false }
    }
}

impl SystemFonts {
    /// Starts loading the families unless they are loaded already (`refresh` loads them
    /// again). `native`: whether the native font system can be asked; otherwise the families
    /// `detect` finds ([`detect_candidate_fonts`]) are suggested.
    pub fn load(&mut self, refresh: bool, native: bool, detect: impl FnOnce() -> Vec<FontFamily>) -> FontLoad {
        if self.pending {
            return FontLoad::Pending;
        }
        if !refresh && self.status != FontStatus::Idle {
            return FontLoad::Done;
        }
        if !native {
            self.fall_back(FontStatus::Browser, detect);
            return FontLoad::Done;
        }
        self.status = FontStatus::Loading;
        self.pending = true;
        FontLoad::Query { refresh }
    }

    /// Takes the answer of the query [`SystemFonts::load`] asked for: the native families, or
    /// `None` when asking failed.
    pub fn finish_load(&mut self, fonts: Option<Vec<FontFamily>>, detect: impl FnOnce() -> Vec<FontFamily>) {
        self.pending = false;
        match fonts {
            Some(fonts) => {
                self.status = if fonts.is_empty() { FontStatus::Empty } else { FontStatus::Ready };
                self.fonts = merge_font_families(&fonts);
                self.source = FontSource::Native;
            }
            None => self.fall_back(FontStatus::Error, detect),
        }
    }

    fn fall_back(&mut self, status: FontStatus, detect: impl FnOnce() -> Vec<FontFamily>) {
        self.status = status;
        // Even an empty successful native catalog is authoritative. A failed refresh must never
        // replace it with the necessarily incomplete candidate list.
        if self.source != FontSource::Native {
            self.fonts = merge_font_families(&detect());
            self.source = FontSource::Limited;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const BASE: &str = "var(--font-mono)";
    const NERD: &str = "JetBrainsMono Nerd Font Mono";

    fn font(name: &str, monospaced: bool) -> FontFamily {
        FontFamily { name: name.to_owned(), monospaced }
    }

    fn user_font(family: &str, index: usize) -> UserFont {
        UserFont { family: family.to_owned(), alias: format!("OpenIDEA User Font {index}") }
    }

    fn names(fonts: &[FontFamily]) -> Vec<&str> {
        fonts.iter().map(|font| font.name.as_str()).collect()
    }

    /// Text widths as a canvas reports them: a named family is wider than the generic ones for
    /// the sample texts it `covers`.
    fn canvas(covers: impl Fn(&str) -> bool) -> impl Fn(&str, &str) -> Option<f64> {
        move |font: &str, text: &str| Some(if covers(text) && font.contains('"') { 200.0 } else { 100.0 })
    }

    fn no_fonts() -> Vec<FontFamily> {
        Vec::new()
    }

    // src/platform/fonts.test.ts › monoFontFamily › %j + fallback %j
    #[test]
    fn mono_font_family_lists_the_family_its_fallback_and_the_base() {
        let bundled = format!(r#""JetBrains Mono", "JetBrains Mono Variable", {BASE}"#);
        let cases = [
            // The default family names the bundled face, so it renders without a system install; the installed
            // family comes first, so its hinted outlines render like IDEA's.
            (Some("JetBrains Mono"), "", bundled.clone()),
            (Some(""), "", bundled.clone()),
            (None, "", bundled),
            (
                Some("JetBrains Mono"),
                "PingFang SC",
                format!(r#""JetBrains Mono", "JetBrains Mono Variable", "PingFang SC", {BASE}"#),
            ),
            (Some("Inter"), "", format!(r#""Inter", "Inter Variable", {BASE}"#)),
            (Some("Fira Code"), "Inter", format!(r#""Fira Code", "Inter", "Inter Variable", {BASE}"#)),
            (
                Some("Inter"),
                "JetBrains Mono",
                format!(r#""Inter", "Inter Variable", "JetBrains Mono", "JetBrains Mono Variable", {BASE}"#),
            ),
            (
                Some("inter"),
                "jetbrains mono",
                format!(r#""Inter", "Inter Variable", "JetBrains Mono", "JetBrains Mono Variable", {BASE}"#),
            ),
            (Some("Fira Code"), "", format!(r#""Fira Code", {BASE}"#)),
            (Some(r#""Cascadia Code""#), " Microsoft YaHei ", format!(r#""Cascadia Code", "Microsoft YaHei", {BASE}"#)),
        ];
        for (family, fallback, expected) in cases {
            assert_eq!(mono_font_family(family, fallback, BASE), expected, "{family:?} + {fallback:?}");
        }
    }

    // src/platform/fonts.test.ts › monoFontFamily › always ends with the bundled stack, so a missing family never
    // reaches the system default
    #[test]
    fn mono_font_family_always_ends_with_the_bundled_stack() {
        assert_eq!(
            mono_font_family(Some("Not Installed"), "", r#""JetBrains Mono Variable", monospace"#),
            r#""Not Installed", "JetBrains Mono Variable", monospace"#
        );
    }

    // src/platform/fonts.test.ts › escapes CSS special characters without turning a family into a list
    #[test]
    fn escapes_css_special_characters_without_turning_a_family_into_a_list() {
        assert_eq!(quote_font_family(r#"A"B\C,D"#), r#""A\22 B\5c C,D""#);
        assert_eq!(quote_font_family("A\nB"), r#""A\a B""#);
        assert!(ui_font_family("JetBrains Mono", "").contains(r#""JetBrains Mono Variable""#));
        assert!(ui_font_family("Inter", "").contains(r#""Inter Variable""#));
    }

    // src/platform/fonts.test.ts › uiFontFamily › %j + %j
    #[test]
    fn ui_font_family_lists_the_family_its_fallback_and_the_system_fonts() {
        let cases = [
            ("Inter", "", r#""Inter", "Inter Variable", system-ui, sans-serif"#),
            ("JetBrains Mono", "", r#""JetBrains Mono", "JetBrains Mono Variable", system-ui, sans-serif"#),
            (r#" "PingFang SC" "#, "", r#""PingFang SC", system-ui, sans-serif"#),
            ("Inter", "思源黑体", r#""Inter", "Inter Variable", "思源黑体", system-ui, sans-serif"#),
            ("Inter", " ", r#""Inter", "Inter Variable", system-ui, sans-serif"#),
        ];
        for (family, fallback, expected) in cases {
            assert_eq!(ui_font_family(family, fallback), expected, "{family:?} + {fallback:?}");
        }
    }

    // src/platform/fonts.test.ts › macOS user font resolver › loads an unavailable family once across primary,
    // fallback and UI, including regular/bold/italic
    #[test]
    fn loads_an_unavailable_family_once_across_primary_fallback_and_ui() {
        let mut fonts = UserFonts::new(true);
        let unavailable = |_: &str| false;
        let mono = fonts.mono_font_family(Some(NERD), "", BASE, &unavailable);
        assert!(mono.family.contains(r#""OpenIDEA User Font 0", "JetBrainsMono Nerd Font Mono""#));
        let ui = fonts.ui_font_family("jetbrainsmono nerd font mono", "", &unavailable);
        let fallback = fonts.mono_font_family(Some("Inter"), NERD, BASE, &unavailable);
        let loads = [&mono.loads[..], &ui.loads[..], &fallback.loads[..]].concat();
        assert_eq!(loads, [user_font(NERD, 0)]);
        assert_eq!(fonts.pending_in(&mono.family), [&user_font(NERD, 0)]);
    }

    // src/platform/fonts.test.ts › macOS user font resolver › uses available local/system families without reading
    // font data
    #[test]
    fn uses_available_local_and_system_families_without_reading_font_data() {
        let mut fonts = UserFonts::new(true);
        let available = |name: &str| font_available(name, canvas(|_| true));
        let ui = fonts.ui_font_family("Menlo", "", &available);
        assert_eq!(ui.family, r#""Menlo", system-ui, sans-serif"#);
        let mono = fonts.mono_font_family(Some("Maple Mono NF CN"), "", BASE, &available);
        assert_eq!([ui.loads, mono.loads].concat(), Vec::<UserFont>::new());
    }

    // src/platform/fonts.test.ts › macOS user font resolver › loads a UI fallback through the shared macOS user
    // font resolver
    #[test]
    fn loads_a_ui_fallback_through_the_shared_user_font_resolver() {
        let mut fonts = UserFonts::new(true);
        let unavailable = |_: &str| false;
        let ui = fonts.ui_font_family("Inter", "Maple Mono NF CN", &unavailable);
        assert_eq!(
            ui.family,
            r#""Inter", "Inter Variable", "OpenIDEA User Font 0", "Maple Mono NF CN", system-ui, sans-serif"#
        );
        let mono = fonts.mono_font_family(Some("Inter"), "Maple Mono NF CN", BASE, &unavailable);
        assert!(mono.family.contains(r#""OpenIDEA User Font 0""#));
        assert_eq!([&ui.loads[..], &mono.loads[..]].concat(), [user_font("Maple Mono NF CN", 0)]);
        assert_eq!(fonts.pending_in(&ui.family), [&user_font("Maple Mono NF CN", 0)]);
    }

    // src/platform/fonts.test.ts › macOS user font resolver › detects user fonts that WKWebView exposes only to
    // connected canvases without loading bytes
    #[test]
    fn detects_measurable_user_fonts_without_loading_bytes() {
        let mut fonts = UserFonts::new(true);
        let available = |name: &str| font_available(name, canvas(|_| true));
        assert!(available("Maple Mono NF CN"));
        let maple = fonts.family_faces("Maple Mono NF CN", &available);
        assert_eq!(maple, FamilyFaces { faces: vec![r#""Maple Mono NF CN""#.to_owned()], load: None });
        let nerd = fonts.family_faces(NERD, &available);
        assert_eq!(nerd, FamilyFaces { faces: vec![r#""JetBrainsMono Nerd Font Mono""#.to_owned()], load: None });
    }

    // src/platform/fonts.test.ts › macOS user font resolver › removes the probe when a font is unavailable or
    // canvas measurement fails
    #[test]
    fn reports_unavailable_fonts_and_assumes_availability_when_measurement_fails() {
        assert!(!font_available("Unavailable", canvas(|_| false)));
        assert!(font_available("Maple Mono NF CN", |_, _| None));
    }

    // src/platform/fonts.test.ts › macOS user font resolver › does not read a locally available symbol-only Nerd
    // fallback
    #[test]
    fn does_not_read_a_locally_available_symbol_only_nerd_fallback() {
        let mut fonts = UserFonts::new(true);
        let available = |name: &str| font_available(name, canvas(|text| text.contains('\u{f418}')));
        let symbols = fonts.family_faces("Symbols Nerd Font Mono", &available);
        assert_eq!(symbols, FamilyFaces { faces: vec![r#""Symbols Nerd Font Mono""#.to_owned()], load: None });
    }

    // src/platform/fonts.test.ts › macOS user font resolver › never reads bundled webfonts even before their CSS
    // faces load
    #[test]
    fn never_reads_bundled_webfonts_even_before_their_faces_load() {
        let mut fonts = UserFonts::new(true);
        let unavailable = |_: &str| false;
        let lists = [
            fonts.mono_font_family(Some("Inter"), "", BASE, &unavailable),
            fonts.mono_font_family(Some("JetBrains Mono"), "", BASE, &unavailable),
            fonts.ui_font_family("Inter Variable", "", &unavailable),
            fonts.mono_font_family(Some("JetBrains Mono Variable"), "", BASE, &unavailable),
        ];
        assert!(lists.iter().all(|list| list.loads.is_empty()));
    }

    // src/platform/fonts.test.ts › macOS user font resolver › does not read bundled faces or alter other platforms
    #[test]
    fn does_not_read_bundled_faces_or_alter_other_platforms() {
        let mut fonts = UserFonts::new(false);
        let unavailable = |_: &str| false;
        let inter = fonts.mono_font_family(Some("Inter"), "", BASE, &unavailable);
        let mono = fonts.mono_font_family(Some("JetBrains Mono"), "", BASE, &unavailable);
        let user = fonts.ui_font_family("User Font", "", &unavailable);
        assert_eq!(user.family, r#""User Font", system-ui, sans-serif"#);
        assert_eq!([inter.loads, mono.loads, user.loads].concat(), Vec::<UserFont>::new());
    }

    // src/platform/candidateFonts.test.ts › probes the legacy list and classifies measured mono and proportional
    // families
    #[test]
    fn probes_the_legacy_list_and_classifies_measured_mono_and_proportional_families() {
        let detected = detect_candidate_fonts(|font, text| {
            Some(if font.contains(r#""Menlo""#) {
                120.0
            } else if font.contains(r#""Georgia""#) {
                if text.starts_with('i') { 60.0 } else { 180.0 }
            } else {
                100.0
            })
        });
        assert_eq!(detected, [font("Menlo", true), font("Georgia", false)]);
    }

    // src/platform/candidateFonts.test.ts › returns no inferred families when Canvas is absent
    #[test]
    fn returns_no_inferred_families_when_nothing_can_be_measured() {
        assert_eq!(detect_candidate_fonts(|_, _| None), no_fonts());
    }

    // src/platform/candidateFonts.test.ts › detects user-installed candidates with a connected probe and removes
    // it afterwards
    #[test]
    fn detects_user_installed_candidates() {
        let width = |font: &str, _: &str| Some(if font.contains(r#""Fira Code""#) { 120.0 } else { 100.0 });
        let detected = detect_candidate_fonts(width);
        assert_eq!(detected, [font("Fira Code", true)]);
    }

    // src/platform/candidateFonts.test.ts › contains failures from Canvas and never claims candidates are installed
    #[test]
    fn contains_measurement_failures_and_never_claims_candidates_are_installed() {
        // Menlo measures as installed before measuring stops working.
        let detected = detect_candidate_fonts(|font, _| {
            if font.contains(r#""Monaco""#) { None } else { Some(if font.contains('"') { 120.0 } else { 100.0 }) }
        });
        assert_eq!(detected, no_fonts());
    }

    // src/platform/candidateFonts.test.ts › does not treat invalid measurements as available fonts
    #[test]
    fn does_not_treat_invalid_measurements_as_available_fonts() {
        assert_eq!(detect_candidate_fonts(|_, _| Some(f64::NAN)), no_fonts());
    }

    // src/platform/systemFonts.test.ts › local font discovery › shares an in-flight request, caches families
    // outside the old list and explicitly refreshes
    #[test]
    fn shares_an_in_flight_request_caches_families_and_explicitly_refreshes() {
        let detected = Cell::new(0u32);
        let detect = || {
            detected.set(detected.get() + 1);
            vec![font("Menlo", true)]
        };
        let mut fonts = SystemFonts::default();
        assert_eq!(fonts.load(false, true, detect), FontLoad::Query { refresh: false });
        assert_eq!(fonts.status, FontStatus::Loading);
        assert_eq!(fonts.load(false, true, detect), FontLoad::Pending);
        fonts.finish_load(Some(vec![font("Maple Mono NF", true), font("maple mono nf", true)]), detect);
        assert_eq!(names(&fonts.fonts), ["Inter", "JetBrains Mono", "maple mono nf"]);
        assert_eq!(fonts.load(false, true, detect), FontLoad::Done);
        assert_eq!(fonts.load(true, true, detect), FontLoad::Query { refresh: true });
        fonts.finish_load(Some(Vec::new()), detect);
        assert_eq!((fonts.status, fonts.source), (FontStatus::Empty, FontSource::Native));
        assert_eq!(detected.get(), 0);
        let [mono, ui] = bundled_fonts();
        assert_eq!(fonts.fonts, [ui, mono]);
    }

    // src/platform/systemFonts.test.ts › local font discovery › reports errors without losing suggestions, then
    // retries
    #[test]
    fn reports_errors_without_losing_suggestions_then_retries() {
        let detect = || vec![font("Menlo", true)];
        let mut fonts = SystemFonts::default();
        assert_eq!(fonts.load(false, true, detect), FontLoad::Query { refresh: false });
        fonts.finish_load(None, detect);
        assert_eq!((fonts.status, fonts.source), (FontStatus::Error, FontSource::Limited));
        assert!(names(&fonts.fonts).contains(&"Menlo"));
        assert_eq!(fonts.load(true, true, detect), FontLoad::Query { refresh: true });
        fonts.finish_load(Some(vec![font("思源宋体", false)]), detect);
        assert_eq!((fonts.status, fonts.source), (FontStatus::Ready, FontSource::Native));
        assert!(!names(&fonts.fonts).contains(&"Menlo"));
        assert!(names(&fonts.fonts).contains(&"思源宋体"));
    }

    // src/platform/systemFonts.test.ts › local font discovery › does not invoke desktop APIs in browser development
    #[test]
    fn does_not_ask_the_native_font_system_where_there_is_none() {
        let detected = Cell::new(0u32);
        let detect = || {
            detected.set(detected.get() + 1);
            vec![font("Menlo", true)]
        };
        let mut fonts = SystemFonts::default();
        assert_eq!(fonts.load(false, false, detect), FontLoad::Done);
        assert_eq!(detected.get(), 1);
        assert_eq!(fonts.status, FontStatus::Browser);
    }

    // src/platform/systemFonts.test.ts › retains successful native catalogs, including empty ones, after refresh
    // failure: %j
    #[test]
    fn retains_successful_native_catalogs_including_empty_ones_after_refresh_failure() {
        for native in [Vec::new(), vec![font("Outside Legacy List", true)]] {
            let detected = Cell::new(0u32);
            let detect = || {
                detected.set(detected.get() + 1);
                vec![font("Menlo", true)]
            };
            let mut fonts = SystemFonts::default();
            assert_eq!(fonts.load(false, true, detect), FontLoad::Query { refresh: false });
            fonts.finish_load(Some(native), detect);
            let previous = fonts.fonts.clone();
            assert_eq!(fonts.load(true, true, detect), FontLoad::Query { refresh: true });
            fonts.finish_load(None, detect);
            assert_eq!((fonts.status, fonts.source), (FontStatus::Error, FontSource::Native));
            assert_eq!(fonts.fonts, previous);
            assert_eq!(detected.get(), 0);
        }
    }

    // src/platform/systemFonts.test.ts › keeps bundled/manual recovery usable when candidate probing also fails
    #[test]
    fn keeps_bundled_recovery_usable_when_candidate_probing_also_fails() {
        let mut fonts = SystemFonts::default();
        assert_eq!(fonts.load(false, true, no_fonts), FontLoad::Query { refresh: false });
        // A probe that fails finds nothing.
        fonts.finish_load(None, no_fonts);
        assert_eq!((fonts.status, fonts.source), (FontStatus::Error, FontSource::Limited));
        assert_eq!(names(&fonts.fonts), ["Inter", "JetBrains Mono"]);
    }
}
