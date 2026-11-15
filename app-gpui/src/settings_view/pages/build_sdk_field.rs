//! The toolchain choice of the Build Tools pages (Gradle JVM, the Maven JRE and Maven home): a
//! drop-down of the installations found on the machine, the empty choice the page names ("Project
//! JDK", "Auto"…) and the path stored when it is none of them, with Detect again and Browse….
//!
//! The installations are looked for once the field is first drawn, through a [`Detector`]: the
//! backend's detection reads the PATH, the system's folders and the version managers' folders, so
//! it never runs on opening the dialog, and a test gives a [`SdkDetection`] of its own instead.

use super::{Binding, ControlEvent, Then, drop_down, drop_down_row, hint_line, row, row_label, select_box};
use crate::backend::Backend;
use crate::settings_model::language_pages::tildify;
use crate::settings_view::controls::tx;
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, EventEmitter, Global, IntoElement, MouseButton, Render, SharedString, Window,
    div, prelude::*, px,
};
use serde_json::Value;
use std::path::PathBuf;
use std::rc::Rc;

/// An installation found: where it is and what it is called (`Eclipse Adoptium 21.0.2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkChoice {
    pub path: String,
    pub name: String,
}

/// Looks for the installations of a kind (`jdk`, `maven`) and hands them to the callback.
pub type Detector = Rc<dyn Fn(&'static str, Then<Vec<SdkChoice>>, &mut App)>;

/// The detection the Settings dialog uses in place of the backend's (tests).
pub struct SdkDetection(pub Detector);

impl Global for SdkDetection {}

/// The detection for the dialog of `window`: the one set as [`SdkDetection`], else the backend's.
/// Test builds never look at the machine: without a detection of their own they find nothing.
pub fn detector(window: &str, cx: &App) -> Detector {
    match cx.try_global::<SdkDetection>() {
        Some(detection) => detection.0.clone(),
        None if cfg!(test) => Rc::new(|_: &'static str, then: Then<Vec<SdkChoice>>, cx: &mut App| then(Vec::new(), cx)),
        None => backend_detector(window),
    }
}

/// The backend's detection for `window`, on the backend's runtime; a failure is logged and lists
/// nothing, as the field then offers the empty choice, the stored path and Browse….
pub fn backend_detector(window: &str) -> Detector {
    let window: Rc<str> = Rc::from(window);
    Rc::new(move |kind: &'static str, then: Then<Vec<SdkChoice>>, cx: &mut App| {
        let Some(backend) = cx.try_global::<Backend>() else {
            then(Vec::new(), cx);
            return;
        };
        let label = window.to_string();
        let work = backend.with_host(move |host| async move {
            let kind = serde_json::from_value(Value::from(kind))
                .map_err(|error| ide_core::IdeError::internal(error.to_string()))?;
            let found = ide_host::ops::sdk::sdk_detect(&host, &label, kind).await?;
            let choices: Vec<SdkChoice> = found
                .into_iter()
                .map(|sdk| SdkChoice { path: sdk.path.to_string_lossy().into_owned(), name: sdk.name })
                .collect();
            Ok::<_, ide_core::IdeError>(choices)
        });
        cx.spawn(async move |cx| {
            let choices = match work.await {
                Ok(Ok(choices)) => choices,
                Ok(Err(error)) => {
                    tracing::warn!("Settings: looking for {kind} installations failed: {}", error.message);
                    Vec::new()
                }
                Err(error) => {
                    tracing::warn!("Settings: looking for {kind} installations failed: {error}");
                    Vec::new()
                }
            };
            cx.update(|cx| then(choices, cx));
        })
        .detach();
    })
}

/// What the field is: its setting, label and hint, the kind looked for, the empty choice, and
/// whether Browse… picks a folder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdkSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub hint: Option<&'static str>,
    pub kind: &'static str,
    /// The empty value's label and detail; none: no empty choice.
    pub empty: Option<(&'static str, Option<&'static str>)>,
    pub directory: bool,
}

