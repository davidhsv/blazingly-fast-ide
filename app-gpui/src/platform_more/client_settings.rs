//! Which settings belong to the client: the preferences of the person at this machine (keymap,
//! appearance, fonts, confirmations), as opposed to the settings of the project and its tools,
//! which belong to the host the project lives on. In a remote window the client's settings are
//! read and written here, whatever the remote host has stored.

use serde_json::{Map, Value};

/// The settings owned by the client, their descendants included. Sections that mix both kinds
/// list their client keys one by one, so that project and tool settings stay with the host.
pub const CLIENT_SETTINGS_KEYS: [&str; 107] = [
    "keymap",
    "keymapOverrides",
    "customKeymaps",
    "appearance",
    "notifications",
    "general.reopenLastProject",
    "general.confirmExit",
    "general.closeWithoutWelcome",
    "general.closeTabsWithWindow",
    "general.openProjectIn",
    "general.updateSource",
    "general.autoCheckUpdates",
    "general.showWhatsNewAfterUpdate",
    "general.lastRunVersion",
    "general.dateFormat",
    "editor.fontFamily",
    "editor.fontSize",
    "editor.lineHeight",
    "editor.ligatures",
    "editor.fallbackFont",
    "editor.fontWeight",
    "editor.boldFontWeight",
    "editor.colorScheme",
    "editor.colorOverrides",
    "editor.customSchemes",
    "editor.semanticHighlighting",
    "editor.docstringFormat",
    "editor.rainbowBrackets",
    "editor.softWrap",
    "editor.showWhitespace",
    "editor.inlayHints",
    "editor.stickyScroll",
    "editor.appearance",
    "editor.breadcrumbs",
    "editor.folding",
    "editor.tabs",
    "editor.codeEditing",
    "editor.inlay",
    "editor.gutterIconsDisabled",
    "editor.showGutterIcons",
    "editor.general.wheelZoom",
    "editor.general.wheelZoomScope",
    "editor.general.dragAndDrop",
    "editor.general.caretScroll",
    "editor.general.softWrapFiles",
    "editor.general.softWrapIndent",
    "editor.general.scrollPastEnd",
    "editor.general.richTextCopy",
    "editor.codeStyle.visualGuides",
    "terminal.tabName",
    "terminal.audibleBell",
    "terminal.closeOnExit",
    "terminal.mouseReporting",
    "terminal.copyOnSelection",
    "terminal.pasteOnMiddleClick",
    "terminal.overrideIdeShortcuts",
    "terminal.highlightHyperlinks",
    "terminal.optionAsMeta",
    "terminal.escapeToEditor",
    "terminal.cursorShape",
    "terminal.cursorBlink",
    "terminal.useEditorFont",
    "terminal.fontFamily",
    "terminal.fontSize",
    "terminal.lineHeight",
    "terminal.fallbackFontFamily",
    "terminal.columnWidth",
    "console.softWrap",
    "console.foldPatterns",
    "console.logColors",
    "debugger.showOnBreakpoint",
    "debugger.focusAppOnBreakpoint",
    "debugger.hideOnTermination",
    "debugger.variablesOnBreakpoint",
    "debugger.runToCursorInlay",
    "debugger.inlineValues",
    "debugger.sortValues",
    "debugger.valueTooltip",
    "debugger.valueTooltipOnSelection",
    "debugger.valueTooltipDelay",
    "run.confirmRerun",
    "run.confirmTerminate",
    "run.showOnStart",
    "run.pinTabs",
    "externalDiff",
    "vcs.showDirtyDirectories",
    "vcs.git.inlineBlame",
    "vcs.git.showBranchInProjectView",
    "vcs.git.showPushDialog",
    "vcs.git.pushDialogOnlyProtected",
    "vcs.git.combineStashAndShelf",
    "vcs.git.stagingArea",
    "vcs.commit.nonModal",
    "vcs.commit.doubleClickOpensDiff",
    "vcs.commit.showRightMargin",
    "agents.showContextUsage",
    "agents.showWorkTime",
    "agents.showUsageLimits",
    "agents.showCost",
    "agents.foldTurns",
    "agents.smoothText",
    "agents.fitWindowWidth",
    "agents.thinking",
    "agents.toolCalls",
    "agents.expandToolOutput",
    "agents.sendWith",
    "agents.whileRunning",
];

