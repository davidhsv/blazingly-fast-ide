//! Help › About OpenIDEA: the product and its version, the build, the platform and runtime, the
//! configuration folder, links to the IDE's own help actions, and the licence lines. Copy Info
//! puts the facts on the clipboard; everything else is reported to the workbench.

use super::{button, footer, frame, t};
use crate::app_models::shell::{BuildInfo, about_rows};
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use gpui_kit::{
    App, ClickEvent, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    Render, SharedString, Window, div, prelude::*, px,
};

const WIDTH: f32 = 520.0;

/// Where Report an Issue goes.
pub const ISSUES_URL: &str = "https://github.com/l0ng-ai/openidea/issues/new/choose";

/// Keymap Reference: shown as a link when the action is registered.
pub const KEYMAP_REFERENCE: &str = "help.keymapReference";
/// Show Log: shown as a link when the action is registered.
pub const SHOW_LOG: &str = "help.showLog";

/// What this build says about itself.
pub fn this_build() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        build: option_env!("OPENIDEA_BUILD").unwrap_or("dev").to_owned(),
        profile: if cfg!(debug_assertions) { "debug" } else { "release" }.to_owned(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        runtime: "GPUI".to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutEvent {
    Closed,
    /// A link to an action was clicked: the dialog is closed first, then the action runs.
    RunAction(&'static str),
    /// Open Configuration Folder: show this folder in the file manager.
    RevealConfig(String),
    /// Report an Issue: open this page in the browser.
    OpenUrl(&'static str),
}

pub struct AboutDialog {
    build: BuildInfo,
    config_dir: Option<String>,
    /// The help actions that are registered, which get a link.
    actions: Vec<&'static str>,
    copied: bool,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<AboutEvent> for AboutDialog {}

impl AboutDialog {
    pub fn new(
        build: BuildInfo,
        config_dir: Option<String>,
        actions: Vec<&'static str>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { build, config_dir, actions, copied: false, focus, key_claim }
    }

    /// The product line: `OpenIDEA <version>`.
    pub fn product(&self) -> String {
        format!("OpenIDEA {}", self.build.version)
    }

    /// The build line: `Build #<build> (<profile>)`, in the UI language.
    pub fn build_line(&self, cx: &mut App) -> String {
        format!("{} #{} ({})", t(cx, "app.about.build"), self.build.build, self.build.profile)
    }

    /// The facts listed under the tagline: every row of About but the version and the build.
    pub fn facts(&self, cx: &mut App) -> Vec<(String, String)> {
        let i18n = &cx.default_global::<Localization>().0;
        let hidden = [i18n.t("app.about.version", &[]), i18n.t("app.about.build", &[])];
        let rows = about_rows(Some(&self.build), &self.build.os, self.config_dir.as_deref(), i18n);
        rows.into_iter().filter(|(label, _)| !hidden.contains(label)).collect()
    }

    /// What Copy Info puts on the clipboard: the title, then `<label>: <value>` per row.
    pub fn info_text(&self, cx: &mut App) -> String {
        let i18n = &cx.default_global::<Localization>().0;
        let rows = about_rows(Some(&self.build), &self.build.os, self.config_dir.as_deref(), i18n);
        std::iter::once(i18n.t("app.about.title", &[]))
            .chain(rows.into_iter().map(|(label, value)| format!("{label}: {value}")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether Copy Info was pressed.
    pub fn copied(&self) -> bool {
        self.copied
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self.info_text(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = true;
        cx.notify();
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() || !matches!(keystroke.key.as_str(), "enter" | "escape") {
            return false;
        }
        cx.emit(AboutEvent::Closed);
        true
    }

    fn link(&self, id: &'static str, label: String, event: AboutEvent, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("about-link-{id}")))
            .debug_selector(move || format!("about-link-{id}"))
            .text_color(theme::color(cx, "link"))
            .cursor_pointer()
            .hover(|el| el.underline())
            .child(SharedString::from(label))
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(event.clone())))
    }
}

impl Focusable for AboutDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AboutDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (strong, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let small = theme::length(cx, "fs-small");
        let facts: Vec<_> = self
            .facts(cx)
            .into_iter()
            .map(|(label, value)| {
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(div().flex_none().text_color(muted).child(SharedString::from(format!("{label}:"))))
                    .child(div().min_w_0().truncate().child(SharedString::from(value)))
            })
            .collect();
        let mut links = Vec::new();
        if self.actions.contains(&KEYMAP_REFERENCE) {
            let label = t(cx, "app.action.keymapReference");
            links.push(self.link("keymap", label, AboutEvent::RunAction(KEYMAP_REFERENCE), cx).into_any_element());
        }
        if self.actions.contains(&SHOW_LOG) {
            let os = [("os", Param::Text(self.build.os.clone()))];
            let label = cx.default_global::<Localization>().0.t("app.action.showLog", &os);
            links.push(self.link("log", label, AboutEvent::RunAction(SHOW_LOG), cx).into_any_element());
        }
        if let Some(dir) = self.config_dir.clone() {
            let label = t(cx, "app.about.showConfig");
            links.push(self.link("config", label, AboutEvent::RevealConfig(dir), cx).into_any_element());
        }
        let label = t(cx, "app.about.reportIssue");
        links.push(self.link("issue", label, AboutEvent::OpenUrl(ISSUES_URL), cx).into_any_element());
        let product = self.product();
        let build_line = self.build_line(cx);
        let [tagline, copyright, license, third_party, disclaimer] = [
            "app.about.tagline",
            "app.about.copyright",
            "app.about.license",
            "app.about.thirdParty",
            "app.about.disclaimer",
        ]
        .map(|key| t(cx, key));
        let copy_title = t(cx, if self.copied { "common.copied" } else { "app.about.copyInfo" });
        let copy = div()
            .id("about-copy")
            .debug_selector(|| "about-copy".to_owned())
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(theme::icon(cx, if self.copied { "check" } else { "copy" }, 16.0, muted))
            .with_tooltip(copy_title)
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.copy(cx)));
        let close_label = t(cx, "common.close");
        let close = button(cx, close_label, true, true)
            .id("about-close")
            .debug_selector(|| "about-close".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(AboutEvent::Closed)));
        frame(cx, WIDTH)
            .track_focus(&self.focus)
            .child(
                div().flex().gap(px(16.0)).px(px(20.0)).pt(px(20.0)).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_color(strong)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(18.0))
                                .child(SharedString::from(product)),
                        )
                        .child(div().child(SharedString::from(build_line)))
                        .child(div().text_color(muted).child(SharedString::from(tagline)))
                        .child(div().flex().flex_col().gap(px(2.0)).children(facts))
                        .child(div().flex().flex_wrap().gap(px(12.0)).children(links))
                        .child(
                            div()
                                .text_color(muted)
                                .text_size(small)
                                .child(SharedString::from(copyright))
                                .child(SharedString::from(license)),
                        )
                        .child(
                            div()
                                .text_color(muted)
                                .text_size(small)
                                .child(SharedString::from(format!("{third_party} {disclaimer}"))),
                        ),
                ),
            )
            .child(footer(Some(copy), vec![close]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<AboutEvent>>>;

    fn open(
        cx: &mut TestAppContext,
        actions: Vec<&'static str>,
    ) -> (Entity<AboutDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let config = Some("/home/u/.config/openidea".to_owned());
        let (dialog, cx) =
            cx.add_window_view(move |window, cx| AboutDialog::new(this_build(), config, actions, window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &AboutEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn names_the_version_of_the_build_and_lists_its_facts(cx: &mut TestAppContext) {
        let (dialog, _events, cx) = open(cx, Vec::new());
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.product()), format!("OpenIDEA {version}"));
        let facts = dialog.update(cx, |dialog, cx| dialog.facts(cx));
        let labels: Vec<&str> = facts.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["Platform", "Runtime", "Configuration"]);
        assert_eq!(facts[2].1, "/home/u/.config/openidea");
        let info = dialog.update(cx, |dialog, cx| dialog.info_text(cx));
        assert!(info.starts_with(&format!("About OpenIDEA\nVersion: {version}\nBuild: ")), "{info}");
    }

    #[gpui_kit::test]
    async fn copy_info_fills_the_clipboard_and_close_and_escape_close(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, Vec::new());
        click(cx, "about-copy");
        assert!(dialog.read_with(cx, |dialog, _| dialog.copied()));
        let copied = cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default();
        assert!(copied.starts_with("About OpenIDEA\n"), "{copied}");
        click(cx, "about-close");
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [AboutEvent::Closed, AboutEvent::Closed]);
    }

    #[gpui_kit::test]
    async fn the_links_report_what_they_open(cx: &mut TestAppContext) {
        let (_dialog, events, cx) = open(cx, vec![KEYMAP_REFERENCE]);
        assert!(cx.debug_bounds("about-link-log").is_none(), "Show Log is not registered");
        click(cx, "about-link-keymap");
        click(cx, "about-link-config");
        click(cx, "about-link-issue");
        assert_eq!(
            *events.borrow(),
            [
                AboutEvent::RunAction(KEYMAP_REFERENCE),
                AboutEvent::RevealConfig("/home/u/.config/openidea".to_owned()),
                AboutEvent::OpenUrl(ISSUES_URL),
            ]
        );
    }
}
