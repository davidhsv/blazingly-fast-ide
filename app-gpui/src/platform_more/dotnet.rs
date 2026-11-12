//! .NET workspaces: solution and `global.json` formats, what an MSBuild evaluation says about a
//! project, and the `dotnet test` command line of each test runner.
//!
//! The `dotnet` process itself is run by the caller; [`super::dotnet_process`] follows one run.

use crate::platform::virtual_docs::dirname;
use ide_run::{RunConfig, RunExtras};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// How `dotnet test` runs a project's tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotnetRunner {
    /// VSTest, the SDK's classic runner.
    Vstest,
    /// Microsoft.Testing.Platform, selected for the whole repository in `global.json`.
    Mtp,
    /// Microsoft.Testing.Platform behind the VSTest command line (the project opts in).
    MtpLegacy,
}

impl DotnetRunner {
    /// The runner as run configuration parameters spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vstest => "vstest",
            Self::Mtp => "mtp",
            Self::MtpLegacy => "mtp-legacy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotnetPackage {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotnetProject {
    pub path: String,
    pub name: String,
    pub frameworks: Vec<String>,
    pub configurations: Vec<String>,
    /// Absolute paths of the referenced projects.
    pub references: Vec<String>,
    pub packages: Vec<DotnetPackage>,
    pub test: bool,
    pub runner: DotnetRunner,
    /// `xunit`, `nunit` or `mstest`.
    pub adapter: String,
    /// The project can write TRX reports on the testing platform.
    pub trx: bool,
    /// Version of Microsoft.Testing.Platform the project restored, once it is restored.
    pub platform_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotnetSolution {
    pub path: String,
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DotnetWorkspace {
    pub projects: Vec<DotnetProject>,
    pub solutions: Vec<DotnetSolution>,
    /// `<project path>: <error>` of the projects that could not be evaluated.
    pub errors: Vec<String>,
}

/// A `dotnet` query as a run configuration: output in English, no terminal. `id` is unique to
/// the query (`dotnet-task:<uuid>`).
pub fn dotnet_config(args: &[String], cwd: &str, id: &str) -> RunConfig {
    RunConfig {
        id: id.to_owned(),
        name: format!("dotnet {}", args.first().map_or("", String::as_str)),
        kind: "shell".to_owned(),
        command: "dotnet".to_owned(),
        args: args.to_vec(),
        cwd: Some(cwd.into()),
        env: BTreeMap::from([("DOTNET_CLI_UI_LANGUAGE".to_owned(), "en".to_owned())]),
        detected: true,
        is_test: false,
        debug_adapter: None,
        allow_parallel: false,
        extras: RunExtras::default(),
    }
}

/// The JSON object in `text`, whatever the SDK printed around it (first-run banners, warnings).
pub fn json_output(text: &str) -> serde_json::Result<Value> {
    let object = match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if start < end => &text[start..=end],
        _ => "",
    };
    serde_json::from_str(object)
}

/// `relative` against the folder `base`, with `/` separators; an absolute path (`/x`, `C:/x`)
/// stays as it is.
pub fn absolute(base: &str, relative: &str) -> String {
    let path = relative.replace('\\', "/");
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    if drive || path.starts_with('/') {
        return path;
    }
    let base = base.replace('\\', "/");
    let mut parts: Vec<&str> = base.split('/').collect();
    for part in path.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "" | "." => {}
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

static SLN_PROJECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^Project\([^\r\n]+?=\s*"[^"]*",\s*"([^"]+)""#).expect("the pattern is valid"));
static SLNX_PROJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<Project\s(?:[^>]*?\s)?Path\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("the pattern is valid")
});
static XML_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(#[0-9]+|#x[0-9A-Fa-f]+|[A-Za-z]+);").expect("the pattern is valid"));

/// `text` with XML character and entity references replaced; unknown ones stay as written.
fn decode_xml(text: &str) -> String {
    XML_ENTITY
        .replace_all(text, |found: &regex::Captures<'_>| {
            let name = &found[1];
            let code = match name.strip_prefix("#x") {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => name.strip_prefix('#').and_then(|decimal| decimal.parse().ok()),
            };
            let named = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => None,
            };
            named.or_else(|| code.and_then(char::from_u32)).map_or_else(|| found[0].to_owned(), String::from)
        })
        .into_owned()
}

/// A C#, F# or Visual Basic project file.
pub fn is_project_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".csproj", ".fsproj", ".vbproj"].iter().any(|extension| lower.ends_with(extension))
}

