//! Languages & Frameworks › Go (Build Tags, Go Modules), Java, Markdown, Node.js, Python
//! (Dependencies, Ruff), PHP, Prettier, oxfmt, Ruby, Rust and TypeScript: each page's controls in
//! their sections, drawn as the page's own view.
//!
//! A page edits the scope its Global / This project switch shows; the Go build tags and modules
//! belong to the project only.

use super::languages_controls::{
    Control, FormatterLanguagesView, JdtlsSharedView, LanguagePageView, PhpInterpreterView, PreviewFontSizeView,
    ProjectOnlyView, Section,
};
use super::languages_sdk::{
    LanguageAsk, PathFieldView, PathPlaceholder, PathSpec, SdkChoices, SdkFieldView, SdkSpec, Toolchain, ToolchainKind,
    Toolchains, answer, sdk,
};
use super::list_editor::{ListEditor, ListShape, ListSpec};
use super::{Binding, ChoicesFn, DetailFn, FieldKind, FormView, Part, ScopeOf, field, fixed_choices, toggle};
use crate::settings_model::language_pages::{JAVA_LANGUAGE_LEVELS, PHP_LANGUAGE_LEVELS, tildify};
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::settings_view::controls::tx;
use crate::settings_view::dialog::SettingsDialogView;
use gpui_kit::{App, AppContext, Entity, SharedString, Subscription};
use serde_json::Value;
use std::rc::Rc;

pub const GO: &str = "languages.go";
pub const GO_BUILD_TAGS: &str = "languages.go.buildTags";
pub const GO_MODULES: &str = "languages.go.modules";
pub const JAVA: &str = "languages.java";
pub const MARKDOWN: &str = "languages.markdown";
pub const NODE: &str = "languages.node";
pub const PYTHON: &str = "python.interpreter";
pub const PHP: &str = "languages.php";
pub const PRETTIER: &str = "languages.prettier";
pub const OXFMT: &str = "languages.oxfmt";
pub const RUFF: &str = "python.ruff";
pub const RUBY: &str = "languages.ruby";
pub const RUST: &str = "languages.rust";
pub const TYPESCRIPT: &str = "languages.typescript";

/// The pages drawn here, in the order the Settings tree lists them.
pub const PAGES: [&str; 14] =
    [GO, GO_BUILD_TAGS, GO_MODULES, JAVA, MARKDOWN, NODE, PYTHON, PHP, PRETTIER, OXFMT, RUFF, RUBY, RUST, TYPESCRIPT];

/// What the pages are built over.
struct Build {
    model: Entity<SettingsDialog>,
    scope: ScopeOf,
    toolchains: Toolchains,
    /// A project is open and the dialog does not edit the settings for new projects.
    project_open: bool,
}

impl Build {
    /// The page's controls edit the scope the page shows.
    fn binding(&self, page: &'static str) -> Binding {
        Binding::new(self.model.clone(), page, self.scope.clone())
    }

    /// The page's controls of the project's settings only.
    fn project(&self, page: &'static str) -> Binding {
        Binding::fixed(self.model.clone(), page, Scope::Project)
    }
}

fn form(binding: Binding, parts: Vec<Part>, cx: &mut App) -> Control {
    Control::Form(cx.new(|cx| FormView::new(binding, parts, cx)))
}

/// A form whose drop-down `key` names a detail after some of its choices.
fn form_with_details(binding: Binding, parts: Vec<Part>, key: &'static str, detail: DetailFn, cx: &mut App) -> Control {
    Control::Form(cx.new(|cx| FormView::new(binding, parts, cx).with_details(key, detail)))
}

/// The detail of a drop-down's empty choice (`from composer.json`).
fn empty_detail(detail: &'static str) -> DetailFn {
    Rc::new(move |value: &Value, cx: &App| (value.as_str() == Some("")).then(|| tx(cx, detail)))
}

fn sdk_field(build: &Build, binding: Binding, spec: SdkSpec, cx: &mut App) -> Control {
    let toolchains = build.toolchains.clone();
    Control::Sdk(cx.new(|cx| SdkFieldView::new(binding, spec, toolchains, cx)))
}

fn path_field(build: &Build, binding: Binding, spec: PathSpec, cx: &mut App) -> Control {
    let toolchains = build.toolchains.clone();
    Control::Path(cx.new(|cx| PathFieldView::new(binding, spec, toolchains, cx)))
}

fn list(binding: Binding, spec: ListSpec, cx: &mut App) -> Control {
    Control::List(cx.new(|cx| ListEditor::from_spec(binding, spec, cx)))
}

/// A drop-down with a help line.
fn select_with_hint(key: &'static str, label: &'static str, choices: ChoicesFn, hint: &'static str) -> Part {
    Part::Select { key, label, choices, hint: Some(SharedString::from(hint)), beside: None, indent: false }
}

