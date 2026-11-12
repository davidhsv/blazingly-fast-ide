//! Query parsing for the choosers: `File.kt:42`, `File.kt:42:5`, pasted absolute paths
//! (`/root/src/File.kt`), `file://` URLs and Windows paths. Lines and columns in the input are 1-based; the
//! result is 0-based, as the backend and the editor count them.

use ide_core::Pos;
use regex::Regex;
use std::sync::LazyLock;

/// A chooser query split into the text to match and the location to open at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Text to match (location suffix removed).
    pub text: String,
    /// 0-based line, when a `:line` suffix was given.
    pub line: Option<u32>,
    /// 0-based column, when a `:line:col` suffix was given.
    pub col: Option<u32>,
}

/// A trailing `:line`, `:line:col`, `(line)` or `(line,col)`, optionally followed by a colon.
static LOCATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.*?)(?::([0-9]+)(?::([0-9]+))?|\(([0-9]+)(?:[,:]([0-9]+))?\)):?$").expect("the pattern is valid")
});

/// Characters a URI keeps escaped when it is decoded as a whole: they separate its parts.
const RESERVED: &[u8] = b";/?:@&=+$,#";

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).and_then(|digit| u8::try_from(digit).ok())
}

/// The byte a `%XX` escape at the start of `bytes` stands for.
fn escaped_byte(bytes: &[u8]) -> Option<u8> {
    match bytes {
        [b'%', high, low, ..] => Some(hex_value(*high)? * 16 + hex_value(*low)?),
        _ => None,
    }
}

/// `text` with its `%XX` escapes decoded as UTF-8, except those of reserved characters. `None` when an
/// escape is malformed or the escaped bytes are not UTF-8: such text is not a URI and is left alone.
fn decode_uri(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte != b'%' {
            out.push(byte);
            at += 1;
            continue;
        }
        let first = escaped_byte(&bytes[at..])?;
        let width = match first {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return None,
        };
        let escaped = bytes.get(at..at + width * 3)?;
        let decoded: Vec<u8> = escaped.chunks(3).map(escaped_byte).collect::<Option<_>>()?;
        std::str::from_utf8(&decoded).ok()?;
        if RESERVED.contains(&first) {
            out.extend_from_slice(escaped);
        } else {
            out.extend_from_slice(&decoded);
        }
        at += width * 3;
    }
    String::from_utf8(out).ok()
}

/// Whether `text` starts with a drive and a backslash (`C:\`).
fn starts_with_drive(text: &str) -> bool {
    matches!(text.as_bytes(), [letter, b':', b'\\', ..] if letter.is_ascii_alphabetic())
}

/// A 1-based line or column as typed; a number too large to hold reads as the largest one.
fn number(digits: &str) -> u32 {
    digits.parse().unwrap_or(u32::MAX)
}

/// Splits `raw` into the text to match and an optional location. Quotes around the query, a `file://`
/// prefix and Windows separators are removed, and a path below `root` is made relative to it.
pub fn parse_location_query(raw: &str, root: Option<&str>) -> ParsedQuery {
    let mut text = raw.trim().to_owned();
    if ['"', '\''].into_iter().any(|quote| text.starts_with(quote) && text.ends_with(quote)) {
        text = text.get(1..text.len().saturating_sub(1)).unwrap_or("").trim().to_owned();
    }
    if text.get(..7).is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://")) {
        text.replace_range(..7, "");
        if let Some(decoded) = decode_uri(&text) {
            text = decoded;
        }
    }
    if starts_with_drive(&text) || (text.contains('\\') && !text.contains('/')) {
        text = text.replace('\\', "/");
    }

    let mut line = None;
    let mut col = None;
    let location = LOCATION.captures(&text).and_then(|found| {
        let typed_line = number(found.get(2).or_else(|| found.get(4))?.as_str());
        let typed_col = found.get(3).or_else(|| found.get(5)).map(|digits| number(digits.as_str()));
        (typed_line > 0).then(|| (found[1].to_owned(), typed_line, typed_col))
    });
    if let Some((before, typed_line, typed_col)) = location {
        text = before;
        line = Some(typed_line - 1);
        col = typed_col.filter(|&typed| typed > 0).map(|typed| typed - 1);
    }

    if let Some(root) = root.filter(|root| !root.is_empty()) {
        let root = root.trim_end_matches('/');
        if text == root {
            text.clear();
        } else if let Some(below) = text.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')) {
            text = below.to_owned();
        }
    }
    ParsedQuery { text, line, col }
}

