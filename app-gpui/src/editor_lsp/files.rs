//! The names navigation choosers show for their targets.

use super::completion_items::is_word_char;
use crate::editor::buffer;
use crate::find::utf16;

/// Longest literal label before it is shortened with an ellipsis (UTF-16 units).
const MAX_LITERAL_LABEL: u32 = 60;

/// The display name of a navigation target starting at UTF-16 column `col` of `line`: the string
/// literal when the target is one (a Spring `@Value("${key}")` consumer, a quoted configuration
/// key), else the identifier there; `None` when neither.
pub fn target_label(line: &str, col: u32) -> Option<String> {
    let start = buffer::byte_of_col(line, col);
    let mut chars = line[start..].chars();
    if let Some(quote) = chars.next().filter(|c| matches!(c, '"' | '\'' | '`')) {
        let mut end = start + quote.len_utf8();
        while let Some(c) = chars.next() {
            end += c.len_utf8();
            if c == '\\' {
                end += chars.next().map_or(0, char::len_utf8);
            } else if c == quote {
                break;
            }
        }
        let literal = &line[start..end.min(line.len())];
        let len = utf16::len(literal);
        if len > 2 {
            return Some(if len > MAX_LITERAL_LABEL {
                format!("{}…", utf16::slice(literal, 0, MAX_LITERAL_LABEL - 1))
            } else {
                literal.to_owned()
            });
        }
    }
    identifier_at(line, col)
}

/// The identifier at (or starting right after) `col` of `line`.
fn identifier_at(line: &str, col: u32) -> Option<String> {
    let mut at = 0u32;
    let mut chars = line.char_indices().peekable();
    while let Some((byte, c)) = chars.next() {
        let width = c.len_utf16() as u32;
        if !is_word_char(c) {
            at += width;
            continue;
        }
        let begin = at;
        let mut end_byte = byte + c.len_utf8();
        at += width;
        while let Some(&(next_byte, next)) = chars.peek().filter(|(_, next)| is_word_char(*next)) {
            end_byte = next_byte + next.len_utf8();
            at += next.len_utf16() as u32;
            chars.next();
        }
        if at < col {
            continue;
        }
        if begin > col + 1 {
            return None;
        }
        let word = &line[byte..end_byte];
        return (!word.starts_with(char::is_numeric)).then(|| word.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: &str, needle: &str) -> Option<String> {
        let byte = line.find(needle).expect("the needle is in the line");
        target_label(line, utf16::len(&line[..byte]))
    }

    #[test]
    fn labels_literals_and_identifiers() {
        for (line, needle, expected) in [
            // Spring configuration consumers start at the string literal, not a name.
            ("    @Value(\"${app.name}\") private String appName;", "\"${app", "\"${app.name}\""),
            ("    @Value('${app.name}') String appName", "'${app", "'${app.name}'"),
            ("  const key = `app.${env}`;", "`app", "`app.${env}`"),
            ("    String s = \"say \\\"hi\\\"\" + x;", "\"say", "\"say \\\"hi\\\"\""),
            // Unterminated (multi-line) literals end at the line end.
            ("    String block = \"unterminated", "\"unterminated", "\"unterminated"),
            // Ordinary declarations keep the identifier.
            ("    private String appName;", "appName", "appName"),
            ("  public int getTimeOut() { return timeOut; }", "getTimeOut", "getTimeOut"),
            ("  int $inner = 1;", "$inner", "$inner"),
            ("  time-out: 30", "time", "time"),
        ] {
            assert_eq!(at(line, needle).as_deref(), Some(expected), "{line}");
        }
    }

    #[test]
    fn shortens_long_literals() {
        let line = format!("x(\"{}\")", "a".repeat(100));
        let label = target_label(&line, 2).expect("a literal");
        assert_eq!(utf16::len(&label), 60);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn has_no_label_for_empty_literals_numbers_or_blank_positions() {
        assert_eq!(target_label("  x = \"\";", 6), None);
        assert_eq!(target_label("  port: 8080", 8), None);
        assert_eq!(target_label("   ", 1), None);
    }
}