/// The project files a solution lists, absolute and each once: `text` is a `.sln` file, or a
/// `.slnx` one when `xml`; `dir` is the solution's folder. Solution folders are not projects.
pub fn solution_paths(text: &str, xml: bool, dir: &str) -> Vec<String> {
    let names: Vec<String> = if xml {
        SLNX_PROJECT
            .captures_iter(text)
            .map(|found| decode_xml(found.get(1).or_else(|| found.get(2)).map_or("", |path| path.as_str())))
            .collect()
    } else {
        SLN_PROJECT.captures_iter(text).map(|found| found[1].to_owned()).collect()
    };
    let mut paths: Vec<String> = Vec::new();
    for name in names.iter().filter(|name| is_project_file(name)) {
        let path = absolute(dir, name);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// The test runner a `global.json` selects. Comments and trailing commas are allowed, as the SDK
/// reads the file; `Err` when it is not JSON even so.
pub fn global_runner(text: &str) -> serde_json::Result<DotnetRunner> {
    let json: Value = serde_json::from_str(&ide_core::jsonc::strip(text))?;
    let runner = json.get("test").and_then(|test| test.get("runner")).and_then(Value::as_str);
    Ok(if runner == Some("Microsoft.Testing.Platform") { DotnetRunner::Mtp } else { DotnetRunner::Vstest })
}

/// The `global.json` files that may apply to a project in `dir`: its own folder's, then each
/// parent's.
pub fn global_json_candidates(dir: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut current = dir;
    while !current.is_empty() {
        candidates.push(format!("{current}/global.json"));
        let parent = dirname(current);
        if parent == current {
            break;
        }
        current = parent;
    }
    candidates
}

/// The runner of a folder, from the text of each of its [`global_json_candidates`] in order
/// (`None`: no such file). The nearest file that reads decides; without one it is VSTest.
pub fn runner_mode<'a>(contents: impl IntoIterator<Item = Option<&'a str>>) -> DotnetRunner {
    contents.into_iter().flatten().find_map(|text| global_runner(text).ok()).unwrap_or(DotnetRunner::Vstest)
}

/// The properties an evaluation asks MSBuild for.
const EVALUATED_PROPERTIES: &str = "MSBuildProjectName,TargetFramework,TargetFrameworks,Configurations,IsTestProject,\
     IsTestingPlatformApplication,TestingPlatformDotnetTestSupport,EnableMSTestRunner,ProjectAssetsFile";

/// The `dotnet` arguments that evaluate the project at `path` (run in its folder); the answer
/// is read with [`json_output`]. `framework` is empty for the project's own choice.
pub fn evaluate_args(path: &str, configuration: &str, framework: &str) -> Vec<String> {
    let mut args = vec!["msbuild".to_owned(), path.to_owned(), "-nologo".to_owned()];
    args.push(format!("-p:Configuration={configuration}"));
    if !framework.is_empty() {
        args.push(format!("-p:TargetFramework={framework}"));
    }
    args.push(format!("-getProperty:{EVALUATED_PROPERTIES}"));
    args.push("-getItem:ProjectReference,PackageReference".to_owned());
    args
}

fn property<'v>(data: &'v Value, name: &str) -> &'v str {
    data.get("Properties").and_then(|properties| properties.get(name)).and_then(Value::as_str).unwrap_or("")
}

fn items<'v>(data: &'v Value, kind: &str) -> impl Iterator<Item = &'v Value> + use<'v> {
    data.get("Items").and_then(|items| items.get(kind)).and_then(Value::as_array).into_iter().flatten()
}

/// The framework to evaluate a multi-targeted project for: its first one. `None` when the
/// evaluation `data` was made for a framework already, or the project has a single target.
pub fn first_target_framework<'v>(data: &'v Value, framework: &str) -> Option<&'v str> {
    let frameworks = property(data, "TargetFrameworks");
    (framework.is_empty() && !frameworks.is_empty()).then(|| frameworks.split(';').next().unwrap_or(""))
}

/// The `project.assets.json` of the evaluated project, which exists once it is restored.
pub fn assets_file(data: &Value) -> Option<&str> {
    Some(property(data, "ProjectAssetsFile")).filter(|file| !file.is_empty())
}

