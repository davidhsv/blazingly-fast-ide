//! Inter and JetBrains Mono, variable in weight, as the latin `.woff2` subsets of
//! `@fontsource-variable/inter` and `@fontsource-variable/jetbrains-mono` (SIL Open Font License,
//! beside them in `assets/fonts`): the faces the interface and code are designed with.
//!
//! The text system takes the files and resolves them as "Inter" and "JetBrains Mono", but it draws a
//! variable face at its default weight only: semibold and bold come out as regular. So the faces are
//! not registered at start; `openidea-gpui --self-check=fonts` measures whether they draw every
//! weight, and fails naming each family whose semibold is drawn with its regular face.

use std::borrow::Cow;

use gpui_kit::App;

/// The bundled faces. The text system loads no face without an "m", which Inter's `latin-ext`
/// subset lacks, so the `latin` subsets are the ones here.
pub const FACES: &[(&str, &[u8])] = &[
    ("inter-latin-wght-normal", include_bytes!("../../assets/fonts/inter/inter-latin-wght-normal.woff2")),
    (
        "jetbrains-mono-latin-wght-normal",
        include_bytes!("../../assets/fonts/jetbrains-mono/jetbrains-mono-latin-wght-normal.woff2"),
    ),
];

/// Registers every bundled face with the text system.
pub fn register(cx: &App) -> anyhow::Result<()> {
    cx.text_system().add_fonts(FACES.iter().map(|(_, bytes)| Cow::Borrowed(*bytes)).collect())
}