/// Where the detection stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Detection {
    /// Not asked for yet: the field was not drawn.
    Idle,
    Loading,
    Ready(Vec<SdkChoice>),
}

pub struct SdkFieldView {
    binding: Binding,
    spec: SdkSpec,
    detector: Detector,
    detection: Detection,
    open: bool,
}

impl EventEmitter<ControlEvent> for SdkFieldView {}

impl SdkFieldView {
    pub fn new(binding: Binding, spec: SdkSpec, detector: Detector, cx: &mut Context<Self>) -> Self {
        cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()).detach();
        Self { binding, spec, detector, detection: Detection::Idle, open: false }
    }

    pub fn detection(&self) -> &Detection {
        &self.detection
    }

    /// Looks for the installations again.
    pub fn detect(&mut self, cx: &mut Context<Self>) {
        self.detection = Detection::Loading;
        let view = cx.weak_entity();
        let then: Then<Vec<SdkChoice>> = Rc::new(move |found: Vec<SdkChoice>, cx: &mut App| {
            view.update(cx, |view, cx| {
                view.detection = Detection::Ready(found);
                cx.notify();
            })
            .ok();
        });
        let (detector, kind) = (self.detector.clone(), self.spec.kind);
        // After this turn: a detection that answers at once finds the view free to update.
        cx.defer(move |cx| detector(kind, then, cx));
        cx.notify();
    }

    /// The choices as (value, label, detail): the empty one, those found, and the stored path when
    /// it is none of them.
    pub fn choices(&self, cx: &App) -> Vec<(String, String, Option<String>)> {
        let mut out: Vec<(String, String, Option<String>)> = Vec::new();
        if let Some((label, detail)) = self.spec.empty {
            out.push((String::new(), tx(cx, label), detail.map(|detail| tx(cx, detail))));
        }
        if let Detection::Ready(found) = &self.detection {
            out.extend(found.iter().map(|sdk| (sdk.path.clone(), sdk.name.clone(), Some(tildify(&sdk.path)))));
        }
        let value = self.binding.text(self.spec.key, cx);
        if !value.is_empty() && !out.iter().any(|(listed, _, _)| *listed == value) {
            out.push((value.clone(), tildify(&value), Some(tx(cx, "custom"))));
        }
        out
    }

    /// The label of the value stored, or the placeholder.
    pub fn shown(&self, cx: &App) -> String {
        let value = self.binding.text(self.spec.key, cx);
        let choices = self.choices(cx);
        match choices.into_iter().find(|(listed, _, _)| *listed == value) {
            Some((_, label, _)) => label,
            None if self.detection == Detection::Loading => tx(cx, "Detecting…"),
            None => tx(cx, "<No interpreter>"),
        }
    }

    fn choose(&mut self, value: String, cx: &mut Context<Self>) {
        self.open = false;
        self.binding.stage(self.spec.key, Value::String(value), cx);
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let (binding, key) = (self.binding.clone(), self.spec.key);
        let then: Then<PathBuf> = Rc::new(move |path: PathBuf, cx: &mut App| {
            binding.stage(key, Value::String(path.to_string_lossy().into_owned()), cx);
        });
        cx.emit(ControlEvent::Browse { directories: self.spec.directory, then });
    }

    /// The field as text: `Label: shown ▾`, the choices while the list is open, the hint.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = vec![format!("{}: {} ▾", tx(cx, self.spec.label), self.shown(cx))];
        if self.open {
            for (_, label, detail) in self.choices(cx) {
                out.push(match detail {
                    Some(detail) => format!("  {label} — {detail}"),
                    None => format!("  {label}"),
                });
            }
        }
        out.extend(self.spec.hint.map(|hint| format!("    {}", tx(cx, hint))));
        out
    }
}

