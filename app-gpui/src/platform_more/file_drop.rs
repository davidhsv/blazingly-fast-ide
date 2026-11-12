//! Files dropped from the OS onto a project (or LightEdit) window open in the editor, one tab
//! each; a dropped folder opens as a project (the "Open project in" policy applies). Regions
//! that take OS drops themselves (the Project view, a prompt box) keep theirs, and an editor
//! may consume a drop before any file is opened (an image dropped into a document).

/// Something that may consume a drop before files are opened.
pub trait FileDropHandler {
    /// Whether it took the drop of `paths` at (`x`, `y`) of the window.
    fn handle(&self, paths: &[String], x: f64, y: f64) -> bool;
}

/// Handle of one [`FileDropHandlers::register`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandlerRegistration(u64);

/// The registered drop handlers, asked in registration order.
#[derive(Debug, Clone)]
pub struct FileDropHandlers<H> {
    handlers: Vec<(u64, H)>,
    next_serial: u64,
}

impl<H> Default for FileDropHandlers<H> {
    fn default() -> Self {
        Self { handlers: Vec::new(), next_serial: 0 }
    }
}

impl<H: FileDropHandler> FileDropHandlers<H> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, handler: H) -> HandlerRegistration {
        self.next_serial += 1;
        self.handlers.push((self.next_serial, handler));
        HandlerRegistration(self.next_serial)
    }

    pub fn unregister(&mut self, registration: HandlerRegistration) {
        self.handlers.retain(|(serial, _)| *serial != registration.0);
    }

    /// Whether a handler took the drop. An empty drop is nobody's.
    pub fn consume(&self, paths: &[String], x: f64, y: f64) -> bool {
        !paths.is_empty() && self.handlers.iter().any(|(_, handler)| handler.handle(paths, x, y))
    }
}

/// The window a drop landed on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DropWindow {
    /// A project is open in it.
    pub has_project: bool,
    /// It edits loose files without a project.
    pub light_edit: bool,
    /// Its project lives on another machine: local paths mean nothing there.
    pub remote: bool,
    /// The drop is over a region that handles OS drops itself.
    pub over_drop_owner: bool,
}

/// One thing to do for a drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAction {
    /// Open the file in a tab of its own; `focus` for the one the editor shows in the end.
    OpenFile { path: String, focus: bool },
    /// Run the open-project action on the folder.
    OpenProject { path: String },
}

/// What a drop comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropOutcome {
    /// A registered handler took it.
    Consumed,
    /// It is left to whatever else handles drops there (the Welcome screen, the region's owner).
    Ignored,
    /// Carry these out in order. A file that fails to open does not stop the rest.
    Open(Vec<DropAction>),
}

/// The dropped files (the last one focused), then the first dropped folder as a project.
/// `is_dir` says whether a path is a folder; a path it cannot tell (`None`) is opened as a file.
pub fn open_dropped(paths: &[String], mut is_dir: impl FnMut(&str) -> Option<bool>) -> Vec<DropAction> {
    let (folders, files): (Vec<&String>, Vec<&String>) = paths.iter().partition(|path| is_dir(path) == Some(true));
    let last = files.len().saturating_sub(1);
    let mut actions: Vec<DropAction> = files
        .iter()
        .enumerate()
        .map(|(index, path)| DropAction::OpenFile { path: (*path).clone(), focus: index == last })
        .collect();
    actions.extend(folders.first().map(|path| DropAction::OpenProject { path: (*path).clone() }));
    actions
}

