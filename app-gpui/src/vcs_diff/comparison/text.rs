//! Character classes, whitespace trimming and text equality under a comparison policy, as the IntelliJ
//! Platform defines them (`TrimUtil`, `ComparisonUtil` and `CharacterUtils`; Apache-2.0, Copyright
//! JetBrains s.r.o. and contributors).
//!
//! Texts are UTF-16 code units and every offset counts code units.

use super::lcs::Range;
use regex::Regex;
use std::sync::LazyLock;

/// How whitespace takes part in a comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComparisonPolicy {
    /// Every character counts.
    Default,
    /// The whitespace around each line is ignored.
    Trim,
    /// All whitespace is ignored.
    Ignore,
}

pub(crate) const NEWLINE: u16 = b'\n' as u16;

/// Whitespace for comparison purposes: space, tab and line break only (Strings.isWhiteSpace).
pub(crate) const fn is_space(unit: u16) -> bool {
    matches!(unit, 0x20 | 0x09 | 0x0A)
}

/// ASCII punctuation, except `_`.
pub(crate) const fn is_punctuation(c: u32) -> bool {
    matches!(c, 33..=47 | 58..=64 | 91..=94 | 96 | 123..=126)
}

pub(crate) const fn is_white_space_code_point(c: u32) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A)
}

/// A character words are made of: anything but whitespace and punctuation.
pub(crate) const fn is_alpha(c: u32) -> bool {
    !is_white_space_code_point(c) && !is_punctuation(c)
}

fn property(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a Unicode property class is a valid pattern")
}

static DECIMAL_DIGIT: LazyLock<Regex> = LazyLock::new(|| property(r"\p{Nd}"));
static IDEOGRAPHIC: LazyLock<Regex> = LazyLock::new(|| property(r"\p{Ideographic}"));
static ALPHABETIC: LazyLock<Regex> = LazyLock::new(|| property(r"\p{Alphabetic}"));
static SPACELESS_SCRIPT: LazyLock<Regex> =
    LazyLock::new(|| property(r"[\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Thai}\p{Script=Javanese}]"));

/// Characters that are words on their own: CJK, Thai, symbols, anything outside the BMP.
pub(crate) fn is_continuous_script(c: u32) -> bool {
    if c < 128 {
        return false;
    }
    // A lone surrogate has no properties: it is not a digit, not ideographic and not alphabetic.
    let Some(ch) = char::from_u32(c) else { return true };
    let mut buffer = [0; 4];
    let s: &str = ch.encode_utf8(&mut buffer);
    if DECIMAL_DIGIT.is_match(s) {
        return false;
    }
    if c > 0xFFFF {
        return true;
    }
    if IDEOGRAPHIC.is_match(s) {
        return true;
    }
    if !ALPHABETIC.is_match(s) {
        return true;
    }
    SPACELESS_SCRIPT.is_match(s)
}

/// The code point starting at `offset`: a surrogate pair read as one, a lone surrogate as itself.
pub(crate) fn code_point_at(text: &[u16], offset: usize) -> u32 {
    let unit = u32::from(text[offset]);
    if (0xD800..=0xDBFF).contains(&unit)
        && let Some(low) = text.get(offset + 1).copied().map(u32::from)
        && (0xDC00..=0xDFFF).contains(&low)
    {
        return 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
    }
    unit
}

/// How many code units a code point takes.
pub(crate) const fn char_count(code_point: u32) -> usize {
    if code_point >= 0x10000 { 2 } else { 1 }
}

/// The code units in `[from, to)`, both clamped to the text.
pub(crate) fn slice(text: &[u16], from: usize, to: usize) -> &[u16] {
    let to = to.min(text.len());
    &text[from.min(to)..to]
}

pub(crate) fn trim_start(text: &[u16], mut start: usize, end: usize) -> usize {
    while start < end && is_space(text[start]) {
        start += 1;
    }
    start
}

pub(crate) fn trim_end(text: &[u16], start: usize, mut end: usize) -> usize {
    while start < end && is_space(text[end - 1]) {
        end -= 1;
    }
    end
}

/// Both sides of a range without their surrounding whitespace.
pub(crate) fn trim_range(text1: &[u16], text2: &[u16], r: Range) -> Range {
    let start1 = trim_start(text1, r.start1, r.end1);
    let start2 = trim_start(text2, r.start2, r.end2);
    Range::new(start1, trim_end(text1, start1, r.end1), start2, trim_end(text2, start2, r.end2))
}

/// Shrinks a range by the equal whitespace at both ends (TrimUtil.expandWhitespaces).
pub(crate) fn expand_whitespaces(text1: &[u16], text2: &[u16], r: Range) -> Range {
    let forward = expand_whitespaces_forward(text1, text2, r);
    let start1 = r.start1 + forward;
    let start2 = r.start2 + forward;
    let backward = expand_whitespaces_backward(text1, text2, Range::new(start1, r.end1, start2, r.end2));
    Range::new(start1, r.end1 - backward, start2, r.end2 - backward)
}

/// How many code units of equal whitespace both sides of `r` start with.
pub(crate) fn expand_whitespaces_forward(text1: &[u16], text2: &[u16], r: Range) -> usize {
    let Range { mut start1, end1, mut start2, end2 } = r;
    while start1 < end1 && start2 < end2 && text1[start1] == text2[start2] && is_space(text1[start1]) {
        start1 += 1;
        start2 += 1;
    }
    start1 - r.start1
}