impl Render for SdkFieldView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.detection == Detection::Idle {
            self.detect(cx);
        }
        let key = self.spec.key;
        let query = self.binding.query(cx);
        let label = tx(cx, self.spec.label);
        let value = self.binding.text(key, cx);
        let boxed = select_box(cx, &self.shown(cx))
            .id(SharedString::from(format!("settings-sdk-{key}")))
            .debug_selector(move || format!("settings-sdk-{key}"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open = !view.open;
                cx.notify();
            }));
        let list = self.open.then(|| {
            let rows: Vec<AnyElement> = self
                .choices(cx)
                .into_iter()
                .enumerate()
                .map(|(index, (choice, text, detail))| {
                    let chosen = choice == value;
                    let detail = detail.map(|detail| super::muted(cx, &detail));
                    drop_down_row(cx, format!("settings-sdk-{key}-choice-{index}"), &text, chosen)
                        .children(detail)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(choice.clone(), cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let icon_button = |cx: &mut Context<Self>, name: &'static str, icon: &str, title: &'static str| {
            let selector = format!("settings-sdk-{key}-{name}");
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector)
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
                .tooltip(crate::ui::tooltip::tooltip(title))
                .child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg-icon")))
        };
        let detect = if self.detection == Detection::Loading {
            super::muted(cx, &tx(cx, "Detecting installations")).into_any_element()
        } else {
            icon_button(cx, "refresh", "refresh", "Detect again")
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.detect(cx)))
                .into_any_element()
        };
        let browse = icon_button(cx, "browse", "folder-open", "Browse…")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse(cx)));
        let drawn = row(cx, self.binding.matches(self.spec.label, cx))
            .child(row_label(cx, &label, &query))
            .child(div().relative().child(boxed).children(list))
            .child(detect)
            .child(browse);
        match self.spec.hint {
            Some(hint) => {
                let hint = tx(cx, hint);
                div().flex().flex_col().child(drawn).child(hint_line(cx, &hint, 4.0)).into_any_element()
            }
            None => drawn.into_any_element(),
        }
    }
}

/// A Build Tools page: its parts one under the other, scrolled; a part with a condition is drawn
/// only while it holds (Gradle home with a local installation). Each part's requests go out as
/// the page's.
pub struct ToolPage {
    id: &'static str,
    parts: Vec<(gpui_kit::AnyView, Option<Shown>)>,
    _subscriptions: Vec<gpui_kit::Subscription>,
}

/// Whether a part of a page is drawn now.
pub type Shown = Rc<dyn Fn(&App) -> bool>;

impl EventEmitter<ControlEvent> for ToolPage {}

impl ToolPage {
    pub fn new(id: &'static str, binding: &Binding, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        Self { id, parts: Vec::new(), _subscriptions: vec![observe] }
    }

    /// Adds `view` below the parts there, drawn while `shown` holds (always without one).
    pub fn add<V: Render + EventEmitter<ControlEvent>>(
        &mut self,
        view: gpui_kit::Entity<V>,
        shown: Option<Shown>,
        cx: &mut Context<Self>,
    ) {
        let forward = cx.subscribe(&view, |_: &mut Self, _, event: &ControlEvent, cx| cx.emit(event.clone()));
        self._subscriptions.push(forward);
        self.parts.push((view.into(), shown));
    }

    /// The parts drawn now.
    pub fn shown_parts(&self, cx: &App) -> Vec<gpui_kit::AnyView> {
        let drawn = self.parts.iter().filter(|(_, shown)| shown.as_ref().is_none_or(|shown| shown(cx)));
        drawn.map(|(view, _)| view.clone()).collect()
    }
}

