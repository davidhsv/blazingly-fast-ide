//! How a diff or merge viewer pane draws its caret and text: as the editor does, following
//! Settings › Editor › General › Appearance and Settings › Editor › Font.

use ide_core::settings::EditorSettings;

/// The font the IDE bundles and uses when none is chosen.
pub const BUNDLED_MONO_FAMILY: &str = "JetBrains Mono";

/// How the caret moves between positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretMotion {
    Snappy,
    Gliding,
}

/// The caret of a viewer pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewerCaret {
    pub block: bool,
    /// Fade in and out instead of toggling.
    pub smooth_blink: bool,
    /// Animated movement; `None` jumps.
    pub motion: Option<CaretMotion>,
    /// A whole blink in milliseconds; `None` when the caret does not blink.
    pub blink_ms: Option<u32>,
}

/// The font of a viewer pane; what is `None` keeps the IDE's own code font metrics.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewerFont {
    /// The font families to try in turn, when another than the bundled one is chosen or a fallback
    /// font is set.
    pub families: Option<Vec<String>>,
    pub size: Option<f32>,
    pub line_height: Option<f32>,
    pub ligatures: bool,
}

/// The caret the editor's appearance settings ask for.
pub fn viewer_caret(editor: &EditorSettings) -> ViewerCaret {
    let appearance = &editor.appearance;
    let gliding = appearance.caret_movement_style == "gliding";
    ViewerCaret {
        block: appearance.block_caret,
        smooth_blink: appearance.smooth_caret_blinking,
        motion: appearance.smooth_caret_movement.then_some(if gliding {
            CaretMotion::Gliding
        } else {
            CaretMotion::Snappy
        }),
        blink_ms: appearance.caret_blinking.then(|| appearance.caret_blink_period.clamp(100, 5000) * 2),
    }
}

/// The font the editor's settings ask for; `None` (no settings yet) is the IDE's own.
pub fn viewer_font(editor: Option<&EditorSettings>) -> ViewerFont {
    let Some(editor) = editor else {
        return ViewerFont { families: None, size: None, line_height: None, ligatures: false };
    };
    let family = editor.font_family.trim();
    let fallback = editor.fallback_font.trim();
    let chosen = (!family.is_empty() && family != BUNDLED_MONO_FAMILY) || !fallback.is_empty();
    let families = chosen.then(|| {
        let first = if family.is_empty() { BUNDLED_MONO_FAMILY } else { family };
        [first, fallback].into_iter().filter(|name| !name.is_empty()).map(str::to_owned).collect()
    });
    ViewerFont {
        families,
        size: (editor.font_size > 0.0).then_some(editor.font_size),
        line_height: (editor.line_height > 0.0).then_some(editor.line_height),
        ligatures: editor.ligatures,
    }
}

/// What a viewer pane takes from the editor's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewerLook {
    pub caret: ViewerCaret,
    pub font: ViewerFont,
}

impl ViewerLook {
    pub fn of(editor: &EditorSettings) -> Self {
        Self { caret: viewer_caret(editor), font: viewer_font(Some(editor)) }
    }

    /// The settings changed: the look to switch to, when it is another one.
    pub fn follow(&self, editor: &EditorSettings) -> Option<Self> {
        Some(Self::of(editor)).filter(|next| next != self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // src/features/vcs/__tests__/viewerCaret.test.ts › viewer caret › follows the editor's caret settings, also when they change
    #[test]
    fn follows_the_editors_caret_settings_also_when_they_change() {
        let mut settings = EditorSettings::default();
        settings.appearance.block_caret = true;
        let look = ViewerLook::of(&settings);
        assert!(look.caret.block);

        settings.appearance.block_caret = false;
        settings.appearance.smooth_caret_movement = true;
        settings.appearance.caret_movement_style = "gliding".to_owned();
        let changed = look.follow(&settings).expect("the caret changed");
        assert!(!changed.caret.block);
        assert_eq!(changed.caret.motion, Some(CaretMotion::Gliding));
        assert_eq!(changed.follow(&settings), None);
    }

    // src/features/vcs/__tests__/viewerFont.test.ts › viewer font › follows the editor's font settings, also when they change
    #[test]
    fn follows_the_editors_font_settings_also_when_they_change() {
        let chosen = EditorSettings {
            font_family: "Fira Code".to_owned(),
            font_size: 16.0,
            line_height: 22.0,
            ..EditorSettings::default()
        };
        let look = ViewerLook::of(&chosen);
        assert!(look.font.families.iter().flatten().any(|family| family == "Fira Code"));
        assert_eq!((look.font.size, look.font.line_height, look.font.ligatures), (Some(16.0), Some(22.0), false));

        let bundled = EditorSettings { font_size: 11.0, ligatures: true, ..EditorSettings::default() };
        let changed = look.follow(&bundled).expect("the font changed");
        assert_eq!(changed.font.families, None);
        assert_eq!((changed.font.size, changed.font.ligatures), (Some(11.0), true));
    }
}
