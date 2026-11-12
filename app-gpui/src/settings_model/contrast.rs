//! WCAG contrast of hex colours and a minimal reader of the rules of `tokens.css`: what checks
//! that every theme defines its tokens and keeps text readable.

use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Red, green and blue, 0–255.
pub type Rgb = [u8; 3];

/// `#rgb` or `#rrggbb` as its channels.
pub fn parse_hex(hex: &str) -> Option<Rgb> {
    let digits = hex.trim().strip_prefix('#')?;
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |text: &str| u8::from_str_radix(text, 16).ok();
    match digits.len() {
        3 => {
            let double = |at: usize| channel(&digits[at..=at]).map(|value| value * 17);
            Some([double(0)?, double(1)?, double(2)?])
        }
        6 => Some([channel(&digits[0..2])?, channel(&digits[2..4])?, channel(&digits[4..6])?]),
        _ => None,
    }
}

/// WCAG relative luminance, 0 (black) to 1 (white).
pub fn relative_luminance([r, g, b]: Rgb) -> f64 {
    let linear = |channel: u8| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.03928 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG 2.x contrast ratio (1–21) of two hex colours; `None` when one is not a hex colour.
pub fn contrast_ratio(a: &str, b: &str) -> Option<f64> {
    let (la, lb) = (relative_luminance(parse_hex(a)?), relative_luminance(parse_hex(b)?));
    Some((la.max(lb) + 0.05) / (la.min(lb) + 0.05))
}

/// The custom properties declared by the rules of `css` whose selector list has `selector`,
/// later rules winning.
pub fn read_tokens(css: &str, selector: &str) -> BTreeMap<String, String> {
    static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("the pattern is valid"));
    static RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([^{}]+)\{([^{}]*)\}").expect("the pattern is valid"));
    let clean = COMMENT.replace_all(css, "");
    let mut out = BTreeMap::new();
    for rule in RULE.captures_iter(&clean) {
        if !rule[1].split(',').any(|listed| listed.trim() == selector) {
            continue;
        }
        for declaration in rule[2].split(';') {
            let Some((name, value)) = declaration.split_once(':') else { continue };
            let (name, value) = (name.trim(), value.trim());
            let is_token = name.strip_prefix("--").is_some_and(|rest| {
                !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            });
            if is_token && !value.is_empty() {
                out.insert(name.to_owned(), value.to_owned());
            }
        }
    }
    out
}

