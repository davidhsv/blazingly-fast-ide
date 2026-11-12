//! Code Style › Scheme (IDEA's Default = IDE-level, Project = stored in the project). The scheme
//! is the scope Code Style and its language pages share; "Copy to Project" / "Copy to IDE" stage
//! every Code Style value in the other scope.

use super::staging::{Scope, SettingsDialog};

/// The scope group of Code Style and its language pages.
pub const CODE_STYLE_SCOPE: &str = "editor.codeStyle";

/// What a Code Style scheme holds.
pub const CODE_STYLE_KEYS: [&str; 13] = [
    "editor.tabSize",
    "editor.insertSpaces",
    "editor.codeStyle.lineSeparator",
    "editor.codeStyle.hardWrapAt",
    "editor.codeStyle.visualGuides",
    "editor.codeStyle.detectIndents",
    "editor.codeStyle.editorconfig",
    "editor.codeStyle.languages",
    "editor.codeStyle.doNotFormat",
    "editor.codeStyle.formatterTagsEnabled",
    "editor.codeStyle.formatterOffTag",
    "editor.codeStyle.formatterOnTag",
    "editor.codeStyle.formatterTagsRegexp",
];

/// Stages the values of the `from` scheme in the `to` scheme; `page` owns the staged edits.
pub fn copy_scheme(dialog: &mut SettingsDialog, page: &str, from: Scope, to: Scope) {
    for key in CODE_STYLE_KEYS {
        if let Some(value) = dialog.staged_value(from, key) {
            dialog.stage(page, to, key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::schema::default_settings;
    use serde_json::json;

    // code style scheme › copies every Code Style value to the other scheme
    #[test]
    fn copies_every_code_style_value_to_the_other_scheme() {
        let page = "editor.codeStyle";
        let mut dialog = SettingsDialog::default();
        dialog.open(Some(page), false, default_settings());
        dialog.stage(page, Scope::Global, "editor.codeStyle.hardWrapAt", json!(100));
        dialog.stage(page, Scope::Global, "editor.codeStyle.formatterTagsEnabled", json!(true));
        let java = json!({ "java": { "formatter": "custom", "formatterPath": "config/formatter.xml" } });
        dialog.stage(page, Scope::Global, "editor.codeStyle.languages", java);
        copy_scheme(&mut dialog, page, Scope::Global, Scope::Project);
        assert_eq!(dialog.staged_value(Scope::Project, "editor.codeStyle.hardWrapAt"), Some(json!(100)));
        assert_eq!(dialog.staged_value(Scope::Project, "editor.codeStyle.formatterTagsEnabled"), Some(json!(true)));
        let off_tag = dialog.staged_value(Scope::Project, "editor.codeStyle.formatterOffTag");
        assert_eq!(off_tag, Some(json!("@formatter:off")));
        for key in CODE_STYLE_KEYS {
            let (project, global) = (dialog.staged_value(Scope::Project, key), dialog.staged_value(Scope::Global, key));
            assert!(project.is_some(), "{key}");
            assert_eq!(project, global, "{key}");
        }
    }
}