/// The choices of a language level: `auto` for the empty value, then each level.
fn levels(levels: &'static [&'static str], auto: &'static str) -> ChoicesFn {
    let list: Vec<(Value, String)> = levels
        .iter()
        .map(|level| {
            if level.is_empty() {
                (Value::from(""), auto.to_owned())
            } else {
                (Value::from(*level), (*level).to_owned())
            }
        })
        .collect();
    Rc::new(move |_, _| list.clone())
}

/// Run in target: the project's default, the local machine, and a stored target.
fn run_target(key: &'static str) -> Part {
    let choices: ChoicesFn = Rc::new(move |binding: &Binding, cx: &App| {
        let mut list =
            vec![(Value::from(""), tx(cx, "Default run target")), (Value::from("local"), tx(cx, "Local machine"))];
        let target = binding.text(key, cx);
        if !target.is_empty() && target != "local" {
            list.push((Value::from(target.clone()), format!("{} {target}", tx(cx, "Run in target"))));
        }
        list
    });
    super::select(key, "Run in target", choices)
}

// ---------------------------------------------------------------------------
// Go

const GOROOT_HINT: &str = "Go SDK used by gopls and the go run / test configurations.";

fn go(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(GO);
    let goroot = sdk("languages.go.goroot", "GOROOT", ToolchainKind::Go)
        .empty("Default", "go on PATH")
        .directory()
        .hint(GOROOT_HINT);
    let gopath = PathSpec {
        key: "languages.go.gopath",
        label: "Project GOPATH",
        placeholder: PathPlaceholder::GoPath,
        directory: true,
        hint: None,
    };
    let sections = vec![
        (Some("GOROOT"), vec![sdk_field(build, binding.clone(), goroot, cx)]),
        (Some("GOPATH"), vec![path_field(build, binding, gopath, cx)]),
    ];
    (sections, vec![ToolchainKind::Go])
}

const OS: &[(&str, &str)] = &[
    ("", "Automatic"),
    ("aix", "aix"),
    ("android", "android"),
    ("darwin", "darwin"),
    ("dragonfly", "dragonfly"),
    ("freebsd", "freebsd"),
    ("illumos", "illumos"),
    ("ios", "ios"),
    ("js", "js"),
    ("linux", "linux"),
    ("netbsd", "netbsd"),
    ("openbsd", "openbsd"),
    ("plan9", "plan9"),
    ("solaris", "solaris"),
    ("wasip1", "wasip1"),
    ("windows", "windows"),
];

const ARCH: &[(&str, &str)] = &[
    ("", "Automatic"),
    ("386", "386"),
    ("amd64", "amd64"),
    ("arm", "arm"),
    ("arm64", "arm64"),
    ("loong64", "loong64"),
    ("mips", "mips"),
    ("mipsle", "mipsle"),
    ("mips64", "mips64"),
    ("mips64le", "mips64le"),
    ("ppc64", "ppc64"),
    ("ppc64le", "ppc64le"),
    ("riscv64", "riscv64"),
    ("s390x", "s390x"),
    ("wasm", "wasm"),
];

const COMPILER: &[(&str, &str)] = &[("", "Automatic"), ("gc", "gc"), ("gccgo", "gccgo")];
const CGO: &[(&str, &str)] = &[("", "Automatic"), ("1", "Enabled"), ("0", "Disabled")];

/// The controls of Build Tags, in the page's order.
pub fn build_tag_parts() -> Vec<Part> {
    vec![
        super::select("languages.go.buildTags.os", "Operating System", fixed_choices(OS)),
        super::select("languages.go.buildTags.arch", "Architecture", fixed_choices(ARCH)),
        field("languages.go.buildTags.version", "Go Version", FieldKind::Text).placeholder("go1.22").into(),
        super::select("languages.go.buildTags.compiler", "Compiler", fixed_choices(COMPILER)),
        super::select("languages.go.buildTags.cgo", "CGO", fixed_choices(CGO)),
        field("languages.go.buildTags.custom", "Custom Tags", FieldKind::Text).placeholder("integration debug").into(),
        field("languages.go.buildTags.experiments", "Experiments", FieldKind::Text)
            .placeholder("loopvar,rangefunc")
            .into(),
    ]
}

/// The variables Go Modules sets for the go command, in the page's order.
pub const GO_MODULE_ENV: [&str; 9] =
    ["GOPROXY", "GOPRIVATE", "GOFLAGS", "GONOPROXY", "GONOSUMDB", "GOSUMDB", "GOMODCACHE", "GOTOOLCHAIN", "GOWORK"];

const GO_MODULE_KEYS: [&str; 9] = [
    "languages.go.modules.env.GOPROXY",
    "languages.go.modules.env.GOPRIVATE",
    "languages.go.modules.env.GOFLAGS",
    "languages.go.modules.env.GONOPROXY",
    "languages.go.modules.env.GONOSUMDB",
    "languages.go.modules.env.GOSUMDB",
    "languages.go.modules.env.GOMODCACHE",
    "languages.go.modules.env.GOTOOLCHAIN",
    "languages.go.modules.env.GOWORK",
];

