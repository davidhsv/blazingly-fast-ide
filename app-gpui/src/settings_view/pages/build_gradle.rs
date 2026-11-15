//! Build, Execution, Deployment › Build Tools › Gradle: who builds and runs a main class, the JDK
//! Gradle runs on, the distribution (the wrapper or a local installation, with its home) and the
//! Gradle user home.

use super::build_sdk_field::{Detector, SdkFieldView, SdkSpec, Shown, ToolPage};
use super::{Binding, FieldKind, FormView, Part, field, fixed_choices};
use gpui_kit::{App, AppContext, Entity};
use std::rc::Rc;

pub const PAGE: &str = "build.gradle";
const DISTRIBUTION: &str = "languages.java.gradle.distribution";

fn run_using() -> Vec<Part> {
    vec![Part::Select {
        key: "languages.java.gradle.runUsing",
        label: "Build and run using",
        choices: fixed_choices(&[("gradle", "Gradle"), ("ide", "OpenIDEA")]),
        hint: Some(
            "Gradle runs a main class as a JavaExec task on its source set's runtime classpath (Gradle's resource \
             processing included). OpenIDEA launches it on the Java language server's classpath."
                .into(),
        ),
        beside: None,
        indent: false,
    }]
}

/// The Gradle JVM choice.
pub fn jvm() -> SdkSpec {
    SdkSpec {
        key: "languages.java.gradle.jvm",
        label: "Gradle JVM",
        hint: Some(
            "JDK used for Gradle sync, tasks, run configurations and tests. An explicit JAVA_HOME in a run \
             configuration takes precedence.",
        ),
        kind: "jdk",
        empty: Some(("Project JDK", None)),
        directory: true,
    }
}

fn distribution() -> Vec<Part> {
    vec![Part::Select {
        key: DISTRIBUTION,
        label: "Distribution",
        choices: fixed_choices(&[("wrapper", "Gradle wrapper"), ("local", "Local installation")]),
        hint: Some(
            "Uses the Gradle wrapper when present, otherwise the project-pinned mise/asdf Gradle, then Gradle on PATH."
                .into(),
        ),
        beside: None,
        indent: false,
    }]
}

fn home() -> Vec<Part> {
    vec![
        field("languages.java.gradle.home", "Gradle home", FieldKind::Text)
            .placeholder("Gradle installation directory")
            .hint("Uses bin/gradle (bin/gradle.bat on Windows) instead of the wrapper or pinned toolchain.")
            .browse(true)
            .into(),
    ]
}

fn user_home() -> Vec<Part> {
    vec![
        field("languages.java.gradle.userHome", "Gradle user home", FieldKind::Text)
            .placeholder("~/.gradle")
            .hint("Sets GRADLE_USER_HOME. Empty uses Gradle's default.")
            .browse(true)
            .into(),
    ]
}

/// The page: its controls in the order shown, Gradle home only with a local installation.
pub fn page(binding: Binding, detector: Detector, cx: &mut App) -> Entity<ToolPage> {
    cx.new(|cx| {
        let mut page = ToolPage::new(PAGE, &binding, cx);
        let form = |parts: Vec<Part>, cx: &mut gpui_kit::Context<ToolPage>| {
            let binding = binding.clone();
            cx.new(|cx| FormView::new(binding, parts, cx))
        };
        let run = form(run_using(), cx);
        page.add(run, None, cx);
        let jvm_binding = binding.clone();
        let jvm = cx.new(|cx| SdkFieldView::new(jvm_binding, jvm(), detector, cx));
        page.add(jvm, None, cx);
        let distribution = form(distribution(), cx);
        page.add(distribution, None, cx);
        let local_binding = binding.clone();
        let local: Shown = Rc::new(move |cx: &App| local_binding.text(DISTRIBUTION, cx) == "local");
        let home = form(home(), cx);
        page.add(home, Some(local), cx);
        let user_home = form(user_home(), cx);
        page.add(user_home, None, cx);
        page
    })
}

#[cfg(test)]
mod tests {
    use super::super::Then;
    use super::super::build_sdk_field::SdkChoice;
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: Gradle home is shown only with a local installation; choosing one shows
    // it, and what is typed there is staged into the project.
    #[gpui_kit::test]
    async fn gradle_home_is_shown_only_for_a_local_installation(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let detector: Detector =
            Rc::new(|_: &'static str, then: Then<Vec<SdkChoice>>, cx: &mut App| then(Vec::new(), cx));
        init(cx);
        let page = cx.update(|cx| page(binding, detector, cx));
        let (_, cx) = cx.add_window_view(move |_, _| PageHost(page.clone()));
        let count = |cx: &mut gpui_kit::VisualTestContext| cx.debug_bounds("settings-text-languages.java.gradle.home");
        assert!(count(cx).is_none(), "the wrapper is the default");
        assert!(cx.debug_bounds("settings-text-languages.java.gradle.userHome").is_some());
        click(cx, "settings-select-languages.java.gradle.distribution");
        click(cx, "settings-choice-languages.java.gradle.distribution-local");
        assert!(count(cx).is_some(), "Gradle home is shown");
        let staged = staged(&model, cx);
        assert_eq!(staged, [("project:languages.java.gradle.distribution".to_owned(), "\"local\"".to_owned())]);
    }

    /// A window drawing a page view.
    struct PageHost(Entity<ToolPage>);

    impl gpui_kit::Render for PageHost {
        fn render(&mut self, _: &mut gpui_kit::Window, _: &mut gpui_kit::Context<Self>) -> impl gpui_kit::IntoElement {
            gpui_kit::div().size_full().child(self.0.clone())
        }
    }
}
