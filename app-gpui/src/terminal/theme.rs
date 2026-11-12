//! The terminal's colours and font, read from the design tokens and the settings so the terminal
//! follows the active theme. The 16-colour palette comes from the `--terminal-ansi-*` tokens,
//! else the console's `--ansi-*`; when a theme lacks both it is derived from the status and
//! syntax tokens.

use crate::platform::css_color::resolve_token_color;
use crate::theme::fonts::{BUNDLED_MONO_FAMILY, family_faces, mono_font_family};
use crate::theme::tokens::{Color, Theme};
use ide_core::settings::EditorSettings;
use ide_core::workflow_settings::TerminalSettings;

/// `#rrggbb`: the colour without its alpha.
pub fn to_hex(color: Color) -> String {
    opaque(color).to_hex()
}

fn opaque(color: Color) -> Color {
    Color { a: 1.0, ..color }
}

/// Linear mix: `t = 0` → `a`, `t = 1` → `b`. The result is opaque.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1.0 }
}

/// The contrast ratio text is lifted to when its colour reads poorly on the background (yellow
/// on white): WCAG AA.
pub const MIN_CONTRAST_RATIO: f32 = 4.5;

const BG_APP: Color = Color::rgb(0x0f, 0x10, 0x12);
const FG: Color = Color::rgb(0xb9, 0xbb, 0xc1);
const FG_STRONG: Color = Color::rgb(0xe2, 0xe3, 0xe6);
const FG_SUBTLE: Color = Color::rgb(0x6d, 0x70, 0x76);
const ACCENT: Color = Color::rgb(0x8f, 0xa5, 0xd6);
const CONTROL_OFF: Color = Color::rgb(0x3a, 0x3b, 0x3f);
const DANGER: Color = Color::rgb(0xc9, 0x7a, 0x6f);
const SUCCESS: Color = Color::rgb(0x8f, 0xae, 0x7e);
const WARNING: Color = Color::rgb(0xc9, 0xa3, 0x5f);
const SYN_KEYWORD: Color = Color::rgb(0x8f, 0xa5, 0xd6);
const SYN_ANNOTATION: Color = Color::rgb(0xa8, 0x9b, 0xc4);
const SYN_STRING: Color = Color::rgb(0x9f, 0xb8, 0x8a);

/// The colours a terminal draws with. All are opaque.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalTheme {
    pub background: Color,
    pub foreground: Color,
    pub cursor: Color,
    /// The character under a block cursor.
    pub cursor_accent: Color,
    pub selection_background: Color,
    /// The selection of a terminal without the focus.
    pub selection_inactive_background: Color,
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
    pub bright_black: Color,
    pub bright_red: Color,
    pub bright_green: Color,
    pub bright_yellow: Color,
    pub bright_blue: Color,
    pub bright_magenta: Color,
    pub bright_cyan: Color,
    pub bright_white: Color,
}

impl TerminalTheme {
    /// The 16 ANSI colours by their index: 0–7 normal, 8–15 bright.
    pub fn ansi(&self) -> [Color; 16] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.magenta,
            self.cyan,
            self.white,
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_magenta,
            self.bright_cyan,
            self.bright_white,
        ]
    }
}