/// The controls of Go Modules, in the page's order.
pub fn go_module_parts() -> Vec<Part> {
    let mut parts: Vec<Part> =
        vec![toggle("languages.go.modules.autoVendor", "Enable vendoring support automatically").into()];
    for (key, name) in GO_MODULE_KEYS.into_iter().zip(GO_MODULE_ENV) {
        parts.push(field(key, name, FieldKind::Text).placeholder("Inherit from environment").into());
    }
    parts
}

fn project_only(build: &Build, page: &'static str, parts: Vec<Part>, message: &'static str, cx: &mut App) -> Control {
    let binding = build.project(page);
    let form = cx.new(|cx| FormView::new(binding, parts, cx));
    let available = build.project_open;
    Control::ProjectOnly(cx.new(|cx| ProjectOnlyView::new(form, available, message, cx)))
}

fn go_build_tags(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let message = "Open a project to configure Go build tags.";
    (vec![(None, vec![project_only(build, GO_BUILD_TAGS, build_tag_parts(), message, cx)])], Vec::new())
}

fn go_modules(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let message = "Open a project to configure Go modules.";
    (vec![(None, vec![project_only(build, GO_MODULES, go_module_parts(), message, cx)])], Vec::new())
}

// ---------------------------------------------------------------------------
// Java

const JDK_HINT: &str = "Compiles and runs the project: the default jdtls runtime, Run/Debug, and JAVA_HOME for Maven \
                        and Gradle.";
const LEVEL_HINT: &str = "Java source and bytecode compliance level when no build descriptor overrides it.";
const JDTLS_JDK_HINT: &str = "Runs jdtls only; the project keeps its Project JDK. When empty, a suitable installed JDK \
                              is used first, otherwise Eclipse Temurin is downloaded automatically into a separate \
                              language-server folder. The jdtls executable itself is set in Language Servers.";
const VM_ARGS_HINT: &str = "Passed to the JVM running jdtls (java.jdt.ls.vmargs), e.g. -Xmx4g. jdtls runs with \
                            -XX:+UseG1GC -XX:G1PeriodicGCInterval=60000 -XX:MinHeapFreeRatio=10 \
                            -XX:MaxHeapFreeRatio=30 -Xmx2G -Xms100m by default; an option set here replaces the \
                            default for the same setting, and another collector (e.g. -XX:+UseZGC) replaces the \
                            default collector options. A Lombok agent is added automatically: the one installed in \
                            Language Servers, else, when the project uses Lombok (its build, an inherited parent POM \
                            or Gradle build, or its sources' imports), the newest from the Maven / Gradle caches.";

fn java(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(JAVA);
    let jdk = sdk("languages.java.jdk", "SDK", ToolchainKind::Jdk)
        .empty("Auto", "per module, the JDK of its Java version")
        .directory()
        .hint(JDK_HINT);
    const LEVEL: &str = "languages.java.languageLevel";
    let level = select_with_hint(LEVEL, "Language level", levels(&JAVA_LANGUAGE_LEVELS, "SDK default"), LEVEL_HINT);
    let jdtls_jdk = sdk("languages.java.jdtlsJdk", "JDK", ToolchainKind::Jdk)
        .empty("Auto", "auto-managed; uses an installed JDK or downloads a runtime when needed")
        .directory()
        .hint(JDTLS_JDK_HINT);
    let vm_args =
        field("languages.java.jdtlsVmArgs", "JVM options", FieldKind::Text).placeholder("-Xmx4g").hint(VM_ARGS_HINT);
    let shared_binding = binding.clone();
    let shared = Control::Shared(cx.new(|cx| JdtlsSharedView::new(shared_binding, cx)));
    let sections = vec![
        (
            Some("Project JDK"),
            vec![
                sdk_field(build, binding.clone(), jdk, cx),
                form_with_details(binding.clone(), vec![level], LEVEL, empty_detail("from build / JDK"), cx),
            ],
        ),
        (
            Some("Language Server"),
            vec![shared, sdk_field(build, binding.clone(), jdtls_jdk, cx), form(binding, vec![vm_args.into()], cx)],
        ),
    ];
    (sections, vec![ToolchainKind::Jdk])
}

// ---------------------------------------------------------------------------
// Markdown

fn markdown(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(MARKDOWN);
    let size = Control::FontSize(cx.new(|cx| PreviewFontSizeView::new(binding, cx)));
    (vec![(Some("Preview"), vec![size])], Vec::new())
}

// ---------------------------------------------------------------------------
// Node.js

const NODE_HINT: &str = "Runs npm scripts and Node-based language servers (vtsls, eslint, pyright…).";
const PACKAGE_MANAGER_HINT: &str = "Used by the npm run configurations detected from package.json scripts.";

