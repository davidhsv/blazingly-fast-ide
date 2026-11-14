use super::*;
use gpui_kit::Modifiers;

fn held(platform: bool, control: bool) -> Modifiers {
    Modifiers { platform, control, ..Modifiers::default() }
}

/// On macOS the default preset is IDEA's macOS one, ⌘ stays ⌘ in shortcuts, bindings and strokes, and ⌘ is the
/// primary modifier.
#[test]
fn macos_keeps_cmd() {
    assert_eq!(default_preset(true), "intellij-macos");
    assert_eq!(format_binding("Meta+Shift+O", true), "⇧⌘O");
    assert_eq!(host_binding("Meta+C", true), "Meta+C");
    assert_eq!(mac_stroke("Ctrl+C", true), "Ctrl+C");
    assert!(has_primary_modifier(&held(true, false), true));
}

/// Off macOS the default preset is IDEA's Windows one, ⌘ is read as Ctrl, shortcuts are spelled out with commas
/// between strokes, and Ctrl, not the Windows key, is the primary modifier.
#[test]
fn windows_uses_ctrl_and_spells_shortcuts_out() {
    assert_eq!(default_preset(false), "intellij-windows");
    assert_eq!(format_binding("Ctrl+Shift+N", false), "Ctrl+Shift+N");
    assert_eq!(format_binding("Ctrl+K Ctrl+I", false), "Ctrl+K, Ctrl+I");
    assert_eq!(format_binding("Shift Shift", false), "Shift, Shift");
    assert_eq!(host_binding("Meta+Shift+C", false), "Ctrl+Shift+C");
    assert_eq!(mac_stroke("Ctrl+Shift+C", false), "Shift+Meta+C");
    assert!(has_primary_modifier(&held(false, true), false));
    assert!(!has_primary_modifier(&held(true, false), false));
}
