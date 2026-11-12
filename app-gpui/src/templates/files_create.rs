//! Creating a file from a template, as the steps its caller carries out: find the package (which
//! may take reading a sibling Go file or a composer.json), render the body, create and open the
//! file, fill the empty editor and put the caret back after Reformat Code.

use super::files_builtins::builtin_file_templates;
use super::files_model::{
    FileTemplate, FileTemplateKind, FileTemplateVars, Includes, PackageStyle, PlannedFile, caret_after_reformat,
    composer_psr4, go_package_of, infer_go_package, infer_jvm_package, infer_php_namespace, plan_file,
    reformats_on_create, render_file_template, with_kind,
};
use crate::platform::project_roots::{FolderRole, RootFolder};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use std::collections::BTreeMap;

/// The message shown for a name no file can have (a text catalog entry).
pub const INVALID_FILE_NAME: &str = "Invalid file name";

/// How the package of files created from `template` is inferred: as the template says, or for a
/// plugin's template that asks for it, as the built-in template of the same extension does. An
/// exact filename decides the extension, whatever the template declares.
pub fn package_style_for(template: &FileTemplate) -> Option<PackageStyle> {
    if template.package_style.is_some() || !template.infer_package {
        return template.package_style;
    }
    let extension = match template.file_name.as_deref() {
        None => template.extension.as_str(),
        Some(file_name) => match file_name.rfind('.') {
            Some(dot) if dot > 0 => &file_name[dot + 1..],
            _ => "",
        },
    };
    builtin_file_templates()
        .iter()
        .find(|builtin| builtin.package_style.is_some() && builtin.extension.eq_ignore_ascii_case(extension))
        .and_then(|builtin| builtin.package_style)
}

/// Whether `detected_from` is a JVM build file (`pom.xml`, `build.gradle`, `build.gradle.kts`).
fn is_jvm_build_file(detected_from: &str) -> bool {
    let name = detected_from.rsplit('/').next().unwrap_or(detected_from);
    matches!(name, "pom.xml" | "build.gradle" | "build.gradle.kts")
}

/// The source and test roots (project-relative) JVM packages are counted from: the user-marked
/// ones and those detected from a JVM build file.
pub fn package_roots(roots: &[RootFolder]) -> Vec<&str> {
    roots
        .iter()
        .filter(|root| matches!(root.kind, FolderRole::Source | FolderRole::Test))
        .filter(|root| root.detected_from.as_deref().is_none_or(|from| from.is_empty() || is_jvm_build_file(from)))
        .map(|root| root.path.as_str())
        .collect()
}

/// A file of the directory a Go file is created in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiblingFile<'a> {
    pub name: &'a str,
    pub path: &'a str,
}

/// Finding the Go package from a file already in the directory: list the directory, read the
/// file [`GoSiblingLookup::pick`] chooses, and [`GoSiblingLookup::finish`] with its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoSiblingLookup {
    pub directory: String,
    project_root: Option<String>,
}

impl GoSiblingLookup {
    /// The file to read the package from: a source file rather than a test, the first listed.
    pub fn pick<'a>(&self, files: &[SiblingFile<'a>]) -> Option<&'a str> {
        let is_go = |file: &&SiblingFile<'a>| file.name.ends_with(".go");
        let source = files.iter().filter(is_go).find(|file| !file.name.ends_with("_test.go"));
        source.or_else(|| files.iter().find(is_go)).map(|file| file.path)
    }

    /// The package, given the picked file's text (`None` when there is no file or it cannot be
    /// read): a test package `x_test` counts as `x`.
    pub fn finish(&self, text: Option<&str>) -> String {
        let sibling = text.and_then(go_package_of).map(|package| package.strip_suffix("_test").unwrap_or(package));
        infer_go_package(&self.directory, self.project_root.as_deref(), sibling)
    }
}

/// Finding the PHP namespace from the nearest composer.json between the directory and the
/// project root: read [`ComposerLookup::path`] and [`ComposerLookup::answer`] with its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerLookup {
    directory: String,
    /// The folder whose composer.json is asked for.
    at: String,
    top: Option<String>,
}

