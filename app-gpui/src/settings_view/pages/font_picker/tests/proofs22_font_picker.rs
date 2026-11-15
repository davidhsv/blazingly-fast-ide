//! The font picker while the installed fonts are loading, could not be read, came back empty or
//! cannot be asked for, and while they are read again: what its status line says and that the
//! name saved or typed stays.

use super::*;

/// The picker of the editor's font, over a saved name, in a window of its own.
fn editor_picker<'a>(
    cx: &'a mut TestAppContext,
    saved: &str,
) -> (Entity<FontPicker>, Entity<crate::settings_model::staging::SettingsDialog>, Seen, &'a mut VisualTestContext) {
    let model = cx.update(|cx| dialog_model(PAGE, &[("editor.fontFamily", json!(saved))], cx));
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
    let (view, seen, cx) = open(cx, move |_, cx| {
        let spec = PickerSpec {
            key: "editor.fontFamily",
            label: "Font",
            name: "editor",
            allow_empty: true,
            mono_only: None,
            gate: Gate::Always,
            hint: None,
        };
        FontPicker::new(binding, spec, cx)
    });
    (view, model, seen, cx)
}

/// The installed fonts as the system answered: `families`, how the reading went and from where.
fn fonts(families: Vec<FontFamily>, status: FontStatus, source: FontSource) -> FontCatalog {
    let mut fonts = SystemFonts::default();
    (fonts.fonts, fonts.status, fonts.source) = (families, status, source);
    FontCatalog(fonts)
}

fn shown_name(view: &Entity<FontPicker>, cx: &mut VisualTestContext) -> String {
    let field = view.read_with(cx, |view, _| view.field().clone());
    field.read_with(cx, |field, _| field.text().to_owned())
}

// FontPicker › explains %s without declaring saved fonts missing
#[gpui_kit::test]
async fn explains_each_font_list_state_without_declaring_saved_fonts_missing(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(catalog(FontStatus::Ready)));
    let (view, _model, _seen, cx) = editor_picker(cx, "Saved Missing Font");
    let cases = [
        (FontStatus::Loading, "Loading system fonts"),
        (FontStatus::Error, "Type a family name"),
        (FontStatus::Empty, "No system fonts"),
        (FontStatus::Browser, "Type a family name"),
    ];
    for (status, message) in cases {
        cx.update(|_, cx| cx.set_global(catalog(status)));
        cx.run_until_parked();
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "Font: Saved Missing Font", "{status:?}");
        assert!(lines[1].trim_start().starts_with(message), "{status:?}: {lines:?}");
        assert!(!lines[1].contains("not found"), "{status:?}: {lines:?}");
        assert_eq!(shown_name(&view, cx), "Saved Missing Font", "{status:?}");
    }
}

// FontPicker › quietly keeps selections and manual input through fallback and native retry
#[gpui_kit::test]
async fn quietly_keeps_selections_and_manual_input_through_fallback_and_native_retry(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(fonts(bundled_fonts().to_vec(), FontStatus::Error, FontSource::Limited)));
    let (view, model, seen, cx) = editor_picker(cx, "Saved Missing Font");
    assert_eq!(shown_name(&view, cx), "Saved Missing Font");
    assert!(view.read_with(cx, |view, cx| view.status(cx)).starts_with("Type a family name"));
    assert!(seen.borrow().is_empty(), "a list that could not be read is not read again unasked");
    click(cx, "settings-font-editor-refresh");
    assert_eq!(seen.borrow().as_slice(), ["refresh-fonts"], "Refresh asks for the installed fonts again");
    let field = view.read_with(cx, |view, _| view.field().clone());
    type_over(&field, "Manual CJK Family", cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(shown_name(&view, cx), "Manual CJK Family");
    let loading = fonts(bundled_fonts().to_vec(), FontStatus::Loading, FontSource::Limited);
    cx.update(|_, cx| cx.set_global(loading));
    cx.run_until_parked();
    assert_eq!(shown_name(&view, cx), "Manual CJK Family", "kept while the fonts are read again");
    let mut families = bundled_fonts().to_vec();
    families.push(FontFamily { name: "New Native Family".to_owned(), monospaced: false });
    cx.update(|_, cx| cx.set_global(fonts(families, FontStatus::Ready, FontSource::Native)));
    cx.run_until_parked();
    assert_eq!(shown_name(&view, cx), "Manual CJK Family", "kept once the installed fonts came");
    click(cx, "settings-font-editor");
    let names: Vec<String> = view.read_with(cx, |view, cx| view.suggestions(cx).into_iter().map(|f| f.name).collect());
    assert!(names.contains(&"New Native Family".to_owned()), "{names:?}");
    assert_eq!(staged(&model, cx), [("global:editor.fontFamily".to_owned(), "\"Manual CJK Family\"".to_owned())]);
}
