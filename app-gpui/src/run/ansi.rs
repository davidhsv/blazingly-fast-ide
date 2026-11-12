//! ANSI escape handling for process output: one stateful parser per stream turns text with SGR colours
//! into styled spans, keeping a style across chunks and dropping the other escapes.

use regex::{Captures, Regex};
use std::sync::LazyLock;

/// A colour of styled output: one of the 16 theme colours, or an explicit one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnsiColor {
    /// Index into the theme's ANSI palette: 0–7 normal, 8–15 bright.
    Named(u8),
    Rgb(u8, u8, u8),
}

const NAMED: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

impl AnsiColor {
    /// The colour as the theme names it: `var(--ansi-bright-red)` or `rgb(r, g, b)`.
    pub fn css(self) -> String {
        match self {
            Self::Named(n) => {
                let bright = if n >= 8 { "bright-" } else { "" };
                format!("var(--ansi-{bright}{})", NAMED[usize::from(n % 8)])
            }
            Self::Rgb(r, g, b) => format!("rgb({r}, {g}, {b})"),
        }
    }
}

/// xterm 256-colour palette entry: the theme colours below 16, then the colour cube and the grey ramp.
pub fn palette_color(n: u8) -> AnsiColor {
    if n < 16 {
        return AnsiColor::Named(n);
    }
    if n >= 232 {
        let v = 8 + (n - 232) * 10;
        return AnsiColor::Rgb(v, v, v);
    }
    const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let i = usize::from(n - 16);
    AnsiColor::Rgb(STEPS[i / 36], STEPS[(i / 6) % 6], STEPS[i % 6])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SpanStyle {
    pub fg: Option<AnsiColor>,
    pub bg: Option<AnsiColor>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

impl SpanStyle {
    /// No colour and no decoration.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

/// A run of text in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: SpanStyle::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decoration {
    Bold,
    Dim,
    Italic,
    Underline,
    Blink,
    Reverse,
    Hidden,
    Strikethrough,
}

/// The graphic rendition in effect: colours and the decorations switched on, in order. A decoration
/// switched on twice needs switching off twice.
#[derive(Clone, Debug, Default)]
struct Rendition {
    fg: Option<AnsiColor>,
    bg: Option<AnsiColor>,
    decorations: Vec<Decoration>,
}

impl Rendition {
    fn remove(&mut self, decoration: Decoration) {
        if let Some(index) = self.decorations.iter().position(|d| *d == decoration) {
            self.decorations.remove(index);
        }
    }

    /// Applies the parameters of one SGR sequence (`1;31`).
    fn apply(&mut self, params: &str) {
        let mut nums = params.split(';');
        // A parameter too large to be a code is ignored; an empty one resets, as a zero does.
        let code = |text: &str| if text.is_empty() { None } else { Some(text.parse::<u32>().unwrap_or(u32::MAX)) };
        let channel = |text: Option<&str>| text.and_then(|t| t.parse::<u8>().ok());
        while let Some(text) = nums.next() {
            match code(text) {
                None | Some(0) => {
                    self.fg = None;
                    self.bg = None;
                    self.decorations.clear();
                }
                Some(1) => self.decorations.push(Decoration::Bold),
                Some(2) => self.decorations.push(Decoration::Dim),
                Some(3) => self.decorations.push(Decoration::Italic),
                Some(4) => self.decorations.push(Decoration::Underline),
                Some(5) => self.decorations.push(Decoration::Blink),
                Some(7) => self.decorations.push(Decoration::Reverse),
                Some(8) => self.decorations.push(Decoration::Hidden),
                Some(9) => self.decorations.push(Decoration::Strikethrough),
                Some(21) => self.remove(Decoration::Bold),
                Some(22) => {
                    self.remove(Decoration::Bold);
                    self.remove(Decoration::Dim);
                }
                Some(23) => self.remove(Decoration::Italic),
                Some(24) => self.remove(Decoration::Underline),
                Some(25) => self.remove(Decoration::Blink),
                Some(27) => self.remove(Decoration::Reverse),
                Some(28) => self.remove(Decoration::Hidden),
                Some(29) => self.remove(Decoration::Strikethrough),
                Some(39) => self.fg = None,
                Some(49) => self.bg = None,
                Some(n @ 30..=37) => self.fg = Some(AnsiColor::Named((n - 30) as u8)),
                Some(n @ 90..=97) => self.fg = Some(AnsiColor::Named((n - 90 + 8) as u8)),
                Some(n @ 40..=47) => self.bg = Some(AnsiColor::Named((n - 40) as u8)),
                Some(n @ 100..=107) => self.bg = Some(AnsiColor::Named((n - 100 + 8) as u8)),
                Some(n @ (38 | 48)) => {
                    // Extended colour: `5;<palette index>` or `2;<r>;<g>;<b>`. The mode and its values
                    // are consumed whether or not they are valid.
                    let mut rest = nums.clone();
                    let color = match rest.next() {
                        Some("5") if rest.clone().next().is_some() => {
                            nums = rest.clone();
                            channel(nums.next()).map(palette_color)
                        }
                        Some("2") if rest.clone().count() >= 3 => {
                            nums = rest.clone();
                            let (r, g, b) = (channel(nums.next()), channel(nums.next()), channel(nums.next()));
                            r.zip(g).zip(b).map(|((r, g), b)| AnsiColor::Rgb(r, g, b))
                        }
                        Some(_) => {
                            nums = rest;
                            None
                        }
                        None => None,
                    };
                    if let Some(color) = color {
                        if n == 38 {
                            self.fg = Some(color);
                        } else {
                            self.bg = Some(color);
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }

    fn style(&self) -> SpanStyle {
        let has = |decoration: Decoration| self.decorations.contains(&decoration);
        let (mut fg, mut bg) = (self.fg, self.bg);
        // Reversed text keeps its colours; a missing one becomes white on black.
        if has(Decoration::Reverse) {
            fg = fg.or(Some(AnsiColor::Named(7)));
            bg = bg.or(Some(AnsiColor::Named(0)));
        }
        SpanStyle {
            fg,
            bg,
            bold: has(Decoration::Bold),
            dim: has(Decoration::Dim),
            italic: has(Decoration::Italic),
            underline: has(Decoration::Underline),
            strike: has(Decoration::Strikethrough),
        }
    }
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the escape sequence patterns are valid")
}

/// OSC (…BEL or …ST).
static OSC: LazyLock<Regex> = LazyLock::new(|| pattern(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)"));
/// Escape sequences other than CSI.
static NON_CSI: LazyLock<Regex> = LazyLock::new(|| pattern(r"\x1b(?:[()][0-9A-Za-z]|[=>78DEHMNOZc])"));
/// A CSI sequence: parameters, intermediates, final byte.
static CSI: LazyLock<Regex> = LazyLock::new(|| pattern(r"\x1b\[([0-9;?]*)([ -/]*)([@-~])"));
/// An escape sequence cut off at the end of a chunk.
static INCOMPLETE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\x1b(?:\[[0-9;?]*[ -/]*|\][^\x07\x1b]*\x1b?)?$"));
/// What follows `ESC [`: private markers, parameters, the command and the text after it.
static SEQUENCE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?s)^([!\x3c-\x3f]*)([0-9;]*)([\x20-\x2c]*[\x40-\x7e])(.*)"));

/// The ANSI state of one output stream.
#[derive(Clone, Debug, Default)]
pub struct AnsiStream {
    rendition: Rendition,
    carry: String,
    /// Style in effect at the end of the previous call.
    carry_style: SpanStyle,
}

impl AnsiStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// Normalises escapes; returns text whose only escapes are SGR (`ESC[…m`). Erasing the line and
    /// moving to its first column become a carriage return; a sequence cut off at the end waits for
    /// the next chunk.
    pub fn clean(&mut self, text: &str) -> String {
        let mut s = std::mem::take(&mut self.carry);
        s.push_str(text);
        if let Some(cut) = INCOMPLETE.find(&s).filter(|cut| !cut.is_empty()) {
            let start = cut.start();
            self.carry = s[start..].to_string();
            s.truncate(start);
        }
        let s = OSC.replace_all(&s, "");
        let s = NON_CSI.replace_all(&s, "");
        let s = CSI.replace_all(&s, |sequence: &Captures| {
            let (params, last) = (&sequence[1], &sequence[3]);
            match last {
                "m" => sequence[0].to_string(),
                "K" if matches!(params, "1" | "2") => "\r".to_string(),
                "G" if matches!(params, "" | "0" | "1") => "\r".to_string(),
                _ => String::new(),
            }
        });
        s.chars()
            .filter(|&c| !matches!(c, '\0'..='\x08' | '\x0b' | '\x0c' | '\x0e'..='\x1a' | '\x1c'..='\x1f' | '\x7f'))
            .collect()
    }

    /// Spans of text already passed through [`Self::clean`].
    pub fn spans(&mut self, text: &str) -> Vec<Span> {
        let mut out = Vec::new();
        let mut chunks = text.split("\x1b[");
        // The text before the first escape continues the previous style.
        let mut style = self.carry_style;
        if let Some(first) = chunks.next().filter(|first| !first.is_empty()) {
            out.push(Span { text: first.to_string(), style });
        }
        for chunk in chunks {
            let content = match SEQUENCE.captures(chunk) {
                Some(sequence) if sequence[1].is_empty() && &sequence[3] == "m" => {
                    self.rendition.apply(&sequence[2]);
                    style = self.rendition.style();
                    sequence.get(4).map_or("", |rest| rest.as_str())
                }
                // Not a graphic rendition: its text shows unstyled.
                Some(sequence) => {
                    style = SpanStyle::default();
                    sequence.get(4).map_or("", |rest| rest.as_str())
                }
                None => {
                    style = SpanStyle::default();
                    chunk
                }
            };
            if !content.is_empty() {
                out.push(Span { text: content.to_string(), style });
            }
        }
        self.carry_style = style;
        out
    }
}

/// Appends `spans` to `into`, merging a span into the previous one of the same style.
pub fn append_spans(into: &mut Vec<Span>, spans: impl IntoIterator<Item = Span>) {
    for span in spans {
        match into.last_mut() {
            Some(previous) if previous.style == span.style => previous.text.push_str(&span.text),
            _ => into.push(span),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_256_colour_cube_and_grey_ramp() {
        assert_eq!(palette_color(1).css(), "var(--ansi-red)");
        assert_eq!(palette_color(9).css(), "var(--ansi-bright-red)");
        assert_eq!(palette_color(16).css(), "rgb(0, 0, 0)");
        assert_eq!(palette_color(231).css(), "rgb(255, 255, 255)");
        assert_eq!(palette_color(232).css(), "rgb(8, 8, 8)");
    }

    // Not from a spec: a decoration switched on twice stays on after one reset of it, and reversed
    // text defaults its missing colours.
    #[test]
    fn decorations_count_and_reverse_defaults_colours() {
        let mut stream = AnsiStream::new();
        let spans = stream.spans("\x1b[1;1;22ma\x1b[22mb\x1b[7mc\x1b[0;7;34md");
        assert!(spans[0].style.bold);
        assert!(!spans[1].style.bold);
        assert_eq!((spans[2].style.fg, spans[2].style.bg), (Some(AnsiColor::Named(7)), Some(AnsiColor::Named(0))));
        assert_eq!((spans[3].style.fg, spans[3].style.bg), (Some(AnsiColor::Named(4)), Some(AnsiColor::Named(0))));
    }

    // Not from a spec: an invalid extended colour consumes its values without changing the colour.
    #[test]
    fn invalid_extended_colours_are_skipped() {
        let mut stream = AnsiStream::new();
        let spans = stream.spans("\x1b[31;38;5;300;1mx\x1b[38;2;1;2mY");
        assert_eq!(spans[0].style.fg, Some(AnsiColor::Named(1)));
        assert!(spans[0].style.bold);
        assert_eq!(spans[1].style.fg, Some(AnsiColor::Named(1)));
        assert!(spans[1].style.dim);
    }

    // Not from a spec: spans of one style merge when appended.
    #[test]
    fn appending_merges_spans_of_the_same_style() {
        let mut spans = vec![Span::plain("a")];
        let red = SpanStyle { fg: Some(AnsiColor::Named(1)), ..SpanStyle::default() };
        append_spans(&mut spans, [Span::plain("b"), Span { text: "c".into(), style: red }]);
        assert_eq!(spans, [Span::plain("ab"), Span { text: "c".into(), style: red }]);
        assert!(spans[0].style.is_plain());
    }
}
