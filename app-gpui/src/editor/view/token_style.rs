//! How each kind of highlighted token is drawn: its colour, weight and slant, from the theme's
//! `--syn-<token>`, `--syn-<token>-bold` and `--syn-<token>-italic` (which the colour scheme and its
//! edits set) and from the scheme's overrides for the editor's language. A token without a weight is
//! drawn in the code font's own (Typography Settings' main weight); comments, doc comments and
//! constants are slanted unless the scheme says otherwise.

use super::syntax_token;
use crate::theme::{self, ActiveTheme, scheme_runtime::SchemeLanguageTokens, tokens::parse_color};
use gpui_kit::{App, Font, FontStyle, FontWeight, Hsla, Rgba};
use ide_language::SyntaxHighlightKind;

/// The colour and the font of every kind of token, for one frame of one editor.
pub(super) struct TokenStyles {
    /// Indexed by `SyntaxHighlightKind as usize`.
    styles: Vec<(Hsla, Font)>,
}

/// The slant a kind has when the scheme gives none: `--syn-comment-style` for comments, italic for
/// doc comments and constants.
fn default_italic(kind: SyntaxHighlightKind, value: &dyn Fn(&str) -> Option<String>) -> bool {
    use SyntaxHighlightKind as K;
    match kind {
        K::Comment => value("syn-comment-style").is_some_and(|style| style.trim() == "italic"),
        K::DocComment | K::Atom | K::Constant => true,
        _ => false,
    }
}

impl TokenStyles {
    /// The styles of every kind for an editor of `language` drawing code in `code_font`.
    pub fn new(code_font: &Font, language: &str, cx: &App) -> Self {
        let active = cx.try_global::<ActiveTheme>().map(|active| &active.theme);
        let overrides = cx.try_global::<SchemeLanguageTokens>();
        // The language's own override, else the theme's value, `var()` references resolved.
        let value = |name: &str| -> Option<String> {
            let own = overrides.and_then(|tokens| tokens.get(language, name)).map(str::to_owned);
            own.or_else(|| active.and_then(|active| active.resolved(name)))
        };
        let styles = SyntaxHighlightKind::ALL
            .iter()
            .map(|kind| {
                let token = syntax_token(*kind);
                let own_colour = overrides.and_then(|tokens| tokens.get(language, token)).and_then(parse_color);
                let colour = own_colour
                    .map(|c| Hsla::from(Rgba { r: c.r, g: c.g, b: c.b, a: c.a }))
                    .unwrap_or_else(|| theme::color(cx, token));
                let weight = value(&format!("{token}-bold")).and_then(|weight| weight.trim().parse::<f32>().ok());
                let italic = match value(&format!("{token}-italic")).as_deref().map(str::trim) {
                    Some("italic") | Some("oblique") => true,
                    Some("normal") => false,
                    _ => default_italic(*kind, &value),
                };
                let font = Font {
                    weight: weight.map_or(code_font.weight, |weight| FontWeight(weight.clamp(100.0, 900.0))),
                    style: if italic { FontStyle::Italic } else { FontStyle::Normal },
                    ..code_font.clone()
                };
                (colour, font)
            })
            .collect();
        Self { styles }
    }

    /// The colour and the font of tokens of `kind`.
    pub fn get(&self, kind: SyntaxHighlightKind) -> (Hsla, &Font) {
        let (colour, font) = &self.styles[kind as usize];
        (*colour, font)
    }
}

impl super::Editor {
    /// The weight and slant the last frame drew the character at `col` (UTF-16) of `line` in, if it
    /// painted the line.
    #[cfg(test)]
    pub fn painted_font_at(&self, line: u32, col: u32) -> Option<(FontWeight, FontStyle)> {
        let layout = self.layout.as_ref()?;
        let row = layout.line_of.iter().position(|painted| *painted == line)?;
        let (text, _) = layout.lines.get(row)?;
        let byte = crate::editor::buffer::byte_of_col(text, col);
        let mut at = 0;
        layout.painted_fonts.get(row)?.iter().find_map(|&(len, weight, style)| {
            at += len;
            (byte < at).then_some((weight, style))
        })
    }
}
