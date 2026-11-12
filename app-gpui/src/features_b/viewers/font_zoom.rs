//! Increase / Decrease / Reset Font Size: the editor font zoom per editor (in px over the
//! configured size, within -6..36), and the other surfaces that zoom their own text (terminals,
//! plugin views). A step goes to the focused surface, else to the active editor, never to the
//! surface under the pointer; Reset Font Size resets every editor and every surface.

use std::collections::{BTreeMap, BTreeSet};

pub const MIN_EDITOR_ZOOM: i32 = -6;
pub const MAX_EDITOR_ZOOM: i32 = 36;

pub const INCREASE_FONT_SIZE: &str = "editor.increaseFontSize";
pub const DECREASE_FONT_SIZE: &str = "editor.decreaseFontSize";
pub const RESET_FONT_SIZE: &str = "editor.resetFontSize";

/// An editor's id or a surface's id, as the caller numbers them.
pub type Id = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomAction {
    Increase,
    Decrease,
    Reset,
}

impl ZoomAction {
    pub fn from_action(id: &str) -> Option<Self> {
        match id {
            INCREASE_FONT_SIZE => Some(ZoomAction::Increase),
            DECREASE_FONT_SIZE => Some(ZoomAction::Decrease),
            RESET_FONT_SIZE => Some(ZoomAction::Reset),
            _ => None,
        }
    }
}

/// What a font size action changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomEffect {
    /// The editor's new font zoom (a Markdown editor publishes it to its preview).
    Editor { editor: Id, zoom: i32 },
    /// The surface steps its own font size by `delta` px; `0` resets it.
    Surface { surface: Id, delta: i32 },
}

#[derive(Debug, Default)]
pub struct FontZooms {
    editors: BTreeMap<Id, i32>,
    surfaces: BTreeSet<Id>,
}

impl FontZooms {
    pub fn zoom(&self, editor: Id) -> i32 {
        self.editors.get(&editor).copied().unwrap_or(0)
    }

    /// Sets an editor's zoom (restored with its session, say), kept within the limits.
    pub fn set(&mut self, editor: Id, zoom: i32) {
        self.editors.insert(editor, zoom.clamp(MIN_EDITOR_ZOOM, MAX_EDITOR_ZOOM));
    }

    pub fn editor_closed(&mut self, editor: Id) {
        self.editors.remove(&editor);
    }

    /// A surface that zooms its own text, until [`FontZooms::release_surface`].
    pub fn register_surface(&mut self, surface: Id) {
        self.surfaces.insert(surface);
    }

    pub fn release_surface(&mut self, surface: Id) {
        self.surfaces.remove(&surface);
    }