/// The terminal theme of the tokens `read` gives: the colour of a token (`--bg-app`), `None`
/// when the theme has none or it is no colour.
pub fn build_theme(read: impl Fn(&str) -> Option<Color>) -> TerminalTheme {
    let color = |name: &str, fallback: Color| opaque(read(name).unwrap_or(fallback));
    let bg = opaque(read("--bg-island").or_else(|| read("--bg-app")).unwrap_or(BG_APP));
    let fg = opaque(read("--fg-editor").or_else(|| read("--fg")).unwrap_or(FG));
    let strong = color("--fg-strong", FG_STRONG);
    let accent = color("--accent", ACCENT);
    let selection = read("--bg-editor-selection").map(opaque);
    let ansi = |name: &str, derived: Color| {
        let token = read(&format!("--terminal-ansi-{name}")).or_else(|| read(&format!("--ansi-{name}")));
        opaque(token.unwrap_or(derived))
    };

    let red = ansi("red", color("--danger", DANGER));
    let green = ansi("green", color("--success", SUCCESS));
    let yellow = ansi("yellow", color("--warning", WARNING));
    let blue = ansi("blue", color("--syn-keyword", SYN_KEYWORD));
    let magenta = ansi("magenta", color("--syn-annotation", SYN_ANNOTATION));
    // No cyan token: halfway between the keyword blue and the string green reads as a calm teal.
    let cyan = ansi("cyan", mix(blue, color("--syn-string", SYN_STRING), 0.5));
    let bright = |name: &str, base: Color| ansi(&format!("bright-{name}"), mix(base, strong, 0.28));

    TerminalTheme {
        background: bg,
        foreground: fg,
        cursor: color("--caret", accent),
        cursor_accent: bg,
        selection_background: selection.unwrap_or_else(|| mix(bg, accent, 0.32)),
        selection_inactive_background: selection.map_or_else(|| mix(bg, accent, 0.18), |s| mix(bg, s, 0.6)),
        black: ansi("black", color("--control-off", CONTROL_OFF)),
        red,
        green,
        yellow,
        blue,
        magenta,
        cyan,
        white: ansi("white", fg),
        bright_black: ansi("bright-black", color("--fg-subtle", FG_SUBTLE)),
        bright_red: bright("red", red),
        bright_green: bright("green", green),
        bright_yellow: bright("yellow", yellow),
        bright_blue: bright("blue", blue),
        bright_magenta: bright("magenta", magenta),
        bright_cyan: bright("cyan", cyan),
        bright_white: ansi("bright-white", strong),
    }
}

/// The terminal theme of `theme`.
pub fn terminal_theme(theme: &Theme) -> TerminalTheme {
    build_theme(|name| resolve_token_color(name, theme))
}

/// How the matches of Find in Terminal are marked. All colours are opaque.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchDecorations {
    pub match_background: Color,
    /// The mark of a match beside the scrollbar.
    pub match_marker: Color,
    pub active_match_background: Color,
    pub active_match_border: Color,
    /// The mark of the current match beside the scrollbar.
    pub active_match_marker: Color,
}

impl SearchDecorations {
    pub fn colors(&self) -> [Color; 5] {
        [
            self.match_background,
            self.match_marker,
            self.active_match_background,
            self.active_match_border,
            self.active_match_marker,
        ]
    }
}

/// The search decorations of the tokens `read` gives: the editor's search result colours, else
/// tints of the match and warning colours on the terminal background.
pub fn build_search_decorations(read: impl Fn(&str) -> Option<Color>) -> SearchDecorations {
    let found = read("--bg-text-search-result").map(opaque);
    let current = read("--bg-text-search-result-current").map(opaque);
    if let (Some(found), Some(current)) = (found, current) {
        return SearchDecorations {
            match_background: found,
            match_marker: found,
            active_match_background: current,
            active_match_border: current,
            active_match_marker: current,
        };
    }
    let bg = opaque(read("--bg-island").or_else(|| read("--bg-app")).unwrap_or(BG_APP));
    let found = opaque(read("--match").unwrap_or(ACCENT));
    let current = opaque(read("--warning").unwrap_or(WARNING));
    SearchDecorations {
        match_background: mix(bg, found, 0.3),
        match_marker: found,
        active_match_background: mix(bg, current, 0.45),
        active_match_border: current,
        active_match_marker: current,
    }
}

/// The search decorations of `theme`.
pub fn search_decorations(theme: &Theme) -> SearchDecorations {
    build_search_decorations(|name| resolve_token_color(name, theme))
}

const NERD_FONT_FALLBACKS: [&str; 9] = [
    "Symbols Nerd Font Mono",
    "JetBrainsMono Nerd Font Mono",
    "JetBrainsMono Nerd Font",
    "MesloLGS NF",
    "MesloLGS Nerd Font",
    "Hack Nerd Font",
    "FiraCode Nerd Font",
    "Hasklug Nerd Font",
    "CaskaydiaCove Nerd Font",
];

