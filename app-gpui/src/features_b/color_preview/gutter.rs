//! Color preview gutter: a swatch on each line with a color literal (the first one), as IDEA's
//! color gutter icons. Clicking it in an editable file opens the system color picker; the picked
//! color replaces the literal in its own format (alpha kept, the picker has none).

use super::colors::{ColorFormat, ColorMatch, Rgba, find_colors, format_color, from_hex6, to_hex6};
use crate::find::utf16;
use regex::Regex;
use std::sync::LazyLock;

/// Longer lines (minified style sheets) are not scanned.
pub const MAX_LINE: u32 = 2000;

/// The CSS named colors (CSS Color 4), as `0xrrggbb`.
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// The components of the CSS named color `word` (any letter case).
pub fn named_color(word: &str) -> Option<Rgba> {
    let word = word.to_ascii_lowercase();
    let (_, rgb) = NAMED_COLORS.iter().find(|(name, _)| *name == word)?;
    let channel = |shift: u32| f64::from((rgb >> shift) & 0xff);
    Some([channel(16), channel(8), channel(0), 1.0])
}

/// Whether `word` is a CSS color name.
pub fn is_color_name(word: &str) -> bool {
    named_color(word).is_some()
}

/// The arguments of a color function, each a number with its unit (`deg`, `%` or none).
static ARGUMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([-+]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:e[-+]?[0-9]+)?)(deg|%)?").expect("the pattern is valid")
});

fn hex_digit(b: u8) -> Option<f64> {
    char::from(b).to_digit(16).map(f64::from)
}

fn parse_hex(digits: &str) -> Option<Rgba> {
    let bytes = digits.as_bytes();
    let channels: Vec<f64> = match bytes.len() {
        3 | 4 => bytes.iter().map(|b| hex_digit(*b).map(|d| d * 17.0)).collect::<Option<_>>()?,
        6 | 8 => {
            bytes.chunks(2).map(|pair| Some(hex_digit(pair[0])? * 16.0 + hex_digit(pair[1])?)).collect::<Option<_>>()?
        }
        _ => return None,
    };
    let alpha = channels.get(3).map_or(1.0, |a| a / 255.0);
    Some([channels[0], channels[1], channels[2], alpha])
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let (h, s, l) = (h.rem_euclid(360.0), (s / 100.0).clamp(0.0, 1.0), (l / 100.0).clamp(0.0, 1.0));
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        (l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)) * 255.0
    };
    [f(0.0), f(8.0), f(4.0)]
}

/// The components of a color literal [`find_colors`] found, as a style sheet reads it.
pub fn rgba_of(color: &ColorMatch) -> Option<Rgba> {
    let text = color.text.as_str();
    match color.format {
        ColorFormat::Hex => parse_hex(text.strip_prefix('#')?),
        ColorFormat::Name => named_color(text),
        ColorFormat::Rgb | ColorFormat::Hsl => {
            let args = &text[text.find('(')? + 1..];
            let values: Vec<(f64, Option<&str>)> = ARGUMENT
                .captures_iter(args)
                .map(|c| (c[1].parse::<f64>().unwrap_or(f64::NAN), c.get(2).map(|unit| unit.as_str())))
                .collect();
            if values.len() < 3 || values.iter().any(|(v, _)| !v.is_finite()) {
                return None;
            }
            let alpha = values.get(3).map_or(1.0, |(v, unit)| if *unit == Some("%") { v / 100.0 } else { *v });
            let [r, g, b] = if color.format == ColorFormat::Rgb {
                let channel = |(v, unit): (f64, Option<&str>)| {
                    let v = if unit == Some("%") { v * 2.55 } else { v };
                    v.clamp(0.0, 255.0)
                };
                [channel(values[0]), channel(values[1]), channel(values[2])]
            } else {
                hsl_to_rgb(values[0].0, values[1].0, values[2].0)
            };
            Some([r, g, b, alpha.clamp(0.0, 1.0)])
        }
    }
}

/// The color whose swatch `line` shows (its first literal) in a document of `language`.
pub fn line_color(line: &str, language: &str) -> Option<ColorMatch> {
    if utf16::len(line) > MAX_LINE {
        return None;
    }
    find_colors(line, language, is_color_name).into_iter().next()
}

/// A replacement of the literal in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorEdit {
    /// UTF-16 document offsets.
    pub from: u32,
    pub to: u32,
    pub insert: String,
}

/// The system color picker open for a literal: each pick replaces it.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorPick {
    /// UTF-16 document offset of the literal.
    pub from: u32,
    original: ColorMatch,
    rgba: Rgba,
    current: String,
}

impl ColorPick {
    /// Opens the picker for `color` of the line starting at document offset `line_from`; `None`
    /// when the literal is no color a style sheet reads.
    pub fn open(line_from: u32, color: ColorMatch) -> Option<Self> {
        let rgba = rgba_of(&color)?;
        Some(Self { from: line_from + color.from, current: color.text.clone(), original: color, rgba })
    }

    /// The picker's initial value (`#rrggbb`).
    pub fn initial_hex(&self) -> String {
        to_hex6(self.rgba)
    }

    /// The document range the literal is expected at now.
    pub fn range(&self) -> (u32, u32) {
        (self.from, self.from + utf16::len(&self.current))
    }

    /// The picker reports `hex`; `found` is the document's text in [`ColorPick::range`]. The
    /// edit to make, or `None` when the literal changed meanwhile or the color is the same.
    pub fn picked(&mut self, hex: &str, found: &str) -> Option<ColorEdit> {
        let next = from_hex6(hex, self.rgba[3])?;
        if found != self.current {
            return None;
        }
        let text = format_color(next, &self.original);
        if text == self.current {
            return None;
        }
        let (from, to) = self.range();
        self.current.clone_from(&text);
        Some(ColorEdit { from, to, insert: text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a literal opens the picker at its color, and a pick replaces it in its
    // own format, once per change, and not when the text moved on.
    #[test]
    fn a_pick_replaces_the_literal_in_its_own_format() {
        let color = line_color("a { color: #abc }", "css").expect("a hex literal");
        let mut pick = ColorPick::open(100, color).expect("a readable color");
        assert_eq!(pick.initial_hex(), "#aabbcc");
        assert_eq!(pick.range(), (111, 115));
        let edit = pick.picked("#ff0000", "#abc").expect("a new color");
        assert_eq!(edit, ColorEdit { from: 111, to: 115, insert: "#f00".to_owned() });
        assert_eq!(pick.picked("#ff0000", "#f00"), None);
        assert_eq!(pick.picked("#00ff00", "#xyz"), None);
        let hsl = line_color("  fill: hsl(120, 100%, 25%);", "css").expect("an hsl literal");
        assert_eq!(rgba_of(&hsl), Some([0.0, 127.5, 0.0, 1.0]));
        let named = line_color("  color: RebeccaPurple;", "css").expect("a named color");
        assert_eq!(rgba_of(&named), Some([102.0, 51.0, 153.0, 1.0]));
    }
}
