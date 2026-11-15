use super::*;
use crate::platform::settings::{SettingsWrite, set_global};
use gpui_kit::Pixels;

/// The x of the column `col` of the first line, a line long enough to hold it.
fn column_x(f: &Fixture, cx: &mut VisualTestContext, col: u32) -> Pixels {
    f.editor.read_with(cx, |e, _| e.position_of(col).expect("the first line is painted").x)
}

fn guides(f: &Fixture, cx: &mut VisualTestContext) -> Vec<Pixels> {
    cx.run_until_parked();
    f.editor.read_with(cx, |e, _| e.painted_right_margins())
}

fn write(cx: &mut VisualTestContext, key: &'static str, value: serde_json::Value) {
    let write = [SettingsWrite { key, value }];
    cx.update(|_, cx| set_global("w1", &write, cx)).expect("the setting is stored");
}

/// Close enough to be the same column: a guide is drawn at the left edge of its column.
fn at_column(guide: Pixels, column: Pixels) -> bool {
    (guide - column).abs() < px(0.5)
}

/// The right margin is drawn at the hard wrap column (120 by default), the language's own column and the
/// visual guides add theirs, and the guides go with Show hard wrap and visual guides turned off.
#[gpui_kit::test]
async fn shows_line_numbers_and_the_right_margin_normally(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "main.rs", &"x".repeat(200)).await;
    let drawn = guides(&f, cx);
    assert_eq!(drawn.len(), 1, "{drawn:?}");
    assert!(at_column(drawn[0], column_x(&f, cx, 120)), "{drawn:?}");

    write(cx, "editor.codeStyle.visualGuides", serde_json::json!([40]));
    settle(cx, |cx| f.editor.read_with(cx, |e, _| e.facts().right_margins == [40, 120]));
    let drawn = guides(&f, cx);
    assert_eq!(drawn.len(), 2, "{drawn:?}");
    assert!(at_column(drawn[0], column_x(&f, cx, 40)) && at_column(drawn[1], column_x(&f, cx, 120)), "{drawn:?}");

    write(cx, "editor.codeStyle.languages", serde_json::json!({ "rust": { "hardWrapAt": 100 } }));
    settle(cx, |cx| f.editor.read_with(cx, |e, _| e.facts().right_margins == [40, 100]));
    let drawn = guides(&f, cx);
    assert!(at_column(drawn[1], column_x(&f, cx, 100)), "{drawn:?}");

    write(cx, "editor.appearance.hardWrapGuides", serde_json::json!(false));
    settle(cx, |cx| f.editor.read_with(cx, |e, _| e.facts().right_margins.is_empty()));
    assert_eq!(guides(&f, cx), []);
    close(cx);
}

/// Distraction Free Mode hides the margin guides; leaving it brings them back.
#[gpui_kit::test]
async fn hides_line_numbers_gutter_margin_guides_and_sticky_lines_and_centres_at_the_configured_width(
    cx: &mut TestAppContext,
) {
    let (f, cx) = open(cx, "main.rs", &"x".repeat(200)).await;
    assert_eq!(guides(&f, cx).len(), 1);
    run(cx, "editor.toggleZen");
    assert_eq!(guides(&f, cx), []);
    run(cx, "editor.toggleZen");
    let drawn = guides(&f, cx);
    assert_eq!(drawn.len(), 1, "{drawn:?}");
    assert!(at_column(drawn[0], column_x(&f, cx, 120)), "{drawn:?}");
    close(cx);
}
