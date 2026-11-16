//! The default target, and the Target Settings section of the target selected: its name and type,
//! then the SSH fields (configuration, project folder with Browse…, upload before each run) or the
//! container fields (Docker connection, Compose files and service or image, container folder,
//! published ports, run options, the image's user), then the interpreters.

use super::{Choice, RunTargetsView, kind_label};
use crate::settings_view::controls::{button, check_box, search_label, section, tx};
use crate::settings_view::pages::{
    WIDE_FIELD, choice_suffix, drop_down, drop_down_row, frame, hint_line, muted, row, row_label, select_box,
};
use gpui_kit::{AnyElement, App, ClickEvent, Context, IntoElement, MouseButton, SharedString, div, prelude::*, px};
use ide_core::settings::{RunTarget, RunTargetKind};
use serde_json::Value;

const DOCKER_HINT: &str = "Only daemons on this machine can mount the project.";
const PROJECT_ROOT_HINT: &str = "Files are uploaded here and the working directory is mapped into it.";
const COMPOSE_FILES_HINT: &str = "Relative to the project; empty uses the project's default file.";
const COMPOSE_ROOT_HINT: &str = "Empty: where the service mounts the project.";
const DOCKER_ROOT_HINT: &str = "The project is mounted here; paths and the working directory are mapped into it.";
const SYNC_LABEL: &str = "Upload changed project files before each run";
const IMAGE_USER_LABEL: &str = "Run as the image's user, not the project folder's owner (Linux)";

/// The fields drawn for a target of `kind`, in order, with their hints, after Name and Type and
/// before the interpreters; the checkboxes and drop-downs between them are drawn by [`RunTargetsView::form`].
fn container_fields(kind: RunTargetKind) -> Vec<(&'static str, Option<&'static str>)> {
    let compose = kind == RunTargetKind::Compose;
    let mut fields = if compose {
        vec![("composeFiles", Some(COMPOSE_FILES_HINT)), ("service", None)]
    } else {
        vec![("image", None)]
    };
    fields.push(("containerRoot", Some(if compose { COMPOSE_ROOT_HINT } else { DOCKER_ROOT_HINT })));
    fields.push(("ports", None));
    fields.push(("runOptions", None));
    fields
}

const INTERPRETERS: [&str; 3] = ["python", "node", "java"];

/// The label of field `name`.
fn label_of(name: &str) -> &'static str {
    super::FIELDS.iter().find(|(own, _, _)| *own == name).map_or("", |(_, label, _)| *label)
}

/// `drawn` with a help line under it, from where its label starts.
fn with_hint(drawn: impl IntoElement, hint: Option<String>, cx: &App) -> AnyElement {
    match hint {
        Some(hint) => div()
            .flex()
            .flex_col()
            .child(drawn)
            .child(div().pl(px(4.0)).child(hint_line(cx, &hint, 0.0)))
            .into_any_element(),
        None => drawn.into_any_element(),
    }
}

impl RunTargetsView {
    /// The text a field shows.
    fn field_text(&self, name: &str, cx: &App) -> String {
        self.field(name).map(|field| field.read(cx).text().to_owned()).unwrap_or_default()
    }

    /// Target Settings as text: `Label: text`, `Label: choice ▾`, `[x] Label`, `[Browse…]` with a
    /// `(disabled)` flag, and the hints indented below their rows.
    pub(super) fn form_lines(&self, target: &RunTarget, cx: &App) -> Vec<String> {
        let mark = |on: bool| if on { "x" } else { " " };
        let text_line = |name: &str| format!("{}: {}", tx(cx, label_of(name)), self.field_text(name, cx));
        let hint = |text: &str| format!("    {}", tx(cx, text));
        let mut out = vec![format!("— {} —", tx(cx, "Target Settings")), text_line("name")];
        out.push(format!("{}: {} ▾", tx(cx, "Type"), tx(cx, kind_label(target.kind))));
        if target.kind == RunTargetKind::Ssh {
            out.push(format!("{}: {} ▾", tx(cx, "SSH configuration"), self.shown_choice(Choice::Ssh, cx)));
            let flag = if self.can_browse(cx) { "" } else { " (disabled)" };
            out.push(format!("{} [{}]{flag}", text_line("projectRoot"), tx(cx, "Browse…")));
            out.push(hint(PROJECT_ROOT_HINT));
            out.extend(self.folder_lines(cx));
            out.push(format!("[{}] {}", mark(target.sync_before_run), tx(cx, SYNC_LABEL)));
        } else {
            out.push(format!("{}: {} ▾", tx(cx, "Docker connection"), self.shown_choice(Choice::Docker, cx)));
            out.push(hint(DOCKER_HINT));
            for (name, help) in container_fields(target.kind) {
                out.push(text_line(name));
                out.extend(help.map(hint));
            }
            out.push(format!("[{}] {}", mark(target.image_user), tx(cx, IMAGE_USER_LABEL)));
        }
        out.extend(INTERPRETERS.map(text_line));
        out
    }

    /// The section of the default run target.
    pub(super) fn default_section(&self, query: &str, cx: &mut Context<Self>) -> gpui_kit::Div {
        let row = self.select_row(Choice::Default, "Default run target", None, query, cx);
        section(cx, None, query, vec![row])
    }

