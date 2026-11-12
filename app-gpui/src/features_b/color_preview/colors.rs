//! Color literals of a line for the gutter's color preview: `#rgb` / `#rrggbb` (with alpha),
//! `rgb()` / `rgba()` / `hsl()` / `hsla()`, and CSS named colors in style sheets; and writing a
//! picked color back in the literal's own format.

use crate::database::model::js_number_string;
use crate::find::utf16;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFormat {
    Hex,
    Rgb,
    Hsl,
    Name,
}

/// A color literal on a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorMatch {
    /// UTF-16 offsets in the line.
    pub from: u32,
    pub to: u32,
    pub text: String,
    pub format: ColorFormat,
}

/// Red, green, blue 0–255 and alpha 0–1.
pub type Rgba = [f64; 4];

/// Style sheet languages: named colors and unquoted hex values are colors there.
const STYLE_SHEETS: &[&str] = &["css", "scss", "sass", "less", "stylus", "postcss"];
/// Markup that embeds style sheets (`<style>`, `style="…"`): unquoted hex values too.
const MARKUP: &[&str] = &["html", "vue", "svelte", "astro", "xml", "xhtml"];
/// Prose: `#123` is an issue, `#fab` a heading or a tag.
const PROSE: &[&str] = &["markdown", "plaintext", "text", "restructuredtext", "asciidoc", "git-commit"];
/// Global keywords and words that are valid colors but say nothing about one.
const NOT_COLORS: &[&str] = &["inherit", "initial", "unset", "revert", "currentcolor", "transparent"];

const NUMBER: &str = r"[-+]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:e[-+]?[0-9]+)?";

/// A color function at the start of the text.
static FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?:rgba?|hsla?)\(\s*{n}(?:deg|%)?(?:\s*[,\s]\s*{n}%?){{2}}(?:\s*[,/]\s*{n}%?)?\s*\)",
        n = NUMBER
    ))
    .expect("the pattern is valid")
});
static NUMBERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(NUMBER).expect("the pattern is valid"));
/// The text before a property value (`color: `, `border: 1px solid `).
static VALUE_POSITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\s*(?:\S+\s+)*$").expect("the pattern is valid"));
static COMPUTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^rgba?\(\s*([0-9.]+)[,\s]+([0-9.]+)[,\s]+([0-9.]+)(?:\s*[,/]\s*([0-9.]+%?))?\s*\)$")
        .expect("the pattern is valid")
});

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Before a name, these make it part of a selector, variable or longer word (`.red`, `$white`, `off-white`).
fn is_name_prefix(c: char) -> bool {
    is_word(c) || matches!(c, '.' | '#' | '$' | '@' | '-')
}

/// The length of the hex color at the start of `rest` (which starts with `#`): 8, 6, 4 or 3
/// digits, tried in that order, not followed by a word character or `-`.
fn hex_len(rest: &str) -> Option<usize> {
    let digits = rest[1..].bytes().take_while(u8::is_ascii_hexdigit).count();
    [8, 6, 4, 3].into_iter().filter(|len| *len <= digits).find_map(|len| {
        let next = rest[1 + len..].chars().next();
        next.is_none_or(|c| !is_word(c) && c != '-').then_some(1 + len)
    })
}

/// The length of the word of 3 to 20 letters at the start of `rest`, not followed by a word
/// character, `(` or `-`.
fn name_len(rest: &str) -> Option<usize> {
    let run = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    let next = rest[run..].chars().next();
    let free = next.is_none_or(|c| !is_word(c) && c != '(' && c != '-');
    ((3..=20).contains(&run) && free).then_some(run)
}

/// The length of the color candidate at byte `i` of `line`, in the order a color function, a
/// hex value, a name.
fn candidate_len(line: &str, i: usize) -> Option<usize> {
    let rest = &line[i..];
    let after_word = line[..i].chars().next_back().is_some_and(is_word);
    if !after_word && let Some(m) = FUNCTION.find(rest) {
        return Some(m.end());
    }
    if rest.starts_with('#')
        && let Some(len) = hex_len(rest)
    {
        return Some(len);
    }
    name_len(rest)
}

