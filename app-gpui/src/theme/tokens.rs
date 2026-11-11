//! Design tokens: the custom properties of `src/styles/tokens.css`, cascaded and resolved the
//! way the webview's CSS engine does it.
//!
//! The stylesheet stays the single source of truth for colours and metrics. A [`Theme`] is the
//! cascade for one set of root attributes (`data-theme="light"`, `data-color-theme="one-dark"`,
//! `data-compact`…); values are resolved through `var()`, `color-mix()`, `rgb()` and `hsl()`.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// The token stylesheet, embedded as written.
pub const TOKENS_CSS: &str = include_str!("../../../src/styles/tokens.css");

/// An sRGB colour with straight (non-premultiplied) alpha, each channel in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    /// `#rrggbb` (or `#rrggbbaa` when not opaque), as the stylesheet writes colours.
    pub fn to_hex(self) -> String {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let (r, g, b, a) = (byte(self.r), byte(self.g), byte(self.b), byte(self.a));
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }

    /// Relative luminance (WCAG 2), ignoring alpha.
    pub fn luminance(self) -> f32 {
        let linear = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// `color-mix(in srgb, self p, other q)` with the weights already normalized to sum 1.
    fn mix(self, weight: f32, other: Color, other_weight: f32, alpha_scale: f32) -> Color {
        let alpha = self.a * weight + other.a * other_weight;
        if alpha <= 0.0 {
            return Color::TRANSPARENT;
        }
        // Interpolation happens on premultiplied channels.
        let channel = |a: f32, b: f32| (a * self.a * weight + b * other.a * other_weight) / alpha;
        Color {
            r: channel(self.r, other.r),
            g: channel(self.g, other.g),
            b: channel(self.b, other.b),
            a: alpha * alpha_scale,
        }
    }
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == separator && depth == 0 => {
                parts.push(input[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(input[start..].trim());
    parts
}

/// `name(args)` → `args`, when the whole value is that one function call.
fn function_args<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let rest = value.strip_prefix(name)?.trim_start().strip_prefix('(')?;
    let inner = rest.strip_suffix(')')?;
    // Reject `a(b) c(d)`: the first paren must close at the end.
    let mut depth = 1usize;
    for c in inner.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some(inner)
}

fn parse_number(text: &str) -> Option<f32> {
    text.trim().parse::<f32>().ok().filter(|n| n.is_finite())
}

/// `45%` → 0.45.
fn parse_percentage(text: &str) -> Option<f32> {
    parse_number(text.trim().strip_suffix('%')?).map(|n| n / 100.0)
}

/// An alpha value: a number (`0.45`) or a percentage (`45%`).
fn parse_alpha(text: &str) -> Option<f32> {
    parse_percentage(text).or_else(|| parse_number(text)).map(|a| a.clamp(0.0, 1.0))
}

fn parse_hex(hex: &str) -> Option<Color> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|d| d * 17);
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 => (digit(0)?, digit(1)?, digit(2)?, 255),
        4 => (digit(0)?, digit(1)?, digit(2)?, digit(3)?),
        6 => (pair(0)?, pair(2)?, pair(4)?, 255),
        8 => (pair(0)?, pair(2)?, pair(4)?, pair(6)?),
        _ => return None,
    };
    Some(Color { a: a as f32 / 255.0, ..Color::rgb(r, g, b) })
}

/// The components of `rgb()` / `hsl()`: comma-separated, or space-separated with `/ alpha`.
fn components(args: &str) -> (Vec<&str>, Option<&str>) {
    if args.contains(',') {
        let mut parts = split_top_level(args, ',');
        let alpha = (parts.len() == 4).then(|| parts.pop()).flatten();
        return (parts, alpha);
    }
    let (channels, alpha) = match args.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha.trim())),
        None => (args, None),
    };
    (channels.split_whitespace().collect(), alpha)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0) / 360.0;
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let channel = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
}

