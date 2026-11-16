//! Not declared: the test below fails until `files_model::plan_file` refuses an extension that names
//! another folder. `plan_file` appends the template's (or the kind's) extension to the file name
//! unchecked, so an extension `/../../x` plans `<folder>/notes./../../x`, two folders up. The New
//! File dialog's creation refuses such a plan on its own (`workbench::template_files`), so nothing
//! is created outside the folder meanwhile. Once the guard lands, declare this module in
//! `templates/mod.rs` with `#[cfg(test)] mod proofs34_extension_guard;` (renamed without `_pending`).

use super::files_model::{FileTemplate, FileTemplateKind, plan_file, with_kind};

fn template(extension: &str) -> FileTemplate {
    FileTemplate {
        id: "notes".to_owned(),
        name: "Notes".to_owned(),
        icon: "file".to_owned(),
        extension: extension.to_owned(),
        kinds: vec![FileTemplateKind {
            id: "file".to_owned(),
            label: "Notes".to_owned(),
            body: "$END$".to_owned(),
            extension: None,
        }],
        ..FileTemplate::default()
    }
}

// An extension holding a path separator plans no file, whether the template or one of its kinds
// carries it.
#[test]
fn an_extension_holding_a_path_separator_plans_no_file() {
    for extension in ["/../../x", "../x", "a/b", "a\\b", "x/"] {
        assert_eq!(plan_file(&template(extension), "/p/src", "notes"), None, "{extension:?}");
        let plain = template("txt");
        let kind = FileTemplateKind { extension: Some(extension.to_owned()), ..plain.kinds[0].clone() };
        assert_eq!(plan_file(&with_kind(&plain, Some(&kind)), "/p/src", "notes"), None, "kind {extension:?}");
    }
    // An ordinary extension still plans the file in the folder.
    let plan = plan_file(&template("md"), "/p/src", "notes").expect("a plan");
    assert_eq!(plan.path, "/p/src/notes.md");
}
