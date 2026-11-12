//! Settings › Debugger › Breakpoints › "Allow placing breakpoints in any file": when off, new line
//! breakpoints go only into files of languages that have a debug adapter (`languages/*.toml`
//! `[debugger]`).

use crate::debugger::effects::BackendCall;
use crate::platform::virtual_docs::class_file_breakpoint_reason;
use std::collections::HashSet;

/// The languages a debugger exists for, once the backend has listed them.
#[derive(Debug, Clone, Default)]
pub struct DebuggableLanguages {
    known: Option<HashSet<String>>,
    loading: bool,
}

impl DebuggableLanguages {
    /// Asks the backend for the language list, once; `desktop`: a backend is attached.
    pub fn load(&mut self, desktop: bool) -> Option<BackendCall> {
        if self.known.is_some() || self.loading || !desktop {
            return None;
        }
        self.loading = true;
        Some(BackendCall::Languages)
    }

    /// The backend answered: `debuggable` are the ids of the languages with a debugger, `None`
    /// when the call failed (asked again on the next occasion).
    pub fn loaded(&mut self, debuggable: Option<Vec<String>>) {
        self.loading = false;
        if let Some(ids) = debuggable {
            self.known = Some(ids.into_iter().collect());
        }
    }

    /// Whether a new breakpoint may be placed in the file `path` of `language`. `any_file`: the
    /// setting that allows every file. Until the language list is known, nothing is blocked.
    pub fn can_place_breakpoint(&self, language: &str, path: &str, any_file: bool) -> bool {
        if class_file_breakpoint_reason(path).is_some() {
            return false;
        }
        any_file || self.known.as_ref().is_none_or(|known| known.contains(language))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: nothing is blocked before the list arrives; afterwards only languages with
    // a debugger take breakpoints, unless the setting allows any file. Output classes never do.
    #[test]
    fn places_breakpoints_only_in_debuggable_languages_once_they_are_known() {
        let mut languages = DebuggableLanguages::default();
        assert!(languages.can_place_breakpoint("markdown", "/p/a.md", false));
        assert!(languages.load(false).is_none());
        assert!(matches!(languages.load(true), Some(BackendCall::Languages)));
        assert!(languages.load(true).is_none());
        languages.loaded(None);
        assert!(languages.load(true).is_some());
        languages.loaded(Some(vec!["python".to_owned()]));
        assert!(languages.load(true).is_none());
        assert!(languages.can_place_breakpoint("python", "/p/a.py", false));
        assert!(!languages.can_place_breakpoint("markdown", "/p/a.md", false));
        assert!(languages.can_place_breakpoint("markdown", "/p/a.md", true));
        assert!(!languages.can_place_breakpoint("java", "/p/out/A.class", true));
    }
}
