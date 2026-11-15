use super::*;
use crate::platform::settings::{SettingsWrite, set_global};
use gpui_kit::{FontStyle, FontWeight, Hsla, Rgba};

/// Applies the appearance settings and the colour scheme as the application does at start.
fn apply_scheme(cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        theme::appearance::init(cx);
        theme::scheme_runtime::init(cx);
    });
    cx.run_until_parked();
}

fn write(cx: &mut VisualTestContext, key: &'static str, value: serde_json::Value) {
    let write = [SettingsWrite { key, value }];
    cx.update(|_, cx| set_global("w1", &write, cx)).expect("the setting is stored");
    cx.run_until_parked();
}

/// The root value of the theme token `name` (without `--`).
fn root(cx: &mut VisualTestContext, name: &str) -> Option<String> {
    cx.update(|_, cx| cx.global::<theme::ActiveTheme>().theme.raw(name).map(str::to_owned))
}

/// Where `needle` starts, as the line and column the painted accessors take.
fn line_col_of(f: &Fixture, cx: &mut VisualTestContext, needle: &str) -> (u32, u32) {
    f.editor.read_with(cx, |editor, _| {
        let at = editor.text().find(needle).expect("the text is in the document");
        crate::editor::buffer::line_col(editor.rope(), utf16(&editor.text()[..at]))
    })
}

fn colour_of(f: &Fixture, cx: &mut VisualTestContext, needle: &str) -> Option<Hsla> {
    cx.run_until_parked();
    let (line, col) = line_col_of(f, cx, needle);
    f.editor.read_with(cx, |editor, _| editor.painted_color_at(line, col))
}

fn font_of(f: &Fixture, cx: &mut VisualTestContext, needle: &str) -> Option<(FontWeight, FontStyle)> {
    cx.run_until_parked();
    let (line, col) = line_col_of(f, cx, needle);
    f.editor.read_with(cx, |editor, _| editor.painted_font_at(line, col))
}

fn hex(r: u8, g: u8, b: u8) -> Hsla {
    Rgba { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a: 1.0 }.into()
}

/// Typography Settings' weights are on the theme's root, follow a change, and go back to 400 and 700.
#[gpui_kit::test]
async fn sets_the_configured_weights_on_the_root_and_follows_settings_changes(cx: &mut TestAppContext) {
    let (_f, cx) = open(cx, "main.rs", "fn main() {}\n").await;
    apply_scheme(cx);
    assert_eq!((root(cx, "fw-code"), root(cx, "fw-code-bold")), (Some("400".into()), Some("700".into())));
    write(cx, "editor.fontWeight", serde_json::json!(300));
    write(cx, "editor.boldFontWeight", serde_json::json!(800));
    assert_eq!((root(cx, "fw-code"), root(cx, "fw-code-bold")), (Some("300".into()), Some("800".into())));
    write(cx, "editor.fontWeight", serde_json::json!(400));
    write(cx, "editor.boldFontWeight", serde_json::json!(700));
    assert_eq!((root(cx, "fw-code"), root(cx, "fw-code-bold")), (Some("400".into()), Some("700".into())));
    close(cx);
}

/// An edit of the scheme's Keyword attribute recolours the keywords of every editor and, made bold,
/// draws them in Typography Settings' bold weight; comments are slanted as the light scheme says.
#[gpui_kit::test]
async fn the_editor_draws_tokens_with_the_schemes_edits_and_the_typography_weights(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "main.rs", "fn main() {} // note\n").await;
    f.highlighted(cx);
    apply_scheme(cx);
    let keyword = serde_json::json!({ "light": { "keyword": { "foreground": "#ff0000", "bold": true } } });
    write(cx, "editor.colorOverrides", keyword);
    write(cx, "editor.boldFontWeight", serde_json::json!(800));
    settle(cx, |cx| colour_of(&f, cx, "fn") == Some(hex(255, 0, 0)));
    assert_eq!(font_of(&f, cx, "fn").map(|(weight, _)| weight), Some(FontWeight(800.0)));
    assert_eq!(font_of(&f, cx, "main").map(|(weight, _)| weight), Some(FontWeight(400.0)));
    assert_eq!(font_of(&f, cx, "// note").map(|(_, style)| style), Some(FontStyle::Italic));
    write(cx, "editor.colorOverrides", serde_json::json!({}));
    settle(cx, |cx| colour_of(&f, cx, "fn") != Some(hex(255, 0, 0)));
    assert_eq!(colour_of(&f, cx, "fn"), Some(cx.update(|_, cx| theme::color(cx, "syn-keyword"))));
    close(cx);
}

/// An edit of a language's own attribute (Java › Keyword) recolours the keywords of that language's
/// editors only.
#[gpui_kit::test]
async fn offers_a_language_scheme_page_and_scopes_its_overrides_to_that_languages_editors(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "A.java", "class A {}\n").await;
    f.highlighted(cx);
    apply_scheme(cx);
    let java = serde_json::json!({ "light": { "java.keyword": { "foreground": "#00ff00" } } });
    write(cx, "editor.colorOverrides", java);
    settle(cx, |cx| colour_of(&f, cx, "class") == Some(hex(0, 255, 0)));
    // The root keeps the theme's keyword colour: other languages are not touched.
    assert_ne!(cx.update(|_, cx| theme::color(cx, "syn-keyword")), hex(0, 255, 0));
    close(cx);
}

/// Choosing the Dark scheme under the light theme draws the editor in the dark scheme's colours.
#[gpui_kit::test]
async fn applies_an_explicit_scheme_to_the_editor(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "main.rs", "fn main() {}\n").await;
    f.highlighted(cx);
    apply_scheme(cx);
    write(cx, "editor.colorScheme", serde_json::json!("dark"));
    settle(cx, |cx| colour_of(&f, cx, "fn") == Some(hex(0xcf, 0x8e, 0x6d)));
    close(cx);
}
