use super::*;

// src/platform/project.test.ts › relativeDir › is empty for files directly in the project root
/// A file directly in the project's folder has no directory to show.
#[test]
fn is_empty_for_files_directly_in_the_project_root() {
    assert_eq!(relative_dir("/p/app", "/p/app/README.md"), "");
}

// src/platform/project.test.ts › relativeDir › is project-relative for nested files
/// A nested file's directory is given from the project's folder.
#[test]
fn is_project_relative_for_nested_files() {
    assert_eq!(relative_dir("/p/app", "/p/app/src/lib/a.ts"), "src/lib");
}

// src/platform/project.test.ts › relativeDir › keeps the absolute directory outside the project
/// A file outside the project keeps its absolute directory, even one whose name starts like the project's.
#[test]
fn keeps_the_absolute_directory_outside_the_project() {
    assert_eq!(relative_dir("/p/app", "/other/x.ts"), "/other");
    assert_eq!(relative_dir("/p/app", "/p/application/x.ts"), "/p/application");
}
