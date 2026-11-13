//! What an editor's code intelligence asks of the window: opening a navigation target, showing the
//! targets chooser as the window's popup, running an action.

use super::{Content, Workbench};
use crate::editor::intelligence::{EditorIntelligence, IntelligenceEvent};
use crate::editor::view::Editor;
use crate::editor_lsp::intentions_view::{ListPopup, ListPopupEvent};
use crate::editor_lsp::positions::pos_to_offset;
use crate::editor_lsp::targets_view::{TargetsChooser, TargetsEvent};
use crate::platform::actions::{ActionSource, execute_action_from};
use gpui_kit::{Context, Entity, Focusable, Window};
use ide_core::Location;

impl Workbench {
    /// Gives `editor` its code intelligence, drawn over its text, and listens to it.
    pub(super) fn install_intelligence(
        &mut self,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let intelligence = EditorIntelligence::install(editor, cx);
        let blurred = intelligence.downgrade();
        cx.on_blur(&editor.focus_handle(cx), window, move |_, _, cx| {
            let _ = blurred.update(cx, |intelligence, cx| intelligence.editor_blurred(cx));
        })
        .detach();
        // The subscription ends with the intelligence, which ends with its editor.
        cx.subscribe_in(&intelligence, window, |workbench, intelligence, event, window, cx| {
            workbench.intelligence_event(intelligence, event, window, cx);
        })
        .detach();
    }

    fn intelligence_event(
        &mut self,
        intelligence: &Entity<EditorIntelligence>,
        event: &IntelligenceEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            IntelligenceEvent::Open(location) => self.open_location(location, window, cx),
            IntelligenceEvent::Choose(chooser) => self.show_targets_chooser(intelligence, chooser, window, cx),
            IntelligenceEvent::Intentions(list) => self.show_intentions(intelligence, list, window, cx),
            &IntelligenceEvent::RunAction(id) => {
                window.defer(cx, move |window, cx| {
                    execute_action_from(ActionSource::Other, id, None, window, cx);
                });
            }
        }
    }

    /// Opens a navigation target with the caret at its start: in its tab of the project, or in the
    /// single-file window's editor when it is that file.
    fn open_location(&mut self, location: &Location, window: &mut Window, cx: &mut Context<Self>) {
        let path = location.path.0.clone();
        let start = location.range.start;
        match &self.content {
            Content::Project { .. } => self.open_project_file_at(path, start.line, start.col, window, cx),
            Content::Editor(editor) if editor.read(cx).path() == path.as_path() => {
                let editor = editor.clone();
                editor.update(cx, |editor, cx| {
                    let offset = pos_to_offset(editor.rope(), start);
                    editor.select(offset, offset, cx);
                });
            }
            _ => self.open_file(path, window, cx),
        }
    }

    fn show_targets_chooser(
        &mut self,
        intelligence: &Entity<EditorIntelligence>,
        chooser: &Entity<TargetsChooser>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let intelligence = intelligence.downgrade();
        let events = cx.subscribe_in(chooser, window, move |workbench, _, event: &TargetsEvent, window, cx| {
            match event {
                TargetsEvent::Open(location) => {
                    workbench.close_popup(window, cx);
                    workbench.open_location(location, window, cx);
                }
                TargetsEvent::Pin { .. } | TargetsEvent::Closed => workbench.close_popup(window, cx),
                TargetsEvent::Active(_) => return,
            }
            let _ = intelligence.update(cx, |intelligence, cx| intelligence.chooser_closed(cx));
        });
        self.open_popup(chooser, events, window, cx);
        chooser.update(cx, |chooser, cx| chooser.opened(cx));
    }

    fn show_intentions(
        &mut self,
        intelligence: &Entity<EditorIntelligence>,
        list: &Entity<ListPopup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let intelligence = intelligence.downgrade();
        let events = cx.subscribe_in(list, window, move |workbench, _, event: &ListPopupEvent, window, cx| {
            match event {
                // The intelligence runs the chosen row once the popup is gone.
                ListPopupEvent::Activated(_) => workbench.close_popup(window, cx),
                // A row's → menu item runs its inspection command instead of the row's fix.
                ListPopupEvent::Control { control, .. } => {
                    workbench.close_popup(window, cx);
                    let _ = intelligence.update(cx, |intelligence, _| intelligence.intentions_closed());
                    if let Some(command) = control.run.clone() {
                        workbench.apply_inspection(command, window, cx);
                    }
                }
                ListPopupEvent::Dismissed | ListPopupEvent::Pin => {
                    workbench.close_popup(window, cx);
                    let _ = intelligence.update(cx, |intelligence, _| intelligence.intentions_closed());
                }
                ListPopupEvent::Active(_) | ListPopupEvent::PreviewWanted(_) => {}
            }
        });
        self.open_popup(list, events, window, cx);
        list.update(cx, |list, cx| list.opened(cx));
    }
}