/// Whether the dotted settings `key` is a client setting or lies below one.
pub fn is_client_setting(key: &str) -> bool {
    CLIENT_SETTINGS_KEYS
        .iter()
        .any(|root| key.strip_prefix(root).is_some_and(|rest| rest.is_empty() || rest.starts_with('.')))
}

fn fields(value: Option<&Value>) -> Map<String, Value> {
    value.and_then(Value::as_object).cloned().unwrap_or_default()
}

/// `target` with the value at `path` taken from `source`; a value `source` lacks is removed.
fn overlay_path(target: Option<&Value>, source: Option<&Value>, path: &[&str]) -> Value {
    let mut out = fields(target);
    let Some((&head, tail)) = path.split_first() else {
        return Value::Object(out);
    };
    let value = source.and_then(Value::as_object).and_then(|source| source.get(head));
    if !tail.is_empty() {
        let nested = overlay_path(out.get(head), value, tail);
        out.insert(head.to_owned(), nested);
    } else if let Some(value) = value {
        out.insert(head.to_owned(), value.clone());
    } else {
        out.remove(head);
    }
    Value::Object(out)
}

/// The settings a remote window works with: the host's (`remote`) with every client setting
/// taken from this machine's global settings (`local`), never from its project layer. A client
/// setting this machine has no value for is removed rather than inherited from the host, so
/// that it takes the client's default.
pub fn overlay_client_settings(remote: &Value, local: &Value) -> Value {
    CLIENT_SETTINGS_KEYS.iter().fold(remote.clone(), |settings, key| {
        let path: Vec<&str> = key.split('.').collect();
        overlay_path(Some(&settings), Some(local), &path)
    })
}

#[cfg(test)]
pub(crate) mod testing {
    /// The client settings of the settings model every client of a host shares, in their order:
    /// what a remote window must keep on this machine.
    pub fn shared_client_keys() -> Vec<&'static str> {
        const SOURCE: &str = include_str!("../../../src/platform/clientSettings.ts");
        let (_, list) = SOURCE.split_once("export const CLIENT_SETTINGS_KEYS = [").expect("the shared list");
        let (list, _) = list.split_once("] as const").expect("the end of the shared list");
        list.split('"').skip(1).step_by(2).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Not from a spec: every client setting of the shared model is one here, in the same order, and nothing else is.
    #[test]
    fn keeps_every_client_setting_of_the_shared_model_and_nothing_else() {
        let shared = testing::shared_client_keys();
        assert!(shared.len() > 80, "the shared list is read: {shared:?}");
        let missing: Vec<&str> = shared.iter().copied().filter(|key| !CLIENT_SETTINGS_KEYS.contains(key)).collect();
        assert_eq!(missing, Vec::<&str>::new(), "client settings a remote window would send to the host");
        assert_eq!(CLIENT_SETTINGS_KEYS.as_slice(), shared.as_slice());
    }

    // Not from a spec: ownership follows whole key segments, and the overlay replaces client
    // values only, creating the sections it needs.
    #[test]
    fn tells_client_keys_by_whole_segments_and_overlays_them() {
        assert!(is_client_setting("keymap"));
        assert!(is_client_setting("customKeymaps.mine"));
        assert!(is_client_setting("editor.fontSize"));
        assert!(!is_client_setting("keymapExtra"));
        assert!(!is_client_setting("editor"));
        assert!(!is_client_setting("editor.tabSize"));
        assert!(!is_client_setting(""));

        let remote =
            json!({ "keymap": "remote", "editor": { "fontSize": 13, "tabSize": 2 }, "tools": { "node": "/n" } });
        let local = json!({ "keymap": "mine", "editor": { "fontSize": 19, "tabSize": 8 }, "vcs": { "git": 3 } });
        let merged = overlay_client_settings(&remote, &local);
        assert_eq!(merged["keymap"], "mine");
        // The client's font size over the host's; the host's tab size stays. Sections of client
        // keys are created on the way, empty when this machine sets none of them.
        assert_eq!(merged["editor"], json!({ "fontSize": 19, "tabSize": 2, "general": {}, "codeStyle": {} }));
        assert_eq!(merged["tools"], json!({ "node": "/n" }));
        // A section the host does not have is created; a malformed local section gives nothing.
        assert_eq!(merged["vcs"], json!({ "git": {}, "commit": {} }));
        assert_eq!(merged["appearance"], Value::Null);
    }
}
