use super::*;

/// A cell shows an empty value, a short one and one of exactly the preview's length as they are; a longer one is
/// cut at the preview's length with `…`, never in the middle of a surrogate pair.
#[gpui_kit::test]
async fn preserves_short_values_and_truncates_only_beyond_the_preview_limit(cx: &mut TestAppContext) {
    let full = full();
    let at_limit = "\u{4e2d}".repeat(CELL_PREVIEW_LIMIT);
    let before_emoji = format!("{}\u{1f600}tail", "a".repeat(CELL_PREVIEW_LIMIT - 1));
    let values = [String::new(), "hello".to_owned(), at_limit.clone(), full.clone(), before_emoji];
    let data =
        GridData::new(vec![column("payload", "jsonb")], values.iter().map(|value| vec![Some(value.clone())]).collect());
    let (grid, _, cx) = open(cx, data, |_, _| {});
    let shown: Vec<String> =
        grid.read_with(cx, |grid, _| (0..values.len()).map(|row| grid.cell_text(row, 0)).collect());
    assert_eq!(shown[0], "");
    assert_eq!(shown[1], "hello");
    assert_eq!(shown[2], at_limit);
    assert_eq!(shown[3], format!("{}…", prefix16(&full, CELL_PREVIEW_LIMIT)));
    assert_eq!(shown[4], format!("{}…", "a".repeat(CELL_PREVIEW_LIMIT - 1)));
    cx.run_until_parked();
    for selector in ["db-grid-cell-0-0", "db-grid-cell-1-0", "db-grid-cell-2-0", "db-grid-cell-3-0"] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector} is drawn");
    }
}
