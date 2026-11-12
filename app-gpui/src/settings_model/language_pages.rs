//! The Languages & Frameworks pages and Project Structure: what the Settings tree lists for them,
//! the language levels their selects offer, and the small helpers of their forms. The settings
//! themselves are the backend's `LanguagesSettings` and `ProjectStructureSettings`.

use super::page_spec::PageSpec;

/// The group of the language pages.
pub const LANGUAGES_GROUP: &str = "Languages & Frameworks";

/// The values of `languages.php.languageLevel`, newest first; `""` is Auto (from `composer.json`).
pub const PHP_LANGUAGE_LEVELS: [&str; 13] =
    ["", "8.5", "8.4", "8.3", "8.2", "8.1", "8.0", "7.4", "7.3", "7.2", "7.1", "7.0", "5.6"];

/// The values of `languages.java.languageLevel`; `""` is the SDK's default.
pub const JAVA_LANGUAGE_LEVELS: [&str; 6] = ["", "8", "11", "17", "21", "25"];

/// A page of a toolchain: it belongs to the project, whose values show first.
fn toolchain(id: &'static str, title: &'static str, order: f64, keywords: &[&'static str]) -> PageSpec {
    PageSpec::new(id, title, order).group(LANGUAGES_GROUP).project_scope().project_first().keywords(keywords)
}

/// The pages, in the order they are registered. Project Structure sits before Build, Execution,
/// Deployment as in IntelliJ IDEA; Markdown is global only, like IDEA's application-level
/// Markdown settings; the Go sub-pages pick their scope themselves.
pub fn language_pages() -> Vec<PageSpec> {
    vec![
        toolchain("languages.serverTargets", "Language server targets", 451.0, &["lsp", "container", "target"]),
        PageSpec::new("languages.pluginFormatters", "Plugin Formatters", 450)
            .group(LANGUAGES_GROUP)
            .project_scope()
            .keywords(&["format", "plugins"]),
        toolchain(
            "python.interpreter",
            "Python Interpreter",
            403.0,
            &[
                "python interpreter",
                "sdk",
                "virtualenv",
                "venv",
                "conda",
                "pyenv",
                "packages",
                "pip",
                "install package",
                "add interpreter",
                "activate virtualenv",
            ],
        ),
        PageSpec::new("projectStructure", "Project Structure", 350).keywords(&[
            "project structure",
            "content root",
            "sources root",
            "test sources",
            "resources",
            "excluded",
            "mark directory as",
            "project sdk",
            "language level",
            "modules",
            "libraries",
            "facets",
            "artifacts",
            "sdks",
        ]),
        toolchain("languages.go", "Go", 390.0, &["goroot", "gopath", "go sdk", "gopls"]),
        toolchain("languages.go.buildTags", "Build Tags", 391.0, &[]).own_scope_switch(),
        toolchain("languages.go.modules", "Go Modules", 392.0, &[]).own_scope_switch(),
        toolchain(
            "languages.java",
            "Java",
            395.0,
            &[
                "jdk",
                "project sdk",
                "java home",
                "jdtls",
                "language server jdk",
                "jvm options",
                "vmargs",
                "lombok",
                "javaagent",
                "eclipse jdt",
                "shared jdtls",
                "linked builds",
                "server process",
            ],
        ),
        toolchain(
            "languages.node",
            "Node.js",
            402.0,
            &["node interpreter", "package manager", "npm", "pnpm", "yarn", "bun", "nvm"],
        ),
        PageSpec::new("languages.markdown", "Markdown", 400).group(LANGUAGES_GROUP).keywords(&[
            "markdown",
            "preview",
            "font size",
            "jupyter",
            "notebook",
        ]),
        toolchain(
            "languages.prettier",
            "Prettier",
            403.5,
            &["formatter", "prettier", "markdown", "yaml", "css", "format on save", "node_modules"],
        ),
        toolchain(
            "languages.oxfmt",
            "oxfmt",
            403.6,
            &["formatter", "oxfmt", "oxc", ".oxfmtrc.json", "format on save", "node_modules"],
        ),
        toolchain(
            "languages.ruff",
            "Ruff",
            403.7,
            &[
                "formatter",
                "ruff",
                "python",
                "configuration directory",
                "pyproject.toml",
                "ruff.toml",
                "format on save",
            ],
        ),
        toolchain(
            "languages.php",
            "PHP",
            403.2,
            &[
                "php interpreter",
                "cli interpreter",
                "language level",
                "php version",
                "xdebug",
                "php",
                "composer",
                "phpunit",
                "phpstan",
                "phpstan.neon",
                "xampp",
                "herd",
                "homebrew",
                "phpbrew",
            ],
        ),
        toolchain(
            "languages.ruby",
            "Ruby",
            403.8,
            &["ruby interpreter", "rbenv", "rvm", "chruby", "bundler", "gems", "rake", "rspec"],
        ),
        toolchain(
            "languages.rust",
            "Rust",
            404.0,
            &[
                "toolchain",
                "rustup",
                "standard library",
                "external linter",
                "clippy",
                "cargo check",
                "rustfmt",
                "rust-analyzer",
            ],
        ),
        toolchain(
            "languages.typescript",
            "TypeScript",
            405.0,
            &["typescript version", "tsdk", "tsserver", "node_modules", "vtsls"],
        ),
    ]
}

/// A package installed for a Python interpreter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PythonPackage {
    pub name: String,
    pub version: String,
}

