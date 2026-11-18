//! Editor › Font › Preview: an editable sample drawn in the font the page shows, before Apply: the
//! family with the fallback font, the size, the line height, ligatures and the main weight. What is
//! typed into it is the preview's own and is never stored.

use super::{Binding, ControlEvent};
use crate::editor::view::code_font_of;
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::text_area::{AreaFont, TextArea, TextAreaEvent};
use gpui_kit::{App, Context, Entity, EventEmitter, IntoElement, Render, Subscription, Window, div, prelude::*, px};

/// The sample the preview starts with: prose, the alphabets, digits, the operator sequences
/// ligatures join, and CJK text the fallback font draws.
pub const SAMPLE: &str = "OpenIDEA is a full-featured IDE
with a high level of usability and outstanding
advanced code editing and refactoring support.

abcdefghijklmnopqrstuvwxyz 0123456789 (){}[]
ABCDEFGHIJKLMNOPQRSTUVWXYZ +-*/= .,;:!? #&$%@|^

<!-- -- != := === >= >- >=> |-> -> <$> </> #[ |||> |= ~@

中文字体回退示例 · 日本語 · 한국어
";

/// The defaults of the page's settings, used while a value is missing or not a number.
const DEFAULT_SIZE: f64 = 13.0;
const DEFAULT_LINE_HEIGHT: f64 = 22.0;
const DEFAULT_WEIGHT: u16 = 400;
const DEFAULT_BOLD_WEIGHT: u16 = 700;

/// What the preview is drawn with, as the page shows the settings now.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewStyle {
    pub family: String,
    pub fallback: String,
    pub size: f32,
    pub line_height: f32,
    pub ligatures: bool,
    pub weight: u16,
    /// The weight of bold text; the sample has none, so it changes nothing drawn.
    pub bold_weight: u16,
}

impl PreviewStyle {
    /// The page's values through `binding`: the staged edit, else the scope's value, else the
    /// default.
    pub fn of(binding: &Binding, cx: &App) -> Self {
        let weight = |key: &str, default: u16| {
            let raw = binding.value(key, cx).and_then(|value| value.as_f64());
            super::editor_font::font_weight_of(raw, default)
        };
        Self {
            family: binding.text("editor.fontFamily", cx).trim().to_owned(),
            fallback: binding.text("editor.fallbackFont", cx).trim().to_owned(),
            size: binding.number("editor.fontSize", DEFAULT_SIZE, cx) as f32,
            line_height: binding.number("editor.lineHeight", DEFAULT_LINE_HEIGHT, cx) as f32,
            ligatures: binding.bool("editor.ligatures", cx),
            weight: weight("editor.fontWeight", DEFAULT_WEIGHT),
            bold_weight: weight("editor.boldFontWeight", DEFAULT_BOLD_WEIGHT),
        }
    }

    /// The font, size and row height the area draws with: resolved as the editor resolves its own
    /// (the family, the fallback, the theme's monospaced families), sizes zoomed with the interface,
    /// a size or line height of zero or less the theme's code metrics.
    pub fn area_font(&self, cx: &App) -> AreaFont {
        let families = [self.family.as_str(), self.fallback.as_str()];
        let font = code_font_of(&families, self.ligatures, self.weight, cx);
        let size = if self.size > 0.0 { theme::zoomed(cx, self.size) } else { theme::length(cx, "fs-code") };
        let line_height =
            if self.line_height > 0.0 { theme::zoomed(cx, self.line_height) } else { theme::length(cx, "lh-code") };
        AreaFont { font, size, line_height }
    }
}

/// The preview under the Font page's controls.
pub struct FontPreviewView {
    binding: Binding,
    area: Entity<TextArea>,
    style: PreviewStyle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for FontPreviewView {}

impl FontPreviewView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let area = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_text(SAMPLE, cx);
            area.set_rows(10, 16, cx);
            area
        });
        // Escape in the sample is the dialog's Cancel, as in every field of the page; Enter is a
        // line break of the sample, not OK.
        let events = cx.subscribe(&area, |_, _, event: &TextAreaEvent, cx| {
            if *event == TextAreaEvent::Cancelled {
                cx.emit(ControlEvent::Cancel);
            }
        });
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| view.restyle(cx));
        let style = PreviewStyle::of(&binding, cx);
        let mut view = Self { binding, area, style, _subscriptions: vec![events, observe] };
        view.apply_style(cx);
        view
    }

    /// The settings changed (an edit staged, Reset, another scope): the preview follows them.
    fn restyle(&mut self, cx: &mut Context<Self>) {
        let style = PreviewStyle::of(&self.binding, cx);
        if style != self.style {
            self.style = style;
            self.apply_style(cx);
        }
        cx.notify();
    }

    fn apply_style(&mut self, cx: &mut Context<Self>) {
        let font = self.style.area_font(cx);
        self.area.update(cx, |area, cx| area.set_font(Some(font), cx));
    }

    /// What the preview is drawn with now.
    pub fn style(&self) -> &PreviewStyle {
        &self.style
    }

    /// The font the sample's area draws with.
    pub fn font(&self, cx: &App) -> Option<AreaFont> {
        self.area.read(cx).font().cloned()
    }

    pub fn area(&self) -> &Entity<TextArea> {
        &self.area
    }

    /// The sample's text as the user left it.
    pub fn text(&self, cx: &App) -> String {
        self.area.read(cx).text().to_owned()
    }
}