/// Parses a resolved CSS colour value (no `var()` left in it).
pub fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex(hex);
    }
    match value.to_ascii_lowercase().as_str() {
        "transparent" => return Some(Color::TRANSPARENT),
        "white" => return Some(Color::rgb(255, 255, 255)),
        "black" => return Some(Color::rgb(0, 0, 0)),
        _ => {}
    }
    if let Some(args) = function_args(value, "rgba").or_else(|| function_args(value, "rgb")) {
        let (channels, alpha) = components(args);
        let [r, g, b] = channels.as_slice() else { return None };
        let channel = |text: &str| {
            parse_percentage(text).or_else(|| parse_number(text).map(|n| n / 255.0)).map(|v| v.clamp(0.0, 1.0))
        };
        let a = match alpha {
            Some(alpha) => parse_alpha(alpha)?,
            None => 1.0,
        };
        return Some(Color { r: channel(r)?, g: channel(g)?, b: channel(b)?, a });
    }
    if let Some(args) = function_args(value, "hsla").or_else(|| function_args(value, "hsl")) {
        let (channels, alpha) = components(args);
        let [h, s, l] = channels.as_slice() else { return None };
        let hue = parse_number(h.trim_end_matches("deg"))?;
        let (r, g, b) = hsl_to_rgb(hue, parse_percentage(s)?.clamp(0.0, 1.0), parse_percentage(l)?.clamp(0.0, 1.0));
        let a = match alpha {
            Some(alpha) => parse_alpha(alpha)?,
            None => 1.0,
        };
        return Some(Color { r, g, b, a });
    }
    if let Some(args) = function_args(value, "color-mix") {
        let parts = split_top_level(args, ',');
        let [space, first, second] = parts.as_slice() else { return None };
        if !space.eq_ignore_ascii_case("in srgb") {
            return None;
        }
        // `<color> <percentage>?`, the percentage on either side of the colour.
        let operand = |text: &str| -> Option<(Color, Option<f32>)> {
            let words = split_top_level(text, ' ').into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>();
            match words.as_slice() {
                [color] => Some((parse_color(color)?, None)),
                [a, b] => match (parse_percentage(a), parse_percentage(b)) {
                    (None, Some(p)) => Some((parse_color(a)?, Some(p))),
                    (Some(p), None) => Some((parse_color(b)?, Some(p))),
                    _ => None,
                },
                _ => None,
            }
        };
        let ((a, pa), (b, pb)) = (operand(first)?, operand(second)?);
        let (pa, pb) = match (pa, pb) {
            (None, None) => (0.5, 0.5),
            (Some(pa), None) => (pa, 1.0 - pa),
            (None, Some(pb)) => (1.0 - pb, pb),
            (Some(pa), Some(pb)) => (pa, pb),
        };
        let sum = pa + pb;
        if sum <= 0.0 {
            return None;
        }
        // Weights that sum to less than 100% scale the result's alpha.
        return Some(a.mix(pa / sum, b, pb / sum, sum.min(1.0)));
    }
    None
}

/// What a rule's selector requires of the root element.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Selector {
    /// `html` is a type selector; `:root` a pseudo-class: it wins over `html` whatever the order.
    pseudo_root: bool,
    attributes: Vec<(String, Option<String>)>,
}

impl Selector {
    /// `None` for selectors that do not address the root element (`.cm-editor[…]`).
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (pseudo_root, mut rest) = if let Some(rest) = text.strip_prefix(":root") {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix("html") {
            (false, rest)
        } else {
            return None;
        };
        let mut attributes = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let end = body.find(']')?;
            let attribute = &body[..end];
            attributes.push(match attribute.split_once('=') {
                Some((name, value)) => {
                    (name.trim().to_owned(), Some(value.trim().trim_matches(['"', '\'']).to_owned()))
                }
                None => (attribute.trim().to_owned(), None),
            });
            rest = &body[end + 1..];
        }
        rest.trim().is_empty().then_some(Self { pseudo_root, attributes })
    }

    fn matches(&self, attributes: &ThemeAttributes) -> bool {
        self.attributes.iter().all(|(name, expected)| match (attributes.0.get(name), expected) {
            (Some(_), None) => true,
            (Some(actual), Some(expected)) => actual == expected,
            (None, _) => false,
        })
    }

    /// (classes and attributes, type selectors).
    fn specificity(&self) -> (usize, usize) {
        (self.attributes.len() + usize::from(self.pseudo_root), usize::from(!self.pseudo_root))
    }
}