    /// Runs `action`: `focused` is the registered surface holding the keyboard focus, if any,
    /// `active` the active editor.
    pub fn run(&mut self, action: ZoomAction, focused: Option<Id>, active: Option<Id>) -> Vec<ZoomEffect> {
        let delta = match action {
            ZoomAction::Increase => 1,
            ZoomAction::Decrease => -1,
            ZoomAction::Reset => {
                let mut effects: Vec<ZoomEffect> = self
                    .editors
                    .iter_mut()
                    .filter(|(_, zoom)| **zoom != 0)
                    .map(|(editor, zoom)| {
                        *zoom = 0;
                        ZoomEffect::Editor { editor: *editor, zoom: 0 }
                    })
                    .collect();
                let surfaces = self.surfaces.iter().map(|surface| ZoomEffect::Surface { surface: *surface, delta: 0 });
                effects.extend(surfaces);
                return effects;
            }
        };
        if let Some(surface) = focused.filter(|surface| self.surfaces.contains(surface)) {
            return vec![ZoomEffect::Surface { surface, delta }];
        }
        let Some(editor) = active else { return Vec::new() };
        let zoom = self.zoom(editor);
        let next = (zoom + delta).clamp(MIN_EDITOR_ZOOM, MAX_EDITOR_ZOOM);
        if next == zoom {
            return Vec::new();
        }
        self.editors.insert(editor, next);
        vec![ZoomEffect::Editor { editor, zoom: next }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::keymap::preset;

    const MD: Id = 1;
    const TS: Id = 2;
    const TERMINAL: Id = 10;
    const FRAME: Id = 11;

    #[test]
    fn zooms_only_the_active_editor_clamped_to_6_36() {
        let mut zooms = FontZooms::default();
        zooms.run(ZoomAction::Increase, None, Some(MD));
        zooms.run(ZoomAction::Increase, None, Some(MD));
        assert_eq!((zooms.zoom(MD), zooms.zoom(TS)), (2, 0));
        assert_eq!(zooms.run(ZoomAction::Decrease, None, Some(TS)), [ZoomEffect::Editor { editor: TS, zoom: -1 }]);
        assert_eq!(zooms.zoom(MD), 2);
        zooms.set(TS, 36);
        assert!(zooms.run(ZoomAction::Increase, None, Some(TS)).is_empty());
        assert_eq!(zooms.zoom(TS), 36);
        zooms.set(TS, -6);
        zooms.run(ZoomAction::Decrease, None, Some(TS));
        assert_eq!(zooms.zoom(TS), -6);
    }

    #[test]
    fn routes_font_size_actions_by_focus_even_when_another_surface_is_hovered() {
        let mut zooms = FontZooms::default();
        zooms.register_surface(TERMINAL);
        zooms.register_surface(FRAME);
        let terminal = |delta| ZoomEffect::Surface { surface: TERMINAL, delta };
        // The hovered plugin frame plays no part: only the focus does.
        assert_eq!(zooms.run(ZoomAction::Increase, Some(TERMINAL), Some(MD)), [terminal(1)]);
        assert_eq!(zooms.run(ZoomAction::Decrease, Some(TERMINAL), Some(MD)), [terminal(-1)]);
        assert_eq!(zooms.zoom(MD), 0);
        let frame = |delta| ZoomEffect::Surface { surface: FRAME, delta };
        assert_eq!(zooms.run(ZoomAction::Increase, Some(FRAME), Some(MD)), [frame(1)]);
        assert_eq!(zooms.run(ZoomAction::Reset, Some(TERMINAL), Some(MD)), [terminal(0), frame(0)]);
        zooms.release_surface(TERMINAL);
        let effects = zooms.run(ZoomAction::Increase, Some(TERMINAL), Some(MD));
        assert_eq!(effects, [ZoomEffect::Editor { editor: MD, zoom: 1 }]);
    }

    #[test]
    fn leaves_meta_equals_and_meta_minus_to_fold_and_unfold() {
        let binding = |keymap: &str, action: &str| preset(keymap).and_then(|map| map.get(action)).cloned();
        for keymap in ["intellij-macos", "intellij-windows"] {
            assert_eq!(binding(keymap, INCREASE_FONT_SIZE), Some(vec!["Alt+Shift+.".to_owned()]));
            assert_eq!(binding(keymap, DECREASE_FONT_SIZE), Some(vec!["Alt+Shift+,".to_owned()]));
        }
        assert_eq!(binding("intellij-macos", "code.fold"), Some(vec!["Meta+-".to_owned()]));
        assert_eq!(binding("intellij-macos", "code.unfold"), Some(vec!["Meta+=".to_owned()]));
        assert_eq!(binding("intellij-windows", "code.fold"), Some(vec!["Ctrl+-".to_owned()]));
        assert_eq!(binding("intellij-windows", "code.unfold"), Some(vec!["Ctrl+=".to_owned()]));
        for keymap in ["intellij-macos", "intellij-windows", "vscode-macos"] {
            let map = preset(keymap).expect("a built-in keymap");
            let keys: Vec<&String> = [INCREASE_FONT_SIZE, DECREASE_FONT_SIZE]
                .iter()
                .flat_map(|action| map.get(*action).into_iter().flatten())
                .collect();
            let fold_keys = [
                "Meta+=",
                "Meta+-",
                "Ctrl+=",
                "Ctrl+-",
                "Meta+Shift+=",
                "Meta+Shift+-",
                "Ctrl+Shift+=",
                "Ctrl+Shift+-",
            ];
            assert!(keys.iter().all(|key| !fold_keys.contains(&key.as_str())), "{keymap}: {keys:?}");
            assert!(map.keys().all(|id| !id.starts_with("markdown.zoom")), "{keymap}");
        }
    }
}
