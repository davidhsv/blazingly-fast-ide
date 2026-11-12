//! Small platform helpers, one module each: the text "Copy Path" copies, paths and labels for the
//! OS file manager, the UI's error log, facts about the platform, and JDK downloads.

pub mod copy_path {
    //! The values of the Copy Path actions, shared by the project view, editor tabs and the
    //! caret's copy chooser.

    use crate::platform::paths::is_same_or_inside;
    use crate::platform::project_roots::{FolderRole, OpenProject, RootFolder, project_relative, root_of_path};
    use crate::platform::virtual_docs::basename;

    /// Which form of a path to copy.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PathKind {
        Absolute,
        /// From the content root.
        Relative,
        /// The file name alone.
        Name,
        /// From the source root.
        Source,
    }

    fn at_or_below(path: &str, folder: &str) -> bool {
        path.strip_prefix(folder).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }

    /// The user-visible folder containing `path`: the project root, or in a workspace the
    /// innermost member holding it. Workspace state is never a content root.
    pub fn content_root<'a>(project: Option<OpenProject<'a>>, path: &str) -> Option<&'a str> {
        let project = project?;
        let Some(members) = project.workspace else { return Some(project.root) };
        members.iter().map(String::as_str).filter(|member| at_or_below(path, member)).max_by_key(|member| member.len())
    }

    fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    /// `text` after its first `units` UTF-16 code units. A root and a path below it may be written
    /// with different separators, so the root is skipped by its length, in the unit the UI
    /// measures strings in, rather than stripped as a prefix.
    fn skip_utf16(text: &str, units: usize) -> &str {
        let mut skipped = 0_usize;
        for (at, c) in text.char_indices() {
            if skipped >= units {
                return &text[at..];
            }
            skipped += c.len_utf16();
        }
        ""
    }

    /// The text to copy for `path`, or `None` when it has no such form: a source path outside
    /// every root, or below an excluded one. `root` is the content root a relative path starts
    /// from; a path outside it is copied whole. `roots` are the project's effective roots.
    pub fn path_to_copy_from(
        kind: PathKind,
        path: &str,
        root: Option<&str>,
        project: Option<OpenProject<'_>>,
        roots: &[RootFolder],
    ) -> Option<String> {
        let below = match kind {
            PathKind::Absolute => return Some(path.to_owned()),
            PathKind::Name => return Some(basename(path).into_owned()),
            PathKind::Source => {
                let source = root_of_path(project, path, roots).filter(|source| source.kind != FolderRole::Excluded)?;
                let relative = project_relative(project, path)?;
                skip_utf16(relative, utf16_len(&source.path) + usize::from(!source.path.is_empty()))
            }
            PathKind::Relative => {
                let Some(root) = root.filter(|root| !root.is_empty() && is_same_or_inside(path, root)) else {
                    return Some(path.to_owned());
                };
                skip_utf16(path, utf16_len(root) + usize::from(!root.ends_with('/')))
            }
        };
        Some(if below.is_empty() { ".".to_owned() } else { below.to_owned() })
    }

    /// [`path_to_copy_from`] the content root of `path` in `project`.
    pub fn path_to_copy(
        kind: PathKind,
        path: &str,
        project: Option<OpenProject<'_>>,
        roots: &[RootFolder],
    ) -> Option<String> {
        path_to_copy_from(kind, path, content_root(project, path), project, roots)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::platform::project_roots::{DetectedRoots, current_roots, validate_project_structure};
        use serde_json::json;

        // src/platform/copyPath.test.ts › preserves root, outside paths and Windows IPC paths
        #[test]
        fn preserves_root_outside_paths_and_windows_ipc_paths() {
            let copy = |kind: PathKind, path: &str, root: &str| path_to_copy_from(kind, path, Some(root), None, &[]);
            assert_eq!(copy(PathKind::Absolute, "/p/a.ts", "/p").as_deref(), Some("/p/a.ts"));
            assert_eq!(copy(PathKind::Name, "/p/a.ts", "/p").as_deref(), Some("a.ts"));
            assert_eq!(copy(PathKind::Relative, "/p/src/a.ts", "/p").as_deref(), Some("src/a.ts"));
            assert_eq!(copy(PathKind::Relative, "/p", "/p").as_deref(), Some("."));
            assert_eq!(copy(PathKind::Relative, "/project/a.ts", "/p").as_deref(), Some("/project/a.ts"));
            assert_eq!(copy(PathKind::Relative, "/src/a.ts", "/").as_deref(), Some("src/a.ts"));
            assert_eq!(copy(PathKind::Relative, "C:/p/src/a.ts", "C:/p").as_deref(), Some("src/a.ts"));
        }

        // src/platform/copyPath.test.ts › uses the containing workspace folder as the content root
        #[test]
        fn uses_the_containing_workspace_folder_as_the_content_root() {
            let members = ["/a".to_owned(), "/b".to_owned()];
            let workspace = Some(members.as_slice());
            let project = Some(OpenProject { root: "/workspace", name: "workspace", workspace });
            let copy = |path: &str| path_to_copy(PathKind::Relative, path, project, &[]);
            assert_eq!(copy("/b/src/a.ts").as_deref(), Some("src/a.ts"));
            assert_eq!(copy("/outside/a.ts").as_deref(), Some("/outside/a.ts"));
        }

        // src/platform/copyPath.test.ts › uses effective nested source, test and resource roots and respects exclusions
        #[test]
        fn uses_effective_nested_source_test_and_resource_roots_and_respects_exclusions() {
            let project = Some(OpenProject { root: "/p", name: "p", workspace: None });
            let settings = json!({ "sourceRoots": ["generated"], "unmarked": ["src/old"] });
            let structure = validate_project_structure(&settings);
            let detected = [
                ("src", FolderRole::Source),
                ("src/test", FolderRole::Test),
                ("res", FolderRole::Resources),
                ("src/out", FolderRole::Excluded),
                ("src/old", FolderRole::Source),
            ]
            .map(|(path, kind)| RootFolder { path: path.to_owned(), kind, detected_from: None });
            let state = DetectedRoots { root: Some("/p".to_owned()), detected: detected.into() };
            let roots = current_roots(&state, Some("/p"), &structure);
            let copy = |path: &str| path_to_copy(PathKind::Source, path, project, &roots);
            assert_eq!(copy("/p/src/a.ts").as_deref(), Some("a.ts"));
            assert_eq!(copy("/p/src/test/a.ts").as_deref(), Some("a.ts"));
            assert_eq!(copy("/p/res/config.json").as_deref(), Some("config.json"));
            assert_eq!(copy("/p/generated/a.ts").as_deref(), Some("a.ts"));
            assert_eq!(copy("/p/src/old/a.ts").as_deref(), Some("old/a.ts"));
            for path in ["/p/src/out/a.ts", "/p/src-other/a.ts", "/outside/a.ts"] {
                assert_eq!(copy(path), None, "{path}");
            }
        }
    }
}

