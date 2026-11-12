//! The built-in file templates of the New ▸ menu, with the includes and code templates they
//! `#parse`.

use super::files_model::{FileTemplate, FileTemplateKind, NameStyle, PackageStyle, TemplateFragment};
use std::sync::LazyLock;

/// Includes tab: fragments file templates pull in with `#parse("…")`.
pub fn builtin_includes() -> &'static [TemplateFragment] {
    static INCLUDES: LazyLock<Vec<TemplateFragment>> = LazyLock::new(|| {
        vec![TemplateFragment { name: "File Header".to_owned(), body: String::new(), description: None }]
    });
    &INCLUDES
}

/// Code tab: the members the built-in test templates generate.
pub fn builtin_code_templates() -> &'static [TemplateFragment] {
    static CODE: LazyLock<Vec<TemplateFragment>> = LazyLock::new(|| {
        [
            ("JUnit5 Test Method", "Java Class › JUnit 5 Test", "@Test\nvoid test() {\n    $END$\n}\n"),
            ("Kotlin JUnit5 Test Method", "Kotlin Class/File › JUnit 5 Test", "@Test\nfun test() {\n    $END$\n}\n"),
            ("Go Test Function", "Go File › Test file", "func Test$NAME$(t *testing.T) {\n\t$END$\n}\n"),
            ("Rust Test Function", "Rust File › Test module", "#[test]\nfn it_works() {\n    $END$\n}\n"),
            ("Vitest Test", "TypeScript File › Test", "it(\"works\", () => {\n  $END$\n});\n"),
            (
                "PHPUnit Test Method",
                "PHP Class › PHPUnit Test",
                "public function testSomething(): void\n{\n    $END$\n}\n",
            ),
            ("pytest Test Function", "Python File › Test (pytest)", "def test_$NAME$():\n    $END$\n"),
        ]
        .into_iter()
        .map(|(name, description, body)| TemplateFragment {
            name: name.to_owned(),
            body: body.to_owned(),
            description: Some(description.to_owned()),
        })
        .collect()
    });
    &CODE
}

fn kind(id: &str, label: &str, body: &str) -> FileTemplateKind {
    FileTemplateKind { id: id.to_owned(), label: label.to_owned(), body: body.to_owned(), extension: None }
}

/// A kind that creates a file with its own extension.
fn kind_ext(id: &str, label: &str, body: &str, extension: &str) -> FileTemplateKind {
    FileTemplateKind { extension: Some(extension.to_owned()), ..kind(id, label, body) }
}

fn template(
    id: &str,
    name: &str,
    icon: &str,
    extension: &str,
    name_style: NameStyle,
    kinds: Vec<FileTemplateKind>,
) -> FileTemplate {
    FileTemplate {
        id: id.to_owned(),
        name: name.to_owned(),
        icon: icon.to_owned(),
        extension: extension.to_owned(),
        name_style,
        kinds,
        ..FileTemplate::default()
    }
}

/// A file with a fixed name, prefilled in the dialog.
fn named(default_name: &str, template: FileTemplate) -> FileTemplate {
    FileTemplate { default_name: Some(default_name.to_owned()), ..template }
}

fn packaged(style: PackageStyle, template: FileTemplate) -> FileTemplate {
    let dotted_packages = style == PackageStyle::Jvm;
    FileTemplate { package_style: Some(style), dotted_packages, ..template }
}

/// A PHP type declaration in its namespace.
fn php_type(id: &str, label: &str, keyword: &str) -> FileTemplateKind {
    let body =
        format!("<?php\n\nnamespace $PACKAGE$;\n\n#parse(\"File Header\")\n{keyword} $NAME$\n{{\n    $END$\n}}\n");
    kind(id, label, &body)
}

/// A Java type declaration in its package.
fn java_type(id: &str, label: &str, keyword: &str) -> FileTemplateKind {
    let body = format!("package $PACKAGE$;\n\n#parse(\"File Header\")\npublic {keyword} $NAME$ {{\n    $END$\n}}\n");
    kind(id, label, &body)
}

/// A Kotlin type declaration in its package.
fn kotlin_type(id: &str, label: &str, keyword: &str) -> FileTemplateKind {
    let body = format!("package $PACKAGE$\n\n#parse(\"File Header\")\n{keyword} $NAME$ {{\n    $END$\n}}\n");
    kind(id, label, &body)
}