/// Whether a token value is a colour (as opposed to a metric or a font).
pub fn is_color_value(value: &str) -> bool {
    let value = value.trim();
    value == "transparent"
        || ["#", "rgb(", "rgba(", "hsl(", "hsla(", "color-mix("].iter().any(|start| value.starts_with(start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::themes::{BUILTIN_THEMES, BuiltinThemeId, ThemeKind};
    use crate::theme::tokens::TOKENS_CSS;
    use std::collections::BTreeSet;

    type Tokens = BTreeMap<String, String>;

    fn over(base: &Tokens, selector: &str) -> Tokens {
        let mut tokens = base.clone();
        tokens.extend(read_tokens(TOKENS_CSS, selector));
        tokens
    }

    fn dark() -> Tokens {
        read_tokens(TOKENS_CSS, ":root")
    }

    fn light() -> Tokens {
        over(&dark(), ":root[data-theme=\"light\"]")
    }

    fn high_contrast() -> Tokens {
        over(&dark(), ":root[data-theme=\"dark-hc\"]")
    }

    const TEXT: [&str; 6] = ["--fg-strong", "--fg-emphasis", "--fg", "--fg-secondary", "--fg-muted", "--fg-subtle"];
    const SURFACES: [&str; 3] = ["--bg-app", "--bg-elev", "--bg-popup"];
    const SYNTAX: [&str; 13] = [
        "--syn-keyword",
        "--syn-string",
        "--syn-type",
        "--syn-function",
        "--syn-comment",
        "--syn-annotation",
        "--syn-number",
        "--syn-punct",
        "--syn-variable",
        "--syn-property",
        "--syn-constant",
        "--syn-invalid",
        "--syn-inline-value",
    ];
    const STATUS: [&str; 6] = ["--danger", "--warning", "--success", "--info", "--accent", "--link"];
    const VCS_TEXT: [&str; 5] = ["--vcs-modified", "--vcs-added", "--vcs-deleted", "--vcs-untracked", "--vcs-conflict"];
    const NON_TEXT: [&str; 6] =
        ["--fg-faint", "--fg-gutter", "--vcs-ignored", "--gutter-modified", "--gutter-added", "--gutter-deleted"];

    fn ratio(tokens: &Tokens, fg: &str, bg: &str) -> f64 {
        let value = |name: &str| tokens.get(name).map(String::as_str).unwrap_or_default();
        contrast_ratio(value(fg), value(bg)).unwrap_or_else(|| panic!("{fg} or {bg} is not a hex colour"))
    }

    /// The pairs whose contrast is below `min`.
    fn failures(tokens: &Tokens, fgs: &[&str], bgs: &[&str], min: f64) -> Vec<String> {
        let mut out = Vec::new();
        for fg in fgs {
            for bg in bgs {
                let contrast = ratio(tokens, fg, bg);
                if contrast < min {
                    out.push(format!("{fg} on {bg}: {contrast:.2} < {min}"));
                }
            }
        }
        out
    }

    fn none() -> Vec<String> {
        Vec::new()
    }

    /// The tokens of `tokens` holding a plain colour.
    fn plain_colours(tokens: &Tokens) -> Vec<&str> {
        let plain = |value: &str| is_color_value(value) && !value.contains("var(");
        tokens.iter().filter(|(_, value)| plain(value)).map(|(name, _)| name.as_str()).collect()
    }

    // theme tokens › light theme redefines every colour token of :root
    #[test]
    fn light_theme_redefines_every_colour_token_of_root() {
        let own = read_tokens(TOKENS_CSS, ":root[data-theme=\"light\"]");
        let dark = dark();
        let colours = plain_colours(&dark);
        assert!(colours.len() > 50);
        let missing: Vec<&str> = colours.into_iter().filter(|name| !own.contains_key(*name)).collect();
        assert_eq!(missing, Vec::<&str>::new());
    }

    // theme tokens › high-contrast dark redefines every plain colour token
    #[test]
    fn high_contrast_dark_redefines_every_plain_colour_token() {
        let own = read_tokens(TOKENS_CSS, ":root[data-theme=\"dark-hc\"]");
        let dark = dark();
        let missing: Vec<&str> = plain_colours(&dark)
            .into_iter()
            .filter(|name| !name.starts_with("--shadow") && !own.contains_key(*name))
            .collect();
        assert_eq!(missing, Vec::<&str>::new());
    }

    // Light values are PyCharm Islands Light's own. Its secondary text grey, brand blue and status
    // colours are about 4.3:1 on the dialog surface, comments / metadata / inlay text about 3.4:1,
    // and line numbers / change bars are decorative (about 2:1); the thresholds pin those values.
    const LIGHT_STRONG_SURFACES: [&str; 2] = ["--bg-app", "--bg-popup"];

    // theme tokens › light text meets WCAG AA on the editor / popup surfaces
    #[test]
    fn light_text_meets_wcag_aa_on_the_editor_and_popup_surfaces() {
        let light = light();
        assert_eq!(failures(&light, &TEXT, &LIGHT_STRONG_SURFACES, 4.5), none());
        assert_eq!(failures(&light, &TEXT, &["--bg-elev"], 4.2), none());
        let selected = ["--bg-selected", "--bg-hover", "--bg-selected-strong"];
        assert_eq!(failures(&light, &["--fg", "--fg-secondary"], &selected, 4.0), none());
        assert_eq!(failures(&light, &["--fg-muted"], &["--bg-selected", "--bg-hover"], 3.5), none());
    }

    // theme tokens › light syntax colours stay readable on the editor and current line
    #[test]
    fn light_syntax_colours_stay_readable_on_the_editor_and_current_line() {
        let light = light();
        let soft = ["--syn-comment", "--syn-annotation", "--syn-inline-value", "--syn-invalid"];
        let main: Vec<&str> = SYNTAX.into_iter().filter(|token| !soft.contains(token)).collect();
        let surfaces = ["--bg-app", "--bg-current-line", "--bg-elev"];
        assert_eq!(failures(&light, &main, &surfaces, 4.5), none());
        assert_eq!(failures(&light, &soft, &surfaces, 3.0), none());
    }

    // theme tokens › light status / VCS colours are readable
    #[test]
    fn light_status_and_vcs_colours_are_readable() {
        let light = light();
        let readable = [STATUS.as_slice(), VCS_TEXT.as_slice()].concat();
        assert_eq!(failures(&light, &readable, &LIGHT_STRONG_SURFACES, 4.5), none());
        assert_eq!(failures(&light, &readable, &["--bg-elev"], 4.2), none());
        assert_eq!(failures(&light, &VCS_TEXT, &["--bg-selected"], 3.5), none());
        assert_eq!(failures(&light, &NON_TEXT, &["--bg-app", "--bg-elev"], 1.8), none());
        assert!(ratio(&light, "--fg-on-accent", "--accent") >= 4.5);
        assert!(ratio(&light, "--fg-tooltip", "--bg-tooltip") >= 4.5);
    }

    // theme tokens › dark theme primary text meets 4.5:1
    #[test]
    fn dark_theme_primary_text_meets_4_5_to_1() {
        let primary = ["--fg-strong", "--fg", "--fg-secondary", "--fg-muted"];
        assert_eq!(failures(&dark(), &primary, &SURFACES, 4.5), none());
    }

    // theme tokens › high-contrast dark reaches 7:1 for text and 4.5:1 for everything readable
    #[test]
    fn high_contrast_dark_reaches_7_to_1_for_text_and_4_5_to_1_for_everything_readable() {
        let hc = high_contrast();
        let text = ["--fg-strong", "--fg-emphasis", "--fg", "--fg-secondary", "--fg-muted"];
        assert_eq!(failures(&hc, &text, &SURFACES, 7.0), none());
        let readable =
            [&["--fg-subtle", "--fg-faint"][..], SYNTAX.as_slice(), STATUS.as_slice(), VCS_TEXT.as_slice()].concat();
        assert_eq!(failures(&hc, &readable, &SURFACES, 4.5), none());
        assert!(ratio(&hc, "--fg-on-accent", "--accent") >= 7.0);
    }

    // theme tokens › parses rules with selector lists
    #[test]
    fn parses_rules_with_selector_lists() {
        let tokens = read_tokens("a, :root[x] { --a: #fff; /* c */ --b: 1px }", ":root[x]");
        let expected: Tokens = [("--a", "#fff"), ("--b", "1px")].map(|(k, v)| (k.to_owned(), v.to_owned())).into();
        assert_eq!(tokens, expected);
        let black_on_white = contrast_ratio("#000", "#fff").unwrap_or_default();
        assert!((black_on_white - 21.0).abs() < 1e-5);
    }

    /// Tokens the palette themes (Solarized, One) take from their light / dark base on purpose:
    /// colours the user assigns (project and data source colours), the platform's caption close
    /// and macOS window buttons, and the tint strength.
    fn inherited_from_base(token: &str) -> bool {
        ["--project-color-", "--db-color-", "--caption-close-", "--fg-caption-close-"]
            .iter()
            .any(|start| token.starts_with(start))
            || token == "--tint-strength"
    }

    fn palettes() -> Vec<(&'static str, ThemeKind)> {
        let islands = [BuiltinThemeId::Light, BuiltinThemeId::Dark, BuiltinThemeId::DarkHc];
        BUILTIN_THEMES.iter().filter(|theme| !islands.contains(&theme.id)).map(|t| (t.id.as_str(), t.kind)).collect()
    }

    // palette themes › have a tokens.css block for every palette theme id, and no other
    #[test]
    fn have_a_tokens_css_block_for_every_palette_theme_id_and_no_other() {
        let block = Regex::new(r#":root\[data-color-theme="([a-z0-9-]+)"\]"#).expect("the pattern is valid");
        let blocks: BTreeSet<&str> =
            block.captures_iter(TOKENS_CSS).filter_map(|found| found.get(1)).map(|id| id.as_str()).collect();
        let ids: BTreeSet<&str> = palettes().into_iter().map(|(id, _)| id).collect();
        assert_eq!(blocks, ids);
        assert!(ids.len() >= 4);
    }

    // palette themes › $id defines every token or inherits it from its $kind base on purpose
    #[test]
    fn every_palette_theme_defines_every_token_or_inherits_it_from_its_base_on_purpose() {
        let palette_ref = Regex::new(r"var\((--palette-[a-z0-9-]+)").expect("the pattern is valid");
        let (dark, light) = (dark(), light());
        let shared = read_tokens(TOKENS_CSS, ":root[data-color-theme]");
        // Every themeable token (the Islands Light block lists them all) and every plain colour of :root.
        let themeable = read_tokens(TOKENS_CSS, ":root[data-theme=\"light\"]");
        let mut required: BTreeSet<String> = themeable.into_keys().collect();
        required.extend(plain_colours(&dark).into_iter().map(str::to_owned));
        assert!(required.len() > 200);
        for (id, kind) in palettes() {
            // A failure here means tokens.css gained a token: map it in the :root[data-color-theme]
            // block (or the theme's own), or let it be inherited when the base's value is right.
            let own = over(&shared, &format!(":root[data-color-theme=\"{id}\"]"));
            let unmapped: Vec<&String> =
                required.iter().filter(|token| !own.contains_key(*token) && !inherited_from_base(token)).collect();
            assert_eq!(unmapped, Vec::<&String>::new(), "{id}");
            let base = if kind == ThemeKind::Light { &light } else { &dark };
            let not_in_base: Vec<&String> =
                required.iter().filter(|token| inherited_from_base(token) && !base.contains_key(*token)).collect();
            assert_eq!(not_in_base, Vec::<&String>::new(), "{id}");
            // Every palette role the shared mapping uses is set by the theme.
            let values = own.values().map(String::as_str).collect::<Vec<_>>().join(" ");
            let unset: Vec<&str> = palette_ref
                .captures_iter(&values)
                .filter_map(|found| found.get(1))
                .map(|role| role.as_str())
                .filter(|role| !own.contains_key(*role))
                .collect();
            assert_eq!(unset, Vec::<&str>::new(), "{id}");
            for token in ["--chart-series", "--bg-frame", "--fg-muted", "--accent", "--ansi-red", "--syn-keyword"] {
                assert!(own.get(token).is_some_and(|value| !value.is_empty()), "{id} {token}");
            }
        }
    }
}