pub mod platform_info {
    //! The operating system the UI runs on and what its windows look like there.

    /// An operating system, as far as the UI tells them apart.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Os {
        Macos,
        Windows,
        Linux,
        /// A browser, or a system the UI has no conventions of its own for.
        Web,
    }

    impl Os {
        /// The system a backend platform name (`macos`, `windows`, `linux`) stands for.
        pub fn from_platform(platform: &str) -> Self {
            match platform {
                "macos" => Self::Macos,
                "windows" => Self::Windows,
                "linux" => Self::Linux,
                _ => Self::Web,
            }
        }

        pub fn as_str(self) -> &'static str {
            match self {
                Self::Macos => "macos",
                Self::Windows => "windows",
                Self::Linux => "linux",
                Self::Web => "web",
            }
        }
    }

    /// What the backend reports about the app and its host.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AppInfo {
        pub version: String,
        pub platform: String,
        pub windows_build_number: Option<u32>,
        /// Linux on a Wayland display.
        pub wayland: bool,
    }

    /// What the UI knows about the platform it runs on.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PlatformInfo {
        pub os: Os,
        /// The app's version; unknown until the backend answers.
        pub version: Option<String>,
        pub windows_build_number: Option<u32>,
        /// Native traffic lights overlay the title bar (desktop app on macOS, Overlay style).
        pub overlay_title_bar: bool,
        /// Frameless window: the title bar draws the main menu and caption buttons (desktop app
        /// on Windows and Linux).
        pub frameless_title_bar: bool,
        /// Linux on a Wayland display: GTK draws the frameless windows' shadow and resize edges.
        pub wayland: bool,
    }

    /// OS from a user agent (before the backend answers, or in a browser). "darwin" must not read
    /// as Windows.
    pub fn guess_os(user_agent: Option<&str>) -> Os {
        let Some(user_agent) = user_agent else { return Os::Web };
        let agent = user_agent.to_ascii_lowercase();
        let names = |words: &[&str]| words.iter().any(|word| agent.contains(*word));
        if names(&["mac", "darwin"]) {
            Os::Macos
        } else if names(&["windows", "win32", "win64"]) {
            Os::Windows
        } else if names(&["linux"]) {
            Os::Linux
        } else {
            Os::Web
        }
    }

    impl PlatformInfo {
        /// What is known before the backend answers. `desktop`: the UI runs in the desktop app
        /// rather than in a browser.
        pub fn guessed(user_agent: Option<&str>, desktop: bool) -> Self {
            let os = guess_os(user_agent);
            Self {
                os,
                version: None,
                windows_build_number: None,
                overlay_title_bar: desktop && os == Os::Macos,
                frameless_title_bar: desktop && matches!(os, Os::Windows | Os::Linux),
                wayland: false,
            }
        }

        /// The facts once the backend was asked. `None`, when it could not answer, keeps the guess.
        pub fn with_app_info(self, info: Option<&AppInfo>) -> Self {
            let Some(info) = info else { return self };
            let os = Os::from_platform(&info.platform);
            Self {
                os,
                version: Some(info.version.clone()),
                windows_build_number: info.windows_build_number,
                overlay_title_bar: os == Os::Macos,
                frameless_title_bar: matches!(os, Os::Windows | Os::Linux),
                wayland: info.wayland,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // src/platform/platformInfo.test.ts › loads %s OS build %s separately from the app version
        #[test]
        fn loads_the_os_build_separately_from_the_app_version() {
            for (platform, os, windows_build_number) in [
                ("windows", Os::Windows, Some(19045_u32)),
                ("windows", Os::Windows, Some(22631)),
                ("macos", Os::Macos, None),
                ("linux", Os::Linux, None),
            ] {
                let info = AppInfo {
                    platform: platform.to_owned(),
                    windows_build_number,
                    version: "0.1.39".to_owned(),
                    wayland: false,
                };
                let loaded = PlatformInfo::guessed(None, true).with_app_info(Some(&info));
                assert_eq!(loaded.os, os, "{platform}");
                assert_eq!(loaded.os.as_str(), platform);
                assert_eq!(loaded.windows_build_number, windows_build_number, "{platform}");
                assert_eq!(loaded.version.as_deref(), Some("0.1.39"), "{platform}");
            }
        }

        // src/platform/platformInfo.test.ts › keeps platform defaults when the query fails
        #[test]
        fn keeps_platform_defaults_when_the_query_fails() {
            let initial = PlatformInfo::guessed(None, true);
            assert_eq!(initial.clone().with_app_info(None), initial);
        }

        /// Not from a spec: the guess from a user agent, and the title bar it implies.
        #[test]
        fn guesses_the_os_from_the_user_agent() {
            let mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
            assert_eq!(guess_os(Some(mac)), Os::Macos);
            assert_eq!(guess_os(Some("node.js/22 (darwin arm64)")), Os::Macos);
            assert_eq!(guess_os(Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")), Os::Windows);
            assert_eq!(guess_os(Some("Mozilla/5.0 (X11; Linux x86_64)")), Os::Linux);
            assert_eq!(guess_os(Some("Mozilla/5.0 (X11; CrOS x86_64)")), Os::Web);
            assert_eq!(guess_os(None), Os::Web);
            let windows = PlatformInfo::guessed(Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"), true);
            assert!(windows.frameless_title_bar && !windows.overlay_title_bar);
            assert!(PlatformInfo::guessed(Some(mac), true).overlay_title_bar);
            assert!(!PlatformInfo::guessed(Some(mac), false).overlay_title_bar);
            assert_eq!(Os::from_platform("freebsd"), Os::Web);
        }
    }
}

pub mod file_manager {
    //! The OS file manager: its name in menus and the paths its Shell APIs accept.

    use super::platform_info::Os;

    /// `path` without `prefix`, which is ASCII and matched whatever its letters' case.
    fn strip_prefix_ignore_ascii_case<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
        let head = path.as_bytes().get(..prefix.len())?;
        if head.eq_ignore_ascii_case(prefix.as_bytes()) { path.get(prefix.len()..) } else { None }
    }

    fn is_drive_path(path: &str) -> bool {
        matches!(path.as_bytes(), [letter, b':', b'/', ..] if letter.is_ascii_alphabetic())
    }

    /// A path inside a WSL distribution as the share Windows reaches it by.
    fn wsl_share_path(path: &str, wsl_distro: Option<&str>) -> String {
        match wsl_distro {
            Some(distro) if path.starts_with('/') => format!(r"\\wsl.localhost\{distro}{}", path.replace('/', "\\")),
            _ => path.to_owned(),
        }
    }

    /// A path in the form the local OS file manager's Shell APIs accept. Project paths use `/`
    /// separators and may be Windows verbatim paths, but Explorer's `ILCreateFromPathW` rejects
    /// `\\?\UNC\...`. `wsl_distro` is the distribution of a remote session into WSL, if one is
    /// open: its paths are reached through the distribution's share.
    pub fn file_manager_path(path: &str, os: Os, wsl_distro: Option<&str>) -> String {
        if os != Os::Windows {
            return path.to_owned();
        }
        let mut path = wsl_share_path(path, wsl_distro);
        if let Some(share) = strip_prefix_ignore_ascii_case(&path, "//?/unc/") {
            path = format!(r"\\{}", share.replace('/', "\\"));
        } else if let Some(rest) = path.strip_prefix("//?/") {
            path = rest.to_owned();
        }
        if let Some(share) = strip_prefix_ignore_ascii_case(&path, r"\\?\unc\") {
            return format!(r"\\{share}");
        }
        if let Some(rest) = path.strip_prefix(r"\\?\") {
            return rest.to_owned();
        }
        if path.starts_with("//") || is_drive_path(&path) {
            return path.replace('/', "\\");
        }
        path
    }

    /// The file manager's name. English source text; callers and the action registry translate it.
    pub fn file_manager_name(os: Os) -> &'static str {
        match os {
            Os::Macos => "Finder",
            Os::Windows => "Explorer",
            Os::Linux | Os::Web => "Files",
        }
    }

    pub fn show_in_file_manager_label(os: Os) -> &'static str {
        match os {
            Os::Macos => "Show in Finder",
            Os::Windows => "Show in Explorer",
            Os::Linux | Os::Web => "Show in Files",
        }
    }

    /// The project action has historically used "Reveal" on macOS.
    pub fn reveal_in_file_manager_label(os: Os) -> &'static str {
        if os == Os::Macos { "Reveal in Finder" } else { show_in_file_manager_label(os) }
    }

    /// The paths to hand to Explorer, Finder or the Linux desktop's file manager to reveal
    /// `paths`. Nothing is revealed when there are none.
    pub fn reveal_paths<P: AsRef<str>>(paths: &[P], os: Os, wsl_distro: Option<&str>) -> Vec<String> {
        paths.iter().map(|path| file_manager_path(path.as_ref(), os, wsl_distro)).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::platform::i18n::I18n;

        fn chinese() -> I18n {
            let mut i18n = I18n::default();
            i18n.set_locale(Some("zh"), "en");
            i18n
        }

        // src/platform/fileManager.test.ts › names the %s file manager in submenus and project actions
        #[test]
        fn names_the_file_manager_in_submenus_and_project_actions() {
            for (os, name, label) in [
                (Os::Macos, "Finder", "Reveal in Finder"),
                (Os::Windows, "Explorer", "Show in Explorer"),
                (Os::Linux, "Files", "Show in Files"),
                (Os::Web, "Files", "Show in Files"),
            ] {
                assert_eq!(file_manager_name(os), name);
                assert_eq!(reveal_in_file_manager_label(os), label);
            }
        }

        // src/platform/fileManager.test.ts › translates the project action on every desktop platform
        #[test]
        fn translates_the_project_action_on_every_desktop_platform() {
            let i18n = chinese();
            assert_eq!(i18n.tx(reveal_in_file_manager_label(Os::Macos)), "在访达中显示");
            assert_eq!(i18n.tx(reveal_in_file_manager_label(Os::Windows)), "在资源管理器中显示");
            assert_eq!(i18n.tx(reveal_in_file_manager_label(Os::Linux)), "在文件管理器中显示");
        }

        // src/platform/fileManager.test.ts › names the file manager of the platform instead of always Finder
        #[test]
        fn names_the_file_manager_of_the_platform_instead_of_always_finder() {
            assert_eq!(show_in_file_manager_label(Os::Macos), "Show in Finder");
            assert_eq!(show_in_file_manager_label(Os::Windows), "Show in Explorer");
            assert_eq!(show_in_file_manager_label(Os::Linux), "Show in Files");
            assert_eq!(show_in_file_manager_label(Os::Web), "Show in Files");
        }

        // src/platform/fileManager.test.ts › is translated into the platform's file manager, not Finder
        #[test]
        fn is_translated_into_the_platforms_file_manager_not_finder() {
            let i18n = chinese();
            assert_eq!(i18n.tx(show_in_file_manager_label(Os::Macos)), "在访达中显示");
            assert_eq!(i18n.tx(show_in_file_manager_label(Os::Windows)), "在资源管理器中显示");
            assert_eq!(i18n.tx(show_in_file_manager_label(Os::Linux)), "在文件管理器中显示");
        }

        // src/platform/fileManager.test.ts › converts verbatim UNC paths to the form Windows Shell accepts
        #[test]
        fn converts_verbatim_unc_paths_to_the_form_windows_shell_accepts() {
            let share = r"\\192.168.10.9\nas\apps\ignis\vaults\我的知识库\111health工作\生产日志分析";
            let verbatim = r"\\?\UNC\192.168.10.9\nas\apps\ignis\vaults\我的知识库\111health工作\生产日志分析";
            assert_eq!(file_manager_path(verbatim, Os::Windows, None), share);
            let slashed = "//?/UNC/192.168.10.9/nas/apps/ignis/vaults/我的知识库/111health工作/生产日志分析";
            assert_eq!(file_manager_path(slashed, Os::Windows, None), share);
            assert_eq!(file_manager_path(r"\\?\unc\server\share", Os::Windows, None), r"\\server\share");
        }

        // src/platform/fileManager.test.ts › converts the UI's slash-separated Windows paths to native paths
        #[test]
        fn converts_the_uis_slash_separated_windows_paths_to_native_paths() {
            let native = |path: &str| file_manager_path(path, Os::Windows, None);
            assert_eq!(native("//server/share/my notes"), r"\\server\share\my notes");
            assert_eq!(native("C:/work/my project"), r"C:\work\my project");
            assert_eq!(native(r"\\?\C:\work\my project"), r"C:\work\my project");
            assert_eq!(native("//?/C:/work/my project"), r"C:\work\my project");
        }

        // src/platform/fileManager.test.ts › leaves native Windows paths and non-Windows paths alone
        #[test]
        fn leaves_native_windows_paths_and_non_windows_paths_alone() {
            assert_eq!(file_manager_path(r"\\server\share\my notes", Os::Windows, None), r"\\server\share\my notes");
            assert_eq!(file_manager_path("/home/me/my notes", Os::Linux, None), "/home/me/my notes");
            assert_eq!(file_manager_path("/Users/me/my notes", Os::Macos, None), "/Users/me/my notes");
        }

        // src/platform/fileManager.test.ts › translates a WSL distro path to a shell-compatible share on Windows
        #[test]
        fn translates_a_wsl_distro_path_to_a_shell_compatible_share_on_windows() {
            assert_eq!(
                file_manager_path("/home/me/my app", Os::Windows, Some("Ubuntu-24.04")),
                r"\\wsl.localhost\Ubuntu-24.04\home\me\my app"
            );
        }
    }
}

