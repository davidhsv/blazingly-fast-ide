//! The Zig live templates.

use super::live_builtins::builtin_templates;
use super::live_model::{ExpansionEnv, TemplateFilter, applicable_templates, expand_template};
use super::snippet::testing::applied;

// Not from a spec: the Zig group holds every template of its list in the list's order, between the
// Go and the Java groups, and applies in Zig files only.
#[test]
fn the_zig_group_holds_its_list_in_order() {
    let zig: Vec<&str> = applicable_templates(builtin_templates(), "zig", TemplateFilter::default())
        .iter()
        .filter(|t| t.group == "Zig")
        .map(|t| t.abbrev.as_str())
        .collect();
    let abbrevs = [
        "fn", "pfn", "main", "test", "st", "en", "un", "if", "ife", "iferr", "while", "for", "defer", "const", "var",
        "try", "catch", "sw", "p",
    ];
    assert_eq!(zig, abbrevs);
    let groups: Vec<&str> = builtin_templates().iter().map(|t| t.group.as_str()).collect();
    let first = groups.iter().position(|&group| group == "Zig").expect("a Zig group");
    assert_eq!(groups[first - 1], "Go");
    assert_eq!(groups[first + abbrevs.len()], "Java");
    let in_go = applicable_templates(builtin_templates(), "go", TemplateFilter::default());
    assert!(!in_go.iter().any(|t| t.group == "Zig"));
}

// Not from a spec: a Zig test block and a function expand with their defaults.
#[test]
fn zig_templates_expand_with_their_defaults() {
    let expanded = |id: &str| {
        let template = builtin_templates().iter().find(|t| t.id == id).unwrap_or_else(|| panic!("{id}"));
        let expansion = expand_template(&template.body, &template.variables, &ExpansionEnv::default());
        applied("", &expansion.snippet, 0, 0).0
    };
    assert_eq!(expanded("zig.test"), "test \"it works\" {\n  \n}");
    assert_eq!(expanded("zig.fn"), "fn () void {\n  \n}");
    assert_eq!(expanded("zig.p"), "std.debug.print(\"{any}\\n\", .{});");
}