/// What a composer.json lookup does after an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerStep {
    Done(String),
    /// No composer.json there: ask the parent folder.
    Next(ComposerLookup),
}

impl ComposerLookup {
    fn new(directory: &str, project_root: Option<&str>) -> Self {
        Self {
            directory: directory.to_owned(),
            at: directory.trim_end_matches('/').to_owned(),
            top: project_root.map(|root| root.trim_end_matches('/').to_owned()),
        }
    }

    /// The file to read.
    pub fn path(&self) -> String {
        format!("{}/composer.json", self.at)
    }

    /// Continues with the file's text, or `None` when there is no such file.
    pub fn answer(self, text: Option<&str>) -> ComposerStep {
        if let Some(text) = text {
            let below = self.directory.get(self.at.len()..).unwrap_or_default();
            return ComposerStep::Done(infer_php_namespace(below, &composer_psr4(text)));
        }
        // Only folders below the project root have a parent worth asking.
        let below_top =
            self.top.as_deref().filter(|top| !top.is_empty()).is_some_and(|top| {
                self.at != top && self.at.strip_prefix(top).is_some_and(|rest| rest.starts_with('/'))
            });
        match self.at.rfind('/') {
            Some(slash) if below_top => {
                let parent = self.at[..slash].to_owned();
                ComposerStep::Next(Self { at: parent, ..self })
            }
            _ => ComposerStep::Done(String::new()),
        }
    }
}

/// How the package of a new file is found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageLookup {
    /// Known without looking at any file.
    Known(String),
    Go(GoSiblingLookup),
    Php(ComposerLookup),
}

/// The package lookup for a file created from `template` in `directory`. `roots` are the
/// project's effective roots.
pub fn package_for(
    template: &FileTemplate,
    directory: &str,
    project_root: Option<&str>,
    roots: &[RootFolder],
) -> PackageLookup {
    match package_style_for(template) {
        Some(PackageStyle::Jvm) => {
            PackageLookup::Known(infer_jvm_package(directory, project_root, &package_roots(roots)))
        }
        Some(PackageStyle::Go) => PackageLookup::Go(GoSiblingLookup {
            directory: directory.to_owned(),
            project_root: project_root.map(str::to_owned),
        }),
        Some(PackageStyle::Php) => PackageLookup::Php(ComposerLookup::new(directory, project_root)),
        None => PackageLookup::Known(String::new()),
    }
}

/// The user's name as the home folder in `root` spells it.
pub fn user_name(root: &str) -> &str {
    ["/Users/", "/home/"]
        .iter()
        .find_map(|home| root.strip_prefix(home))
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default()
}

/// The open project, as far as a new file's body shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectInfo<'a> {
    pub name: &'a str,
    pub root: &'a str,
}

/// A file about to be created: where, and with what content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creation {
    pub plan: PlannedFile,
    /// The template of the kind's extension, which decides how the package is found.
    template: FileTemplate,
    body: String,
    variables: BTreeMap<String, String>,
    reformat: bool,
}

/// The first step of creating `input` (a name, possibly with sub-directories) in `directory` from
/// `kind` of `template`. `Err` is the user-facing message for a name no file can have.
pub fn plan_creation(
    template: &FileTemplate,
    kind: &FileTemplateKind,
    directory: &str,
    input: &str,
    variables: BTreeMap<String, String>,
) -> Result<Creation, &'static str> {
    let of_kind = with_kind(template, Some(kind));
    let plan = plan_file(&of_kind, directory, input).ok_or(INVALID_FILE_NAME)?;
    Ok(Creation {
        plan,
        template: of_kind.into_owned(),
        body: kind.body.clone(),
        variables,
        reformat: reformats_on_create(template),
    })
}

/// What creating the file does once its content is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateEffect {
    /// Create the (empty) file on disk.
    CreateFile(String),
    /// Tell the file views that the file was created.
    NotifyCreated(String),
    /// Open the file in an editor and focus it.
    OpenFile(String),
}

/// What goes into the new file's editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorFill {
    pub insert: String,
    /// Where the caret goes (UTF-16 offset).
    pub caret: u32,
    /// Run Reformat Code afterwards, then save.
    pub reformat: bool,
}