impl Render for ToolPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.id;
        div()
            .id(SharedString::from(format!("settings-page-{id}-body")))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(self.shown_parts(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use std::cell::RefCell;

    const PAGE: &str = "build.maven";

    fn spec() -> SdkSpec {
        SdkSpec {
            key: "languages.java.maven.runnerJre",
            label: "JRE",
            hint: None,
            kind: "jdk",
            empty: Some(("Use Project JDK", None)),
            directory: true,
        }
    }

    // Not from a spec: the installations are looked for once the field is drawn; one chosen is
    // staged, Detect again looks again, and a stored path that is none of them is a custom choice.
    #[gpui_kit::test]
    async fn the_installations_found_are_offered_once_the_field_is_drawn(cx: &mut TestAppContext) {
        let asked: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let log = asked.clone();
        let detector: Detector = Rc::new(move |kind: &'static str, then: Then<Vec<SdkChoice>>, cx: &mut App| {
            log.borrow_mut().push(kind);
            then(vec![SdkChoice { path: "/jdks/21".to_owned(), name: "Temurin 21".to_owned() }], cx);
        });
        let stored = [("languages.java.maven.runnerJre", Value::from("/opt/custom-jdk"))];
        let model = cx.update(|cx| dialog_model(PAGE, &stored, cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, _, cx) = open(cx, move |_, cx| SdkFieldView::new(binding, spec(), detector, cx));
        cx.run_until_parked();
        assert_eq!(asked.borrow().as_slice(), ["jdk"]);
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["JRE: /opt/custom-jdk ▾"]);
        click(cx, "settings-sdk-languages.java.maven.runnerJre");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[1..], ["  Use Project JDK", "  Temurin 21 — /jdks/21", "  /opt/custom-jdk — custom"]);
        click(cx, "settings-sdk-languages.java.maven.runnerJre-choice-1");
        assert_eq!(
            staged(&model, cx),
            [("project:languages.java.maven.runnerJre".to_owned(), "\"/jdks/21\"".to_owned())]
        );
        click(cx, "settings-sdk-languages.java.maven.runnerJre-refresh");
        assert_eq!(asked.borrow().len(), 2, "Detect again looks again");
    }

    // Not from a spec: without a detection of its own a test build looks for nothing: the PATH, the
    // system's folders and the version managers are never read, and no tool runs.
    #[gpui_kit::test]
    async fn a_test_build_without_a_detection_of_its_own_finds_nothing(cx: &mut TestAppContext) {
        let found: Rc<RefCell<Option<Vec<SdkChoice>>>> = Rc::default();
        let keep = found.clone();
        cx.update(|cx| {
            assert!(!cx.has_global::<SdkDetection>());
            let detect = detector("w1", cx);
            let then: Then<Vec<SdkChoice>> = Rc::new(move |choices: Vec<SdkChoice>, _: &mut App| {
                *keep.borrow_mut() = Some(choices);
            });
            detect("jdk", then, cx);
        });
        assert_eq!(found.borrow().as_deref(), Some(&[][..]), "answered at once, with nothing");
    }

    // Not from a spec: Browse… asks for a folder and the one chosen is staged.
    #[gpui_kit::test]
    async fn browse_asks_for_a_folder_and_stages_it(cx: &mut TestAppContext) {
        let detector: Detector =
            Rc::new(|_: &'static str, then: Then<Vec<SdkChoice>>, cx: &mut App| then(Vec::new(), cx));
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, seen, cx) = open(cx, move |_, cx| SdkFieldView::new(binding, spec(), detector, cx));
        let asked: Rc<RefCell<Option<Then<PathBuf>>>> = Rc::default();
        let keep = asked.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ControlEvent, _| {
                if let ControlEvent::Browse { directories: true, then } = event {
                    *keep.borrow_mut() = Some(then.clone());
                }
            })
            .detach();
        });
        click(cx, "settings-sdk-languages.java.maven.runnerJre-browse");
        assert_eq!(seen.borrow().as_slice(), ["browse"]);
        let then = asked.borrow_mut().take().expect("a folder is asked for");
        cx.update(|_, cx| then(PathBuf::from("/jdks/17"), cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.shown(cx)), "/jdks/17");
    }
}