const PACKAGE_MANAGER: &str = "languages.node.packageManager";
const PACKAGE_MANAGERS: [&str; 4] = ["npm", "pnpm", "yarn", "bun"];

/// Package manager: Auto, then each manager.
fn package_managers() -> ChoicesFn {
    Rc::new(|_: &Binding, cx: &App| {
        let mut list = vec![(Value::from(""), tx(cx, "Auto"))];
        list.extend(PACKAGE_MANAGERS.map(|name| (Value::from(name), name.to_owned())));
        list
    })
}

/// What a package manager's choice says after its name: where Auto looks, or the version and
/// place of the manager found, else that it is not found.
fn package_manager_detail(value: &str, found: &[Toolchain]) -> Option<String> {
    if value.is_empty() {
        return Some("from packageManager / lock file".to_owned());
    }
    let detail = match found.iter().find(|sdk| sdk.name == value) {
        Some(sdk) => format!("{} {}", sdk.version.clone().unwrap_or_default(), tildify(&sdk.path)).trim().to_owned(),
        None => "not found".to_owned(),
    };
    Some(detail)
}

fn node(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(NODE);
    let interpreter = sdk("languages.node.interpreter", "Node interpreter", ToolchainKind::Node)
        .empty("Default", "node on PATH")
        .hint(NODE_HINT);
    let manager = select_with_hint(PACKAGE_MANAGER, "Package manager", package_managers(), PACKAGE_MANAGER_HINT);
    let toolchains = build.toolchains.clone();
    let detail: DetailFn = Rc::new(move |value: &Value, cx: &App| {
        let found = toolchains.found(ToolchainKind::PackageManager, cx);
        package_manager_detail(value.as_str()?, &found).map(|detail| tx(cx, &detail))
    });
    let controls = vec![
        sdk_field(build, binding.clone(), interpreter, cx),
        form(binding.clone(), vec![run_target("languages.node.runTarget")], cx),
        form_with_details(binding, vec![manager], PACKAGE_MANAGER, detail, cx),
    ];
    (vec![(None, controls)], vec![ToolchainKind::Node, ToolchainKind::PackageManager])
}

// ---------------------------------------------------------------------------
// Python Interpreter

const ACTIVATE_HINT: &str = "New terminal sessions run the interpreter's virtualenv activate script.";

fn python(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(PYTHON);
    let interpreter =
        sdk("languages.python.interpreter", "Python Interpreter", ToolchainKind::Python).choices(SdkChoices::Python);
    let activate = toggle("languages.python.activateVirtualenv", "Activate virtualenv").hint(ACTIVATE_HINT);
    let sections = vec![
        (None, vec![sdk_field(build, binding.clone(), interpreter, cx)]),
        (Some("Terminal"), vec![form(binding, vec![activate.into()], cx)]),
    ];
    (sections, vec![ToolchainKind::Python])
}

// ---------------------------------------------------------------------------
// PHP

const PHP_LEVEL_HINT: &str = "The PHP version Intelephense checks the code against: newer syntax and functions are \
                              reported. Auto uses config.platform.php from composer.json, else the lowest version \
                              require.php allows.";
const INCLUDE_PATHS_HINT: &str = "Folders outside the project whose classes and functions Intelephense indexes too, \
                                  such as the platform a module extends. Absolute, or relative to the project.";
const STUBS_HINT: &str = "Intelephense stub sets loaded on top of its defaults, such as wordpress. Each ext-* in \
                          require or require-dev of composer.json adds its own.";
const PHPSTAN_HINT: &str = "The configuration vendor/bin/phpstan checks saved files with. Empty uses phpstan.neon, \
                            phpstan.neon.dist or phpstan.dist.neon in the project root. Absolute, or relative to the \
                            project.";
const PHP_HINT: &str = "Runs PHP scripts, PHPUnit, Composer and artisan, and the PHP-based language servers \
                        (phpactor). Its folder is put first on PATH, so vendor/bin/phpunit and other #!/usr/bin/env \
                        php scripts use it too.";

