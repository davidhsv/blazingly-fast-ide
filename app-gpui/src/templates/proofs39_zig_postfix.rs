//! The Zig postfix templates and their tab.

use super::live_model::{ExpansionEnv, expand_template};
use super::postfix_model::{POSTFIX_GROUPS, PostfixTemplate, find_postfix_target, postfix_templates_for};
use ropey::Rope;
use std::collections::{BTreeMap, BTreeSet};

fn zig(key: &str) -> &'static PostfixTemplate {
    let all = postfix_templates_for("zig", &BTreeSet::new());
    all.into_iter().find(|t| t.key == key).unwrap_or_else(|| panic!("zig has .{key}"))
}

// postfix target › Zig: optional unwrap and expansion
#[test]
fn zig_optional_unwrap_and_expansion() {
    let doc = "const value = maybe.nn";
    let end = doc.encode_utf16().count() as u32;
    let target = find_postfix_target(&Rope::from_str(doc), end, "zig", &[]).expect("a target");
    assert_eq!(target.key, "nn");
    assert_eq!(target.candidates[0].text, "maybe");
    let template = zig("nn");
    let env = ExpansionEnv {
        predefined: BTreeMap::from([("EXPR".to_owned(), "maybe".to_owned())]),
        ..ExpansionEnv::default()
    };
    let expansion = expand_template(&template.body, &template.variables, &env);
    assert_eq!(expansion.snippet, "if (maybe) |${1:item}| \\{\n\t${0}\n\\}");
}

// Not from a spec: the Zig set holds every template of its list in the list's order, and its tab
// comes after Go's.
#[test]
fn the_zig_postfix_set_holds_its_list_in_order() {
    let keys: Vec<&str> = postfix_templates_for("zig", &BTreeSet::new()).iter().map(|t| t.key.as_str()).collect();
    let expected = ["if", "not", "const", "var", "return", "try", "defer", "nn", "null", "switch", "print", "par"];
    assert_eq!(keys, expected);
    assert_eq!(zig("print").body, "std.debug.print(\"{any}\\n\", .{$EXPR$});$END$");
    assert_eq!(zig("print").description, "std.debug.print(\"{any}\\n\", .{expr})");
    assert!(zig("not").wrap_compound && !zig("not").statement);
    assert!(!zig("par").statement && zig("nn").statement);
    assert_eq!(zig("if").id, "zig.if");
    let titles: Vec<&str> = POSTFIX_GROUPS.iter().map(|&(_, title)| title).collect();
    assert_eq!(titles, ["JavaScript / TypeScript", "Rust", "Python", "Go", "Zig", "Java", "Kotlin"]);
}
