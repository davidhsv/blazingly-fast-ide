//! Locate Duplicates: the preview of a duplicated fragment.

/// A source line of a fragment with its 1-based number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentLine {
    pub no: u32,
    pub text: String,
}

/// The lines of a fragment that starts at the 0-based line `first_line`; `\r\n`, `\r` and `\n` all end a
/// line.
pub fn fragment_lines(text: &str, first_line: u32) -> Vec<FragmentLine> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized.split('\n').zip(first_line + 1..).map(|(text, no)| FragmentLine { no, text: text.to_owned() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_fragment_lines_from_the_fragments_first_line() {
        let lines: Vec<(u32, String)> = fragment_lines("a\r\nb\nc", 9).into_iter().map(|l| (l.no, l.text)).collect();
        assert_eq!(lines, [(10, "a".to_owned()), (11, "b".to_owned()), (12, "c".to_owned())]);
    }
}