fn php(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(PHP);
    const LEVEL: &str = "languages.php.languageLevel";
    let level = select_with_hint(LEVEL, "PHP language level", levels(&PHP_LANGUAGE_LEVELS, "Auto"), PHP_LEVEL_HINT);
    let include_paths = ListSpec {
        label: "Include paths",
        key: "languages.php.includePaths",
        shape: ListShape::Strings { placeholder: "/path/to/platform" },
        empty: "No include paths",
        hint: Some(INCLUDE_PATHS_HINT),
    };
    let stubs = ListSpec {
        label: "Additional stubs",
        key: "languages.php.stubs",
        shape: ListShape::Strings { placeholder: "Stub name, e.g. wordpress" },
        empty: "No additional stubs",
        hint: Some(STUBS_HINT),
    };
    let phpstan = PathSpec {
        key: "languages.php.phpstanConfig",
        label: "Configuration file",
        placeholder: PathPlaceholder::Fixed("phpstan.neon, phpstan.neon.dist or phpstan.dist.neon"),
        directory: false,
        hint: Some(PHPSTAN_HINT),
    };
    let interpreter = sdk("languages.php.interpreter", "PHP interpreter", ToolchainKind::Php)
        .empty("Default", "php on PATH")
        .hint(PHP_HINT);
    let (info_binding, toolchains) = (binding.clone(), build.toolchains.clone());
    let info = Control::Php(cx.new(|cx| PhpInterpreterView::new(info_binding, toolchains, cx)));
    let sections = vec![
        (
            None,
            vec![
                form_with_details(binding.clone(), vec![level], LEVEL, empty_detail("from composer.json"), cx),
                list(binding.clone(), include_paths, cx),
                list(binding.clone(), stubs, cx),
            ],
        ),
        (Some("PHPStan"), vec![path_field(build, binding.clone(), phpstan, cx)]),
        (
            Some("CLI Interpreter"),
            vec![
                form(binding.clone(), vec![run_target("languages.php.runTarget")], cx),
                sdk_field(build, binding, interpreter, cx),
                info,
            ],
        ),
    ];
    (sections, vec![ToolchainKind::Php])
}

// ---------------------------------------------------------------------------
// Prettier, oxfmt, Ruff