/// A new file with its rendered content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedFile {
    pub plan: PlannedFile,
    pub text: String,
    /// UTF-16 offset of `$END$` in `text`.
    pub caret: u32,
    reformat: bool,
}

impl Creation {
    /// How the package of the new file is found.
    pub fn package_lookup(&self, project_root: Option<&str>, roots: &[RootFolder]) -> PackageLookup {
        package_for(&self.template, &self.plan.directory, project_root, roots)
    }

    /// Renders the body with the package the lookup found.
    pub fn render(self, package_name: &str, project: Option<ProjectInfo<'_>>, includes: Includes<'_>) -> PreparedFile {
        let vars = FileTemplateVars {
            name: self.plan.name.clone(),
            file_name: self.plan.file_name.clone(),
            package_name: Some(package_name.to_owned()),
            project_name: project.map(|project| project.name.to_owned()),
            user: Some(user_name(project.map_or("", |project| project.root)).to_owned()),
            now: None,
            variables: self.variables,
        };
        let rendered = render_file_template(&self.body, &vars, includes);
        PreparedFile { plan: self.plan, text: rendered.text, caret: rendered.caret, reformat: self.reformat }
    }
}

impl PreparedFile {
    /// What to do now, in order. Nothing when the request was cancelled while the package was
    /// being looked up: the file is not created.
    pub fn effects(&self, cancelled: bool) -> Vec<CreateEffect> {
        if cancelled {
            return Vec::new();
        }
        let path = &self.plan.path;
        vec![
            CreateEffect::CreateFile(path.clone()),
            CreateEffect::NotifyCreated(path.clone()),
            CreateEffect::OpenFile(path.clone()),
        ]
    }

    /// The notification for a file whose editor did not open in time.
    pub fn editor_missing(&self) -> NotificationRequest {
        let title = format!("Created {}", self.plan.file_name);
        let mut request = NotificationRequest::new(NotificationLevel::Warning, title);
        request.body = Some("The template content could not be inserted.".to_owned());
        request
    }

    /// What to insert into the opened editor holding `doc_len` UTF-16 units: nothing for an empty
    /// body, or when the file already has content.
    pub fn fill(&self, doc_len: u32) -> Option<EditorFill> {
        (!self.text.is_empty() && doc_len == 0).then(|| EditorFill {
            insert: self.text.clone(),
            caret: self.caret.min(self.text.encode_utf16().count() as u32),
            reformat: self.reformat,
        })
    }
}