/// The packages whose name holds the query, whatever its case; all of them without a query.
pub fn filter_packages<'p>(packages: &'p [PythonPackage], query: &str) -> Vec<&'p PythonPackage> {
    let query = query.trim().to_lowercase();
    packages.iter().filter(|package| package.name.to_lowercase().contains(&query)).collect()
}

/// `~/…` for a path in a home directory (`/Users/<name>`, `/home/<name>`, `C:/Users/<name>`),
/// whoever's it is: detected toolchains are shown this way on every machine.
pub fn tildify(path: &str) -> String {
    let is_separator = |c: char| c == '/' || c == '\\';
    let drive_home = || {
        let mut chars = path.chars();
        let drive = chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.next() == Some(':');
        let rest = path.get(2..).filter(|_| drive)?.strip_prefix(is_separator)?;
        rest.strip_prefix("Users")
    };
    let homes = path.strip_prefix("/Users").or_else(|| path.strip_prefix("/home")).or_else(drive_home);
    let Some(name_and_rest) = homes.and_then(|rest| rest.strip_prefix(is_separator)) else {
        return path.to_owned();
    };
    let name_len = name_and_rest.find(is_separator).unwrap_or(name_and_rest.len());
    if name_len == 0 { path.to_owned() } else { format!("~{}", &name_and_rest[name_len..]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::pages::{SettingsPage, build_page_tree};
    use crate::settings_model::schema::{parse_settings, settings_value};
    use crate::settings_model::staging::Scope;
    use ide_core::settings::{
        ExternalFormatterSettings, GoBuildTags, GoModules, LanguagesSettings, MarkdownSettings, PhpSettings,
        PythonSettings, Settings,
    };
    use serde_json::{Value, json};

    fn languages(stored: Value) -> LanguagesSettings {
        parse_settings(&json!({ "languages": stored })).expect("the backend takes the value").languages
    }

    fn refused(stored: Value) -> bool {
        parse_settings(&json!({ "languages": stored })).is_err()
    }

    fn page(id: &str) -> PageSpec {
        language_pages().into_iter().find(|page| page.id == id).unwrap_or_else(|| panic!("no page {id}"))
    }

    // Go build tags › keeps old project settings unchanged and ignores malformed values
    #[test]
    fn keeps_old_project_settings_unchanged() {
        let go = languages(json!({ "go": { "goroot": "/go" } })).go;
        assert_eq!(go.goroot, "/go");
        assert_eq!(go.build_tags, GoBuildTags::default());
        // Malformed constraints are refused by the backend, not replaced by the defaults.
        assert!(refused(json!({ "go": { "buildTags": { "os": false, "arch": [], "custom": 42 } } })));
    }

    // Go build tags › preserves independent project constraints
    #[test]
    fn preserves_independent_project_constraints() {
        let stored = json!({
            "os": "linux",
            "arch": "arm64",
            "cgo": "0",
            "version": "go1.22",
            "compiler": "gc",
            "custom": "integration debug",
            "experiments": "loopvar",
        });
        let tags = languages(json!({ "go": { "buildTags": stored.clone() } })).go.build_tags;
        assert_eq!(serde_json::to_value(&tags).ok(), Some(stored));
        // Values are kept as written; the webview trimmed them.
        assert_eq!(languages(json!({ "go": { "buildTags": { "os": " linux " } } })).go.build_tags.os, " linux ");
        let page = page("languages.go.buildTags");
        assert_eq!(page.default_scope, Some(Scope::Project));
        assert!(page.own_scope_switch);
    }

    // languages settings › defaults and normalises malformed values
    #[test]
    fn defaults_and_keeps_stored_values() {
        assert_eq!(parse_settings(&json!({})).map(|settings| settings.languages), Ok(LanguagesSettings::default()));
        let v = languages(json!({
            "pluginFormatters": { "sql": "example.formatter" },
            "python": { "interpreter": "/usr/bin/python3", "activateVirtualenv": true },
            "node": { "packageManager": "pip" },
            "rust": { "checkCommand": "clippy", "checkOnSave": false },
            "java": {
                "jdtlsVmArgs": "-Xmx2g",
                "jdtlsJdk": "/opt/jdk-25",
                "maven": { "home": "/opt/maven", "threads": "2C" },
            },
        }));
        assert_eq!(v.plugin_formatters.get("sql").map(String::as_str), Some("example.formatter"));
        assert_eq!(v.python, PythonSettings { interpreter: "/usr/bin/python3".to_owned(), activate_virtualenv: true });
        // Any package manager name is kept; the webview only kept the ones it knows.
        assert_eq!(v.node.package_manager, "pip");
        assert_eq!((v.rust.check_command.as_str(), v.rust.check_on_save), ("clippy", false));
        assert_eq!(v.typescript.version, "bundled");
        assert_eq!((v.java.jdtls_vm_args.as_str(), v.java.jdk.as_str()), ("-Xmx2g", ""));
        assert_eq!(v.java.jdtls_jdk, "/opt/jdk-25");
        let maven = &v.java.maven;
        assert_eq!((maven.home.as_str(), maven.threads.as_str(), maven.daemon), ("/opt/maven", "2C", false));
        assert!(maven.runner_jre.is_empty() && maven.user_settings.is_empty() && maven.local_repository.is_empty());
        assert!(maven.linked_projects.is_empty() && maven.unlinked_projects.is_empty());
        // Wrong types are refused; the webview fell back to the defaults.
        for malformed in [
            json!({ "pluginFormatters": { "text": 42 } }),
            json!({ "python": { "activateVirtualenv": "yes" } }),
            json!({ "java": { "jdk": 21 } }),
            json!({ "java": { "maven": { "daemon": "yes" } } }),
            json!({ "java": { "maven": { "threads": 4 } } }),
        ] {
            assert!(refused(malformed.clone()), "{malformed}");
        }
    }

    // languages settings › defaults the Maven runner JRE to the project JDK and preserves a chosen JDK
    #[test]
    fn defaults_the_maven_runner_jre_to_the_project_jdk_and_preserves_a_chosen_jdk() {
        assert_eq!(Settings::default().languages.java.maven.runner_jre, "");
        assert!(refused(json!({ "java": { "maven": { "runnerJre": 21 } } })));
        let stored = json!({ "languages": { "java": { "maven": { "runnerJre": "./jdks/21" } } } });
        let settings = parse_settings(&stored).expect("the backend takes the value");
        let again = parse_settings(&settings_value(&settings)).expect("the settings round-trip");
        assert_eq!(again.languages.java.maven.runner_jre, "./jdks/21");
    }

    // languages settings › keeps the Ruby interpreter through settings normalization and defaults to Auto
    #[test]
    fn keeps_the_ruby_interpreter_and_defaults_to_auto() {
        assert_eq!(Settings::default().languages.ruby.interpreter, "");
        assert!(refused(json!({ "ruby": { "interpreter": 42 } })));
        let stored = json!({ "languages": { "ruby": { "interpreter": "/opt/ruby/bin/ruby" } } });
        let settings = parse_settings(&stored).expect("the backend takes the value");
        assert_eq!(settings.languages.ruby.interpreter, "/opt/ruby/bin/ruby");
        let again = parse_settings(&settings_value(&settings)).expect("the settings round-trip");
        assert_eq!(again.languages.ruby.interpreter, "/opt/ruby/bin/ruby");
    }

    // languages settings › defaults the PHP language level to Auto and keeps only known levels
    #[test]
    fn defaults_the_php_language_level_to_auto() {
        assert_eq!(Settings::default().languages.php, PhpSettings::default());
        let php = PhpSettings::default();
        assert!(php.run_target.is_empty() && php.interpreter.is_empty() && php.language_level.is_empty());
        assert!(php.include_paths.is_empty() && php.stubs.is_empty() && php.phpstan_config.is_empty());
        let stored = languages(json!({ "php": { "phpstanConfig": "build/phpstan.neon", "languageLevel": "7.4" } })).php;
        assert_eq!((stored.phpstan_config.as_str(), stored.language_level.as_str()), ("build/phpstan.neon", "7.4"));
        assert_eq!(languages(json!({ "php": { "includePaths": ["../platform"] } })).php.include_paths, ["../platform"]);
        // The backend keeps a level it does not know; the select offers only these.
        assert_eq!(languages(json!({ "php": { "languageLevel": "9.0" } })).php.language_level, "9.0");
        for malformed in [json!({ "phpstanConfig": 3 }), json!({ "stubs": "redis" }), json!({ "languageLevel": 8.1 })] {
            assert!(refused(json!({ "php": malformed })));
        }
        assert_eq!(PHP_LANGUAGE_LEVELS[0], "");
        assert!(PHP_LANGUAGE_LEVELS.contains(&"5.6"));
        // Newest first after Auto.
        assert_eq!(PHP_LANGUAGE_LEVELS[1], "8.5");
        assert_eq!(PHP_LANGUAGE_LEVELS.last(), Some(&"5.6"));
    }

    // languages settings › finds the PHP page by its language level
    #[test]
    fn finds_the_php_page_by_its_language_level() {
        assert!(page("languages.php").keywords.contains(&"language level"));
    }

    // languages settings › keeps Prettier opt-in on save and normalizes its enabled languages
    #[test]
    fn keeps_prettier_opt_in_on_save() {
        let prettier = languages(json!({})).prettier;
        assert!(!prettier.prefer && !prettier.run_on_save && prettier.path.is_empty());
        let stored = json!({ "path": "./bin/prettier", "languages": ["markdown"], "prefer": true, "runOnSave": true });
        let expected = ExternalFormatterSettings {
            path: "./bin/prettier".to_owned(),
            configuration_directory: String::new(),
            languages: vec!["markdown".to_owned()],
            prefer: true,
            run_on_save: true,
        };
        assert_eq!(languages(json!({ "prettier": stored })).prettier, expected);
        assert!(languages(json!({ "prettier": { "languages": [] } })).prettier.languages.is_empty());
        assert!(refused(json!({ "prettier": { "languages": ["markdown", 4] } })));
    }

    // languages settings › gives oxfmt the same opt-in settings as Prettier, kept apart from them
    #[test]
    fn gives_oxfmt_the_same_opt_in_settings_as_prettier_kept_apart_from_them() {
        let defaults = LanguagesSettings::default();
        assert_eq!(defaults.oxfmt, defaults.prettier);
        let v = languages(json!({
            "prettier": { "runOnSave": true },
            "oxfmt": { "path": "./bin/oxfmt", "languages": ["typescript"], "prefer": true },
        }));
        assert_eq!(v.oxfmt.path, "./bin/oxfmt");
        assert_eq!(v.oxfmt.languages, ["typescript"]);
        assert!(v.oxfmt.prefer && !v.oxfmt.run_on_save && v.oxfmt.configuration_directory.is_empty());
        assert!(v.prettier.run_on_save && !v.prettier.prefer && v.prettier.path.is_empty());
    }

    // languages settings › enables Ruff for Python by default and keeps its settings separate
    #[test]
    fn enables_ruff_for_python_by_default_and_keeps_its_settings_separate() {
        let ruff = languages(json!({})).ruff;
        assert_eq!(ruff.languages, ["python"]);
        assert!(ruff.path.is_empty() && ruff.configuration_directory.is_empty() && !ruff.prefer && !ruff.run_on_save);
        let v = languages(json!({
            "ruff": {
                "path": "./bin/ruff",
                "configurationDirectory": "./config/ruff",
                "languages": ["python", "pyi"],
                "prefer": true,
                "runOnSave": true,
            },
        }));
        assert_eq!((v.ruff.path.as_str(), v.ruff.configuration_directory.as_str()), ("./bin/ruff", "./config/ruff"));
        assert_eq!(v.ruff.languages, ["python", "pyi"]);
        assert!(v.ruff.prefer && v.ruff.run_on_save);
        assert_eq!(v.prettier, LanguagesSettings::default().prettier);
    }

    // languages settings › leaves the Markdown preview font size unset (editor font) and keeps a set one in 8..72
    #[test]
    fn leaves_the_markdown_preview_font_size_unset() {
        assert_eq!(languages(json!({})).markdown, MarkdownSettings { preview_font_size: None });
        // The backend keeps the size as stored; the webview kept it within 8..72.
        assert_eq!(languages(json!({ "markdown": { "previewFontSize": 90 } })).markdown.preview_font_size, Some(90));
        assert!(refused(json!({ "markdown": { "previewFontSize": "x" } })));
    }

    // languages settings › keeps the Markdown page global, like IDEA's application-level setting
    #[test]
    fn keeps_the_markdown_page_global() {
        let markdown = page("languages.markdown");
        assert!(!markdown.project_scope);
        assert_eq!(markdown.default_scope, None);
    }

    // languages settings › defaults shared JDT LS to off and accepts only boolean values
    #[test]
    fn defaults_shared_jdt_ls_to_off_and_accepts_only_boolean_values() {
        assert!(!LanguagesSettings::default().java.jdtls_shared);
        assert!(!languages(json!({})).java.jdtls_shared);
        assert!(languages(json!({ "java": { "jdtlsShared": true } })).java.jdtls_shared);
        assert!(!languages(json!({ "java": { "jdtlsShared": false } })).java.jdtls_shared);
        for invalid in [json!("true"), json!(1), json!(null), json!([])] {
            assert!(refused(json!({ "java": { "jdtlsShared": invalid } })));
        }
    }

    // languages settings › defaults Java language level to SDK default and keeps only supported levels
    #[test]
    fn defaults_java_language_level_to_sdk_default() {
        assert_eq!(languages(json!({})).java.language_level, "");
        for level in ["8", "21"] {
            assert_eq!(languages(json!({ "java": { "languageLevel": level } })).java.language_level, level);
        }
        // The backend keeps a level the select does not offer; other types are refused.
        assert_eq!(languages(json!({ "java": { "languageLevel": "6" } })).java.language_level, "6");
        assert!(refused(json!({ "java": { "languageLevel": 17 } })));
        assert!(refused(json!({ "java": { "languageLevel": null } })));
        assert_eq!(JAVA_LANGUAGE_LEVELS[0], "");
        assert!(JAVA_LANGUAGE_LEVELS.contains(&"8") && JAVA_LANGUAGE_LEVELS.contains(&"25"));
    }

    // languages settings › normalizes project structure modules, libraries, facets, and artifacts
    //
    // The backend keeps a project structure as stored: this is what the webview's normalisation
    // produced, and it reads back unchanged.
    #[test]
    fn keeps_project_structure_modules_libraries_facets_and_artifacts() {
        let stored = json!({
            "compilerOutput": "out/production",
            "modules": {
                "modules/core": {
                    "jdk": "/opt/jdk-21",
                    "languageLevel": "17",
                    "compilerOutput": "target/classes",
                    "testCompilerOutput": "target/test-classes",
                    "dependencies": [
                        { "target": "common-lib", "kind": "library", "scope": "provided", "exported": true },
                    ],
                },
            },
            "libraries": [{
                "name": "servlet-api",
                "classes": ["lib/servlet-api.jar"],
                "sources": ["lib/servlet-api-sources.jar"],
                "javadocs": [],
                "scope": "provided",
            }],
            "facets": [{
                "kind": "web",
                "module": "modules/web",
                "descriptor": "src/main/webapp/WEB-INF/web.xml",
                "webRoot": "src/main/webapp",
            }],
            "artifacts": [{
                "name": "app:war exploded",
                "kind": "warExploded",
                "module": "modules/web",
                "outputDirectory": "target/app-exploded",
                "mainClass": "com.example.Main",
                "includeLibraries": ["servlet-api"],
            }],
        });
        let settings = parse_settings(&json!({ "projectStructure": stored.clone() })).expect("the backend takes it");
        let kept = settings_value(&settings)["projectStructure"].clone();
        for key in ["compilerOutput", "modules", "libraries", "facets", "artifacts"] {
            assert_eq!(kept[key], stored[key], "{key}");
        }
        let bad_scope = json!({ "modules": { "m": { "dependencies": [{ "scope": "invalid" }] } } });
        assert!(parse_settings(&json!({ "projectStructure": bad_scope })).is_err());
    }

    // language pages › keeps every language under Languages & Frameworks, as in IntelliJ IDEA
    #[test]
    fn keeps_every_language_under_languages_and_frameworks_as_in_intellij_idea() {
        let pages: Vec<SettingsPage> = language_pages().iter().map(SettingsPage::from).collect();
        let tree = build_page_tree(&pages);
        let titles: Vec<&str> = tree.iter().map(|node| node.title.as_str()).collect();
        assert_eq!(titles, ["Project Structure", "Languages & Frameworks"]);
        let languages: Vec<&str> = tree[1].children.iter().map(|node| node.title.as_str()).collect();
        assert_eq!(
            languages,
            [
                "Go",
                "Java",
                "Markdown",
                "Node.js",
                "Python Interpreter",
                "PHP",
                "Prettier",
                "oxfmt",
                "Ruff",
                "Ruby",
                "Rust",
                "TypeScript",
                "Plugin Formatters",
                "Language server targets",
            ]
        );
    }

    // language pages › filters packages and shortens home paths
    #[test]
    fn filters_packages_and_shortens_home_paths() {
        let package = |name: &str, version: &str| PythonPackage { name: name.to_owned(), version: version.to_owned() };
        let packages = [package("Django", "5.1"), package("requests", "2.32.0")];
        assert_eq!(filter_packages(&packages, "dj").iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Django"]);
        assert_eq!(filter_packages(&packages, "").len(), 2);
        assert_eq!(tildify("/Users/me/p/.venv/bin/python"), "~/p/.venv/bin/python");
        let wrapper = "C:/Users/me/.m2/wrapper/dists/apache-maven-3.9.16";
        assert_eq!(tildify(wrapper), "~/.m2/wrapper/dists/apache-maven-3.9.16");
        assert_eq!(tildify("C:/Program Files/Java/jdk-21"), "C:/Program Files/Java/jdk-21");
    }

    // Go module settings › keeps automatic vendoring by default and normalizes malformed environment entries
    #[test]
    fn keeps_automatic_vendoring_by_default() {
        assert_eq!(languages(json!({ "go": {} })).go.modules, GoModules { env: Default::default(), auto_vendor: true });
        let env = json!({ "GOPROXY": "https://proxy.example", "GOPRIVATE": "example.com/private", "GOFLAGS": "-race" });
        let modules = languages(json!({ "go": { "modules": { "autoVendor": false, "env": env.clone() } } })).go.modules;
        assert!(!modules.auto_vendor);
        assert_eq!(serde_json::to_value(&modules.env).ok(), Some(env));
        // Malformed entries are refused by the backend, not dropped.
        assert!(refused(json!({ "go": { "modules": { "env": { "INVALID": 3 } } } })));
        assert!(refused(json!({ "go": { "modules": { "autoVendor": "no", "env": [] } } })));
    }

    // Not from a spec: home folders are recognised with either separator, and a path that only
    // starts like one is left alone.
    #[test]
    fn shortens_home_folders_of_every_platform() {
        assert_eq!(tildify("/home/ann"), "~");
        assert_eq!(tildify(r"C:\Users\ann\go"), r"~\go");
        assert_eq!(tildify("/Users"), "/Users");
        assert_eq!(tildify("/Users/"), "/Users/");
        assert_eq!(tildify("/Usersx/ann"), "/Usersx/ann");
        assert_eq!(tildify("relative/Users/ann"), "relative/Users/ann");
        assert_eq!(page("languages.pluginFormatters").default_scope, None);
        assert!(page("languages.rust").project_scope);
    }
}