/// The languages Prettier and oxfmt can format, with their names.
pub const NODE_FORMATTER_LANGUAGES: &[(&str, &str)] = &[
    ("markdown", "Markdown"),
    ("yaml", "YAML"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("less", "Less"),
    ("html", "HTML"),
    ("graphql", "GraphQL"),
    ("javascript", "JavaScript"),
    ("javascriptreact", "JavaScript JSX"),
    ("typescript", "TypeScript"),
    ("typescriptreact", "TypeScript JSX"),
    ("json", "JSON"),
    ("jsonc", "JSON with Comments"),
    ("vue", "Vue"),
];

/// The languages Ruff can format.
pub const RUFF_LANGUAGES: &[(&str, &str)] = &[("python", "Python")];

const AUTO_PATH_HINT: &str = "Auto: nearest project node_modules, managed installation, then PATH. An explicit path \
                              overrides auto detection.";
const RUFF_PATH_HINT: &str = "Auto: managed installation, then PATH. An explicit path overrides auto detection.";
const RUFF_CONFIG_HINT: &str = "Empty uses the nearest project pyproject.toml ([tool.ruff]), ruff.toml, or \
                                .ruff.toml. A configured directory must contain one of them.";
const OXFMT_PROVIDER_HINT: &str = "oxfmt formats an enabled language instead of Prettier below an oxfmt configuration \
                                   file (.oxfmtrc.json, oxfmt.config.ts) or when it is the selected provider.";
const RUFF_PROVIDER_HINT: &str = "Ruff reads the nearest pyproject.toml ([tool.ruff]), ruff.toml, or .ruff.toml. Use \
                                  it before the language server when preferred.";

/// One of the external formatter pages.
struct Formatter {
    page: &'static str,
    name: &'static str,
    path_key: &'static str,
    path_label: &'static str,
    path_hint: &'static str,
    configuration: Option<&'static str>,
    prefer: &'static str,
    provider_hint: Option<&'static str>,
    run_on_save: &'static str,
    save_hint: &'static str,
    languages_key: &'static str,
    languages: &'static [(&'static str, &'static str)],
}

const PRETTIER_PAGE: Formatter = Formatter {
    page: PRETTIER,
    name: "Prettier",
    path_key: "languages.prettier.path",
    path_label: "Prettier path",
    path_hint: AUTO_PATH_HINT,
    configuration: None,
    prefer: "languages.prettier.prefer",
    provider_hint: None,
    run_on_save: "languages.prettier.runOnSave",
    save_hint: "Run Prettier on save for enabled languages when it is the selected formatting provider.",
    languages_key: "languages.prettier.languages",
    languages: NODE_FORMATTER_LANGUAGES,
};

const OXFMT_PAGE: Formatter = Formatter {
    page: OXFMT,
    name: "oxfmt",
    path_key: "languages.oxfmt.path",
    path_label: "oxfmt path",
    path_hint: AUTO_PATH_HINT,
    configuration: None,
    prefer: "languages.oxfmt.prefer",
    provider_hint: Some(OXFMT_PROVIDER_HINT),
    run_on_save: "languages.oxfmt.runOnSave",
    save_hint: "Run oxfmt on save for enabled languages when it is the selected formatting provider.",
    languages_key: "languages.oxfmt.languages",
    languages: NODE_FORMATTER_LANGUAGES,
};

const RUFF_PAGE: Formatter = Formatter {
    page: RUFF,
    name: "Ruff",
    path_key: "languages.ruff.path",
    path_label: "Ruff path",
    path_hint: RUFF_PATH_HINT,
    configuration: Some("languages.ruff.configurationDirectory"),
    prefer: "languages.ruff.prefer",
    provider_hint: Some(RUFF_PROVIDER_HINT),
    run_on_save: "languages.ruff.runOnSave",
    save_hint: "Run Ruff on save for enabled languages when it is the selected formatting provider.",
    languages_key: "languages.ruff.languages",
    languages: RUFF_LANGUAGES,
};

/// The controls of a formatter's own section, in the page's order.
fn formatter_parts(formatter: &Formatter) -> Vec<Part> {
    let mut parts: Vec<Part> = vec![
        field(formatter.path_key, formatter.path_label, FieldKind::Text)
            .placeholder("Auto")
            .hint(formatter.path_hint)
            .into(),
    ];
    if let Some(key) = formatter.configuration {
        let configuration = field(key, "Configuration directory", FieldKind::Text)
            .placeholder("Project configuration")
            .hint(RUFF_CONFIG_HINT)
            .browse(true);
        parts.push(configuration.into());
    }
    let name = formatter.name;
    // The provider is stored as whether the formatter is preferred to the language server.
    let providers: ChoicesFn = Rc::new(move |_: &Binding, cx: &App| {
        vec![(Value::Bool(false), tx(cx, "Automatic (language server first)")), (Value::Bool(true), name.to_owned())]
    });
    parts.push(Part::Select {
        key: formatter.prefer,
        label: "Formatting provider",
        choices: providers,
        hint: formatter.provider_hint.map(SharedString::from),
        beside: None,
        indent: false,
    });
    parts.push(toggle(formatter.run_on_save, "Run on save").hint(formatter.save_hint).into());
    parts
}

fn formatter(build: &Build, formatter: &Formatter, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(formatter.page);
    let (key, languages) = (formatter.languages_key, formatter.languages);
    let enabled_binding = binding.clone();
    let enabled = Control::Languages(cx.new(|cx| FormatterLanguagesView::new(enabled_binding, key, languages, cx)));
    let sections = vec![
        (Some(formatter.name), vec![form(binding, formatter_parts(formatter), cx)]),
        (Some("Enabled languages"), vec![enabled]),
    ];
    (sections, Vec::new())
}

// ---------------------------------------------------------------------------
// Ruby, Rust, TypeScript

const RUBY_HINT: &str = "Runs the ruby, rake and rspec configurations and lists the standard library and gems under \
                         External Libraries. Installs from rbenv, rvm, chruby, mise and asdf are detected.";

fn ruby(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let spec = sdk("languages.ruby.interpreter", "Ruby interpreter", ToolchainKind::Ruby)
        .empty("Auto", ".ruby-version / mise / asdf, else ruby on PATH")
        .hint(RUBY_HINT);
    (vec![(None, vec![sdk_field(build, build.binding(RUBY), spec, cx)])], vec![ToolchainKind::Ruby])
}

const TOOLCHAIN_HINT: &str = "cargo run configurations and rust-analyzer use this toolchain (PATH, RUSTUP_TOOLCHAIN).";
const STDLIB_HINT: &str = "Sources of std for navigation and completion (RUST_SRC_PATH).";
const LINTER_HINT: &str = "rust-analyzer runs the tool after each save and reports its diagnostics.";
const RUSTFMT_HINT: &str = "Passed to rustfmt when reformatting, e.g. --edition 2024";

fn rust(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let binding = build.binding(RUST);
    let toolchain = sdk("languages.rust.toolchain", "Toolchain location", ToolchainKind::Rust)
        .empty("Default", "rustup default / PATH")
        .directory()
        .hint(TOOLCHAIN_HINT);
    let stdlib = PathSpec {
        key: "languages.rust.stdlib",
        label: "Standard library",
        placeholder: PathPlaceholder::RustStdlib,
        directory: true,
        hint: Some(STDLIB_HINT),
    };
    let linter = vec![
        super::select(
            "languages.rust.checkCommand",
            "External tool",
            fixed_choices(&[("check", "Cargo Check"), ("clippy", "Clippy")]),
        ),
        toggle("languages.rust.checkOnSave", "Run external linter on save").hint(LINTER_HINT).into(),
    ];
    let rustfmt = field("languages.rust.rustfmtArgs", "Additional arguments", FieldKind::Text).hint(RUSTFMT_HINT);
    let sections = vec![
        (
            Some("Toolchain"),
            vec![sdk_field(build, binding.clone(), toolchain, cx), path_field(build, binding.clone(), stdlib, cx)],
        ),
        (Some("External Linters"), vec![form(binding.clone(), linter, cx)]),
        (Some("Rustfmt"), vec![form(binding, vec![rustfmt.into()], cx)]),
    ];
    (sections, vec![ToolchainKind::Rust])
}

const TYPESCRIPT_HINT: &str = "Version used by vtsls / typescript-language-server (typescript.tsdk, tsserver.path).";

fn typescript(build: &Build, cx: &mut App) -> (Vec<Section>, Vec<ToolchainKind>) {
    let spec = sdk("languages.typescript.version", "TypeScript", ToolchainKind::TypeScript)
        .directory()
        .hint(TYPESCRIPT_HINT)
        .choices(SdkChoices::TypeScript);
    (vec![(None, vec![sdk_field(build, build.binding(TYPESCRIPT), spec, cx)])], vec![ToolchainKind::TypeScript])
}

// ---------------------------------------------------------------------------
// Search and mounting

/// The texts the Settings search finds a page by, besides its keywords: its labels and choices.
pub fn search_labels(page: &str) -> Vec<&'static str> {
    match page {
        GO => vec!["GOROOT", "GOPATH", "Project GOPATH", "Detect again", "Browse…"],
        GO_BUILD_TAGS => {
            vec!["Operating System", "Architecture", "Go Version", "Compiler", "CGO", "Custom Tags", "Experiments"]
        }
        GO_MODULES => {
            let mut labels = vec!["Enable vendoring support automatically"];
            labels.extend(GO_MODULE_ENV);
            labels
        }
        JAVA => vec![
            "Project JDK",
            "SDK",
            "Language level",
            "Language Server",
            "Share JDT LS across builds",
            "JDK",
            "JVM options",
            "jdk for importer",
        ],
        MARKDOWN => vec!["Preview", "Preview font size"],
        NODE => vec!["Node interpreter", "Run in target", "Package manager"],
        PYTHON => vec!["Python Interpreter", "Terminal", "Activate virtualenv"],
        PHP => vec![
            "PHP language level",
            "Include paths",
            "Additional stubs",
            "PHPStan",
            "Configuration file",
            "CLI Interpreter",
            "Run in target",
            "PHP interpreter",
            "Xdebug",
        ],
        PRETTIER | OXFMT | RUFF => {
            let mut labels = vec!["Formatting provider", "Run on save", "Enabled languages"];
            let formatter = match page {
                PRETTIER => &PRETTIER_PAGE,
                OXFMT => &OXFMT_PAGE,
                _ => &RUFF_PAGE,
            };
            labels.push(formatter.path_label);
            if formatter.configuration.is_some() {
                labels.push("Configuration directory");
            }
            labels.extend(formatter.languages.iter().map(|(_, name)| *name));
            labels
        }
        RUBY => vec!["Ruby interpreter"],
        RUST => vec![
            "Toolchain",
            "Toolchain location",
            "Standard library",
            "External Linters",
            "External tool",
            "Cargo Check",
            "Clippy",
            "Run external linter on save",
            "Rustfmt",
            "Additional arguments",
        ],
        TYPESCRIPT => vec!["TypeScript", "Bundled"],
        _ => Vec::new(),
    }
}