/// Where to put the caret after Reformat Code ran on the filled editor: `before` and `at` are the
/// text and the caret before it, `after` the text now. `None` when nothing was reformatted.
pub fn caret_after_fill_reformat(before: &str, at: u32, after: &str, applied: bool) -> Option<u32> {
    (applied && after != before).then(|| caret_after_reformat(before, at, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(extension: &str) -> FileTemplate {
        FileTemplate {
            id: "plugin.demo.repo".to_owned(),
            name: "Repository".to_owned(),
            icon: "file".to_owned(),
            extension: extension.to_owned(),
            infer_package: true,
            kinds: vec![FileTemplateKind {
                id: "file".to_owned(),
                label: "Repository".to_owned(),
                body: "$PACKAGE$".to_owned(),
                extension: None,
            }],
            ..FileTemplate::default()
        }
    }

    fn fixed_name(extension: &str, file_name: &str) -> FileTemplate {
        FileTemplate { file_name: Some(file_name.to_owned()), ..plugin(extension) }
    }

    fn known(template: &FileTemplate, directory: &str, roots: &[RootFolder]) -> Option<String> {
        match package_for(template, directory, Some("/p"), roots) {
            PackageLookup::Known(package) => Some(package),
            _ => None,
        }
    }

    // plugin package inference uses built-in rules › uses custom source roots for %s
    #[test]
    fn uses_custom_source_roots_for_java_and_kotlin() {
        let roots = [RootFolder { path: "generated".to_owned(), kind: FolderRole::Source, detected_from: None }];
        let dir = "/p/generated/com/acme";
        for extension in ["java", "kt"] {
            assert_eq!(known(&plugin(extension), dir, &roots).as_deref(), Some("com.acme"), "{extension}");
            let builtin = builtin_file_templates().iter().find(|t| t.extension == extension).expect("a built-in");
            assert_eq!(known(&plugin(extension), dir, &roots), known(builtin, dir, &roots));
        }
    }

    // plugin package inference uses built-in rules › infers language from an exact filename when extension is omitted
    #[test]
    fn infers_language_from_an_exact_filename_when_extension_is_omitted() {
        let template = fixed_name("", "Order.java");
        assert_eq!(known(&template, "/p/src/main/java/com/acme", &[]).as_deref(), Some("com.acme"));
    }

    // plugin package inference uses built-in rules › uses the actual fixed filename instead of a conflicting
    // declared extension
    #[test]
    fn uses_the_actual_fixed_filename_instead_of_a_conflicting_declared_extension() {
        let dir = "/p/src/main/java/com/acme";
        assert_eq!(known(&fixed_name("txt", "Order.java"), dir, &[]).as_deref(), Some("com.acme"));
        assert_eq!(known(&fixed_name("java", ".dockerignore"), dir, &[]).as_deref(), Some(""));
    }

    // plugin package inference uses built-in rules › uses an existing Go package
    #[test]
    fn uses_an_existing_go_package() {
        let PackageLookup::Go(lookup) = package_for(&plugin("go"), "/p/pkg/store", Some("/p"), &[]) else {
            panic!("a Go file looks at its siblings");
        };
        assert_eq!(lookup.directory, "/p/pkg/store");
        let files = [SiblingFile { name: "sibling.go", path: "/p/sibling.go" }];
        assert_eq!(lookup.pick(&files), Some("/p/sibling.go"));
        assert_eq!(lookup.finish(Some("package service\n")), "service");
    }

    // plugin package inference uses built-in rules › uses Composer PSR-4 namespaces for PHP
    #[test]
    fn uses_composer_psr4_namespaces_for_php() {
        let PackageLookup::Php(lookup) = package_for(&plugin("php"), "/p/src/Http", Some("/p"), &[]) else {
            panic!("a PHP file looks for a composer.json");
        };
        // Only /p/composer.json exists.
        assert_eq!(lookup.path(), "/p/src/Http/composer.json");
        let ComposerStep::Next(lookup) = lookup.answer(None) else { panic!("the parent folder is asked next") };
        assert_eq!(lookup.path(), "/p/src/composer.json");
        let ComposerStep::Next(lookup) = lookup.answer(None) else { panic!("the project root is asked last") };
        assert_eq!(lookup.path(), "/p/composer.json");
        let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
        assert_eq!(lookup.answer(Some(composer)), ComposerStep::Done("App\\Http".to_owned()));
    }

    // plugin package inference uses built-in rules › does not infer a package for ordinary file formats
    #[test]
    fn does_not_infer_a_package_for_ordinary_file_formats() {
        assert_eq!(known(&plugin("md"), "/p/src/main/java/com/acme", &[]).as_deref(), Some(""));
    }

    // plugin package inference uses built-in rules › checks cancellation after async package inference and
    // before filesystem creation
    #[test]
    fn checks_cancellation_after_package_inference_and_before_filesystem_creation() {
        let template = plugin("md");
        let creation =
            plan_creation(&template, &template.kinds[0], "/p", "Readme", BTreeMap::new()).expect("a valid name");
        assert_eq!(creation.package_lookup(None, &[]), PackageLookup::Known(String::new()));
        let prepared = creation.render("", None, Includes::builtin());
        assert_eq!(prepared.plan.path, "/p/Readme.md");
        assert!(prepared.effects(true).is_empty());
        assert_eq!(
            prepared.effects(false),
            [
                CreateEffect::CreateFile("/p/Readme.md".to_owned()),
                CreateEffect::NotifyCreated("/p/Readme.md".to_owned()),
                CreateEffect::OpenFile("/p/Readme.md".to_owned()),
            ]
        );
    }

    // Not from a spec: the composer.json search stops at the project root, and without a project
    // only the file's own folder is asked.
    #[test]
    fn the_composer_search_stops_at_the_project_root() {
        let lookup = ComposerLookup::new("/p/src", Some("/p"));
        let ComposerStep::Next(at_root) = lookup.answer(None) else { panic!("the root is asked") };
        assert_eq!(at_root.path(), "/p/composer.json");
        assert_eq!(at_root.answer(None), ComposerStep::Done(String::new()));
        assert_eq!(ComposerLookup::new("/elsewhere/src", Some("/p")).answer(None), ComposerStep::Done(String::new()));
        assert_eq!(ComposerLookup::new("/p/src", None).answer(None), ComposerStep::Done(String::new()));
    }

    // Not from a spec: a Go test package names the package under test, a source file is preferred
    // to a test file, and roots detected from other build files do not count for JVM packages.
    #[test]
    fn sibling_packages_and_jvm_roots_are_chosen_as_the_ide_does() {
        let lookup = GoSiblingLookup { directory: "/p/pkg/store".to_owned(), project_root: Some("/p".to_owned()) };
        let files = [
            SiblingFile { name: "a_test.go", path: "/p/a_test.go" },
            SiblingFile { name: "notes.md", path: "/p/notes.md" },
            SiblingFile { name: "b.go", path: "/p/b.go" },
        ];
        assert_eq!(lookup.pick(&files), Some("/p/b.go"));
        assert_eq!(lookup.pick(&files[..2]), Some("/p/a_test.go"));
        assert_eq!(lookup.pick(&files[1..2]), None);
        assert_eq!(lookup.finish(Some("package store_test\n")), "store");
        assert_eq!(lookup.finish(None), "store");
        let root = |path: &str, kind: FolderRole, from: Option<&str>| RootFolder {
            path: path.to_owned(),
            kind,
            detected_from: from.map(str::to_owned),
        };
        let roots = [
            root("src/main/java", FolderRole::Source, Some("app/pom.xml")),
            root("web/src", FolderRole::Source, Some("package.json")),
            root("it", FolderRole::Test, None),
            root("res", FolderRole::Resources, None),
            root("gen", FolderRole::Source, Some("build.gradle.kts")),
        ];
        assert_eq!(package_roots(&roots), ["src/main/java", "it", "gen"]);
        assert_eq!(user_name("/Users/ann/work/p"), "ann");
        assert_eq!(user_name("/srv/p"), "");
    }

    // Not from a spec: the rendered body fills only an empty editor, with the caret at `$END$`,
    // and a file whose editor did not open is reported.
    #[test]
    fn fills_an_empty_editor_and_reports_a_missing_one() {
        let java = builtin_file_templates().iter().find(|t| t.id == "java").expect("the Java template");
        let creation = plan_creation(java, &java.kinds[0], "/p/src/main/java", "com.acme.Foo", BTreeMap::new())
            .expect("a valid name");
        let PackageLookup::Known(package) = creation.package_lookup(Some("/p"), &[]) else { panic!("a JVM package") };
        assert_eq!(package, "com.acme");
        let project = ProjectInfo { name: "p", root: "/p" };
        let prepared = creation.render(&package, Some(project), Includes::builtin());
        let fill = prepared.fill(0).expect("an empty editor is filled");
        assert_eq!(fill.insert, "package com.acme;\n\npublic class Foo {\n    \n}\n");
        assert_eq!(&fill.insert[fill.caret as usize..], "\n}\n");
        assert!(fill.reformat);
        assert_eq!(prepared.fill(3), None);
        let missing = prepared.editor_missing();
        assert_eq!((missing.level, missing.title.as_str()), (NotificationLevel::Warning, "Created Foo.java"));
        assert_eq!(caret_after_fill_reformat("a  b", 4, "a b", true), Some(3));
        assert_eq!(caret_after_fill_reformat("a b", 3, "a b", true), None);
        assert_eq!(caret_after_fill_reformat("a  b", 4, "a b", false), None);
        assert_eq!(plan_creation(java, &java.kinds[0], "/p", " ", BTreeMap::new()), Err(INVALID_FILE_NAME));
    }
}