    /// Target Settings of `target`.
    pub(super) fn form(&self, target: &RunTarget, query: &str, cx: &mut Context<Self>) -> gpui_kit::Div {
        let mut rows = vec![self.text_row("name", None, None, query, cx)];
        rows.push(self.select_row(Choice::Kind, "Type", None, query, cx));
        if target.kind == RunTargetKind::Ssh {
            rows.push(self.select_row(Choice::Ssh, "SSH configuration", None, query, cx));
            let browse = self.browse_button(cx);
            rows.push(self.text_row("projectRoot", Some(PROJECT_ROOT_HINT), Some(browse), query, cx));
            rows.extend(self.folder_panel(cx));
            rows.push(self.toggle_row("syncBeforeRun", SYNC_LABEL, target.sync_before_run, query, cx));
        } else {
            rows.push(self.select_row(Choice::Docker, "Docker connection", Some(DOCKER_HINT), query, cx));
            for (name, help) in container_fields(target.kind) {
                rows.push(self.text_row(name, help, None, query, cx));
            }
            rows.push(self.toggle_row("imageUser", IMAGE_USER_LABEL, target.image_user, query, cx));
        }
        for name in INTERPRETERS {
            rows.push(self.text_row(name, None, None, query, cx));
        }
        let title = tx(cx, "Target Settings");
        section(cx, Some(&title), query, rows)
    }

    /// `Label [field] extra`, the field drawn as `settings-text-target.<name>`, mono as paths and
    /// commands are.
    fn text_row(
        &self,
        name: &'static str,
        hint: Option<&str>,
        extra: Option<AnyElement>,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(field) = self.field(name).cloned() else { return div().into_any_element() };
        let label = label_of(name);
        let text = tx(cx, label);
        let selector = format!("settings-text-target.{name}");
        let drawn = row(cx, self.binding.matches(label, cx))
            .child(row_label(cx, &text, query))
            .child(
                frame(cx, WIDE_FIELD, false)
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .font_family("monospace")
                    .child(field),
            )
            .children(extra);
        with_hint(drawn, hint.map(|hint| tx(cx, hint)), cx)
    }

    /// `[x] Label`, drawn as `settings-toggle-target.<name>`.
    fn toggle_row(&self, name: &'static str, label: &str, on: bool, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let text = tx(cx, label);
        row(cx, self.binding.matches(label, cx))
            .id(SharedString::from(format!("settings-toggle-target.{name}")))
            .debug_selector(move || format!("settings-toggle-target.{name}"))
            .child(check_box(cx, on))
            .child(search_label(cx, &text, query))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(name, cx)))
            .into_any_element()
    }

    /// Browse…, drawn as `settings-targets-browse`: opens the folder browser on the project folder
    /// typed (`~` while it is empty), faint while the target has no SSH configuration or the browser
    /// no backend.
    fn browse_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.can_browse(cx);
        let label = tx(cx, "Browse…");
        div()
            .id("settings-targets-browse")
            .debug_selector(|| "settings-targets-browse".to_owned())
            .child(button(cx, &label, false, enabled))
            .when(enabled, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.open_folder(cx))))
            .into_any_element()
    }

    /// `Label: [choice ▾]` with the list open under it, drawn as `settings-select-<key>` and its
    /// choices as `settings-choice-<key>-<value or place>`.
    fn select_row(
        &self,
        which: Choice,
        label: &str,
        hint: Option<&str>,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = which.key();
        let (picks, current) = self.picks(which, cx);
        let shown = picks.iter().find(|pick| pick.value == current).map(|pick| pick.label.clone()).unwrap_or_default();
        let boxed = select_box(cx, &shown)
            .id(SharedString::from(format!("settings-select-{key}")))
            .debug_selector(move || format!("settings-select-{key}"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.open = if view.open == Some(which) { None } else { Some(which) };
                cx.notify();
            }));
        let list = (self.open == Some(which)).then(|| {
            let rows: Vec<AnyElement> = picks
                .into_iter()
                .enumerate()
                .map(|(index, pick)| {
                    let suffix = choice_suffix(&Value::String(pick.value.clone()), index);
                    let chosen = pick.value == current;
                    let detail = pick.detail.as_ref().map(|detail| muted(cx, detail));
                    let value = pick.value;
                    drop_down_row(cx, format!("settings-choice-{key}-{suffix}"), &pick.label, chosen)
                        .when(!pick.enabled, |el| el.opacity(0.5))
                        .children(detail)
                        .when(pick.enabled, |el| {
                            el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.choose(which, value.clone(), cx);
                            }))
                        })
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let text = tx(cx, label);
        let drawn = row(cx, self.binding.matches(label, cx))
            .child(row_label(cx, &text, query))
            .child(div().relative().child(boxed).children(list));
        with_hint(drawn, hint.map(|hint| tx(cx, hint)), cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a Compose target edits its files and service, a Docker one its image; both
    // then the container folder, the ports and the run options.
    #[test]
    fn container_fields_follow_the_kind() {
        let names = |kind| container_fields(kind).into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        assert_eq!(names(RunTargetKind::Compose), ["composeFiles", "service", "containerRoot", "ports", "runOptions"]);
        assert_eq!(names(RunTargetKind::Docker), ["image", "containerRoot", "ports", "runOptions"]);
        assert_eq!(label_of("python"), "Python interpreter");
    }
}