const TRX_EXTENSION: &str = "Microsoft.Testing.Extensions.TrxReport";
const TESTING_PLATFORM: &str = "Microsoft.Testing.Platform";
/// Packages that make a project a test project unless it says otherwise.
const TEST_PACKAGES: [&str; 7] =
    ["Microsoft.NET.Test.Sdk", "MSTest", "NUnit", "xunit", "xunit.v3", "xunit.v3.mtp-v1", "xunit.v3.mtp-v2"];

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// The project at `path` as the evaluation `data` describes it. `mode` is the repository's
/// runner ([`runner_mode`]); `assets` is the parsed [`assets_file`], when it could be read.
pub fn evaluated_project(path: &str, data: &Value, mode: DotnetRunner, assets: Option<&Value>) -> DotnetProject {
    let packages: Vec<DotnetPackage> = items(data, "PackageReference")
        .map(|item| {
            let text = |name: &str| item.get(name).and_then(Value::as_str).unwrap_or("").to_owned();
            DotnetPackage { id: text("Identity"), version: text("Version") }
        })
        .collect();
    let has_package = |matches: &dyn Fn(&str) -> bool| packages.iter().any(|package| matches(&package.id));
    let opts_in = ["IsTestingPlatformApplication", "TestingPlatformDotnetTestSupport", "EnableMSTestRunner"]
        .iter()
        .any(|name| property(data, name) == "true");
    let mtp = opts_in || has_package(&|id| id.to_ascii_lowercase().contains("xunit.v3.mtp"));
    let libraries: Vec<&str> = assets
        .and_then(|assets| assets.get("libraries"))
        .and_then(Value::as_object)
        .map(|libraries| libraries.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let library = |name: &str| {
        let prefix = format!("{name}/");
        libraries.iter().copied().find(|key| starts_with_ignore_case(key, &prefix))
    };
    let is_test = property(data, "IsTestProject");
    let listed_frameworks = property(data, "TargetFrameworks");
    let frameworks = if listed_frameworks.is_empty() { property(data, "TargetFramework") } else { listed_frameworks };
    let configurations = Some(property(data, "Configurations")).filter(|c| !c.is_empty()).unwrap_or("Debug;Release");
    let file_name = path.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(path);
    let name = Some(property(data, "MSBuildProjectName")).filter(|name| !name.is_empty()).unwrap_or(file_name);
    let folder = dirname(path);
    DotnetProject {
        path: path.to_owned(),
        name: name.to_owned(),
        frameworks: frameworks.split(';').filter(|framework| !framework.is_empty()).map(str::to_owned).collect(),
        configurations: configurations.split(';').map(str::to_owned).collect(),
        references: items(data, "ProjectReference")
            .filter_map(|item| item.get("Identity").and_then(Value::as_str))
            .map(|reference| absolute(folder, reference))
            .collect(),
        test: is_test == "true"
            || mtp
            || (is_test != "false"
                && has_package(&|id| TEST_PACKAGES.iter().any(|package| package.eq_ignore_ascii_case(id)))),
        runner: match mode {
            DotnetRunner::Mtp => DotnetRunner::Mtp,
            _ if mtp => DotnetRunner::MtpLegacy,
            _ => DotnetRunner::Vstest,
        },
        adapter: if has_package(&|id| starts_with_ignore_case(id, "xunit")) {
            "xunit"
        } else if has_package(&|id| starts_with_ignore_case(id, "nunit")) {
            "nunit"
        } else {
            "mstest"
        }
        .to_owned(),
        trx: has_package(&|id| id.eq_ignore_ascii_case(TRX_EXTENSION)) || library(TRX_EXTENSION).is_some(),
        platform_version: library(TESTING_PLATFORM).and_then(|key| key.split('/').nth(1)).map(str::to_owned),
        packages,
    }
}

/// What a directory entry is to the search for a workspace's projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEntry {
    /// A folder to look into.
    Folder,
    Project,
    /// A solution: `xml` for the `.slnx` format.
    Solution {
        xml: bool,
    },
    Other,
}

/// Classifies the entry `name` of a folder. Hidden, excluded, build output and package folders
/// are not searched.
pub fn workspace_entry(name: &str, is_dir: bool, excluded: bool) -> WorkspaceEntry {
    let skipped = name.starts_with('.') || matches!(name, "bin" | "obj" | "node_modules" | "target" | "packages");
    let lower = name.to_ascii_lowercase();
    if is_dir && !excluded && !skipped {
        WorkspaceEntry::Folder
    } else if is_project_file(name) {
        WorkspaceEntry::Project
    } else if lower.ends_with(".sln") || lower.ends_with(".slnx") {
        WorkspaceEntry::Solution { xml: name.ends_with(".slnx") }
    } else {
        WorkspaceEntry::Other
    }
}

