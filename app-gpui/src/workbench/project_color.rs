//! Project identity, IDEA style: a colour slot (1–9, like IDEA's `RecentProject.ColorN`) derived
//! from the project name, and the initials shown in the title bar's project widget.

use crate::theme::tokens::{Color, Theme, parse_color};

pub const PROJECT_COLOR_COUNT: u32 = 9;

/// The custom property carrying a project's colour (avatar, gradient start).
pub const PROJECT_COLOR_VAR: &str = "--project-color";
/// The custom property carrying the gradient's end colour.
pub const PROJECT_COLOR_END_VAR: &str = "--project-color-end";

/// The tokens a slot falls back to (start, end) in themes without `--project-color-N(-end)`:
/// they keep the hues of the slots distinct.
const FALLBACK: [(&str, &str); PROJECT_COLOR_COUNT as usize] = [
    ("syn-number", "danger"),
    ("warning", "syn-type"),
    ("success", "vcs-added"),
    ("info", "link"),
    ("accent", "syn-annotation"),
    ("danger", "syn-annotation"),
    ("syn-annotation", "accent"),
    ("success", "info"),
    ("vcs-added", "success"),
];

/// The separators a name is split into words by, one class at a time.
const WORD_SEPARATORS: [&[char]; 6] = [&[' '], &[','], &['-'], &['_'], &['.'], &['`', '\'', '"']];

/// Stable 1-based colour slot for a project name.
pub fn project_color_index(name: &str) -> u32 {
    // A 32-bit string hash over UTF-16 code units, wrapping on overflow.
    let hash = name.encode_utf16().fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)));
    hash.unsigned_abs() % PROJECT_COLOR_COUNT + 1
}

/// IDEA's `AvatarUtils.initials`: two camel-case capitals, else the first letters of the first
/// and last words, else one letter. "py-demo" → "PD", "MyProject" → "MP", "idea" → "I".
pub fn project_initials(name: &str) -> String {
    let text = name.trim();
    let camel = camel_initials(text);
    if camel.encode_utf16().count() == 2 {
        return camel;
    }
    for separators in WORD_SEPARATORS {
        let words: Vec<&str> = text.split(separators).filter(|word| !word.is_empty()).collect();
        if let [first, .., last] = words.as_slice() {
            return first.chars().take(1).chain(last.chars().take(1)).collect::<String>().to_uppercase();
        }
    }
    if camel.is_empty() { "?".to_owned() } else { camel }
}

/// First letter plus the next capital of the leading word: "MyProject" → "MP", "idea" → "I".
fn camel_initials(text: &str) -> String {
    let mut word = text.chars().skip_while(|c| !c.is_alphabetic()).take_while(|c| c.is_alphabetic() || c.is_numeric());
    let Some(first) = word.next() else { return String::new() };
    // A capital is whatever lower-casing changes: title-case letters count too.
    let capital = word.find(|c| !c.to_lowercase().eq(std::iter::once(*c)));
    let mut initials = String::from(first);
    initials.extend(capital);
    initials.to_uppercase()
}

/// The slot of `name` as CSS: [`PROJECT_COLOR_VAR`] and [`PROJECT_COLOR_END_VAR`] with their
/// values. Theme tokens `--project-color-N(-end)` win when the theme defines them.
pub fn project_color_vars(name: &str) -> [(&'static str, String); 2] {
    let slot = project_color_index(name);
    let (start, end) = fallback(slot);
    [
        (PROJECT_COLOR_VAR, format!("var(--project-color-{slot}, var(--{start}))")),
        (PROJECT_COLOR_END_VAR, format!("var(--project-color-{slot}-end, var(--{end}))")),
    ]
}

/// The colours of a project in one theme; `None` where the theme resolves to no colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectColors {
    /// Avatar colour and gradient start.
    pub start: Option<Color>,
    /// Gradient end.
    pub end: Option<Color>,
}

/// What [`project_color_vars`] computes to in `theme`.
pub fn project_colors(name: &str, theme: &Theme) -> ProjectColors {
    let slot = project_color_index(name);
    let (start, end) = fallback(slot);
    ProjectColors {
        start: slot_color(theme, &format!("project-color-{slot}"), start),
        end: slot_color(theme, &format!("project-color-{slot}-end"), end),
    }
}

fn fallback(slot: u32) -> (&'static str, &'static str) {
    FALLBACK[(slot as usize - 1) % FALLBACK.len()]
}

/// `var(--token, var(--fallback))` as a colour: the fallback applies only when the token is not
/// defined, not when it holds something that is no colour.
fn slot_color(theme: &Theme, token: &str, fallback: &str) -> Option<Color> {
    match theme.resolved(token) {
        Some(value) => parse_color(&value),
        None => theme.color(fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tokens::{ThemeAttributes, Tokens};

    // src/workbench/projectColor.test.ts › projectInitials › follows IDEA's AvatarUtils.initials
    #[test]
    fn follows_ideas_avatar_utils_initials() {
        assert_eq!(project_initials("idea"), "I");
        assert_eq!(project_initials("MyProject"), "MP");
        assert_eq!(project_initials("OpenIDEA"), "OI");
        assert_eq!(project_initials("py-demo"), "PD");
        assert_eq!(project_initials("web demo"), "WD");
        assert_eq!(project_initials("a.b.c"), "AC");
        assert_eq!(project_initials("123abc"), "A");
        assert_eq!(project_initials(""), "?");
    }

    #[test]
    fn colour_slots_are_stable_and_wrap_like_a_32_bit_hash() {
        assert_eq!(project_color_index(""), 1);
        assert_eq!(project_color_index("a"), 8);
        assert_eq!(project_color_index("idea"), 2);
        assert_eq!(project_color_index("openidea"), 4);
        assert_eq!(project_color_index("py-demo"), 7);
        assert_eq!(project_color_index("a-very-long-project-name-that-overflows"), 4);
        assert_eq!(project_color_index("日本語プロジェクト"), 4);
        assert_eq!(project_color_index("😀 emoji project"), 1);
    }

    #[test]
    fn slot_colours_come_from_the_theme_then_from_the_fallback_tokens() {
        assert_eq!(
            project_color_vars("idea"),
            [
                ("--project-color", "var(--project-color-2, var(--warning))".to_owned()),
                ("--project-color-end", "var(--project-color-2-end, var(--syn-type))".to_owned()),
            ]
        );
        let tokens = Tokens::parse(
            ":root { --warning: #b08b14; --syn-type: #112233; --project-color-2-end: #bb7f19; --project-color-8: 12px; \
             --success: #00ff00; }",
        );
        let theme = tokens.theme(&ThemeAttributes::dark());
        // Slot 2: no `--project-color-2`, so the fallback; the theme's own end colour wins.
        assert_eq!(
            project_colors("idea", &theme),
            ProjectColors { start: Some(Color::rgb(0xb0, 0x8b, 0x14)), end: Some(Color::rgb(0xbb, 0x7f, 0x19)) }
        );
        // Slot 8: a defined token that is no colour does not fall back; the end has no token at all.
        assert_eq!(project_colors("a", &theme), ProjectColors { start: None, end: None });
    }
}
