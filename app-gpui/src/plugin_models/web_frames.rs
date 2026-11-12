//! What a plugin's web frame exchanges with the workbench, checked on the workbench side: its
//! document edits, and the URL its package files are served under.

use ide_core::TextChange;
use serde_json::Value;

/// The largest text a frame's edits may insert, in UTF-16 code units.
pub const MAX_EDIT: usize = 64 << 20;
/// The most edits one message may carry.
pub const MAX_EDITS: usize = 100_000;

/// The smallest change that turns `from` into `to` (UTF-16 offsets); `None` when they are equal.
pub fn minimal_change(from: &str, to: &str) -> Option<TextChange> {
    if from == to {
        return None;
    }
    let (a, b): (Vec<u16>, Vec<u16>) = (from.encode_utf16().collect(), to.encode_utf16().collect());
    let max = a.len().min(b.len());
    let start = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let end = a.iter().rev().zip(b.iter().rev()).take(max - start).take_while(|(x, y)| x == y).count();
    Some(TextChange {
        from: start as u32,
        to: (a.len() - end) as u32,
        insert: String::from_utf16_lossy(&b[start..b.len() - end]),
    })
}

/// A frame's edits checked against the text they apply to (`length` UTF-16 units): whole offsets,
/// sorted, in range, not overlapping, and bounded in size.
pub fn valid_changes(value: &Value, length: usize) -> Result<Vec<TextChange>, &'static str> {
    let items = value.as_array().filter(|items| items.len() <= MAX_EDITS).ok_or("edits must be an array")?;
    let (mut at, mut size) = (0usize, 0usize);
    items
        .iter()
        .map(|change| {
            let offset = |name: &str| {
                let n = change.get(name).and_then(Value::as_f64)?;
                (n.fract() == 0.0 && n >= 0.0).then_some(n as usize)
            };
            let (Some(from), Some(to), Some(insert)) =
                (offset("from"), offset("to"), change.get("insert").and_then(Value::as_str))
            else {
                return Err("an edit is { from, to, insert }");
            };
            if from < at || to < from || to > length {
                return Err("edits must be sorted, in range and not overlap");
            }
            size += insert.encode_utf16().count();
            if size > MAX_EDIT {
                return Err("the edit is too large");
            }
            at = to;
            Ok(TextChange { from: from as u32, to: to as u32, insert: insert.to_owned() })
        })
        .collect()
}

/// `text` as `encodeURIComponent` writes it.
fn encode_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The URL of a package file (`<plugin>/<revision>/<entry>`) under the plugin scheme: Windows web
/// views reach custom schemes through `http://<scheme>.localhost`.
pub fn web_scheme_url(path: &str, windows: bool) -> String {
    let encoded = path.split('/').map(encode_component).collect::<Vec<_>>().join("/");
    if windows {
        format!("http://openidea-plugin.localhost/{encoded}")
    } else {
        format!("openidea-plugin://localhost/{encoded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn change(from: u32, to: u32, insert: &str) -> Option<TextChange> {
        Some(TextChange { from, to, insert: insert.to_owned() })
    }

    #[test]
    fn turns_a_replacement_into_the_smallest_change() {
        assert_eq!(minimal_change("abcdef", "abXYef"), change(2, 4, "XY"));
        assert_eq!(minimal_change("aaa", "aaaa"), change(3, 3, "a"));
        assert_eq!(minimal_change("abc", ""), change(0, 3, ""));
        assert_eq!(minimal_change("same", "same"), None);
    }

    #[test]
    fn accepts_only_sorted_in_range_non_overlapping_edits() {
        let sorted = json!([{ "from": 0, "to": 1, "insert": "x" }, { "from": 2, "to": 2, "insert": "y" }]);
        assert_eq!(valid_changes(&sorted, 3).map(|c| c.len()), Ok(2));
        let unsorted = json!([{ "from": 2, "to": 2, "insert": "" }, { "from": 0, "to": 1, "insert": "" }]);
        assert!(valid_changes(&unsorted, 3).is_err());
        assert!(valid_changes(&json!([{ "from": 0, "to": 4, "insert": "" }]), 3).is_err());
        assert!(valid_changes(&json!([{ "from": 0.5, "to": 1, "insert": "" }]), 3).is_err());
        assert!(valid_changes(&json!("nope"), 3).is_err());
    }

    #[test]
    fn spells_scheme_urls_the_way_each_webview_expects() {
        let unix = web_scheme_url("acme.draw/abc/web/index.html", false);
        assert_eq!(unix, "openidea-plugin://localhost/acme.draw/abc/web/index.html");
        assert_eq!(web_scheme_url("p/r/web/a b.html", true), "http://openidea-plugin.localhost/p/r/web/a%20b.html");
    }
}