/// The `dotnet test` arguments for `project`: `extra` goes to the SDK, or behind `--` to the
/// test application when the testing platform sits behind the VSTest command line. `framework`
/// is empty for the project's default.
pub fn dotnet_test_args(
    project: &DotnetProject,
    framework: &str,
    extra: &[String],
    configuration: &str,
) -> Vec<String> {
    let mut args = vec!["test".to_owned()];
    if project.runner == DotnetRunner::Mtp {
        args.push("--project".to_owned());
    }
    args.extend([project.path.clone(), "--configuration".to_owned(), configuration.to_owned()]);
    if !framework.is_empty() {
        args.extend(["--framework".to_owned(), framework.to_owned()]);
    }
    if project.runner == DotnetRunner::MtpLegacy {
        args.push("-p:TestingPlatformCaptureOutput=false".to_owned());
        args.push("-p:TestingPlatformDotnetTestSupport=true".to_owned());
        if !extra.is_empty() {
            args.push("--".to_owned());
        }
    }
    args.extend_from_slice(extra);
    args
}

/// Whether a test run has to restore the project first: the testing platform's version is only
/// known from the restored assets.
pub fn needs_restore(project: &DotnetProject) -> bool {
    project.runner != DotnetRunner::Vstest && project.platform_version.is_none()
}

