//! Where an action sits in the Keymap tree, mirroring IDEA's top-level groups: Main Menu › File /
//! Edit / View / Navigate / Code / Refactor / Run / Tools / Git / Window / Help, Editor Actions,
//! Tool Windows, Other, Plugins. Derived from the action's category (and the `plugin.` id prefix
//! of plugin commands).

pub const MAIN_MENU: &str = "Main Menu";
pub const EDITOR_ACTIONS: &str = "Editor Actions";
pub const TOOL_WINDOWS: &str = "Tool Windows";
pub const OTHER: &str = "Other";
pub const PLUGINS: &str = "Plugins";

/// Top-level groups in display order.
pub const ROOT_ORDER: [&str; 5] = [MAIN_MENU, EDITOR_ACTIONS, TOOL_WINDOWS, OTHER, PLUGINS];

/// Main menu submenus in menu-bar order.
pub const MAIN_MENU_ORDER: [&str; 11] =
    ["File", "Edit", "View", "Navigate", "Code", "Refactor", "Run", "Tools", "Git", "Window", "Help"];

/// The path a category sits at, below (and including) its root group.
fn category_path(category: &str) -> Option<&'static [&'static str]> {
    Some(match category {
        "File" => &[MAIN_MENU, "File"],
        "Edit" => &[MAIN_MENU, "Edit"],
        "Find" => &[MAIN_MENU, "Edit", "Find"],
        "View" => &[MAIN_MENU, "View"],
        "Navigate" => &[MAIN_MENU, "Navigate"],
        "Bookmarks" => &[MAIN_MENU, "Navigate", "Bookmarks"],
        "Code" => &[MAIN_MENU, "Code"],
        "Folding" => &[MAIN_MENU, "Code", "Folding"],
        "Code Completion" => &[MAIN_MENU, "Code", "Code Completion"],
        "Refactor" => &[MAIN_MENU, "Refactor"],
        "Run" => &[MAIN_MENU, "Run"],
        "Debug" => &[MAIN_MENU, "Run", "Debugging Actions"],
        "Test" => &[MAIN_MENU, "Run", "Testing"],
        "Tools" => &[MAIN_MENU, "Tools"],
        "Git" | "VCS" => &[MAIN_MENU, "Git"],
        "Window" => &[MAIN_MENU, "Window"],
        "Help" => &[MAIN_MENU, "Help"],
        "Editor" | "Editor Actions" => &[EDITOR_ACTIONS],
        "Tool Windows" => &[TOOL_WINDOWS],
        // IDEA lists the bundled Terminal plugin's actions under Plugins.
        "Terminal" => &[PLUGINS, "Terminal"],
        "Plugins" => &[PLUGINS],
        "Project View" => &[OTHER, "Project View Popup Menu"],
        _ => return None,
    })
}

/// Group path of an action, e.g. `["Main Menu", "Edit", "Find"]`.
pub fn group_path(id: &str, category: Option<&str>) -> Vec<String> {
    let path = |segments: &[&str]| segments.iter().map(|&segment| segment.to_owned()).collect();
    if id.starts_with("plugin.") {
        return path(&[PLUGINS, category.filter(|category| !category.is_empty()).unwrap_or(OTHER)]);
    }
    match category.map(str::trim).filter(|category| !category.is_empty()) {
        None if id.starts_with("editor.") => path(&[EDITOR_ACTIONS]),
        None => path(&[OTHER]),
        Some(category) => category_path(category).map_or_else(|| path(&[OTHER, category]), path),
    }
}

/// Sort rank of a group among its siblings (lower first; unknown groups come last, by name).
pub fn group_rank(parent_path: &[String], name: &str) -> usize {
    let order: &[&str] = match parent_path {
        [] => &ROOT_ORDER,
        [only] if only == MAIN_MENU => &MAIN_MENU_ORDER,
        _ => &[],
    };
    order.iter().position(|&group| group == name).unwrap_or(order.len())
}

/// Icon of a group row, like IDEA's keymap tree.
pub fn group_icon(path: &[String], expanded: bool) -> &'static str {
    let root_icon = match path {
        [only] => match only.as_str() {
            MAIN_MENU => Some("list"),
            EDITOR_ACTIONS => Some("edit"),
            TOOL_WINDOWS => Some("layout"),
            PLUGINS => Some("plugin"),
            _ => None,
        },
        _ => None,
    };
    root_icon.unwrap_or(if expanded { "folder-open" } else { "folder" })
}

#[cfg(test)]
mod tests {
    use super::*;

    // keymap tree groups › places actions like IDEA: menu order, submenus after actions, other roots last
    // (the path of an action whose category has no place of its own)
    #[test]
    fn puts_unknown_categories_under_other() {
        assert_eq!(group_path("x.misc", Some("Weird")), ["Other", "Weird"]);
    }

    // Not from a spec: plugin commands go under Plugins by category, uncategorised editor actions
    // under Editor Actions, and groups rank in menu order with unknown ones last.
    #[test]
    fn groups_rank_in_menu_order() {
        assert_eq!(group_path("plugin.hello.say", Some("Hello")), ["Plugins", "Hello"]);
        assert_eq!(group_path("plugin.hello.say", None), ["Plugins", "Other"]);
        assert_eq!(group_path("editor.showHint", None), ["Editor Actions"]);
        assert_eq!(group_path("misc", Some("  ")), ["Other"]);
        assert_eq!(group_path("debug.stepOver", Some("Debug")), ["Main Menu", "Run", "Debugging Actions"]);
        assert_eq!(group_path("vcs.commit", Some(" VCS ")), ["Main Menu", "Git"]);
        let main = [MAIN_MENU.to_owned()];
        assert!(group_rank(&[], MAIN_MENU) < group_rank(&[], PLUGINS));
        assert_eq!(group_rank(&[], "Nope"), 5);
        assert!(group_rank(&main, "File") < group_rank(&main, "Edit"));
        assert_eq!(group_rank(&["Other".to_owned()], "File"), 0);
        assert_eq!(group_icon(&main, false), "list");
        assert_eq!(group_icon(&["Main Menu".to_owned(), "File".to_owned()], true), "folder-open");
        assert_eq!(group_icon(&["Other".to_owned()], false), "folder");
    }
}