#[derive(Debug, Clone)]
struct Rule {
    selectors: Vec<Selector>,
    declarations: Vec<(String, String)>,
}

/// The attributes of the root element a theme is computed for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ThemeAttributes(BTreeMap<String, String>);

impl ThemeAttributes {
    /// Islands Dark: the stylesheet's `:root` block.
    pub fn dark() -> Self {
        Self::default()
    }

    /// Islands Light, the default theme.
    pub fn light() -> Self {
        Self::default().with("data-theme", "light")
    }

    pub fn dark_high_contrast() -> Self {
        Self::default().with("data-theme", "dark-hc")
    }

    /// Sets an attribute (`data-compact` with an empty value for presence-only ones).
    pub fn with(mut self, name: &str, value: &str) -> Self {
        self.0.insert(name.to_owned(), value.to_owned());
        self
    }

    pub fn without(mut self, name: &str) -> Self {
        self.0.remove(name);
        self
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    pub fn is_light(&self) -> bool {
        self.get("data-theme") == Some("light")
    }
}

/// The parsed token stylesheet.
#[derive(Debug)]
pub struct Tokens {
    rules: Vec<Rule>,
}

impl Tokens {
    /// The stylesheet the app ships with.
    pub fn builtin() -> &'static Tokens {
        static TOKENS: OnceLock<Tokens> = OnceLock::new();
        TOKENS.get_or_init(|| Tokens::parse(TOKENS_CSS))
    }

    pub fn parse(css: &str) -> Self {
        let mut source = String::with_capacity(css.len());
        let mut rest = css;
        while let Some(start) = rest.find("/*") {
            source.push_str(&rest[..start]);
            rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
        }
        source.push_str(rest);

        let mut rules = Vec::new();
        let mut rest = source.as_str();
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            let selectors: Vec<Selector> =
                split_top_level(&rest[..open], ',').into_iter().filter_map(Selector::parse).collect();
            let declarations = rest[open + 1..open + close]
                .split(';')
                .filter_map(|declaration| declaration.split_once(':'))
                .map(|(name, value)| (name.trim(), value.trim()))
                .filter(|(name, value)| name.starts_with("--") && !value.is_empty())
                .map(|(name, value)| (name[2..].to_owned(), value.split_whitespace().collect::<Vec<_>>().join(" ")))
                .collect::<Vec<_>>();
            if !selectors.is_empty() && !declarations.is_empty() {
                rules.push(Rule { selectors, declarations });
            }
            rest = &rest[open + close + 1..];
        }
        Self { rules }
    }

    /// The cascade for a root element with `attributes`: for each property the declaration of
    /// the most specific matching selector wins, the later one among equals.
    pub fn theme(&self, attributes: &ThemeAttributes) -> Theme {
        let mut winners: HashMap<&str, ((usize, usize), &str)> = HashMap::new();
        for rule in &self.rules {
            let Some(specificity) =
                rule.selectors.iter().filter(|s| s.matches(attributes)).map(Selector::specificity).max()
            else {
                continue;
            };
            for (name, value) in &rule.declarations {
                match winners.get(name.as_str()) {
                    Some((existing, _)) if *existing > specificity => {}
                    _ => {
                        winners.insert(name, (specificity, value));
                    }
                }
            }
        }
        Theme {
            attributes: attributes.clone(),
            values: winners.into_iter().map(|(name, (_, value))| (name.to_owned(), value.to_owned())).collect(),
            overrides: HashMap::new(),
        }
    }

    /// Every custom property the stylesheet declares on the root element.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> =
            self.rules.iter().flat_map(|r| r.declarations.iter().map(|(name, _)| name.as_str())).collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