const HTML5: &str = "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"UTF-8\">\n  \
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>$NAME$</title>\n\
    </head>\n<body>\n$END$\n</body>\n</html>\n";

const EDITORCONFIG: &str = "root = true\n\n[*]\ncharset = utf-8\nend_of_line = lf\nindent_style = space\n\
    indent_size = 4\ninsert_final_newline = true\ntrim_trailing_whitespace = true\n$END$";

fn build() -> Vec<FileTemplate> {
    use NameStyle::{Asis, Pascal, Snake};
    vec![
        template("file", "File", "file", "", Asis, vec![kind("file", "File", "$END$")]),
        template(
            "rust",
            "Rust File",
            "file-rust",
            "rs",
            Snake,
            vec![
                kind("module", "Module", "$END$\n"),
                kind("struct", "Struct", "pub struct $NAME$ {\n    $END$\n}\n"),
                kind(
                    "test",
                    "Test module",
                    "#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #parse(\"Rust Test Function\")\n}\n",
                ),
            ],
        ),
        template(
            "typescript",
            "TypeScript File",
            "file-ts",
            "ts",
            Asis,
            vec![
                kind("file", "File", "$END$\n"),
                kind("class", "Class", "export class $NAME$ {\n  $END$\n}\n"),
                kind(
                    "test",
                    "Test",
                    "import { describe, expect, it } from \"vitest\";\n\ndescribe(\"$NAME$\", () => {\n  \
                     #parse(\"Vitest Test\")\n});\n",
                ),
            ],
        ),
        template(
            "tsx",
            "React Component",
            "file-tsx",
            "tsx",
            Pascal,
            vec![
                kind(
                    "component",
                    "Function component",
                    "export interface $NAME$Props {}\n\nexport function $NAME$(props: $NAME$Props) {\n  \
                     return <div>$END$</div>;\n}\n",
                ),
                kind(
                    "styled",
                    "Component with CSS module",
                    "import styles from \"./$NAME$.module.css\";\n\nexport function $NAME$() {\n  \
                     return <div className={styles.root}>$END$</div>;\n}\n",
                ),
            ],
        ),
        template("javascript", "JavaScript File", "file-js", "js", Asis, vec![kind("file", "File", "$END$\n")]),
        template(
            "python",
            "Python File",
            "file-python",
            "py",
            Snake,
            vec![
                kind("module", "Module", "\"\"\"$NAME$ module.\"\"\"\n\n$END$\n"),
                kind("class", "Class", "class $NAME$:\n    def __init__(self):\n        $END$\n"),
                kind("test", "Test (pytest)", "#parse(\"pytest Test Function\")\n"),
                kind("script", "Script", "def main():\n    $END$\n\n\nif __name__ == \"__main__\":\n    main()\n"),
            ],
        ),
        packaged(
            PackageStyle::Go,
            template(
                "go",
                "Go File",
                "file-go",
                "go",
                Snake,
                vec![
                    kind("file", "Empty file", "package $PACKAGE$\n\n$END$\n"),
                    kind(
                        "test",
                        "Test file",
                        "package $PACKAGE$\n\nimport \"testing\"\n\n#parse(\"Go Test Function\")\n",
                    ),
                    kind("main", "Main", "package main\n\nfunc main() {\n\t$END$\n}\n"),
                ],
            ),
        ),
        named(
            "go",
            template(
                "go-mod",
                "Go Module",
                "file-gomod",
                "mod",
                Asis,
                vec![kind("module", "Module", "module $PROJECT_NAME$\n\ngo 1.20\n")],
            ),
        ),
        template("php", "PHP File", "file-php", "php", Asis, vec![kind("file", "File", "<?php\n\n$END$\n")]),
        packaged(
            PackageStyle::Php,
            template(
                "php-class",
                "PHP Class",
                "class",
                "php",
                Pascal,
                vec![
                    php_type("class", "Class", "class"),
                    php_type("interface", "Interface", "interface"),
                    php_type("trait", "Trait", "trait"),
                    php_type("enum", "Enum", "enum"),
                    kind(
                        "test",
                        "PHPUnit Test",
                        "<?php\n\nnamespace $PACKAGE$;\n\nuse PHPUnit\\Framework\\TestCase;\n\n\
                         #parse(\"File Header\")\nfinal class $NAME$ extends TestCase\n{\n    \
                         #parse(\"PHPUnit Test Method\")\n}\n",
                    ),
                ],
            ),
        ),
        packaged(
            PackageStyle::Jvm,
            template(
                "java",
                "Java Class",
                "class",
                "java",
                Pascal,
                vec![
                    java_type("class", "Class", "class"),
                    java_type("interface", "Interface", "interface"),
                    java_type("enum", "Enum", "enum"),
                    kind(
                        "record",
                        "Record",
                        "package $PACKAGE$;\n\n#parse(\"File Header\")\npublic record $NAME$($END$) {\n}\n",
                    ),
                    java_type("annotation", "Annotation", "@interface"),
                    kind(
                        "test",
                        "JUnit 5 Test",
                        "package $PACKAGE$;\n\nimport org.junit.jupiter.api.Test;\n\n\
                         import static org.junit.jupiter.api.Assertions.*;\n\n#parse(\"File Header\")\n\
                         class $NAME$ {\n\n    #parse(\"JUnit5 Test Method\")\n}\n",
                    ),
                ],
            ),
        ),
        packaged(
            PackageStyle::Jvm,
            template(
                "kotlin",
                "Kotlin Class/File",
                "class",
                "kt",
                Pascal,
                vec![
                    kotlin_type("class", "Class", "class"),
                    kind("file", "File", "package $PACKAGE$\n\n$END$\n"),
                    kotlin_type("interface", "Interface", "interface"),
                    kind(
                        "data",
                        "Data class",
                        "package $PACKAGE$\n\n#parse(\"File Header\")\ndata class $NAME$($END$)\n",
                    ),
                    kotlin_type("enum", "Enum class", "enum class"),
                    kotlin_type("object", "Object", "object"),
                    kind(
                        "test",
                        "JUnit 5 Test",
                        "package $PACKAGE$\n\nimport org.junit.jupiter.api.Test\n\
                         import org.junit.jupiter.api.Assertions.*\n\n#parse(\"File Header\")\n\
                         class $NAME$ {\n\n    #parse(\"Kotlin JUnit5 Test Method\")\n}\n",
                    ),
                ],
            ),
        ),
        template(
            "html",
            "HTML File",
            "file-html",
            "html",
            Asis,
            vec![kind("html5", "HTML 5 file", HTML5), kind("fragment", "Fragment", "$END$\n")],
        ),
        template(
            "markdown",
            "Markdown File",
            "file-md",
            "md",
            Asis,
            vec![kind("md", "Markdown file", "# $NAME$\n\n$END$\n")],
        ),
        // IDEA's "Stylesheet": one dialog, the kind picks the language.
        template(
            "stylesheet",
            "Stylesheet",
            "file-css",
            "css",
            Asis,
            vec![
                kind("css", "CSS File", "$END$\n"),
                kind_ext("scss", "SCSS File", "$END$\n", "scss"),
                kind_ext("sass", "Sass File", "$END$\n", "sass"),
                kind_ext("less", "Less File", "$END$\n", "less"),
            ],
        ),
        template("json", "JSON File", "file-json", "json", Asis, vec![kind("json", "JSON file", "{\n  $END$\n}\n")]),
        template("yaml", "YAML File", "file-yaml", "yaml", Asis, vec![kind("yaml", "YAML file", "$END$\n")]),
        template("toml", "TOML File", "file-toml", "toml", Asis, vec![kind("toml", "TOML file", "$END$\n")]),
        template(
            "shell",
            "Shell Script",
            "file-shell",
            "sh",
            Asis,
            vec![
                kind("bash", "Bash script", "#!/usr/bin/env bash\nset -euo pipefail\n\n$END$\n"),
                kind("sh", "POSIX sh script", "#!/bin/sh\n\n$END$\n"),
                kind_ext("zsh", "Zsh script", "#!/usr/bin/env zsh\n\n$END$\n", "zsh"),
            ],
        ),
        named(
            "Dockerfile",
            template(
                "dockerfile",
                "Dockerfile",
                "file-docker",
                "",
                Asis,
                vec![kind("dockerfile", "Dockerfile", "FROM $END$\n")],
            ),
        ),
        named(
            ".editorconfig",
            template(
                "editorconfig",
                "EditorConfig File",
                "file-config",
                "",
                Asis,
                vec![
                    kind("default", "Default properties", EDITORCONFIG),
                    kind("root", "Root only", "root = true\n\n[*]\n$END$\n"),
                ],
            ),
        ),
        named(
            ".gitignore",
            template(
                "gitignore",
                ".gitignore File",
                "file-git",
                "",
                Asis,
                vec![kind("gitignore", ".gitignore file", "$END$\n")],
            ),
        ),
        named(
            "context",
            template(
                "tomcat-context",
                "Tomcat Context Descriptor",
                "file-config",
                "xml",
                Asis,
                vec![kind(
                    "context",
                    "Tomcat Context Descriptor (context.xml)",
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Context antiJARLocking=\"true\" path=\"$END$\">\n\
                     </Context>\n",
                )],
            ),
        ),
    ]
}

