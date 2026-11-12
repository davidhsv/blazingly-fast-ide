//! The focused editor's file as the plugins learn it (`editor-focused` events through
//! `plugin_set_active_file`). A window reports its empty focus before its project opens, so plugins
//! would hear of the project only once a file opens: the focus is reported again with the project.

/// What was last reported for one window.
#[derive(Debug, Default)]
pub struct ActiveFile {
    /// `None` before the first report.
    last: Option<Option<String>>,
    root: Option<String>,
}

impl ActiveFile {
    /// The focused editor changed (`path`: its file, `None` without one); returns what to report.
    pub fn focused(&mut self, path: Option<&str>) -> Option<Option<String>> {
        let path = path.map(str::to_owned);
        if self.last.as_ref() == Some(&path) {
            return None;
        }
        self.last = Some(path.clone());
        Some(path)
    }

    /// The window's project changed (opened, closed, trusted); a newly opened project gets the focus
    /// reported again. `current` is the focused editor's file now.
    pub fn project_changed(&mut self, root: Option<&str>, current: Option<&str>) -> Option<Option<String>> {
        let changed = self.root.as_deref() != root;
        self.root = root.map(str::to_owned);
        if !changed || root.is_none() {
            return None;
        }
        self.last = None;
        self.focused(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_focused_editor_again_once_the_windows_project_opens() {
        let mut active = ActiveFile::default();
        assert_eq!(active.focused(None), Some(None));
        assert_eq!(active.focused(None), None, "the same focus is reported once");
        assert_eq!(active.project_changed(Some("/ws"), None), Some(None));
        assert_eq!(active.project_changed(Some("/ws"), None), None, "trusting it changes nothing");
        assert_eq!(active.focused(Some("/ws/a.rs")), Some(Some("/ws/a.rs".to_owned())));
    }
}
