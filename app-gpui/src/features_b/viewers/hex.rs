//! The hex view's model: fixed 16-byte rows over pages read from the backend on demand
//! (`ide_vfs::read_range`, base64 data), and a scroll range capped and scaled onto the rows so a
//! file of hundreds of megabytes still scrolls.

use std::collections::HashMap;

pub const BYTES_PER_ROW: u64 = 16;
/// Bytes per backend read (4096 rows).
pub const PAGE_SIZE: u64 = 64 * 1024;
/// The largest scrolled height: past it, the scroll range maps onto the rows proportionally.
pub const MAX_SCROLL_PX: f64 = 8_000_000.0;

/// Decodes standard base64 (padding optional); `None` for anything else.
pub fn decode_base64(data: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    }
    let digits = data.trim_end_matches('=').as_bytes();
    if digits.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= value(*c)? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    Some(out)
}

/// Hex digits of offsets: at least 8, more for files past 4 GiB.
pub fn offset_digits(total: u64) -> usize {
    format!("{:x}", total.saturating_sub(1)).len().max(8)
}

pub fn format_offset(offset: u64, digits: usize) -> String {
    format!("{offset:0digits$X}")
}

pub fn hex_byte(byte: u8) -> String {
    format!("{byte:02X}")
}

/// Printable ASCII as is, everything else as a dot (like IDEA and xxd).
pub fn ascii_char(byte: u8) -> char {
    if (0x20..0x7f).contains(&byte) { char::from(byte) } else { '.' }
}

pub fn row_count(total: u64) -> u64 {
    total.div_ceil(BYTES_PER_ROW)
}

pub fn page_of(offset: u64) -> u64 {
    offset / PAGE_SIZE
}

/// The bytes of `row` from the loaded pages, or `None` while its page is missing. Rows never
/// straddle pages ([`PAGE_SIZE`] is a multiple of the row size).
pub fn row_bytes(pages: &HashMap<u64, Vec<u8>>, row: u64, total: u64) -> Option<&[u8]> {
    let start = row * BYTES_PER_ROW;
    let page = pages.get(&page_of(start))?;
    let from = (start - page_of(start) * PAGE_SIZE) as usize;
    let len = BYTES_PER_ROW.min(total.saturating_sub(start)) as usize;
    page.get(from..(from + len).min(page.len()))
}

/// The pages needed to show rows `first..=last`.
pub fn pages_for_rows(first: u64, last: u64) -> Vec<u64> {
    (page_of(first * BYTES_PER_ROW)..=page_of(last * BYTES_PER_ROW)).collect()
}

/// The scrolled content's height, the first (partly) visible row and its offset above the
/// viewport's top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HexViewport {
    pub spacer: f64,
    pub first: u64,
    pub shift: f64,
}

pub fn hex_viewport(scroll_top: f64, view_height: f64, rows: u64, row_height: f64, max_px: f64) -> HexViewport {
    let full = rows as f64 * row_height;
    let spacer = full.min(max_px);
    let range = (spacer - view_height).max(0.0);
    let virtual_range = (full - view_height).max(0.0);
    let top = if range > 0.0 { scroll_top.max(0.0).min(range) / range * virtual_range } else { 0.0 };
    let first = (top / row_height).floor();
    HexViewport { spacer, first: first as u64, shift: top - first * row_height }
}

/// The `scroll_top` that puts `row` at the top of the viewport.
pub fn scroll_top_for_row(row: u64, view_height: f64, rows: u64, row_height: f64, max_px: f64) -> f64 {
    let full = rows as f64 * row_height;
    let range = (full.min(max_px) - view_height).max(0.0);
    let virtual_range = (full - view_height).max(0.0);
    if virtual_range == 0.0 {
        return 0.0;
    }
    (row as f64 * row_height).min(virtual_range) / virtual_range * range
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_base64_and_formats_bytes() {
        assert_eq!(decode_base64("AAH/"), Some(vec![0, 1, 255]));
        assert_eq!(format_offset(255, 8), "000000FF");
        assert_eq!(offset_digits(16), 8);
        assert_eq!(offset_digits(1 << 36), 9);
        assert_eq!(ascii_char(0x41), 'A');
        assert_eq!(ascii_char(0), '.');
        assert_eq!(ascii_char(0x7f), '.');
        assert_eq!(hex_byte(10), "0A");
    }

    #[test]
    fn slices_rows_out_of_loaded_pages() {
        let total = PAGE_SIZE + 20;
        let mut pages = HashMap::from([(0, (0..PAGE_SIZE).map(|i| (i % 256) as u8).collect::<Vec<u8>>())]);
        assert_eq!(row_count(total), PAGE_SIZE / BYTES_PER_ROW + 2);
        assert_eq!(row_bytes(&pages, 1, total), Some(&(16..32).collect::<Vec<u8>>()[..]));
        assert_eq!(row_bytes(&pages, PAGE_SIZE / BYTES_PER_ROW, total), None);
        pages.insert(1, vec![7; 20]);
        assert_eq!(row_bytes(&pages, PAGE_SIZE / BYTES_PER_ROW + 1, total).map(<[u8]>::len), Some(4));
        assert_eq!(pages_for_rows(0, PAGE_SIZE / BYTES_PER_ROW), [0, 1]);
    }

    #[test]
    fn maps_a_capped_scroll_range_onto_all_rows() {
        let rows = 50_000_000;
        let top = hex_viewport(0.0, 400.0, rows, 20.0, MAX_SCROLL_PX);
        assert_eq!(top, HexViewport { spacer: MAX_SCROLL_PX, first: 0, shift: 0.0 });
        let end = hex_viewport(MAX_SCROLL_PX - 400.0, 400.0, rows, 20.0, MAX_SCROLL_PX);
        assert_eq!(end.first, rows - 20);
        let st = scroll_top_for_row(rows / 2, 400.0, rows, 20.0, MAX_SCROLL_PX);
        assert!(hex_viewport(st, 400.0, rows, 20.0, MAX_SCROLL_PX).first.abs_diff(rows / 2) <= 1);
        // Small files scroll 1:1.
        let small = hex_viewport(45.0, 400.0, 100, 20.0, MAX_SCROLL_PX);
        assert_eq!(small, HexViewport { spacer: 2000.0, first: 2, shift: 5.0 });
    }
}
