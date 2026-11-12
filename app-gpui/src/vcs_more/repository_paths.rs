//! Paths git stores relative to their repository, whatever project is open. They are joined by
//! [`crate::vcs_diff::diff_input::repository_path`].

#[cfg(test)]
mod tests {
    use crate::vcs_diff::diff_input::repository_path;

    // src/features/vcs/__tests__/repositoryPaths.test.ts › repository-relative paths › %s + %s → %s
    #[test]
    fn joins_a_root_and_a_repository_relative_path() {
        let cases = [
            (Some("/repo"), "src/a.txt", "/repo/src/a.txt"),
            (Some("/project/项目A"), "src/a.txt", "/project/项目A/src/a.txt"),
            (Some("/project/repoB/"), "src/a.txt", "/project/repoB/src/a.txt"),
            (Some("/repo"), "subproject/a.txt", "/repo/subproject/a.txt"),
            (Some(r"C:\project\项目A"), r"src\a.txt", "C:/project/项目A/src/a.txt"),
            (Some("C:/project/repoB"), r"src\a.txt", "C:/project/repoB/src/a.txt"),
            (Some(r"C:\"), "a.txt", "C:/a.txt"),
            (Some(r"\\server\share\repo"), r"src\a.txt", "//server/share/repo/src/a.txt"),
            (Some(r"\\?\C:\repo"), r"src\a.txt", "C:/repo/src/a.txt"),
            (Some("/"), "a.txt", "/a.txt"),
            (Some("/repo"), r"src/a\b.txt", r"/repo/src/a\b.txt"),
            (None, "src/a.txt", "src/a.txt"),
        ];
        for (root, path, expected) in cases {
            assert_eq!(repository_path(root, path), expected, "{root:?} + {path}");
        }
    }
}