pub mod error_log {
    //! Uncaught errors of a window go to the log file, so that the log attached to a report shows
    //! what failed in the UI.

    use serde_json::Value;

    /// Per window session: a loop of the same failure must not flood the log.
    pub const MAX_REPORTS: usize = 50;

    /// The kinds of failure nothing caught.
    pub const UNCAUGHT_ERROR: &str = "uncaught error";
    pub const UNHANDLED_REJECTION: &str = "unhandled rejection";

    /// What failed.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Reason<'a> {
        /// An error: its kind (`TypeError`), its message and, when it has one, the stack trace.
        Error {
            name: &'a str,
            message: &'a str,
            stack: Option<&'a str>,
        },
        Text(&'a str),
        /// Any other value; `None` for a failure that carries none.
        Value(Option<&'a Value>),
    }

    /// White space in the ECMAScript sense. Unlike `char::is_whitespace` it has U+FEFF and lacks
    /// U+0085.
    fn is_space(c: char) -> bool {
        matches!(c, '\t'..='\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}')
            || matches!(c, '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
    }

    /// The text of a failure for the log: an error with its stack trace, a value as JSON.
    pub fn describe_error(reason: Reason<'_>) -> String {
        match reason {
            // A stack trace usually starts with the error itself.
            Reason::Error { message, stack: Some(stack), .. } if stack.contains(message) => stack.to_owned(),
            Reason::Error { name, message, stack } => {
                let title = match (name.is_empty(), message.is_empty()) {
                    (true, _) => message.to_owned(),
                    (false, true) => name.to_owned(),
                    (false, false) => format!("{name}: {message}"),
                };
                format!("{title}\n{}", stack.unwrap_or_default())
            }
            Reason::Text(text) => text.to_owned(),
            Reason::Value(Some(value)) => value.to_string(),
            Reason::Value(None) => "undefined".to_owned(),
        }
    }

    /// The log line of a caught error worth keeping.
    pub fn error_message(context: &str, reason: Reason<'_>) -> String {
        format!("{context}: {}", describe_error(reason).trim_matches(is_space))
    }

    /// Turns the failures nothing caught in one window into log lines.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ErrorReporter {
        label: String,
        sent: usize,
        last: String,
    }

    impl ErrorReporter {
        /// A reporter for the window `label`; `None` when the window is not known.
        pub fn new(label: Option<&str>) -> Self {
            Self { label: label.unwrap_or("?").to_owned(), sent: 0, last: String::new() }
        }

        /// The line to log (as an error) for a failure of `kind`. `None` when it repeats the
        /// line before it, or once the window has reported [`MAX_REPORTS`] failures.
        pub fn report(&mut self, kind: &str, reason: Reason<'_>) -> Option<String> {
            let message = format!("[{}] {kind}: {}", self.label, describe_error(reason).trim_matches(is_space));
            if message == self.last || self.sent >= MAX_REPORTS {
                return None;
            }
            self.last.clone_from(&message);
            self.sent += 1;
            Some(message)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde_json::json;

        // src/platform/errorLog.test.ts › writes uncaught errors and unhandled rejections once each, with the window
        #[test]
        fn writes_uncaught_errors_and_unhandled_rejections_once_each_with_the_window() {
            let mut reporter = ErrorReporter::new(Some("project-2"));
            let stack = "Error: boom\n    at run (app.js:1:1)";
            let boom = Reason::Error { name: "Error", message: "boom", stack: Some(stack) };
            let first = reporter.report(UNCAUGHT_ERROR, boom).unwrap_or_default();
            assert!(first.starts_with("[project-2] uncaught error: Error: boom\n    at "), "{first}");
            assert_eq!(reporter.report(UNCAUGHT_ERROR, boom), None);
            assert_eq!(
                reporter.report(UNHANDLED_REJECTION, Reason::Text("no project")).as_deref(),
                Some("[project-2] unhandled rejection: no project")
            );
        }

        // src/platform/errorLog.test.ts › describes errors, strings and other values
        #[test]
        fn describes_errors_strings_and_other_values() {
            assert_eq!(describe_error(Reason::Text("x")), "x");
            assert_eq!(describe_error(Reason::Value(Some(&json!({ "kind": "notFound" })))), r#"{"kind":"notFound"}"#);
            assert_eq!(describe_error(Reason::Value(None)), "undefined");
        }
    }
}

pub mod jdk {
    //! Download JDK, shared by the JDK pickers in Settings and the "Download JDK N" fixes offered
    //! where a launch or sync lacks a JDK.

    use crate::platform::i18n::{I18n, Param};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    /// The action behind the one-click fix.
    pub const DOWNLOAD_JDK_ACTION: &str = "java.downloadJdk";
    /// The vendor downloads default to.
    pub const TEMURIN: &str = "temurin";

    /// The argument of [`DOWNLOAD_JDK_ACTION`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct DownloadJdkArgs {
        /// Java feature version, e.g. 25.
        pub version: u32,
        /// Run jdtls on it: chosen as the language server JDK unless Auto already picks it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub language_server: Option<bool>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct JdkRequirement {
        pub version: u32,
        /// The JDK runs the Java language server (jdtls), not the project.
        pub language_server: bool,
    }

    fn is_word(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }

    /// Whether `message` is about the JDK of the language server: it names `jdtls` as a word, or
    /// the "language server JDK", in any letter case.
    fn names_language_server(message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        let mut jdtls = lower.match_indices("jdtls").map(|(at, word)| (&lower[..at], &lower[at + word.len()..]));
        lower.contains("language server jdk")
            || jdtls.any(|(before, after)| !before.ends_with(is_word) && !after.starts_with(is_word))
    }

    /// The JDK a launch / sync error asks for: the backend's messages say
    /// "jdtls needs JDK 25 or newer" / "it needs JDK 21 or newer".
    pub fn required_jdk(message: Option<&str>) -> Option<JdkRequirement> {
        let message = message?;
        let version = message.match_indices("needs JDK ").find_map(|(at, phrase)| {
            let rest = &message[at + phrase.len()..];
            let (number, after) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len()));
            (!number.is_empty() && after.starts_with(" or newer")).then_some(number)
        })?;
        Some(JdkRequirement { version: version.parse::<u32>().ok()?, language_server: names_language_server(message) })
    }

