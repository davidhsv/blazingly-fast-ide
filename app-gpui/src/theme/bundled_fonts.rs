//! Inter and JetBrains Mono, the faces the interface and code are designed with, as static faces
//! (SIL Open Font License, beside them in `assets/fonts`). They are registered with the text system
//! at start, before the first window, so the theme's "Inter" and "JetBrains Mono" draw with them
//! whatever the system has installed.
//!
//! The text system draws a variable face at its default weight only, so each weight is a face of
//! its own: Inter in regular and semibold (with their italics), JetBrains Mono in every upright
//! weight the editor's main and bold font weight settings can pick, with italic and bold italic.

use std::borrow::Cow;

use gpui_kit::App;

macro_rules! face {
    ($family:literal, $file:literal) => {
        ($file, include_bytes!(concat!("../../assets/fonts/", $family, "/", $file)) as &[u8])
    };
}

/// The bundled faces, by file name.
pub const FACES: &[(&str, &[u8])] = &[
    face!("inter", "Inter-Regular.otf"),
    face!("inter", "Inter-Italic.otf"),
    face!("inter", "Inter-SemiBold.otf"),
    face!("inter", "Inter-SemiBoldItalic.otf"),
    face!("jetbrains-mono", "JetBrainsMono-Thin.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-ExtraLight.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-Light.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-Regular.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-Italic.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-Medium.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-SemiBold.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-Bold.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-BoldItalic.ttf"),
    face!("jetbrains-mono", "JetBrainsMono-ExtraBold.ttf"),
];

/// Registers every bundled face with the text system.
pub fn register(cx: &App) -> anyhow::Result<()> {
    cx.text_system().add_fonts(FACES.iter().map(|(_, bytes)| Cow::Borrowed(*bytes)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    // Every face is a font file (an OpenType or TrueType header), none is empty, and the text
    // system takes them all.
    #[gpui_kit::test]
    fn the_bundled_faces_register_with_the_text_system(cx: &mut TestAppContext) {
        assert_eq!(FACES.len(), 14);
        for (name, bytes) in FACES {
            let tag = bytes.get(..4).unwrap_or_default();
            let expected: &[u8] = if name.ends_with(".otf") { b"OTTO" } else { &[0, 1, 0, 0] };
            assert_eq!(tag, expected, "{name} is not a font file");
        }
        cx.update(|cx| register(cx).expect("the bundled faces register"));
    }
}