/// The controls of `page` in their sections and the installations it offers.
fn page_sections(page: &str, build: &Build, cx: &mut App) -> Option<(Vec<Section>, Vec<ToolchainKind>)> {
    Some(match page {
        GO => go(build, cx),
        GO_BUILD_TAGS => go_build_tags(build, cx),
        GO_MODULES => go_modules(build, cx),
        JAVA => java(build, cx),
        MARKDOWN => markdown(build, cx),
        NODE => node(build, cx),
        PYTHON => python(build, cx),
        PHP => php(build, cx),
        PRETTIER => formatter(build, &PRETTIER_PAGE, cx),
        OXFMT => formatter(build, &OXFMT_PAGE, cx),
        RUFF => formatter(build, &RUFF_PAGE, cx),
        RUBY => ruby(build, cx),
        RUST => rust(build, cx),
        TYPESCRIPT => typescript(build, cx),
        _ => return None,
    })
}

/// What the pages are mounted with: the scope the dialog shows, the dialog's toolchains, and
/// whether its window has a project.
pub struct Mount {
    pub scope: ScopeOf,
    pub toolchains: Toolchains,
    pub has_project: bool,
}

/// Builds the view of `page` over the dialog's model.
pub fn page_view(
    page: &'static str,
    model: &Entity<SettingsDialog>,
    mount: &Mount,
    cx: &mut App,
) -> Option<Entity<LanguagePageView>> {
    let project_open = mount.has_project && !model.read(cx).new_projects;
    let toolchains = mount.toolchains.clone();
    let build =
        Build { model: model.clone(), scope: mount.scope.clone(), toolchains: toolchains.clone(), project_open };
    let (sections, detect) = page_sections(page, &build, cx)?;
    let binding = build.binding(page);
    Some(cx.new(|cx| LanguagePageView::new(page, binding, sections, toolchains, detect, cx)))
}

