//! The names of the installed font families: read from the system once, off the window's thread,
//! and again only when asked (the font pickers' refresh). Every view resolves its font from this
//! list, so no frame asks the system for its fonts.

use gpui_kit::{App, Global, SharedString, TextSystem};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The installed font families, once they are read (or given by a test).
#[derive(Debug, Clone, Default)]
pub struct InstalledFontNames(pub Vec<String>);

impl Global for InstalledFontNames {}

/// A read of the system's fonts is under way: a second request waits for its answer.
struct ReadingFontNames;

impl Global for ReadingFontNames {}

/// The family drawn when none of the wanted ones is installed.
const LAST_RESORT: &str = "Menlo";

static SYSTEM_READS: AtomicUsize = AtomicUsize::new(0);

std::thread_local! {
    /// How many reads of the list began on this thread, the thread an app runs on.
    static LIST_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times this process asked the system for its font families.
pub fn system_font_reads() -> usize {
    SYSTEM_READS.load(Ordering::SeqCst)
}

/// How many reads of the installed families began on this thread's app: from the system, or, in
/// test builds, from what the test set.
pub fn font_list_reads() -> usize {
    LIST_READS.with(std::cell::Cell::get)
}

/// The system's font families: the only place that asks for them. Test builds never get here;
/// the tests set [`InstalledFontNames`] themselves, so no test depends on the machine's fonts.
pub fn system_font_names(text_system: &TextSystem) -> Vec<String> {
    SYSTEM_READS.fetch_add(1, Ordering::SeqCst);
    #[cfg(test)]
    {
        let _ = text_system;
        panic!("test builds never enumerate the system's fonts: set InstalledFontNames instead");
    }
    #[cfg(not(test))]
    text_system.all_font_names()
}

/// Starts reading the installed families unless they are known already or being read (`refresh`
/// reads them again). `then` runs with the list once it is stored; the windows repaint with it.
pub fn load(cx: &mut App, refresh: bool, then: impl FnOnce(&[String], &mut App) + 'static) {
    if cx.has_global::<ReadingFontNames>() {
        // The read under way answers this request too.
        let mut then = Some(then);
        cx.observe_global::<InstalledFontNames>(move |cx| {
            if let Some(then) = then.take() {
                let names = cx.global::<InstalledFontNames>().0.clone();
                then(&names, cx);
            }
        })
        .detach();
        return;
    }
    if !refresh && let Some(known) = cx.try_global::<InstalledFontNames>() {
        let names = known.0.clone();
        then(&names, cx);
        return;
    }
    LIST_READS.with(|reads| reads.set(reads.get() + 1));
    #[cfg(test)]
    {
        let names = cx.try_global::<InstalledFontNames>().map(|known| known.0.clone()).unwrap_or_default();
        cx.set_global(InstalledFontNames(names.clone()));
        cx.refresh_windows();
        then(&names, cx);
    }
    #[cfg(not(test))]
    {
        cx.set_global(ReadingFontNames);
        let text_system: std::sync::Arc<TextSystem> = cx.text_system().clone();
        let read = cx.background_executor().spawn(async move { system_font_names(&text_system) });
        cx.spawn(async move |cx| {
            let names = read.await;
            cx.update(|cx| {
                cx.remove_global::<ReadingFontNames>();
                cx.set_global(InstalledFontNames(names.clone()));
                cx.refresh_windows();
                then(&names, cx);
            });
        })
        .detach();
    }
}

/// The installed families, or `None` while they are not read yet.
pub fn installed(cx: &App) -> Option<&[String]> {
    cx.try_global::<InstalledFontNames>().map(|known| known.0.as_slice())
}

/// The first of `candidates` that is installed; while the installed families are not known, the
/// first candidate. `None` when the list is known and holds none of them (or there are none).
pub fn first_installed(cx: &App, candidates: &[String]) -> Option<String> {
    match installed(cx) {
        Some(names) => candidates.iter().find(|family| names.contains(family)).cloned(),
        None => candidates.first().cloned(),
    }
}

/// The theme's code font: the first family of its `--font-mono` list that is installed; the
/// list's first family until the installed ones are known; Menlo when none of them is installed.
pub fn mono_family(cx: &App) -> SharedString {
    let families =
        cx.try_global::<super::ActiveTheme>().map(|active| active.theme.font_families("font-mono")).unwrap_or_default();
    SharedString::from(first_installed(cx, &families).unwrap_or_else(|| LAST_RESORT.to_owned()))
}

/// The family drawn when none of the wanted ones is installed.
pub fn last_resort() -> &'static str {
    LAST_RESORT
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext as _, TestAppContext};

    fn theme_families(cx: &App) -> Vec<String> {
        cx.global::<super::super::ActiveTheme>().theme.font_families("font-mono")
    }

    // Until the installed families are known the theme's first code font is used; once they are,
    // the first installed one, and Menlo when none is.
    #[gpui_kit::test]
    fn the_code_font_is_the_themes_default_until_the_installed_families_are_known(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(super::super::ActiveTheme::default());
            let families = theme_families(cx);
            assert!(families.len() > 1, "{families:?}");
            assert_eq!(mono_family(cx).as_ref(), families[0]);
            cx.set_global(InstalledFontNames(vec![families[1].clone()]));
            assert_eq!(mono_family(cx).as_ref(), families[1]);
            cx.set_global(InstalledFontNames(vec!["Nothing Of The Theme".to_owned()]));
            assert_eq!(mono_family(cx).as_ref(), "Menlo");
        });
    }

    // A request while the list is known answers from it without reading the system.
    #[gpui_kit::test]
    fn a_known_list_answers_without_reading_the_system(cx: &mut TestAppContext) {
        let reads = system_font_reads();
        let answered = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        cx.update(|cx| {
            cx.set_global(InstalledFontNames(vec!["Test Mono".to_owned()]));
            let answered = answered.clone();
            load(cx, false, move |names, _| answered.borrow_mut().extend_from_slice(names));
        });
        assert_eq!(*answered.borrow(), ["Test Mono"]);
        assert_eq!(system_font_reads(), reads);
    }

    /// The editor, a diff viewer and a terminal, side by side in one window.
    struct Views {
        editor: gpui_kit::Entity<crate::editor::view::Editor>,
        diff: gpui_kit::Entity<crate::vcs_diff::view::DiffView>,
        terminal: gpui_kit::Entity<crate::terminal::view::TerminalView>,
    }

    impl gpui_kit::Render for Views {
        fn render(&mut self, _: &mut gpui_kit::Window, _: &mut gpui_kit::Context<Self>) -> impl gpui_kit::IntoElement {
            use gpui_kit::{ParentElement as _, Styled as _};
            gpui_kit::div()
                .size_full()
                .flex()
                .child(gpui_kit::div().flex_1().h_full().child(self.editor.clone()))
                .child(gpui_kit::div().flex_1().h_full().child(self.diff.clone()))
                .child(gpui_kit::div().flex_1().h_full().child(self.terminal.clone()))
        }
    }

    // Drawing the editor, a diff and a terminal frame after frame never asks the system for its
    // fonts: they draw with the theme's default family until the installed families are known,
    // and repaint with the first installed one once they are.
    #[gpui_kit::test]
    async fn drawing_the_editor_a_diff_and_a_terminal_reads_the_system_fonts_at_most_once(cx: &mut TestAppContext) {
        use crate::backend::{AppDirs, Backend};
        use crate::terminal::test_support::{isolate_shells, plain_shell, trusted_folder};
        use std::time::Duration;
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        let path = root.join("a.txt");
        std::fs::write(&path, "alpha\nbeta\n").expect("the file is written");
        cx.update(|cx| {
            cx.set_global(super::super::ActiveTheme::default());
            crate::platform::input::init(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            isolate_shells(home.path(), cx);
            trusted_folder("w1", &root, cx);
        });
        let opened = cx.update(|cx| {
            cx.global::<Backend>()
                .with_host(move |host| async move { ide_host::ops::doc::open(&host, "w1", path).await })
        });
        let info = opened.await.expect("the task completes").expect("the document opens");
        let host = cx.update(|cx| cx.global::<Backend>().host.clone());
        let options = ide_terminal::TerminalOptions {
            cwd: ide_core::IpcPath(root.clone()),
            shell: Some(plain_shell(home.path()).to_string_lossy().into_owned()),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
            cols: 80,
            rows: 24,
            shell_integration: false,
        };
        let (views, cx) = cx.add_window_view(move |_, cx| Views {
            editor: cx.new(|cx| crate::editor::view::Editor::new(host, "w1", &info, cx).expect("the editor opens")),
            diff: cx.new(|cx| crate::vcs_diff::view::DiffView::new("a\nb\n", "a\nc\n", Default::default(), cx)),
            terminal: cx.new(|cx| crate::terminal::view::TerminalView::new("w1", options, cx)),
        });
        let frames = |cx: &mut gpui_kit::VisualTestContext| {
            for _ in 0..5 {
                cx.update(|window, _| window.refresh());
                cx.executor().advance_clock(Duration::from_millis(20));
                cx.run_until_parked();
            }
        };
        frames(cx);
        let editor = views.read_with(cx, |views, _| views.editor.clone());
        let family = |cx: &mut gpui_kit::VisualTestContext| {
            editor.read_with(cx, |editor, cx| editor.text_style(cx).0.family.to_string())
        };
        assert!(editor.read_with(cx, |editor, _| editor.painted_bounds().is_some()), "the editor is drawn");
        let families = cx.update(|_, cx| theme_families(cx));
        assert_eq!(family(cx), families[0], "the theme's default family until the list is known");

        assert_eq!(font_list_reads(), 0, "drawing reads no font list");

        // The list arrives, read once as the app does at start: the views repaint with the first
        // installed family of the theme, and further frames and requests read it no more.
        cx.update(|_, cx| {
            // What the system would answer; a refresh reads it in test builds.
            cx.set_global(InstalledFontNames(vec![families[1].clone()]));
            load(cx, true, |_, _| {});
        });
        frames(cx);
        assert_eq!(family(cx), families[1]);
        cx.update(|_, cx| load(cx, false, |_, _| {}));
        frames(cx);
        assert_eq!(font_list_reads(), 1, "the list was read once");
        assert_eq!(system_font_reads(), 0, "test builds never ask the system");
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }
}
