//! Not declared: the two chooser cases of the template views' spec, written against
//! `templates::chooser_view::TemplateChooser`, which is not compiled yet. Declare this module in
//! `templates/mod.rs` with `#[cfg(test)] mod proofs34_chooser;` (renamed without `_pending`) once
//! `pub mod chooser_view;` is declared there and builds.

use super::chooser_view::{ChooserEvent, TemplateChooser};
use super::popup_store::{ChooserItem, ChooserRequest};
use crate::platform::input;
use crate::theme;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext};
use std::cell::RefCell;
use std::rc::Rc;

type Events = Rc<RefCell<Vec<ChooserEvent>>>;

fn open(cx: &mut TestAppContext, request: ChooserRequest) -> (Entity<TemplateChooser>, Events, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        if !cx.has_global::<input::Keymap>() {
            input::init(cx);
        }
    });
    let (chooser, cx) = cx.add_window_view(move |_, cx| TemplateChooser::new(request, cx));
    let events: Events = Rc::default();
    let seen = Rc::clone(&events);
    cx.update(|window, cx| {
        cx.subscribe(&chooser, move |_, event: &ChooserEvent, _| seen.borrow_mut().push(event.clone())).detach();
        window.activate_window();
        let focus = chooser.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (chooser, events, cx)
}

fn picked_key(events: &Events) -> Option<String> {
    events.borrow().iter().rev().find_map(|event| match event {
        ChooserEvent::Picked { key, .. } => Some(key.clone()),
        ChooserEvent::Cancelled => None,
    })
}

fn mnemonic(key: &str, label: &str, mnemonic: &str, index: Option<u32>) -> ChooserItem {
    ChooserItem { mnemonic: Some(mnemonic.to_owned()), mnemonic_index: index, ..ChooserItem::new(key, label) }
}

// src/features/templates/__tests__/views.test.tsx › dialogs › chooser filters by typing and picks with Enter
#[gpui_kit::test]
async fn chooser_filters_by_typing_and_picks_with_enter(cx: &mut TestAppContext) {
    let request = ChooserRequest {
        title: "Test".to_owned(),
        filterable: true,
        items: vec![ChooserItem::new("a", "fori"), ChooserItem::new("b", "iter")],
        ..ChooserRequest::default()
    };
    let (_, events, cx) = open(cx, request);
    cx.simulate_keystrokes("i t enter");
    assert_eq!(picked_key(&events).as_deref(), Some("b"));
}

// src/features/templates/__tests__/views.test.tsx › dialogs › chooser picks by mnemonic and skips section rows
#[gpui_kit::test]
async fn chooser_picks_by_mnemonic_and_skips_section_rows(cx: &mut TestAppContext) {
    let first = ChooserRequest {
        title: "Surround With".to_owned(),
        mnemonics: true,
        items: vec![
            mnemonic("if", "if", "1", None),
            ChooserItem::section("s", "Live templates"),
            ChooserItem { detail: Some("tag".to_owned()), ..mnemonic("t", "T", "T", Some(0)) },
        ],
        ..ChooserRequest::default()
    };
    {
        let (_, events, cx) = open(cx, first);
        cx.simulate_keystrokes("down enter");
        assert_eq!(picked_key(&events).as_deref(), Some("t"));
    }
    let second = ChooserRequest {
        title: "Surround With".to_owned(),
        mnemonics: true,
        items: vec![mnemonic("if", "if", "1", None), mnemonic("t", "T", "T", Some(0))],
        ..ChooserRequest::default()
    };
    let (_, events, cx) = open(cx, second);
    cx.simulate_keystrokes("t");
    assert_eq!(picked_key(&events).as_deref(), Some("t"));
}