/// Handles one drop of `paths` at (`x`, `y`) on `window`.
pub fn handle_file_drop<H: FileDropHandler>(
    paths: &[String],
    x: f64,
    y: f64,
    handlers: &FileDropHandlers<H>,
    window: DropWindow,
    is_dir: impl FnMut(&str) -> Option<bool>,
) -> DropOutcome {
    if handlers.consume(paths, x, y) {
        return DropOutcome::Consumed;
    }
    let editable = window.has_project || window.light_edit;
    if paths.is_empty() || !editable || window.remote || window.over_drop_owner {
        return DropOutcome::Ignored;
    }
    DropOutcome::Open(open_dropped(paths, is_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Calls = Rc<RefCell<Vec<(Vec<String>, f64, f64)>>>;

    struct Taking {
        takes: bool,
        calls: Calls,
    }

    impl FileDropHandler for Taking {
        fn handle(&self, paths: &[String], x: f64, y: f64) -> bool {
            self.calls.borrow_mut().push((paths.to_vec(), x, y));
            self.takes
        }
    }

    const PROJECT: DropWindow =
        DropWindow { has_project: true, light_edit: false, remote: false, over_drop_owner: false };

    fn paths(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|&path| path.to_owned()).collect()
    }

    /// Drops `dropped` on `window` with no handler registered; `dirs` are the folders among them.
    fn drop_on(window: DropWindow, dropped: &[&str], dirs: &[&str]) -> DropOutcome {
        let none: FileDropHandlers<Taking> = FileDropHandlers::new();
        handle_file_drop(&paths(dropped), 1.0, 1.0, &none, window, |path| Some(dirs.contains(&path)))
    }

    /// The outcome as the spec's log lines: `file <path>[ (focus)]`, `project <path>`.
    fn log(outcome: &DropOutcome) -> Vec<String> {
        let DropOutcome::Open(actions) = outcome else {
            return Vec::new();
        };
        actions
            .iter()
            .map(|action| match action {
                DropAction::OpenFile { path, focus: true } => format!("file {path} (focus)"),
                DropAction::OpenFile { path, focus: false } => format!("file {path}"),
                DropAction::OpenProject { path } => format!("project {path}"),
            })
            .collect()
    }

    // files dropped from the OS › lets an editor consume image drops before opening files, including remote windows
    #[test]
    fn lets_an_editor_consume_image_drops_before_opening_files_including_remote_windows() {
        let calls = Calls::default();
        let mut handlers = FileDropHandlers::new();
        let registration = handlers.register(Taking { takes: true, calls: calls.clone() });
        let remote = DropWindow { remote: true, ..PROJECT };
        let dropped = paths(&["/local/image.png"]);
        let outcome = handle_file_drop(&dropped, 1.0, 1.0, &handlers, remote, |_| Some(false));
        assert_eq!(outcome, DropOutcome::Consumed);
        assert_eq!(*calls.borrow(), [(dropped.clone(), 1.0, 1.0)]);
        handlers.unregister(registration);
        assert_eq!(handle_file_drop(&dropped, 1.0, 1.0, &handlers, remote, |_| Some(false)), DropOutcome::Ignored);
    }

    // files dropped from the OS › opens each dropped file in its own tab, focusing the last; works for files outside
    // the project
    #[test]
    fn opens_each_dropped_file_in_its_own_tab_focusing_the_last() {
        let outcome = drop_on(PROJECT, &["/p/a.ts", "/elsewhere/notes.md"], &[]);
        assert_eq!(log(&outcome), ["file /p/a.ts", "file /elsewhere/notes.md (focus)"]);
    }

    // files dropped from the OS › opens a dropped folder as a project, after the files
    #[test]
    fn opens_a_dropped_folder_as_a_project_after_the_files() {
        let dirs = ["/work/other", "/work/third"];
        let outcome = drop_on(PROJECT, &["/work/other", "/x/readme.txt", "/work/third"], &dirs);
        assert_eq!(log(&outcome), ["file /x/readme.txt (focus)", "project /work/other"]);
    }

    // files dropped from the OS › opens a path it cannot classify as a file
    #[test]
    fn opens_a_path_it_cannot_classify_as_a_file() {
        let none: FileDropHandlers<Taking> = FileDropHandlers::new();
        let outcome = handle_file_drop(&paths(&["/gone.txt"]), 1.0, 1.0, &none, PROJECT, |_| None);
        assert_eq!(log(&outcome), ["file /gone.txt (focus)"]);
    }

    // files dropped from the OS › keeps opening the rest when one file fails
    #[test]
    fn keeps_opening_the_rest_when_one_file_fails() {
        // The plan names every file; the one that fails to open does not take the focus away
        // from the last.
        let outcome = drop_on(PROJECT, &["/bad", "/good"], &[]);
        assert_eq!(log(&outcome), ["file /bad", "file /good (focus)"]);
    }

    // files dropped from the OS › leaves drops over %s to that region
    #[test]
    fn leaves_drops_over_a_drop_owner_to_that_region() {
        let over_owner = DropWindow { over_drop_owner: true, ..PROJECT };
        assert_eq!(drop_on(over_owner, &["/a.ts", "/dir"], &["/dir"]), DropOutcome::Ignored);
    }

    // files dropped from the OS › acts on drops outside any element (window chrome)
    #[test]
    fn acts_on_drops_outside_any_element() {
        assert_eq!(log(&drop_on(PROJECT, &["/a.ts"], &[])), ["file /a.ts (focus)"]);
    }

    // files dropped from the OS › leaves drops to the Welcome screen when no project is open, and ignores remote
    // windows
    #[test]
    fn leaves_drops_to_the_welcome_screen_when_no_project_is_open_and_ignores_remote_windows() {
        assert_eq!(drop_on(DropWindow::default(), &["/a.ts"], &[]), DropOutcome::Ignored);
        assert_eq!(drop_on(DropWindow { remote: true, ..PROJECT }, &["/a.ts"], &[]), DropOutcome::Ignored);
        assert_eq!(drop_on(PROJECT, &[], &[]), DropOutcome::Ignored);
    }

    // files dropped from the OS › opens drops on a LightEdit window as more tabs there; a folder still opens as a
    // project
    #[test]
    fn opens_drops_on_a_light_edit_window_as_more_tabs_there_a_folder_still_opens_as_a_project() {
        let light_edit = DropWindow { light_edit: true, ..DropWindow::default() };
        let outcome = drop_on(light_edit, &["/x/b.txt", "/work/app"], &["/work/app"]);
        assert_eq!(log(&outcome), ["file /x/b.txt (focus)", "project /work/app"]);
    }

    // Not from a spec: a handler that declines leaves the drop to the window, and an empty drop
    // is not offered to handlers at all.
    #[test]
    fn a_declining_handler_leaves_the_drop_to_the_window() {
        let calls = Calls::default();
        let mut handlers = FileDropHandlers::new();
        handlers.register(Taking { takes: false, calls: calls.clone() });
        let outcome = handle_file_drop(&paths(&["/a.ts"]), 2.0, 3.0, &handlers, PROJECT, |_| Some(false));
        assert_eq!(log(&outcome), ["file /a.ts (focus)"]);
        assert_eq!(handle_file_drop(&[], 2.0, 3.0, &handlers, PROJECT, |_| Some(false)), DropOutcome::Ignored);
        assert_eq!(calls.borrow().len(), 1);
    }
}