/// How many code units of equal whitespace both sides of `r` end with.
pub(crate) fn expand_whitespaces_backward(text1: &[u16], text2: &[u16], r: Range) -> usize {
    let Range { start1, mut end1, start2, mut end2 } = r;
    while start1 < end1 && start2 < end2 && text1[end1 - 1] == text2[end2 - 1] && is_space(text1[end1 - 1]) {
        end1 -= 1;
        end2 -= 1;
    }
    r.end1 - end1
}

/// The text without whitespace (StringsKmp.equalsIgnoreWhitespaces compares these).
pub fn strip_spaces(text: &[u16]) -> Vec<u16> {
    significant(text).collect()
}

/// The code units of a text that are not whitespace.
fn significant(text: &[u16]) -> impl Iterator<Item = u16> {
    text.iter().copied().filter(|unit| !is_space(*unit))
}

/// Whether the texts are equal once their whitespace is removed.
pub(crate) fn equals_ignore_whitespaces(text1: &[u16], text2: &[u16]) -> bool {
    significant(text1).eq(significant(text2))
}

/// The text without surrounding whitespace.
pub(crate) fn trim_spaces(text: &[u16]) -> &[u16] {
    let start = trim_start(text, 0, text.len());
    &text[start..trim_end(text, start, text.len())]
}

/// Texts equal under a policy, every line being trimmed for [`ComparisonPolicy::Trim`]
/// (ComparisonUtil.isEqualTexts).
pub(crate) fn is_equal_texts(text1: &[u16], text2: &[u16], policy: ComparisonPolicy) -> bool {
    match policy {
        ComparisonPolicy::Default => text1 == text2,
        ComparisonPolicy::Ignore => equals_ignore_whitespaces(text1, text2),
        ComparisonPolicy::Trim => {
            let lines1 = text1.split(|unit| *unit == NEWLINE).map(trim_spaces);
            let lines2 = text2.split(|unit| *unit == NEWLINE).map(trim_spaces);
            lines1.eq(lines2)
        }
    }
}

pub(crate) fn is_equals_range(text1: &[u16], text2: &[u16], r: Range) -> bool {
    slice(text1, r.start1, r.end1) == slice(text2, r.start2, r.end2)
}

pub(crate) fn is_equals_ignore_whitespaces_range(text1: &[u16], text2: &[u16], r: Range) -> bool {
    equals_ignore_whitespaces(slice(text1, r.start1, r.end1), slice(text2, r.start2, r.end2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    // Not from a spec: letters and digits of spaced scripts join into words, the rest stand alone.
    #[test]
    fn tells_the_characters_that_are_words_on_their_own() {
        for joined in ['a', 'é', 'я', '٣', '한'] {
            assert!(!is_continuous_script(u32::from(joined)), "{joined}");
        }
        for alone in ['漢', 'あ', 'カ', 'ก', '€', '😂'] {
            assert!(is_continuous_script(u32::from(alone)), "{alone}");
        }
        assert!(is_continuous_script(0xD83D));
    }

    // Not from a spec: only `_` is left out of the ASCII punctuation, and only space, tab and line
    // break are whitespace.
    #[test]
    fn classifies_ascii_characters() {
        let punctuation: String = (0_u8..128).map(char::from).filter(|c| is_punctuation(u32::from(*c))).collect();
        assert_eq!(punctuation, "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~");
        assert!(is_alpha(u32::from('_')) && is_alpha(u32::from('\r')) && !is_alpha(u32::from('\t')));
    }

    // Not from a spec: a surrogate pair is one code point of two units, a lone surrogate is itself.
    #[test]
    fn reads_code_points_from_code_units() {
        let text = [0x61, 0xD83D, 0xDE02, 0xD83D, 0x62, 0xDE02];
        assert_eq!(code_point_at(&text, 0), 0x61);
        assert_eq!(code_point_at(&text, 1), 0x1F602);
        assert_eq!(char_count(code_point_at(&text, 1)), 2);
        assert_eq!(code_point_at(&text, 2), 0xDE02);
        assert_eq!(code_point_at(&text, 3), 0xD83D);
        assert_eq!(code_point_at(&text, 5), 0xDE02);
    }

    // Not from a spec: texts compare by policy, "trim" line by line.
    #[test]
    fn compares_texts_by_policy() {
        let (a, b) = (units(" x \n y"), units("x\ny "));
        assert!(!is_equal_texts(&a, &b, ComparisonPolicy::Default));
        assert!(is_equal_texts(&a, &b, ComparisonPolicy::Trim));
        assert!(is_equal_texts(&a, &b, ComparisonPolicy::Ignore));
        let joined = units("x y");
        assert!(!is_equal_texts(&a, &joined, ComparisonPolicy::Trim));
        assert!(!is_equal_texts(&units("x y"), &units("xy"), ComparisonPolicy::Trim));
        assert!(is_equal_texts(&units("x y"), &units("xy"), ComparisonPolicy::Ignore));
        assert_eq!(strip_spaces(&units(" a\tb\nc\r ")), units("abc\r"));
        assert_eq!(trim_spaces(&units("\t a b \n")), units("a b"));
    }
}
