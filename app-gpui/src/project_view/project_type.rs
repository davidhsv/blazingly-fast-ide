//! The type of a project root, told by the build and SDK markers among its files only: no
//! dependency is parsed and no tool is run. The first type whose markers match wins, so the specific
//! ones (Flutter, Vite) come before their generic fallback (Dart, Node.js).

use std::collections::BTreeSet;

/// A project type, by its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectType {
    Flutter,
    Dart,
    Vite,
    Gradle,
    Maven,
    Go,
    Cargo,
    Python,
    Node,
    Php,
    Ruby,
    Swift,
    Zig,
    Godot,
    Cmake,
    Make,
    Dotnet,
}

/// A file name a type is recognised by.
enum Marker {
    Name(&'static str),
    /// Any name the test accepts.
    Matching(fn(&str) -> bool),
}

struct Definition {
    kind: ProjectType,
    markers: &'static [Marker],
    /// Files that must be there as well.
    requires: &'static [&'static str],
}

/// `vite.config.js`, `.ts`, `.mjs`, `.cts`…
fn vite_config(name: &str) -> bool {
    name.strip_prefix("vite.config.").is_some_and(|ext| {
        let ext = ext.strip_prefix(['c', 'm']).unwrap_or(ext);
        matches!(ext, "js" | "ts")
    })
}

/// `App.csproj`, `App.fsproj`, `App.sln`, `App.slnx`, any case.
fn dotnet_project(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some((stem, ext)) = lower.rsplit_once('.') else { return false };
    !stem.is_empty() && !name.contains('/') && matches!(ext, "csproj" | "fsproj" | "sln" | "slnx")
}

const DEFINITIONS: &[Definition] = &[
    Definition { kind: ProjectType::Flutter, markers: &[Marker::Name("pubspec.yaml")], requires: &[".metadata"] },
    Definition { kind: ProjectType::Dart, markers: &[Marker::Name("pubspec.yaml")], requires: &[] },
    Definition { kind: ProjectType::Vite, markers: &[Marker::Matching(vite_config)], requires: &["package.json"] },
    Definition {
        kind: ProjectType::Gradle,
        markers: &[
            Marker::Name("build.gradle.kts"),
            Marker::Name("build.gradle"),
            Marker::Name("settings.gradle.kts"),
            Marker::Name("settings.gradle"),
        ],
        requires: &[],
    },
    Definition { kind: ProjectType::Maven, markers: &[Marker::Name("pom.xml")], requires: &[] },
    Definition { kind: ProjectType::Go, markers: &[Marker::Name("go.mod"), Marker::Name("go.work")], requires: &[] },
    Definition { kind: ProjectType::Cargo, markers: &[Marker::Name("Cargo.toml")], requires: &[] },
    Definition {
        kind: ProjectType::Python,
        markers: &[
            Marker::Name("pyproject.toml"),
            Marker::Name("requirements.txt"),
            Marker::Name("Pipfile"),
            Marker::Name("setup.py"),
            Marker::Name("setup.cfg"),
        ],
        requires: &[],
    },
    Definition { kind: ProjectType::Node, markers: &[Marker::Name("package.json")], requires: &[] },
    Definition { kind: ProjectType::Php, markers: &[Marker::Name("composer.json")], requires: &[] },
    Definition { kind: ProjectType::Ruby, markers: &[Marker::Name("Gemfile")], requires: &[] },
    Definition { kind: ProjectType::Swift, markers: &[Marker::Name("Package.swift")], requires: &[] },
    Definition { kind: ProjectType::Zig, markers: &[Marker::Name("build.zig")], requires: &[] },
    Definition { kind: ProjectType::Godot, markers: &[Marker::Name("project.godot")], requires: &[] },
    Definition { kind: ProjectType::Cmake, markers: &[Marker::Name("CMakeLists.txt")], requires: &[] },
    Definition {
        kind: ProjectType::Make,
        markers: &[Marker::Name("Makefile"), Marker::Name("makefile"), Marker::Name("GNUmakefile")],
        requires: &[],
    },
    Definition { kind: ProjectType::Dotnet, markers: &[Marker::Matching(dotnet_project)], requires: &[] },
];

fn matches(name: &str, marker: &Marker) -> bool {
    match marker {
        Marker::Name(marker) => name == *marker,
        Marker::Matching(test) => test(name),
    }
}

/// The type of a root holding the files named `files`; `None` when no type's markers are there.
pub fn detect_project_type(files: &BTreeSet<String>) -> Option<ProjectType> {
    DEFINITIONS
        .iter()
        .filter(|definition| definition.requires.iter().all(|name| files.contains(*name)))
        .find(|definition| definition.markers.iter().any(|marker| files.iter().any(|name| matches(name, marker))))
        .map(|definition| definition.kind)
}

/// The type of the root at `root`, from the names of its files (a link to a file counts as one). A
/// root that cannot be listed has no type.
pub fn project_type_of(root: &std::path::Path) -> Option<ProjectType> {
    let entries = std::fs::read_dir(root).ok()?;
    let files: BTreeSet<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    detect_project_type(&files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(names: &[&str]) -> Option<ProjectType> {
        detect_project_type(&names.iter().map(|name| (*name).to_owned()).collect())
    }

    // Not from a spec: the first type whose markers are there wins, a type with required files only when
    // they are there too, and names are matched exactly except for the patterns.
    #[test]
    fn the_first_type_with_its_markers_and_required_files_wins() {
        assert_eq!(of(&["pubspec.yaml", ".metadata"]), Some(ProjectType::Flutter));
        assert_eq!(of(&["pubspec.yaml"]), Some(ProjectType::Dart));
        assert_eq!(of(&["vite.config.mts", "package.json"]), Some(ProjectType::Vite));
        assert_eq!(of(&["vite.config.ts"]), None);
        assert_eq!(of(&["package.json", "Cargo.toml"]), Some(ProjectType::Cargo));
        assert_eq!(of(&["settings.gradle", "pom.xml"]), Some(ProjectType::Gradle));
        assert_eq!(of(&["requirements.txt"]), Some(ProjectType::Python));
        assert_eq!(of(&["build.zig"]), Some(ProjectType::Zig));
        assert_eq!(of(&["Shop.CSPROJ"]), Some(ProjectType::Dotnet));
        assert_eq!(of(&[".csproj", "cargo.toml", "README.md"]), None);
        assert_eq!(of(&[]), None);
    }
}