/// The token values of one theme. Names are written without the leading `--`.
#[derive(Debug, Clone)]
pub struct Theme {
    attributes: ThemeAttributes,
    values: HashMap<String, String>,
    /// Values set at run time (theme plugins, the colour-scheme editor, font settings): they win
    /// over the stylesheet, as inline custom properties on the root element do.
    overrides: HashMap<String, String>,
}

impl Theme {
    pub fn attributes(&self) -> &ThemeAttributes {
        &self.attributes
    }

    /// Sets a property as an inline style on the root would.
    pub fn set(&mut self, name: &str, value: &str) {
        self.overrides.insert(name.trim_start_matches("--").to_owned(), value.trim().to_owned());
    }

    pub fn unset(&mut self, name: &str) {
        self.overrides.remove(name.trim_start_matches("--"));
    }

    /// The declared value, `var()` references included.
    pub fn raw(&self, name: &str) -> Option<&str> {
        self.overrides.get(name).or_else(|| self.values.get(name)).map(String::as_str)
    }

    /// The value with every `var()` substituted; `None` when the property (or one it needs
    /// without a fallback) is not defined, or the references form a cycle.
    pub fn resolved(&self, name: &str) -> Option<String> {
        self.resolve_property(name, &mut Vec::new())
    }

    fn resolve_property(&self, name: &str, stack: &mut Vec<String>) -> Option<String> {
        if stack.iter().any(|n| n == name) {
            return None;
        }
        let raw = self.raw(name)?;
        stack.push(name.to_owned());
        let resolved = self.substitute(raw, stack);
        stack.pop();
        resolved
    }

    fn substitute(&self, value: &str, stack: &mut Vec<String>) -> Option<String> {
        let Some(start) = value.find("var(") else { return Some(value.to_owned()) };
        let body = &value[start + 4..];
        let mut depth = 1usize;
        let end = body.char_indices().find_map(|(i, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(i)
        })?;
        let arguments = &body[..end];
        let (reference, fallback) = match arguments.split_once(',') {
            Some((reference, fallback)) => (reference.trim(), Some(fallback.trim())),
            None => (arguments.trim(), None),
        };
        let replacement = match self.resolve_property(reference.strip_prefix("--")?, stack) {
            Some(value) => value,
            None => self.substitute(fallback?, stack)?,
        };
        let tail = self.substitute(&body[end + 1..], stack)?;
        Some(format!("{}{replacement}{tail}", &value[..start]))
    }

    pub fn color(&self, name: &str) -> Option<Color> {
        parse_color(&self.resolved(name)?)
    }

    /// A length in CSS pixels (`12px`, or a bare number).
    pub fn px(&self, name: &str) -> Option<f32> {
        let value = self.resolved(name)?;
        parse_number(value.strip_suffix("px").unwrap_or(&value))
    }

    /// A unitless number (`--fw-code: 400`, `--line-height: 1.4`).
    pub fn number(&self, name: &str) -> Option<f32> {
        parse_number(&self.resolved(name)?)
    }