    /// A "Download JDK N" notification / banner action: running it executes
    /// [`DOWNLOAD_JDK_ACTION`] with `args`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DownloadJdkFix {
        pub args: DownloadJdkArgs,
    }

    impl DownloadJdkFix {
        /// The English source text of the action's label.
        pub fn label(&self) -> String {
            format!("Download JDK {}", self.args.version)
        }

        /// The label in the active locale.
        pub fn localized_label(&self, i18n: &mut I18n) -> String {
            i18n.tx_built(&["Download JDK ", ""], &[Param::Number(f64::from(self.args.version))])
        }
    }

    /// The fix for `message`; none when it names no JDK or nothing can download
    /// (`action_registered`: a handler of [`DOWNLOAD_JDK_ACTION`] exists).
    pub fn download_jdk_fix(message: Option<&str>, action_registered: bool) -> Option<DownloadJdkFix> {
        let need = required_jdk(message).filter(|_| action_registered)?;
        Some(DownloadJdkFix {
            args: DownloadJdkArgs { version: need.version, language_server: Some(need.language_server) },
        })
    }

    /// Whether `arg` is the argument of [`DOWNLOAD_JDK_ACTION`]: an object with a numeric version.
    pub fn is_download_jdk_args(arg: Option<&Value>) -> bool {
        arg.and_then(|arg| arg.get("version")).is_some_and(Value::is_number)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde_json::json;

        // Verbatim from a run console (Windows, JDK 8 / 17 / 21 installed).
        const JDTLS_TOO_OLD: &str = concat!(
            "jdtls needs JDK 25 or newer to run (found JDK 8, 17, 21). Pick one in Settings › Languages & ",
            "Frameworks › Java › Language Server › JDK (it restarts automatically), or install a newer JDK and ",
            "restart the server; the project can still target an older Java version."
        );

        // src/platform/jdk.test.ts › %s
        #[test]
        fn finds_the_jdk_a_message_asks_for() {
            let language_server = |version: u32| Some(JdkRequirement { version, language_server: true });
            let project = |version: u32| Some(JdkRequirement { version, language_server: false });
            for (message, expected) in [
                (Some(JDTLS_TOO_OLD), language_server(25)),
                (
                    Some(concat!(
                        r"The language server JDK (C:\Java\jdk-21, JDK 21) is too old: jdtls needs JDK 25 or newer. ",
                        "Download JDK 25, choose another one in Settings › Languages & Frameworks › Java › Language ",
                        "Server › JDK, or set it to Auto."
                    )),
                    language_server(25),
                ),
                (
                    Some(concat!(
                        "This project targets Java 21: it needs JDK 21 or newer (found JDK 17). Download JDK 21, or ",
                        "choose a Project JDK in Settings › Languages & Frameworks › Java."
                    )),
                    project(21),
                ),
                (
                    Some(concat!(
                        "This project targets Java 17: it needs JDK 17 or newer (no JDK was found). Download JDK 17, ",
                        "or choose a Project JDK in Settings › Languages & Frameworks › Java.\n",
                        "[ERROR] Failed to execute goal"
                    )),
                    project(17),
                ),
                (
                    Some("The language server JDK /x is not a JDK home (Settings › Languages & Frameworks › Java)."),
                    None,
                ),
                (Some("`rust-analyzer` was not found on PATH"), None),
                (None, None),
            ] {
                assert_eq!(required_jdk(message), expected, "{message:?}");
            }
        }

        // src/platform/jdk.test.ts › offers Download JDK N, which runs the download action
        #[test]
        fn offers_download_jdk_n_which_runs_the_download_action() {
            let fix = download_jdk_fix(Some(JDTLS_TOO_OLD), true).expect("a fix");
            assert_eq!(fix.label(), "Download JDK 25");
            assert_eq!(serde_json::to_value(fix.args).unwrap(), json!({ "version": 25, "languageServer": true }));
            assert_eq!(download_jdk_fix(Some("jdtls crashed"), true), None);
        }

        // src/platform/jdk.test.ts › offers nothing where no window can download (e.g. without the tools feature)
        #[test]
        fn offers_nothing_where_no_window_can_download() {
            assert_eq!(download_jdk_fix(Some(JDTLS_TOO_OLD), false), None);
        }

        // src/platform/jdk.test.ts › recognises the action's argument
        #[test]
        fn recognises_the_actions_argument() {
            assert!(is_download_jdk_args(Some(&json!({ "version": 25 }))));
            assert!(!is_download_jdk_args(Some(&json!({ "version": "25" }))));
            assert!(!is_download_jdk_args(None));
        }
    }
}
