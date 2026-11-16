//! Settings › Editor › Color Scheme (the scheme and its attribute edits) and Editor › Font ›
//! Typography Settings (the main and the bold weight) applied to the active theme. Every time the
//! theme is cascaded afresh the scheme's tokens are set over it as root overrides; the overrides of
//! one language's attributes are kept apart for the editors of that language. A change of those
//! settings cascades the theme again.

use super::ActiveTheme;
use super::color_scheme::{ColorOverrides, SCHEME_LANGUAGES, SchemeSource, custom_schemes, scheme_style};
use super::tokens::TOKENS_CSS;
use crate::backend::Backend;
use crate::platform::settings;
use gpui_kit::{App, Global};
use ide_core::IdeEvent;
use std::sync::LazyLock;

/// The resolved tokens of the built-in schemes, read once from the token stylesheet.
static SOURCE: LazyLock<SchemeSource> = LazyLock::new(|| SchemeSource::new(TOKENS_CSS));

/// The settings whose change applies the scheme again.
const SCHEME_KEYS: [&str; 5] = [
    "editor.colorScheme",
    "editor.colorOverrides",
    "editor.customSchemes",
    "editor.fontWeight",
    "editor.boldFontWeight",
];

/// The token overrides of the scheme for the editors of one language, by the backend's language id:
/// `(token with its leading --, value)` in order, a later entry for a token winning.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct SchemeLanguageTokens(pub Vec<(&'static str, Vec<(String, String)>)>);

impl Global for SchemeLanguageTokens {}

impl SchemeLanguageTokens {
    /// The value the scheme gives `token` (without `--`) in the editors of `language`, if it overrides it.
    pub fn get(&self, language: &str, token: &str) -> Option<&str> {
        let (_, declarations) = self.0.iter().find(|(id, _)| *id == language)?;
        declarations
            .iter()
            .rev()
            .find(|(name, _)| name.strip_prefix("--") == Some(token))
            .map(|(_, value)| value.as_str())
    }
}

/// A CSS weight from 100 to 900, rounded to the nearest 50. An unset weight never gets here: the
/// settings fill it with its default.
pub fn font_weight_of(weight: u16) -> u16 {
    ((f32::from(weight.min(900)) / 50.0).round() as u16 * 50).clamp(100, 900)
}

/// `--fw-code` and `--fw-code-bold`: the weights code text and its bold runs are drawn with.
pub fn font_weight_vars(main: u16, bold: u16) -> [(&'static str, String); 2] {
    [("--fw-code", font_weight_of(main).to_string()), ("--fw-code-bold", font_weight_of(bold).to_string())]
}

/// Whether a change of the setting `key` changes the scheme or the weights.
fn concerns(key: &str) -> bool {
    key.is_empty()
        || key == "editor"
        || SCHEME_KEYS
            .iter()
            .any(|scheme| key == *scheme || key.strip_prefix(scheme).is_some_and(|r| r.starts_with('.')))
}

/// Applies the scheme after every cascade of the theme, and cascades the theme again when the scheme's
/// settings change.
pub fn init(cx: &mut App) {
    super::appearance::after_apply(apply, cx);
    if cx.has_global::<Backend>() {
        cx.global_mut::<Backend>().listen(|event, cx| {
            if let IdeEvent::SettingsChanged { key } = &event.event
                && concerns(key)
            {
                super::appearance::apply(cx);
            }
        });
    }
    apply(cx);
}

/// Sets the chosen scheme's tokens, its edits and the typography weights on the active theme, and
/// keeps the per-language edits for the editors.
pub fn apply(cx: &mut App) {
    let editor = settings::global(cx).editor;
    let customs = serde_json::to_value(&editor.custom_schemes).map(|raw| custom_schemes(&raw)).unwrap_or_default();
    let overrides: ColorOverrides = match serde_json::to_value(&editor.color_overrides).and_then(serde_json::from_value)
    {
        Ok(overrides) => overrides,
        Err(error) => {
            tracing::warn!("the color scheme edits cannot be read: {error}");
            ColorOverrides::new()
        }
    };
    let Some(active) = cx.try_global::<ActiveTheme>() else { return };
    let style =
        scheme_style(&SOURCE, Some(&editor.color_scheme), Some(&overrides), Some(active.theme.attributes()), &customs);
    let weights = font_weight_vars(editor.font_weight, editor.bold_font_weight);
    let theme = &mut cx.global_mut::<ActiveTheme>().theme;
    for (name, value) in &style.root {
        theme.set(name, value);
    }
    for (name, value) in &weights {
        theme.set(name, value);
    }
    let languages = style.languages.into_iter().filter_map(|(name, declarations)| {
        let (id, _) = SCHEME_LANGUAGES.iter().find(|(_, language)| *language == name)?;
        Some((*id, declarations))
    });
    cx.set_global(SchemeLanguageTokens(languages.collect()));
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_typography_settings_weights() {
        let defaults = ide_core::settings::EditorSettings::default();
        assert_eq!(
            font_weight_vars(defaults.font_weight, defaults.bold_font_weight),
            [("--fw-code", "400".to_owned()), ("--fw-code-bold", "700".to_owned())]
        );
        assert_eq!((font_weight_of(349), font_weight_of(1200)), (350, 900));
        assert_eq!((font_weight_of(0), font_weight_of(20)), (100, 100));
        assert_eq!(font_weight_vars(500, defaults.bold_font_weight)[1].1, "700");
        assert_eq!(font_weight_vars(450, 650), [("--fw-code", "450".to_owned()), ("--fw-code-bold", "650".to_owned())]);
    }

    #[test]
    fn a_language_override_is_found_by_the_backends_id_and_the_latest_wins() {
        let tokens = SchemeLanguageTokens(vec![(
            "java",
            vec![
                ("--syn-keyword".to_owned(), "#111111".to_owned()),
                ("--syn-keyword".to_owned(), "#222222".to_owned()),
            ],
        )]);
        assert_eq!(tokens.get("java", "syn-keyword"), Some("#222222"));
        assert_eq!(tokens.get("go", "syn-keyword"), None);
        assert_eq!(tokens.get("java", "syn-type"), None);
    }

    #[test]
    fn the_scheme_settings_apply_the_scheme_again() {
        assert!(concerns("editor.colorOverrides"));
        assert!(concerns("editor.colorOverrides.light"));
        assert!(concerns("editor.boldFontWeight"));
        assert!(concerns(""));
        assert!(!concerns("editor.colorSchemeX"));
        assert!(!concerns("editor.fontSize"));
    }
}