/// The colors on `line` of a document in `language`. Outside style sheets and markup, only
/// quoted hex values (`"#1b766c"`) and color functions count: `#add` may be a comment, `#123`
/// an issue. `is_color_name` says whether a word is a CSS color name.
pub fn find_colors(line: &str, language: &str, is_color_name: impl Fn(&str) -> bool) -> Vec<ColorMatch> {
    if PROSE.contains(&language) {
        return Vec::new();
    }
    let sheet = STYLE_SHEETS.contains(&language);
    let loose_hex = sheet || MARKUP.contains(&language);
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let Some(len) = candidate_len(line, i) else {
            i += line[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        let text = &line[i..i + len];
        let before = &line[..i];
        let format = if text.starts_with('#') {
            ColorFormat::Hex
        } else if text.contains('(') {
            if text.starts_with(['r', 'R']) { ColorFormat::Rgb } else { ColorFormat::Hsl }
        } else {
            ColorFormat::Name
        };
        let keep = match format {
            // A property value (`color: red`), not a property, selector or at-rule.
            ColorFormat::Name => {
                sheet
                    && before.chars().next_back().is_none_or(|c| !is_name_prefix(c))
                    && !NOT_COLORS.contains(&text.to_lowercase().as_str())
                    && VALUE_POSITION.is_match(before)
                    && is_color_name(text)
            }
            ColorFormat::Hex => loose_hex || before.ends_with(['"', '\'', '`']),
            ColorFormat::Rgb | ColorFormat::Hsl => true,
        };
        if keep {
            let from = utf16::len(before);
            out.push(ColorMatch { from, to: from + utf16::len(text), text: text.to_owned(), format });
        }
        i += len;
    }
    out
}

fn hex2(v: f64) -> String {
    format!("{:02x}", v.clamp(0.0, 255.0).round() as u32)
}

/// `v` with at most `digits` decimals, as JavaScript writes `Number(v.toFixed(digits))`.
fn round(v: f64, digits: i32) -> String {
    let scale = 10f64.powi(digits);
    js_number_string((v * scale).round() / scale)
}

/// `#rrggbb` of `rgba`, as the system color picker takes it.
pub fn to_hex6(rgba: Rgba) -> String {
    format!("#{}{}{}", hex2(rgba[0]), hex2(rgba[1]), hex2(rgba[2]))
}

/// `#rrggbb` → its components with `alpha`.
pub fn from_hex6(hex: &str, alpha: f64) -> Option<Rgba> {
    let digits = hex.strip_prefix('#').filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok().map(f64::from);
    Some([channel(0)?, channel(2)?, channel(4)?, alpha])
}

/// A number as JavaScript's `Number()` reads the digits and dots the patterns allow (`NaN` when
/// they are no number).
fn js_number(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

/// Components of a computed color (`rgb(1, 2, 3)` / `rgba(1, 2, 3, 0.5)`); `None` for other spaces.
pub fn parse_computed(color: &str) -> Option<Rgba> {
    let m = COMPUTED.captures(color.trim())?;
    let a = match m.get(4).map(|a| a.as_str()) {
        None => 1.0,
        Some(a) => match a.strip_suffix('%') {
            Some(percent) => js_number(percent) / 100.0,
            None => js_number(a),
        },
    };
    Some([js_number(&m[1]), js_number(&m[2]), js_number(&m[3]), a])
}

fn to_hsl(rgba: Rgba) -> [f64; 3] {
    let (r, g, b) = (rgba[0] / 255.0, rgba[1] / 255.0, rgba[2] / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return [0.0, 0.0, l * 100.0];
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d) % 6.0
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [(h * 60.0 + 360.0) % 360.0, s * 100.0, l * 100.0]
}

/// `#aabbcc` → `#abc` when every channel's two digits are the same.
fn shorten(hex: &str) -> Option<String> {
    let digits = hex.strip_prefix('#')?.as_bytes();
    let short = !digits.is_empty() && digits.chunks(2).all(|pair| pair.len() == 2 && pair[0] == pair[1]);
    short.then(|| format!("#{}", digits.chunks(2).map(|pair| char::from(pair[0])).collect::<String>()))
}

/// `rgba` written in the original's format: hex keeps its case and whether it had alpha, a
/// function keeps its name and separators; a named color becomes hex.
pub fn format_color(rgba: Rgba, original: &ColorMatch) -> String {
    let alpha = rgba[3];
    let has_alpha = alpha < 1.0;
    let text = original.text.as_str();
    if matches!(original.format, ColorFormat::Hex | ColorFormat::Name) {
        let short = matches!(text.len(), 4 | 5);
        let had_alpha = original.format == ColorFormat::Hex && matches!(text.len(), 5 | 9);
        let mut hex = to_hex6(rgba);
        if had_alpha || has_alpha {
            hex.push_str(&hex2(alpha * 255.0));
        }
        // `#abc` stays short when the new color has a short form.
        if short && let Some(shorter) = shorten(&hex) {
            hex = shorter;
        }
        return if text.bytes().any(|b| (b'A'..=b'F').contains(&b)) { hex.to_uppercase() } else { hex };
    }
    let name: String = text.chars().take_while(|c| is_word(*c)).collect();
    let commas = text.contains(',');
    // The original's fourth component is its alpha.
    let with_alpha = has_alpha || NUMBERS.find_iter(text).count() > 3;
    let parts: Vec<String> = if original.format == ColorFormat::Rgb {
        rgba[..3].iter().map(|v| js_number_string(v.clamp(0.0, 255.0).round())).collect()
    } else {
        let [h, s, l] = to_hsl(rgba);
        vec![round(h, 0), format!("{}%", round(s, 1)), format!("{}%", round(l, 1))]
    };
    let a = round(alpha, 2);
    if commas {
        let alpha = if with_alpha { format!(", {a}") } else { String::new() };
        format!("{name}({}{alpha})", parts.join(", "))
    } else {
        let alpha = if with_alpha { format!(" / {a}") } else { String::new() };
        format!("{name}({}{alpha})", parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_name(word: &str) -> bool {
        ["red", "rebeccapurple", "white"].contains(&word.to_lowercase().as_str())
    }

    fn texts(line: &str, language: &str) -> Vec<String> {
        find_colors(line, language, is_name).into_iter().map(|c| c.text).collect()
    }

    fn color(text: &str, format: ColorFormat) -> ColorMatch {
        ColorMatch { from: 0, to: utf16::len(text), text: text.to_owned(), format }
    }

    #[test]
    fn finds_every_format_in_style_sheets() {
        assert_eq!(texts("  background-color: #1b766c;", "css"), ["#1b766c"]);
        assert_eq!(texts("  border: 1px solid #FFF8;", "scss"), ["#FFF8"]);
        assert_eq!(
            texts("  color: rgba(0, 0, 0, .5); fill: hsl(210deg 40% 50% / 0.3);", "css"),
            ["rgba(0, 0, 0, .5)", "hsl(210deg 40% 50% / 0.3)"]
        );
        assert_eq!(texts("  color: red; outline: 2px solid RebeccaPurple;", "less"), ["red", "RebeccaPurple"]);
        let found = find_colors("a { color: #abc }", "css", is_name);
        let first = &found[0];
        assert_eq!((first.from, first.to, first.format), (11, 15, ColorFormat::Hex));
    }

    #[test]
    fn skips_names_that_are_no_property_value_keywords_and_non_colors() {
        assert_eq!(texts(".red, #main { white-space: nowrap; color: inherit }", "css"), Vec::<String>::new());
        assert_eq!(texts("@media screen { }", "css"), Vec::<String>::new());
        assert_eq!(texts("  color: $white; background: var(--off-white) off-white;", "scss"), Vec::<String>::new());
        assert_eq!(texts("  color: #12345;", "css"), Vec::<String>::new());
    }

    #[test]
    fn takes_only_quoted_hex_values_in_code_and_no_names() {
        assert_eq!(texts(r##"const accent = "#1b766c"; // see #123 and #add"##, "typescript"), ["#1b766c"]);
        assert_eq!(texts("color = 'rgb(255, 0, 0)'  # red", "python"), ["rgb(255, 0, 0)"]);
        assert_eq!(texts(r#"<div style="color: #fff">"#, "html"), ["#fff"]);
    }

    #[test]
    fn leaves_prose_alone() {
        assert_eq!(texts("Fixed in #123, color `#fff`", "markdown"), Vec::<String>::new());
    }

    #[test]
    fn keeps_hex_case_length_and_alpha() {
        assert_eq!(format_color([255.0, 0.0, 0.0, 1.0], &color("#abc", ColorFormat::Hex)), "#f00");
        assert_eq!(format_color([18.0, 52.0, 86.0, 1.0], &color("#ABC", ColorFormat::Hex)), "#123456");
        assert_eq!(format_color([18.0, 52.0, 86.0, 1.0], &color("#1B766C", ColorFormat::Hex)), "#123456");
        assert_eq!(format_color([18.0, 52.0, 86.0, 0.5], &color("#1b766c80", ColorFormat::Hex)), "#12345680");
        assert_eq!(format_color([18.0, 52.0, 86.0, 1.0], &color("red", ColorFormat::Name)), "#123456");
    }

    #[test]
    fn keeps_a_functions_name_and_separators() {
        assert_eq!(format_color([1.0, 2.0, 3.0, 1.0], &color("rgb(0, 0, 0)", ColorFormat::Rgb)), "rgb(1, 2, 3)");
        assert_eq!(
            format_color([1.0, 2.0, 3.0, 0.5], &color("rgba(0, 0, 0, .5)", ColorFormat::Rgb)),
            "rgba(1, 2, 3, 0.5)"
        );
        assert_eq!(format_color([1.0, 2.0, 3.0, 1.0], &color("rgb(0 0 0 / 50%)", ColorFormat::Rgb)), "rgb(1 2 3 / 1)");
        assert_eq!(
            format_color([255.0, 0.0, 0.0, 1.0], &color("hsl(0, 0%, 0%)", ColorFormat::Hsl)),
            "hsl(0, 100%, 50%)"
        );
    }

    #[test]
    fn round_trips_the_color_inputs_hex_and_reads_computed_colors() {
        assert_eq!(to_hex6([27.0, 118.0, 108.0, 1.0]), "#1b766c");
        assert_eq!(from_hex6("#1b766c", 0.5), Some([27.0, 118.0, 108.0, 0.5]));
        assert_eq!(parse_computed("rgb(27, 118, 108)"), Some([27.0, 118.0, 108.0, 1.0]));
        assert_eq!(parse_computed("rgba(0, 0, 0, 0.25)"), Some([0.0, 0.0, 0.0, 0.25]));
        assert_eq!(parse_computed("color(display-p3 1 0 0)"), None);
    }
}