/// The font a terminal draws its cells with.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalFont {
    /// The font family list, most wanted first.
    pub family: String,
    pub size: f32,
    /// Multiple of the cell height.
    pub line_height: f32,
    /// Extra pixels between cells.
    pub letter_spacing: f32,
    pub weight: u16,
    pub bold_weight: u16,
}

const REGULAR_WEIGHT: u16 = 400;
const BOLD_WEIGHT: u16 = 700;
/// The font size when neither the editor settings nor the theme give one.
const DEFAULT_FONT_SIZE: f32 = 13.0;
/// The advance of a monospace glyph, in ems.
const MONO_ADVANCE_EM: f64 = 0.6;

/// CSS weight from 100 to 900, rounded to the nearest 50.
fn font_weight(raw: u16) -> u16 {
    (raw.saturating_add(25) / 50 * 50).clamp(100, 900)
}

/// `family` without a trailing generic `monospace`, which goes after the fallbacks added to it.
fn without_generic_monospace(family: &str) -> &str {
    const GENERIC: &str = "monospace";
    let trimmed = family.trim_end();
    let Some(at) = trimmed.len().checked_sub(GENERIC.len()).filter(|&at| trimmed.is_char_boundary(at)) else {
        return family;
    };
    if !trimmed[at..].eq_ignore_ascii_case(GENERIC) {
        return family;
    }
    trimmed[..at].trim_end().strip_suffix(',').unwrap_or(family)
}

/// Prompt themes (powerlevel10k, starship) use Nerd Font private-use glyphs; common installs are
/// listed before the generic family so per-glyph fallback finds them.
fn with_nerd_fonts(family: &str) -> String {
    let fallbacks: Vec<String> = NERD_FONT_FALLBACKS.iter().flat_map(|name| family_faces(name)).collect();
    format!("{}, {}, monospace", without_generic_monospace(family), fallbacks.join(", "))
}

/// A size setting that is set: zero and NaN mean "none".
fn set_size(size: f32) -> Option<f32> {
    (size != 0.0 && !size.is_nan()).then_some(size)
}