/// Where to put the caret when opening a parsed location; `None` when no line was given.
pub fn selection_of(line: Option<u32>, col: Option<u32>) -> Option<Pos> {
    line.map(|line| Pos::new(line, col.unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str, line: Option<u32>, col: Option<u32>) -> ParsedQuery {
        ParsedQuery { text: text.to_owned(), line, col }
    }

    // src/features/search/__tests__/query.test.ts › parseLocationQuery › passes plain text through
    #[test]
    fn passes_plain_text_through() {
        assert_eq!(parse_location_query("OrderController", None), parsed("OrderController", None, None));
    }

    // src/features/search/__tests__/query.test.ts › parseLocationQuery › parses :line and :line:col (1-based in,
    // 0-based out)
    #[test]
    fn parses_line_and_line_col_one_based_in_zero_based_out() {
        assert_eq!(parse_location_query("File.kt:42", None), parsed("File.kt", Some(41), None));
        assert_eq!(parse_location_query("File.kt:42:5", None), parsed("File.kt", Some(41), Some(4)));
        assert_eq!(parse_location_query("src/a.ts:10:", None), parsed("src/a.ts", Some(9), None));
    }

    // src/features/search/__tests__/query.test.ts › parseLocationQuery › parses stack-trace style (line) suffixes
    #[test]
    fn parses_stack_trace_style_line_suffixes() {
        assert_eq!(parse_location_query("Main.java(12)", None), parsed("Main.java", Some(11), None));
        assert_eq!(parse_location_query("lib.rs(3,7)", None), parsed("lib.rs", Some(2), Some(6)));
    }

    // src/features/search/__tests__/query.test.ts › parseLocationQuery › ignores :0 and non-numeric suffixes
    #[test]
    fn ignores_zero_and_non_numeric_suffixes() {
        assert_eq!(parse_location_query("File.kt:0", None).line, None);
        assert_eq!(parse_location_query("a:b", None).text, "a:b");
    }

    // src/features/search/__tests__/query.test.ts › parseLocationQuery › strips the project root from pasted
    // absolute paths
    #[test]
    fn strips_the_project_root_from_pasted_absolute_paths() {
        assert_eq!(
            parse_location_query("/dev/acme/src/Order.kt:3", Some("/dev/acme")),
            parsed("src/Order.kt", Some(2), None)
        );
        assert_eq!(parse_location_query("/elsewhere/x.kt", Some("/dev/acme")).text, "/elsewhere/x.kt");
    }

    // src/features/search/__tests__/query.test.ts › parseLocationQuery › handles quotes, file:// URLs and Windows
    // separators
    #[test]
    fn handles_quotes_file_urls_and_windows_separators() {
        assert_eq!(parse_location_query("\"src/a.ts\"", None).text, "src/a.ts");
        assert_eq!(parse_location_query("file:///dev/acme/My%20File.kt", Some("/dev/acme")).text, "My File.kt");
        assert_eq!(parse_location_query("src\\main\\App.kt", None).text, "src/main/App.kt");
    }

    // src/features/search/__tests__/query.test.ts › parseLocationQuery › selectionOf maps to an editor selection
    #[test]
    fn selection_of_maps_to_an_editor_selection() {
        assert_eq!(selection_of(None, None), None);
        assert_eq!(selection_of(Some(4), None), Some(Pos::new(4, 0)));
        assert_eq!(selection_of(Some(4), Some(2)), Some(Pos::new(4, 2)));
    }

    // Not from a spec: a URL keeps the escapes of reserved characters, and one that does not decode is left
    // as typed.
    #[test]
    fn decodes_urls_as_a_whole_or_not_at_all() {
        assert_eq!(parse_location_query("file:///p/a%2Fb%C3%A9.kt", None).text, "/p/a%2Fbé.kt");
        assert_eq!(parse_location_query("FILE:///p/100%.kt", None).text, "/p/100%.kt");
        assert_eq!(parse_location_query("file:///p/%E9.kt", None).text, "/p/%E9.kt");
    }

    // Not from a spec: a lone quote, a root with a trailing slash, the root itself and an oversized line.
    #[test]
    fn edge_inputs_do_not_panic_and_keep_their_meaning() {
        assert_eq!(parse_location_query("\"", None).text, "");
        assert_eq!(parse_location_query("/dev/acme", Some("/dev/acme/")).text, "");
        assert_eq!(parse_location_query("/dev/acme/a.kt", Some("/dev/acme/")).text, "a.kt");
        assert_eq!(parse_location_query("a.kt:99999999999999999999", None).line, Some(u32::MAX - 1));
        assert_eq!(parse_location_query("C:\\dev\\a.kt:7", None), parsed("C:/dev/a.kt", Some(6), None));
    }
}