impl Render for FontPreviewView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "Preview");
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .mt(px(8.0))
            .child(div().px(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(title))
            .child(
                div()
                    .id("settings-font-preview")
                    .debug_selector(|| "settings-font-preview".to_owned())
                    .ml(px(4.0))
                    .max_w(px(640.0))
                    .p(px(8.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .bg(theme::color(cx, "bg-input"))
                    .child(self.area.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use crate::theme::installed_fonts::InstalledFontNames;
    use gpui_kit::{Focusable, TestAppContext, VisualTestContext};
    use serde_json::json;

    const PAGE: &str = "editor.font";

    fn preview(
        cx: &mut TestAppContext,
    ) -> (Entity<FontPreviewView>, Seen, Entity<crate::settings_model::staging::SettingsDialog>, &mut VisualTestContext)
    {
        cx.update(|cx| cx.set_global(InstalledFontNames(vec!["Test Mono".to_owned(), "Test Fallback".to_owned()])));
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, seen, cx) = open(cx, move |_, cx| FontPreviewView::new(binding, cx));
        (view, seen, model, cx)
    }

    // Font › the preview shows the sample in the editor's default font
    #[gpui_kit::test]
    async fn the_preview_shows_the_sample_in_the_default_font(cx: &mut TestAppContext) {
        let (view, _, _, cx) = preview(cx);
        assert_eq!(view.read_with(cx, |view, cx| view.text(cx)), SAMPLE);
        let style = view.read_with(cx, |view, _| view.style().clone());
        assert_eq!((style.size, style.line_height, style.ligatures, style.weight), (13.0, 22.0, false, 400));
        let font = view.read_with(cx, |view, cx| view.font(cx)).expect("the area has the page's font");
        assert_eq!((f32::from(font.size), f32::from(font.line_height)), (13.0, 22.0));
        assert_eq!(font.font.features.is_calt_enabled(), Some(false), "no ligatures by default");
        assert!(cx.debug_bounds("settings-font-preview").is_some());
    }

    // Font › the preview follows the family, size, line height, ligatures and weights being edited,
    // before Apply
    #[gpui_kit::test]
    async fn the_preview_follows_the_edits_before_apply(cx: &mut TestAppContext) {
        let (view, _, model, cx) = preview(cx);
        model.update(cx, |dialog, cx| {
            for (key, value) in [
                ("editor.fontFamily", json!("Test Mono")),
                ("editor.fontSize", json!(15)),
                ("editor.lineHeight", json!(24)),
                ("editor.ligatures", json!(true)),
                ("editor.fontWeight", json!(500)),
                ("editor.boldFontWeight", json!(800)),
            ] {
                dialog.stage(PAGE, Scope::Global, key, value);
            }
            cx.notify();
        });
        cx.run_until_parked();
        let style = view.read_with(cx, |view, _| view.style().clone());
        assert_eq!(
            style,
            PreviewStyle {
                family: "Test Mono".to_owned(),
                fallback: String::new(),
                size: 15.0,
                line_height: 24.0,
                ligatures: true,
                weight: 500,
                bold_weight: 800,
            }
        );
        let font = view.read_with(cx, |view, cx| view.font(cx)).expect("the area has the page's font");
        assert_eq!(font.font.family.as_ref(), "Test Mono");
        assert_eq!((f32::from(font.size), f32::from(font.line_height)), (15.0, 24.0));
        assert_ne!(font.font.features.is_calt_enabled(), Some(false), "ligatures are on");
        assert_eq!(font.font.weight, gpui_kit::FontWeight(500.0));

        // A family that is not installed gives way to the fallback font.
        model.update(cx, |dialog, cx| {
            dialog.stage(PAGE, Scope::Global, "editor.fontFamily", json!("No Such Family Anywhere"));
            dialog.stage(PAGE, Scope::Global, "editor.fallbackFont", json!("Test Fallback"));
            cx.notify();
        });
        cx.run_until_parked();
        let font = view.read_with(cx, |view, cx| view.font(cx)).expect("the area has the page's font");
        assert_eq!(font.font.family.as_ref(), "Test Fallback");

        // Reset: the preview is drawn with the stored values again.
        model.update(cx, |dialog, cx| {
            dialog.reset_page(PAGE);
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.style().size), 13.0);
    }

    // Font › the preview's text can be typed into and is not stored
    #[gpui_kit::test]
    async fn the_previews_text_can_be_typed_into_and_is_not_stored(cx: &mut TestAppContext) {
        let (view, seen, model, cx) = preview(cx);
        let area = view.read_with(cx, |view, _| view.area().clone());
        click(cx, "settings-font-preview");
        assert!(cx.update(|window, cx| area.focus_handle(cx).is_focused(window)), "the sample has the keyboard");
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("fi => ligature");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("second line");
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.text(cx)), "fi => ligature\nsecond line");
        assert!(staged(&model, cx).is_empty(), "the sample is not a setting");
        assert!(seen.borrow().is_empty(), "Enter is a line break, not OK");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(*seen.borrow(), ["cancel"]);
    }
}