/// Font from Settings › Tools › Terminal: the editor font (Editor › Font, with its fallback and
/// Typography Settings weights) or the terminal's own family and size. The line height (a
/// multiple of the cell height) is always the terminal's: the editor's roomier spacing would
/// stretch box drawing and ASCII art. The family resolves like the editor's, so the bundled
/// JetBrains Mono is used where it isn't installed; `base` is the resolved `--font-mono` stack
/// ([`crate::theme::fonts::mono_stack`]) and `code_font_size` the theme's `--fs-code`. The column
/// width (a multiple of the cell width) becomes letter spacing, from the ≈ 0.6 em advance of
/// monospace fonts.
pub fn terminal_font(
    terminal: &TerminalSettings,
    editor: Option<&EditorSettings>,
    base: &str,
    code_font_size: Option<f32>,
) -> TerminalFont {
    let (family, fallback, size, weight, bold_weight) = if terminal.use_editor_font {
        let family = editor.map(|e| e.font_family.as_str()).filter(|family| !family.is_empty());
        let editor_fallback = editor.map_or("", |e| e.fallback_font.as_str());
        let own_fallback = terminal.fallback_font_family.trim();
        let fallback = if own_fallback.is_empty() { editor_fallback } else { own_fallback };
        let size = editor.and_then(|e| set_size(e.font_size)).or(code_font_size.and_then(set_size));
        (
            family.unwrap_or(BUNDLED_MONO_FAMILY),
            fallback,
            size.unwrap_or(DEFAULT_FONT_SIZE),
            editor.map_or(REGULAR_WEIGHT, |e| font_weight(e.font_weight)),
            editor.map_or(BOLD_WEIGHT, |e| font_weight(e.bold_font_weight)),
        )
    } else {
        let (family, fallback) = (terminal.font_family.as_str(), terminal.fallback_font_family.as_str());
        (family, fallback, terminal.font_size, REGULAR_WEIGHT, BOLD_WEIGHT)
    };
    let spacing = (f64::from(terminal.column_width) - 1.0) * f64::from(size) * MONO_ADVANCE_EM;
    TerminalFont {
        family: with_nerd_fonts(&mono_font_family(Some(family), fallback, base)),
        size,
        line_height: terminal.line_height,
        letter_spacing: ((spacing * 10.0).round() / 10.0) as f32,
        weight,
        bold_weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::css_color::parse_color;
    use std::collections::BTreeMap;

    fn is_hex(color: Color) -> bool {
        let hex = to_hex(color);
        hex.len() == 7 && hex.starts_with('#') && hex[1..].bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    // src/features/terminal/terminal.test.ts › theme › parses colour formats
    #[test]
    fn parses_colour_formats() {
        assert_eq!(parse_color("#0f1012"), Some(Color::rgb(15, 16, 18)));
        assert_eq!(parse_color("#fff"), Some(Color::rgb(255, 255, 255)));
        assert!(parse_color("#00000080").is_some_and(|color| (color.a - 0.5).abs() < 0.005));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Color::rgb(1, 2, 3)));
        assert_eq!(parse_color("rgba(1 2 3 / 50%)"), Some(Color { a: 0.5, ..Color::rgb(1, 2, 3) }));
        assert_eq!(parse_color("color(srgb 1 0 0.5)"), Some(Color { r: 1.0, g: 0.0, b: 0.5, a: 1.0 }));
        assert_eq!(parse_color("var(--x)"), None);
    }

    // src/features/terminal/terminal.test.ts › theme › mixes and formats
    #[test]
    fn mixes_and_formats() {
        assert_eq!(to_hex(mix(Color::rgb(0, 0, 0), Color::rgb(255, 255, 255), 0.5)), "#808080");
    }

    // src/features/terminal/terminal.test.ts › theme › derives a full palette from tokens
    #[test]
    fn derives_a_full_palette_from_tokens() {
        let tokens = BTreeMap::from([
            ("--bg-app", "#0f1012"),
            ("--fg", "#b9bbc1"),
            ("--fg-strong", "#e2e3e6"),
            ("--accent", "#8fa5d6"),
            ("--danger", "#c97a6f"),
            ("--success", "#8fae7e"),
            ("--warning", "#c9a35f"),
            ("--syn-keyword", "#8fa5d6"),
            ("--syn-annotation", "#a89bc4"),
            ("--syn-string", "#9fb88a"),
            ("--fg-subtle", "#6d7076"),
            ("--control-off", "#3a3b3f"),
        ]);
        let theme = build_theme(|name| tokens.get(name).and_then(|value| parse_color(value)));
        assert_eq!(to_hex(theme.background), "#0f1012");
        assert_eq!(to_hex(theme.foreground), "#b9bbc1");
        assert_eq!(to_hex(theme.cursor), "#8fa5d6");
        assert_eq!(to_hex(theme.red), "#c97a6f");
        assert_eq!(to_hex(theme.bright_white), "#e2e3e6");
        for color in [theme.black, theme.cyan, theme.bright_red, theme.bright_cyan, theme.selection_background] {
            assert!(is_hex(color));
        }
        assert_ne!(theme.bright_red, theme.red);
        assert_eq!(theme.ansi()[9], theme.bright_red);
    }

    // src/features/terminal/terminal.test.ts › theme › falls back when tokens are missing
    #[test]
    fn falls_back_when_tokens_are_missing() {
        let theme = build_theme(|_| None);
        assert_eq!(to_hex(theme.background), "#0f1012");
        assert!(is_hex(theme.red));
    }

    // src/features/terminal/terminal.test.ts › terminal find › builds #rrggbb search decorations from tokens
    #[test]
    fn builds_opaque_search_decorations_from_tokens() {
        let decorations = build_search_decorations(|name| match name {
            "--match" => parse_color("#8fa5d6"),
            "--bg-app" => parse_color("#000000"),
            _ => None,
        });
        assert!(decorations.colors().into_iter().all(is_hex));
        assert_eq!(to_hex(decorations.match_marker), "#8fa5d6");
    }

    const BASE: &str = r#""JetBrains Mono", "JetBrains Mono Variable", ui-monospace, Menlo, monospace"#;
    /// `BASE` without its generic family, as it stands before the Nerd Font fallbacks.
    const BASE_HEAD: &str = r#""JetBrains Mono", "JetBrains Mono Variable", ui-monospace, Menlo"#;

    fn terminal() -> TerminalSettings {
        TerminalSettings {
            use_editor_font: true,
            font_family: "JetBrains Mono".to_owned(),
            font_size: 10.0,
            line_height: 1.0,
            fallback_font_family: String::new(),
            column_width: 1.0,
            ..TerminalSettings::default()
        }
    }

    fn own_font() -> TerminalSettings {
        TerminalSettings { use_editor_font: false, ..terminal() }
    }

    fn editor() -> EditorSettings {
        EditorSettings {
            font_family: "JetBrains Mono".to_owned(),
            font_size: 10.0,
            line_height: 12.0,
            ..EditorSettings::default()
        }
    }

    fn font(terminal: &TerminalSettings, editor: Option<&EditorSettings>) -> TerminalFont {
        terminal_font(terminal, editor, BASE, None)
    }

    /// The family list up to the Nerd Font fallbacks.
    fn head(family: &str) -> &str {
        &family[..family.find(r#""Symbols Nerd Font Mono""#).unwrap_or(family.len())]
    }

    // src/features/terminal/terminal.test.ts › terminalFont › %s resolves through the bundled stack
    #[test]
    fn resolves_through_the_bundled_stack() {
        const BUNDLED: &str = r#""JetBrains Mono", "JetBrains Mono Variable""#;
        let fira = EditorSettings { font_family: "Fira Code".to_owned(), ..editor() };
        let with_fallback = EditorSettings { fallback_font: "PingFang SC".to_owned(), ..editor() };
        let hack = TerminalSettings {
            font_family: "Hack".to_owned(),
            fallback_font_family: "PingFang SC".to_owned(),
            ..own_font()
        };
        let cases = [
            // "Use editor font" with the default family must reach the bundled face, not the system monospace.
            ("editor font, default family", terminal(), Some(editor()), format!("{BUNDLED}, {BASE_HEAD}, ")),
            ("editor font, no editor settings", terminal(), None, format!("{BUNDLED}, {BASE_HEAD}, ")),
            ("editor font, custom family", terminal(), Some(fira), format!(r#""Fira Code", {BASE_HEAD}, "#)),
            (
                "editor font, editor fallback",
                terminal(),
                Some(with_fallback),
                format!(r#"{BUNDLED}, "PingFang SC", {BASE_HEAD}, "#),
            ),
            ("own font, default family", own_font(), Some(editor()), format!("{BUNDLED}, {BASE_HEAD}, ")),
            ("own font, custom + fallback", hack, Some(editor()), format!(r#""Hack", "PingFang SC", {BASE_HEAD}, "#)),
        ];
        for (name, terminal, editor, expected) in cases {
            let font = font(&terminal, editor.as_ref());
            assert_eq!(head(&font.family), expected, "{name}");
            assert!(font.family.ends_with(", monospace"), "{name}");
            assert!(!font.family.contains("var("), "{name}");
        }
    }

    // src/features/terminal/terminal.test.ts › terminalFont › resolves bundled Inter in inherited, independent and
    // fallback selections
    #[test]
    fn resolves_bundled_inter_in_inherited_independent_and_fallback_selections() {
        const INTER: &str = r#""Inter", "Inter Variable""#;
        let inter_editor = EditorSettings { font_family: "Inter".to_owned(), ..editor() };
        assert!(font(&terminal(), Some(&inter_editor)).family.starts_with(&format!("{INTER},")));
        let inter_terminal = TerminalSettings { font_family: "Inter".to_owned(), ..own_font() };
        assert!(font(&inter_terminal, Some(&editor())).family.starts_with(&format!("{INTER},")));
        let inter_fallback = TerminalSettings { fallback_font_family: "Inter".to_owned(), ..terminal() };
        assert!(font(&inter_fallback, Some(&editor())).family.contains(INTER));
        let editor_fallback = EditorSettings { fallback_font: "Inter".to_owned(), ..editor() };
        assert!(font(&terminal(), Some(&editor_fallback)).family.contains(INTER));
    }

    // src/features/terminal/terminal.test.ts › terminalFont › prefers the terminal's fallback over the editor's
    #[test]
    fn prefers_the_terminals_fallback_over_the_editors() {
        let terminal = TerminalSettings { fallback_font_family: "Sarasa Mono SC".to_owned(), ..terminal() };
        let editor = EditorSettings { fallback_font: "PingFang SC".to_owned(), ..editor() };
        let font = font(&terminal, Some(&editor));
        assert!(font.family.contains(r#""Sarasa Mono SC""#));
        assert!(!font.family.contains("PingFang SC"));
    }

    // src/features/terminal/terminal.test.ts › terminalFont › follows the editor's Typography Settings weights with
    // Use editor font
    #[test]
    fn follows_the_editors_typography_settings_weights_with_use_editor_font() {
        let light = EditorSettings { font_weight: 300, bold_font_weight: 600, ..editor() };
        let f = font(&terminal(), Some(&light));
        assert_eq!((f.weight, f.bold_weight), (300, 600));
        let d = font(&terminal(), Some(&editor()));
        assert_eq!((d.weight, d.bold_weight), (400, 700));
    }

    // src/features/terminal/terminal.test.ts › terminalFont › keeps regular / bold for the terminal's own font
    #[test]
    fn keeps_regular_and_bold_for_the_terminals_own_font() {
        let light = EditorSettings { font_weight: 300, bold_font_weight: 600, ..editor() };
        let f = font(&own_font(), Some(&light));
        assert_eq!((f.weight, f.bold_weight), (400, 700));
    }

    // src/features/terminal/terminal.test.ts › terminalFont › keeps the terminal's line height with Use editor font
    #[test]
    fn keeps_the_terminals_line_height_with_use_editor_font() {
        let roomy = EditorSettings { line_height: 21.0, ..editor() };
        assert_eq!(font(&terminal(), Some(&roomy)).line_height, 1.0);
        let taller = TerminalSettings { line_height: 1.2, ..terminal() };
        assert_eq!(font(&taller, Some(&editor())).line_height, 1.2);
    }

    // src/features/terminal/terminal.test.ts › terminalFont › turns the column width into letter spacing
    #[test]
    fn turns_the_column_width_into_letter_spacing() {
        assert_eq!(font(&own_font(), Some(&editor())).letter_spacing, 0.0);
        let wide = TerminalSettings { column_width: 1.5, ..own_font() };
        assert_eq!(font(&wide, Some(&editor())).letter_spacing, 3.0);
    }

    // Not from a spec: the size comes from the editor, else the theme's code size, else 13; weights snap to 50s.
    #[test]
    fn takes_the_size_from_the_editor_then_the_theme() {
        assert_eq!(font(&terminal(), Some(&editor())).size, 10.0);
        assert_eq!(terminal_font(&terminal(), None, BASE, Some(12.0)).size, 12.0);
        assert_eq!(terminal_font(&terminal(), None, BASE, None).size, 13.0);
        assert_eq!(font(&TerminalSettings { font_size: 15.0, ..own_font() }, None).size, 15.0);
        assert_eq!((font_weight(0), font_weight(325), font_weight(449), font_weight(2000)), (100, 350, 450, 900));
    }
}
