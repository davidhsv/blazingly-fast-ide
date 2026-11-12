//! A document as the diff, blame and marker models see it: UTF-16 code units with a line index.
//!
//! Positions are UTF-16 offsets from the start of the text, as the backend and the text input report
//! them. Lines break at `\n` only and a document always has one line; a trailing `\n` starts an empty
//! last line. Line numbers are 1-based, [`Line::number`] of the first line being 1.

use ide_core::TextChange;
use std::fmt;

/// One line of a [`Doc`]: its 1-based number and the offsets of its text, without the line break.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub number: usize,
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc {
    units: Vec<u16>,
    /// Offset at which each line starts.
    starts: Vec<usize>,
}

const NEWLINE: u16 = b'\n' as u16;

impl Doc {
    pub fn new(text: &str) -> Self {
        Self::from_units(text.encode_utf16().collect())
    }

    pub fn from_units(units: Vec<u16>) -> Self {
        let mut starts = vec![0];
        starts.extend(units.iter().enumerate().filter(|(_, unit)| **unit == NEWLINE).map(|(i, _)| i + 1));
        Self { units, starts }
    }

    /// Length in UTF-16 code units.
    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Number of lines, at least one.
    pub fn lines(&self) -> usize {
        self.starts.len()
    }

    pub fn units(&self) -> &[u16] {
        &self.units
    }

    /// Line `number` (1-based); a number outside the document is the nearest line.
    pub fn line(&self, number: usize) -> Line {
        let number = number.clamp(1, self.starts.len());
        let from = self.starts[number - 1];
        let to = self.starts.get(number).map_or(self.units.len(), |next| next - 1);
        Line { number, from, to }
    }

    /// The line holding `pos`; a position past the end is on the last line.
    pub fn line_at(&self, pos: usize) -> Line {
        let pos = pos.min(self.units.len());
        self.line(self.starts.partition_point(|start| *start <= pos))
    }

    /// The code units in `[from, to)`, both clamped to the document.
    pub fn slice(&self, from: usize, to: usize) -> &[u16] {
        let to = to.min(self.units.len());
        &self.units[from.min(to)..to]
    }

    /// The text in `[from, to)`, both clamped to the document.
    pub fn slice_string(&self, from: usize, to: usize) -> String {
        String::from_utf16_lossy(self.slice(from, to))
    }

    /// The text of `line`, without its line break.
    pub fn line_text(&self, line: Line) -> String {
        self.slice_string(line.from, line.to)
    }

    /// The document with `change` applied.
    pub fn apply(&self, change: &TextChange) -> Self {
        self.replace(change.from as usize, change.to as usize, &change.insert)
    }

    /// The document with `[from, to)` replaced by `insert`.
    pub fn replace(&self, from: usize, to: usize, insert: &str) -> Self {
        let to = to.min(self.units.len());
        let from = from.min(to);
        let mut units = Vec::with_capacity(self.units.len() - (to - from) + insert.len());
        units.extend_from_slice(&self.units[..from]);
        units.extend(insert.encode_utf16());
        units.extend_from_slice(&self.units[to..]);
        Self::from_units(units)
    }
}

impl fmt::Display for Doc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf16_lossy(&self.units))
    }
}

/// Which way a move through an ordered list goes: to the next entry or to the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Next,
    Previous,
}

/// Length of `text` in UTF-16 code units.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// FNV-1a over UTF-16 code units, each folded in whole (32-bit).
pub fn fnv1a_utf16(units: impl IntoIterator<Item = u16>) -> u32 {
    units.into_iter().fold(0x811c_9dc5_u32, |hash, unit| (hash ^ u32::from(unit)).wrapping_mul(0x0100_0193))
}

/// `n` in base 36 with lowercase digits.
pub fn base36(mut n: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = Vec::new();
    loop {
        digits.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.reverse();
    String::from_utf8_lossy(&digits).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: lines break at `\n` only and a trailing break starts an empty last line.
    #[test]
    fn indexes_lines_by_utf16_offsets() {
        let doc = Doc::new("a😀\nbc\n");
        assert_eq!(doc.len(), 7);
        assert_eq!(doc.lines(), 3);
        assert_eq!(doc.line(1), Line { number: 1, from: 0, to: 3 });
        assert_eq!(doc.line(2), Line { number: 2, from: 4, to: 6 });
        assert_eq!(doc.line(3), Line { number: 3, from: 7, to: 7 });
        assert_eq!(doc.line_at(3).number, 1);
        assert_eq!(doc.line_at(4).number, 2);
        assert_eq!(doc.line_at(7).number, 3);
        assert_eq!(doc.line_at(99).number, 3);
        assert_eq!(doc.line_text(doc.line(1)), "a😀");
        assert_eq!(Doc::new("").lines(), 1);
    }

    // Not from a spec: a replacement rebuilds the line index.
    #[test]
    fn replaces_a_range() {
        let doc = Doc::new("one\ntwo").replace(4, 7, "2\n3");
        assert_eq!(doc.to_string(), "one\n2\n3");
        assert_eq!(doc.lines(), 3);
    }

    // Not from a spec: the hash and its base-36 form match JavaScript's `(h >>> 0).toString(36)`.
    #[test]
    fn hashes_code_units_and_prints_base36() {
        assert_eq!(fnv1a_utf16("".encode_utf16()), 0x811c_9dc5);
        assert_eq!(fnv1a_utf16("a".encode_utf16()), 0xe40c_292c);
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(u32::MAX), "1z141z3");
    }
}
