//! TeamCity service messages (`##teamcity[testStarted name='…' …]`): PHPUnit's `--teamcity` output and
//! the Gradle test init script report each test with them.

use crate::run_testing::types::TestStdout;
use std::collections::HashMap;

pattern!(MESSAGE, r"^\s*##teamcity\[([A-Za-z0-9_]+)((?:\s+[A-Za-z0-9_.-]+='(?:[^|']|\|.)*')*)\s*\]\s*$");
pattern!(ATTR, r"([A-Za-z0-9_.-]+)='((?:[^|']|\|.)*)'");

/// TeamCity escapes: `|n` `|r` `|'` `|[` `|]` `||` and `|0xNNNN` (a UTF-16 code unit).
fn unescape(value: &str) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('|') {
        units.extend(rest[..at].encode_utf16());
        let after = &rest[at + 1..];
        let hex = after.strip_prefix("0x").and_then(|digits| digits.get(..4));
        let unit = hex.filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()));
        if let Some(unit) = unit.and_then(|hex| u16::from_str_radix(hex, 16).ok()) {
            units.push(unit);
            rest = &after[6..];
            continue;
        }
        let mut chars = after.chars();
        match chars.next() {
            Some('n') => units.push(u16::from(b'\n')),
            Some('r') => units.push(u16::from(b'\r')),
            Some(escaped) => units.extend_from_slice(escaped.encode_utf16(&mut [0; 2])),
            None => units.push(u16::from(b'|')),
        }
        rest = chars.as_str();
    }
    units.extend(rest.encode_utf16());
    String::from_utf16_lossy(&units)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceMessage {
    pub kind: String,
    pub attrs: HashMap<String, String>,
}

impl ServiceMessage {
    /// The value of an attribute; empty when the message does not have it.
    pub fn attr(&self, name: &str) -> &str {
        self.attrs.get(name).map_or("", String::as_str)
    }
}

/// The service message on `line`, or `None` for any other output.
pub fn service_message(line: &str) -> Option<ServiceMessage> {
    let found = MESSAGE.captures(line)?;
    let attrs = ATTR.captures_iter(&found[2]).map(|attr| (attr[1].to_string(), unescape(&attr[2]))).collect();
    Some(ServiceMessage { kind: found[1].to_string(), attrs })
}

/// Console text of a run reporting through service messages: the messages only feed the test tree.
pub fn teamcity_output() -> TestStdout {
    TestStdout::new(|line| if MESSAGE.is_match(line) { String::new() } else { format!("{line}\n") })
}