/// The parameters a test run configuration takes from its evaluated project.
pub fn test_run_params(project: &DotnetProject) -> [(&'static str, String); 3] {
    [
        ("runner", project.runner.as_str().to_owned()),
        ("adapter", project.adapter.clone()),
        ("trx", project.trx.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(runner: DotnetRunner) -> DotnetProject {
        DotnetProject {
            path: "C:/My Repo/Tests.csproj".to_owned(),
            name: "Tests".to_owned(),
            frameworks: vec!["net9.0".to_owned()],
            configurations: vec!["Debug".to_owned(), "Release".to_owned()],
            references: Vec::new(),
            packages: Vec::new(),
            test: true,
            runner,
            adapter: "mstest".to_owned(),
            trx: true,
            platform_version: None,
        }
    }

    // .NET workspace formats and runners › reads global.json comments and trailing commas without changing string
    // values
    #[test]
    fn reads_global_json_comments_and_trailing_commas_without_changing_string_values() {
        let text = concat!(
            "{ // comment\n \"test\": { \"runner\": \"Microsoft.Testing.Platform\", }, ",
            "\"unused\": \"https://example.com/,}\", }"
        );
        assert_eq!(global_runner(text).ok(), Some(DotnetRunner::Mtp));
    }

    // .NET workspace formats and runners › resolves solution projects, XML entities and external references
    #[test]
    fn resolves_solution_projects_xml_entities_and_external_references() {
        let sln = concat!(
            "Project(\"id\") = \"Tests\", \"..\\outside\\Tests.csproj\", \"id\"\n",
            "Project(\"id\") = \"Folder\", \"Folder\", \"id\""
        );
        assert_eq!(solution_paths(sln, false, "C:/repo"), ["C:/outside/Tests.csproj"]);
        let slnx = r#"<Solution><Folder Name="src"><Project Path="A&amp;B/App.csproj" /></Folder></Solution>"#;
        assert_eq!(solution_paths(slnx, true, "/repo"), ["/repo/A&B/App.csproj"]);
    }

    // .NET workspace formats and runners › reads JSON despite SDK banners
    #[test]
    fn reads_json_despite_sdk_banners() {
        let output = "Welcome\n{\"Properties\":{\"TargetFramework\":\"net9.0\"}}\n";
        assert_eq!(json_output(output).ok(), Some(json!({ "Properties": { "TargetFramework": "net9.0" } })));
    }

    // .NET workspace formats and runners › keeps %s discovery arguments in the correct command layer
    #[test]
    fn keeps_discovery_arguments_in_the_correct_command_layer() {
        for runner in [DotnetRunner::Vstest, DotnetRunner::Mtp, DotnetRunner::MtpLegacy] {
            let args = dotnet_test_args(&project(runner), "net9.0", &["--list-tests".to_owned()], "Release");
            assert_eq!(args.iter().any(|arg| arg == "--project"), runner == DotnetRunner::Mtp, "{runner:?}");
            assert_eq!(args.iter().any(|arg| arg == "--"), runner == DotnetRunner::MtpLegacy, "{runner:?}");
            assert_eq!(args.last().map(String::as_str), Some("--list-tests"), "{runner:?}");
        }
    }

    // Not from a spec: what an evaluation says about a project, and the files and folders around it.
    #[test]
    fn describes_an_evaluated_project() {
        let data = json!({
            "Properties": {
                "MSBuildProjectName": "",
                "TargetFramework": "net9.0",
                "TargetFrameworks": "net9.0;net8.0",
                "IsTestProject": "",
                "TestingPlatformDotnetTestSupport": "true",
                "ProjectAssetsFile": "/repo/tests/obj/project.assets.json",
            },
            "Items": {
                "ProjectReference": [{ "Identity": "..\\src\\App.csproj" }],
                "PackageReference": [{ "Identity": "xunit.v3", "Version": "3.0.0" }, { "Identity": "Moq" }],
            },
        });
        assert_eq!(first_target_framework(&data, ""), Some("net9.0"));
        assert_eq!(first_target_framework(&data, "net8.0"), None);
        assert_eq!(assets_file(&data), Some("/repo/tests/obj/project.assets.json"));
        let assets = json!({ "libraries": {
            "Microsoft.Testing.Platform/1.6.2": {},
            "microsoft.testing.extensions.trxreport/1.6.2": {},
        } });
        let evaluated = evaluated_project("/repo/tests/Tests.csproj", &data, DotnetRunner::Vstest, Some(&assets));
        assert_eq!(evaluated.name, "Tests.csproj");
        assert_eq!(evaluated.frameworks, ["net9.0", "net8.0"]);
        assert_eq!(evaluated.configurations, ["Debug", "Release"]);
        assert_eq!(evaluated.references, ["/repo/src/App.csproj"]);
        assert_eq!(evaluated.packages[1], DotnetPackage { id: "Moq".to_owned(), version: String::new() });
        assert!(evaluated.test && evaluated.trx);
        assert_eq!((evaluated.runner, evaluated.adapter.as_str()), (DotnetRunner::MtpLegacy, "xunit"));
        assert_eq!(evaluated.platform_version.as_deref(), Some("1.6.2"));
        assert!(!needs_restore(&evaluated));
        assert_eq!(test_run_params(&evaluated)[0], ("runner", "mtp-legacy".to_owned()));

        let plain = evaluated_project("/repo/App.csproj", &json!({}), DotnetRunner::Mtp, None);
        assert_eq!((plain.test, plain.runner, plain.adapter.as_str()), (false, DotnetRunner::Mtp, "mstest"));
        assert!(needs_restore(&plain));

        assert_eq!(evaluate_args("/repo/App.csproj", "Debug", "")[3], "-p:Configuration=Debug");
        assert_eq!(evaluate_args("/repo/App.csproj", "Debug", "net9.0")[4], "-p:TargetFramework=net9.0");
        assert_eq!(
            global_json_candidates("/repo/tests"),
            ["/repo/tests/global.json", "/repo/global.json", "//global.json"]
        );
        let mtp = r#"{ "test": { "runner": "Microsoft.Testing.Platform" } }"#;
        assert_eq!(runner_mode([None, Some("not json"), Some(mtp)]), DotnetRunner::Mtp);
        assert_eq!(runner_mode([None, Some("{}")]), DotnetRunner::Vstest);
        assert_eq!(workspace_entry("src", true, false), WorkspaceEntry::Folder);
        assert_eq!(workspace_entry("obj", true, false), WorkspaceEntry::Other);
        assert_eq!(workspace_entry("App.FSPROJ", false, false), WorkspaceEntry::Project);
        assert_eq!(workspace_entry("All.slnx", false, false), WorkspaceEntry::Solution { xml: true });
        assert_eq!(workspace_entry("All.SLN", false, false), WorkspaceEntry::Solution { xml: false });
        let config = dotnet_config(&["msbuild".to_owned()], "/repo", "dotnet-task:1");
        assert_eq!((config.name.as_str(), config.command.as_str(), config.env.len()), ("dotnet msbuild", "dotnet", 1));
    }
}