/// Puts the view of every page this module draws in `dialog`, with the texts the search finds it
/// by, and answers what the pages ask the backend for the dialog's window. Returns the views,
/// whose dialog events the caller answers, and the subscriptions answering the backend questions;
/// the caller keeps both as long as the dialog is open.
pub fn install(
    dialog: &Entity<SettingsDialogView>,
    model: &Entity<SettingsDialog>,
    mount: Mount,
    cx: &mut App,
) -> (Vec<Entity<LanguagePageView>>, Vec<Subscription>) {
    let label: Rc<str> = Rc::from(mount.toolchains.window());
    let mut views = Vec::new();
    let mut subscriptions = Vec::new();
    for page in PAGES {
        let Some(view) = page_view(page, model, &mount, cx) else { continue };
        let label = label.clone();
        subscriptions.push(cx.subscribe(&view, move |_, ask: &LanguageAsk, cx| answer(ask.clone(), &label, cx)));
        let labels: Vec<String> = search_labels(page).into_iter().map(str::to_owned).collect();
        let drawn = view.clone();
        dialog.update(cx, |dialog, cx| {
            dialog.set_page_view(page, drawn.into(), cx);
            dialog.set_page_labels(page, labels, cx);
        });
        views.push(view);
    }
    (views, subscriptions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::builtin_pages::settings_registry;
    use crate::settings_model::schema::{default_settings, get_at};
    use ide_core::settings::LanguagesSettings;

    // Not from a spec: every page drawn here is a page of the Settings tree, and every page of
    // Languages & Frameworks is drawn here except Language Servers, which another module draws, and
    // the Python folder, which the dialog draws as a folder.
    #[test]
    fn draws_every_language_page_of_the_settings_tree() {
        let registry = settings_registry(false);
        let ids: Vec<&str> = registry.iter().map(|spec| spec.id).collect();
        for page in PAGES {
            assert!(ids.contains(&page), "{page} is not in the Settings tree");
            assert!(!search_labels(page).is_empty(), "{page} has no search labels");
        }
        let languages = registry.iter().filter(|spec| spec.group == Some("Languages & Frameworks"));
        let missing: Vec<&str> = languages.map(|spec| spec.id).filter(|id| !PAGES.contains(id)).collect();
        assert_eq!(missing, ["languages.servers", "python"]);
    }

    // Not from a spec: the languages a formatter page offers are those the backend enables by
    // default for it.
    #[test]
    fn offers_the_languages_the_backend_enables_by_default() {
        let defaults = LanguagesSettings::default();
        let ids = |list: &[(&str, &str)]| list.iter().map(|(id, _)| (*id).to_owned()).collect::<Vec<_>>();
        assert_eq!(ids(NODE_FORMATTER_LANGUAGES), defaults.prettier.languages);
        assert_eq!(ids(NODE_FORMATTER_LANGUAGES), defaults.oxfmt.languages);
        assert_eq!(ids(RUFF_LANGUAGES), defaults.ruff.languages);
    }

    // Not from a spec: every setting a page's standard controls write exists in the backend's
    // settings, the Go module variables excepted (a map that starts empty).
    #[test]
    fn the_forms_write_settings_of_the_backend() {
        let defaults = default_settings();
        let mut keys: Vec<&str> = build_tag_parts().iter().filter_map(part_key).collect();
        keys.extend(go_module_parts().iter().filter_map(part_key).filter(|key| !key.contains(".env.")));
        for formatter in [&PRETTIER_PAGE, &OXFMT_PAGE, &RUFF_PAGE] {
            keys.extend(formatter_parts(formatter).iter().filter_map(part_key));
            keys.push(formatter.languages_key);
        }
        for key in keys {
            assert!(get_at(&defaults, key).is_some(), "{key} is not a setting");
        }
        assert_eq!(GO_MODULE_KEYS.map(|key| key.rsplit('.').next().unwrap_or_default()), GO_MODULE_ENV);
    }

    // Not from a spec: Auto says where it looks; a manager found names its version and place, one
    // not found says so.
    #[test]
    fn a_package_manager_names_the_version_and_place_found() {
        let pnpm = Toolchain {
            path: "/Users/ann/.local/bin/pnpm".to_owned(),
            name: "pnpm".to_owned(),
            version: Some("9.1.0".to_owned()),
            ..Toolchain::default()
        };
        let found = [pnpm];
        assert_eq!(package_manager_detail("", &found).as_deref(), Some("from packageManager / lock file"));
        assert_eq!(package_manager_detail("pnpm", &found).as_deref(), Some("9.1.0 ~/.local/bin/pnpm"));
        assert_eq!(package_manager_detail("bun", &found).as_deref(), Some("not found"));
    }

    fn part_key(part: &Part) -> Option<&'static str> {
        match part {
            Part::Toggle(spec) => Some(spec.key),
            Part::Field(spec) => Some(spec.key),
            Part::Select { key, .. } | Part::Radio { key, .. } => Some(*key),
            _ => None,
        }
    }
}