/// The built-in templates, in menu order.
pub fn builtin_file_templates() -> &'static [FileTemplate] {
    static TEMPLATES: LazyLock<Vec<FileTemplate>> = LazyLock::new(build);
    &TEMPLATES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(template: &str, kind: &str) -> &'static str {
        let template = builtin_file_templates().iter().find(|t| t.id == template).expect("a built-in template");
        template.kinds.iter().find(|k| k.id == kind).map(|k| k.body.as_str()).expect("a kind of the template")
    }

    // Not from a spec: the bodies built from shared pieces and the long ones split over lines
    // read exactly as the templates the menu creates.
    #[test]
    fn bodies_are_spelled_out_in_full() {
        assert_eq!(
            body("php-class", "interface"),
            "<?php\n\nnamespace $PACKAGE$;\n\n#parse(\"File Header\")\ninterface $NAME$\n{\n    $END$\n}\n"
        );
        assert_eq!(
            body("java", "annotation"),
            "package $PACKAGE$;\n\n#parse(\"File Header\")\npublic @interface $NAME$ {\n    $END$\n}\n"
        );
        assert_eq!(
            body("kotlin", "enum"),
            "package $PACKAGE$\n\n#parse(\"File Header\")\nenum class $NAME$ {\n    $END$\n}\n"
        );
        assert!(body("html", "html5").contains("<head>\n  <meta charset=\"UTF-8\">\n  <meta name=\"viewport\""));
        assert!(body("html", "html5").ends_with("<title>$NAME$</title>\n</head>\n<body>\n$END$\n</body>\n</html>\n"));
        assert!(body("editorconfig", "default").contains("indent_style = space\nindent_size = 4\n"));
        assert!(body("tsx", "component").ends_with("{\n  return <div>$END$</div>;\n}\n"));
        assert!(body("typescript", "test").ends_with("() => {\n  #parse(\"Vitest Test\")\n});\n"));
        let java_test = body("java", "test");
        assert!(java_test.contains("Assertions.*;\n\n#parse(\"File Header\")\nclass $NAME$ {\n\n    #parse("));
        assert!(body("tomcat-context", "context").ends_with("path=\"$END$\">\n</Context>\n"));
    }

    // Not from a spec: the JVM templates take dotted names, and only the templates with a package
    // rule declare one.
    #[test]
    fn package_rules_are_declared_per_language() {
        let style = |id: &str| {
            let template = builtin_file_templates().iter().find(|t| t.id == id);
            template.map(|t| (t.package_style, t.dotted_packages))
        };
        assert_eq!(style("java"), Some((Some(PackageStyle::Jvm), true)));
        assert_eq!(style("kotlin"), Some((Some(PackageStyle::Jvm), true)));
        assert_eq!(style("go"), Some((Some(PackageStyle::Go), false)));
        assert_eq!(style("php-class"), Some((Some(PackageStyle::Php), false)));
        assert_eq!(style("php"), Some((None, false)));
        assert_eq!(builtin_code_templates().len(), 7);
        assert_eq!(builtin_includes().first().map(|f| f.name.as_str()), Some("File Header"));
    }
}
