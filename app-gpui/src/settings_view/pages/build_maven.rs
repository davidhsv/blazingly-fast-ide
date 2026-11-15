//! Build, Execution, Deployment › Build Tools › Maven: the JDK Maven runs on, the Maven home, the
//! user settings and toolchains files, the local repository, the active profiles, the thread count
//! and the Maven Daemon.

use super::build_sdk_field::{Detector, SdkFieldView, SdkSpec, ToolPage};
use super::{Binding, FieldKind, FormView, Part, field, toggle};
use gpui_kit::{App, AppContext, Entity};

pub const PAGE: &str = "build.maven";

/// The JRE choice.
pub fn jre() -> SdkSpec {
    SdkSpec {
        key: "languages.java.maven.runnerJre",
        label: "JRE",
        hint: Some(
            "JDK used for Maven sync, builds, goals and tests. An explicit JAVA_HOME in a run configuration takes \
             precedence.",
        ),
        kind: "jdk",
        empty: Some(("Use Project JDK", None)),
        directory: true,
    }
}

/// The Maven home choice.
pub fn home() -> SdkSpec {
    SdkSpec {
        key: "languages.java.maven.home",
        label: "Maven home path",
        hint: Some(
            "Runs sync, builds and Maven run configurations. A chosen home is used instead of the project's mvnw. \
             Bypassed while the Maven daemon is enabled.",
        ),
        kind: "maven",
        empty: Some(("Auto", Some("Maven wrapper, else mvn on PATH"))),
        directory: true,
    }
}

/// The controls under the two choices.
pub fn rest() -> Vec<Part> {
    let path = |key: &'static str, label: &'static str, placeholder: &'static str, hint: &'static str, dirs: bool| {
        Part::from(field(key, label, FieldKind::Trimmed).placeholder(placeholder).hint(hint).browse(dirs))
    };
    vec![
        path(
            "languages.java.maven.userSettings",
            "User settings file",
            "~/.m2/settings.xml",
            "Passed as -s (mirrors, servers, proxies).",
            false,
        ),
        path(
            "languages.java.maven.toolchains",
            "Toolchains file",
            "~/.m2/toolchains.xml",
            "Passed as -t; selects the JDKs Maven compiles with.",
            false,
        ),
        path(
            "languages.java.maven.localRepository",
            "Local repository",
            "From settings.xml, else ~/.m2/repository",
            "Passed as -Dmaven.repo.local.",
            true,
        ),
        field("languages.java.maven.activeProfiles", "Active profiles", FieldKind::Text)
            .placeholder("dev,integration")
            .hint(
                "Comma-separated profiles (-P), combined with Maven tool window choices; !id excludes a profile. A \
                 run configuration’s own -P replaces these profiles.",
            )
            .into(),
        field("languages.java.maven.threads", "Thread count", FieldKind::Trimmed)
            .placeholder("4, 2C")
            .hint(
                "Passed as -T to Maven builds and run configurations: a thread count (4) or a core multiplier (2C, \
                 2.5C) for parallel builds of multi-module projects. Empty runs Maven serially.",
            )
            .into(),
        toggle("languages.java.maven.daemon", "Use Maven Daemon (mvnd)")
            .hint(
                "Builds, sync and Maven run configurations run through mvnd for speed. It embeds its own Maven (the \
                 home and mvnw are bypassed), needs Java 17+ and builds in parallel by default. Falls back to mvn \
                 when mvnd is missing or the JDK is too old.",
            )
            .into(),
        path(
            "languages.java.maven.mvndHome",
            "Maven Daemon home path",
            "MVND_HOME, else auto-detected",
            "The mvnd home (or its executable). Empty = $MVND_HOME, else mvnd on PATH.",
            true,
        ),
    ]
}

/// The page: the JRE and Maven home choices, then the other controls.
pub fn page(binding: Binding, detector: Detector, cx: &mut App) -> Entity<ToolPage> {
    cx.new(|cx| {
        let mut page = ToolPage::new(PAGE, &binding, cx);
        for spec in [jre(), home()] {
            let (binding, detector) = (binding.clone(), detector.clone());
            let view = cx.new(|cx| SdkFieldView::new(binding, spec, detector, cx));
            page.add(view, None, cx);
        }
        let form_binding = binding.clone();
        let form = cx.new(|cx| FormView::new(form_binding, rest(), cx));
        page.add(form, None, cx);
        page
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: the path fields, the profiles, the thread count and the daemon checkbox
    // show their defaults and stage what is typed.
    #[gpui_kit::test]
    async fn the_maven_paths_profiles_threads_and_daemon_stage(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, rest(), cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        let labels: Vec<&str> = lines.iter().filter(|line| !line.starts_with("    ")).map(String::as_str).collect();
        assert_eq!(
            labels,
            [
                "User settings file: ",
                "Toolchains file: ",
                "Local repository: ",
                "Active profiles: ",
                "Thread count: ",
                "[ ] Use Maven Daemon (mvnd)",
                "Maven Daemon home path: ",
            ]
        );
        let threads = view.read_with(cx, |view, _| view.field("languages.java.maven.threads").cloned()).expect("field");
        type_over(&threads, "2C", cx);
        click(cx, "settings-toggle-languages.java.maven.daemon");
        let staged = staged(&model, cx);
        assert!(staged.contains(&("project:languages.java.maven.threads".to_owned(), "\"2C\"".to_owned())));
        assert!(staged.contains(&("project:languages.java.maven.daemon".to_owned(), "true".to_owned())));
    }
}