    /// The font families of a `font-family` list, quotes removed, in preference order.
    pub fn font_families(&self, name: &str) -> Vec<String> {
        self.resolved(name)
            .map(|value| {
                split_top_level(&value, ',')
                    .into_iter()
                    .map(|f| f.trim_matches(['"', '\'']).to_owned())
                    .filter(|f| !f.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.values.keys().chain(self.overrides.keys().filter(|k| !self.values.contains_key(*k))).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        [(a.r, b.r), (a.g, b.g), (a.b, b.b), (a.a, b.a)].into_iter().all(|(x, y)| (x - y).abs() < 0.003)
    }

    #[test]
    fn parses_colour_syntaxes() {
        assert_eq!(parse_color("#191a1c"), Some(Color::rgb(0x19, 0x1a, 0x1c)));
        assert_eq!(parse_color("#fff"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(parse_color("#00000080").map(|c| (c.a * 255.0).round()), Some(128.0));
        assert!(close(parse_color("rgba(0, 0, 0, 0.45)").unwrap(), Color { a: 0.45, ..Color::rgb(0, 0, 0) }));
        assert!(close(parse_color("rgb(255 128 0 / 50%)").unwrap(), Color { a: 0.5, ..Color::rgb(255, 128, 0) }));
        assert!(close(parse_color("hsl(0, 100%, 50%)").unwrap(), Color::rgb(255, 0, 0)));
        assert!(close(parse_color("hsl(210 50% 40% / 0.5)").unwrap(), Color { a: 0.5, ..Color::rgb(51, 102, 153) }));
        assert_eq!(parse_color("transparent"), Some(Color::TRANSPARENT));
        for invalid in
            ["", "#12", "#gggggg", "rgb(1, 2)", "12px", "color-mix(in oklab, red, blue)", "rgb(0 0 0) rgb(1 1 1)"]
        {
            assert_eq!(parse_color(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn mixes_colours_like_css_color_mix() {
        let mixed = parse_color("color-mix(in srgb, #ff0000 25%, #0000ff)").unwrap();
        assert!(close(mixed, Color { r: 0.25, g: 0.0, b: 0.75, a: 1.0 }));
        assert!(close(
            parse_color("color-mix(in srgb, #ffffff, #000000)").unwrap(),
            Color { r: 0.5, g: 0.5, b: 0.5, a: 1.0 }
        ));
        // Mixing with transparent keeps the colour and scales the alpha.
        assert!(close(
            parse_color("color-mix(in srgb, #ff0000 40%, transparent)").unwrap(),
            Color { r: 1.0, g: 0.0, b: 0.0, a: 0.4 }
        ));
        // Weights below 100% in total scale the alpha.
        assert!(close(
            parse_color("color-mix(in srgb, #ff0000 20%, #ff0000 30%)").unwrap(),
            Color { r: 1.0, g: 0.0, b: 0.0, a: 0.5 }
        ));
        // Nested functions as operands.
        assert!(close(
            parse_color("color-mix(in srgb, rgb(255, 0, 0) 50%, hsl(240, 100%, 50%))").unwrap(),
            Color { r: 0.5, g: 0.0, b: 0.5, a: 1.0 }
        ));
        assert_eq!(Color::rgb(255, 0, 0).to_hex(), "#ff0000");
        assert_eq!(Color { a: 0.5, ..Color::rgb(0, 0, 0) }.to_hex(), "#00000080");
    }

    #[test]
    fn cascades_by_specificity_then_order() {
        let tokens = Tokens::parse(
            r#"
            html { --w: 400; --only-html: 1; }
            :root { --a: #111111; --b: var(--a); --w: 500; --gap: 8px; }
            :root[data-theme="light"] { --a: #eeeeee; }
            :root[data-compact] { --gap: 4px; /* comment; with: punctuation */ }
            :root, :root[data-theme="light"] { --c: color-mix(in srgb, var(--a) 50%, #000000); }
            :root[data-theme="light"][data-compact] { --gap: 2px; }
            :root[data-compact] { --late: var(--missing, 3px); --cycle-a: var(--cycle-b); --cycle-b: var(--cycle-a); }
            .cm-editor[data-language="PHP"] { --a: #ff0000; }
            "#,
        );
        let dark = tokens.theme(&ThemeAttributes::dark());
        assert_eq!(dark.color("a"), Some(Color::rgb(0x11, 0x11, 0x11)));
        assert_eq!(dark.color("b"), dark.color("a"));
        // `:root` is more specific than `html`, even though `html` could come later.
        assert_eq!(dark.number("w"), Some(500.0));
        assert_eq!(dark.number("only-html"), Some(1.0));
        assert_eq!(dark.px("gap"), Some(8.0));

        let light = tokens.theme(&ThemeAttributes::light());
        assert_eq!(light.color("b"), Some(Color::rgb(0xee, 0xee, 0xee)));
        assert!(close(light.color("c").unwrap(), Color::rgb(0x77, 0x77, 0x77)));
        assert_eq!(light.px("gap"), Some(8.0));

        let compact = tokens.theme(&ThemeAttributes::light().with("data-compact", ""));
        assert_eq!(compact.px("gap"), Some(2.0));
        assert_eq!(compact.px("late"), Some(3.0));
        assert_eq!(compact.resolved("cycle-a"), None);
        assert_eq!(tokens.theme(&ThemeAttributes::dark().with("data-compact", "")).px("gap"), Some(4.0));
        assert_eq!(dark.raw("late"), None);
    }

    #[test]
    fn runtime_overrides_win_like_inline_styles() {
        let mut theme = Tokens::builtin().theme(&ThemeAttributes::dark());
        assert_eq!(theme.color("bg-app"), Some(Color::rgb(0x19, 0x1a, 0x1c)));
        theme.set("--bg-app", "#000000");
        assert_eq!(theme.color("bg-app"), Some(Color::rgb(0, 0, 0)));
        theme.unset("bg-app");
        assert_eq!(theme.color("bg-app"), Some(Color::rgb(0x19, 0x1a, 0x1c)));
    }

    #[test]
    fn builtin_themes_define_their_surfaces() {
        let tokens = Tokens::builtin();
        let dark = tokens.theme(&ThemeAttributes::dark());
        let light = tokens.theme(&ThemeAttributes::light());
        assert_eq!(dark.color("bg-app"), Some(Color::rgb(0x19, 0x1a, 0x1c)));
        assert_eq!(light.color("bg-app"), Some(Color::rgb(0xff, 0xff, 0xff)));
        assert_eq!(dark.px("space-2"), Some(8.0));
        assert_eq!(dark.number("fw-code"), Some(400.0));
        assert_eq!(dark.font_families("font-mono").first().map(String::as_str), Some("JetBrains Mono"));
        // Light text is dark, dark text is light.
        assert!(light.color("fg").unwrap().luminance() < 0.2);
        assert!(dark.color("fg").unwrap().luminance() > 0.5);
    }

    /// Every property of every built-in theme resolves, and those that look like colours parse.
    #[test]
    fn every_token_resolves_in_every_builtin_theme() {
        let tokens = Tokens::builtin();
        assert!(tokens.names().len() > 300, "{} tokens parsed", tokens.names().len());
        let variants = [
            ThemeAttributes::dark(),
            ThemeAttributes::light(),
            ThemeAttributes::dark_high_contrast(),
            ThemeAttributes::dark().with("data-color-theme", "one-dark"),
            ThemeAttributes::light().with("data-color-theme", "solarized-light"),
            ThemeAttributes::light()
                .with("data-compact", "")
                .with("data-red-green", "")
                .with("data-contrast-scrollbars", ""),
        ];
        let mut failures = Vec::new();
        for attributes in &variants {
            let theme = tokens.theme(attributes);
            for name in theme.names() {
                let Some(value) = theme.resolved(name) else {
                    failures.push(format!("{attributes:?}: --{name} does not resolve ({:?})", theme.raw(name)));
                    continue;
                };
                let looks_like_color =
                    value.starts_with('#') || ["rgb", "hsl", "color-mix("].iter().any(|f| value.starts_with(f));
                if looks_like_color && parse_color(&value).is_none() {
                    failures.push(format!("{attributes:?}: --{name}: {value} is not a colour the parser accepts"));
                }
            }
        }
        assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
    }
}
