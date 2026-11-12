//! Shared, bounded navigation results of YAML / properties keys: the ⌘-hover probe and the click
//! that follows it ask the server once.
//!
//! A configuration key's target is a Java class or a `@ConfigurationProperties` field, which a
//! server finds only once the Java import progressed; so answers are kept only while nothing they
//! depend on changed. Events invalidate them (edits of any open document, file changes, server
//! restarts…); an age limit is the backstop for servers and remote file systems that do not report
//! every change. Empty and failed answers are never kept: the import may still be running.

use crate::editor::buffer;
use crate::editor_lsp::completion_items::is_word_char;
use ide_core::error::ErrorKind;
use ide_core::event::ServerStatus;
use ide_core::{DocId, IdeError, IdeEvent, Location};
use ide_lsp::types::GotoKind;
use ropey::Rope;
use std::collections::HashMap;
use std::ops::Range;

/// How long a non-empty answer is reused at most.
pub const CONFIGURATION_NAVIGATION_MAX_AGE_MS: u64 = 30_000;
/// How long a lookup still in flight is shared with new ones for the same key.
const PENDING_SHARE_MS: u64 = 3000;
const MAX_ENTRIES: usize = 128;
const MAX_LOCATIONS: usize = 4096;

/// Whether `language` is a configuration language whose keys navigate through this cache.
pub fn is_configuration_language(language: &str) -> bool {
    matches!(language, "yaml" | "properties")
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-')
}

/// The name navigation underlines and looks up at `offset`. Configuration names include dots and
/// hyphens even where the language's word rules split them (`spring.datasource.driver-class-name`),
/// but not quotes or the value after them; elsewhere it is the identifier there. `None` off a name.
pub fn navigation_word(doc: &Rope, offset: u32, configuration: bool) -> Option<Range<u32>> {
    if !configuration {
        let word = buffer::word_at(doc, offset);
        let text = buffer::slice(doc, word.start, word.end);
        return text.chars().next().is_some_and(is_word_char).then_some(word);
    }
    let (line, col) = buffer::line_col(doc, offset);
    let start = buffer::line_range(doc, line).start;
    let text = buffer::line_text(doc, line);
    let at = buffer::byte_of_col(&text, col);
    let before: u32 = text[..at].chars().rev().take_while(|c| is_name_char(*c)).map(|c| c.len_utf16() as u32).sum();
    let after: u32 = text[at..].chars().take_while(|c| is_name_char(*c)).map(|c| c.len_utf16() as u32).sum();
    if before + after == 0 {
        return None;
    }
    Some(start + col - before..start + col + after)
}

/// The editor a navigation starts in.
#[derive(Debug, Clone, Copy)]
pub struct OpenDoc<'a> {
    pub doc: DocId,
    pub path: &'a str,
    pub language: &'a str,
    /// The document's text now.
    pub text: &'a Rope,
}

/// A lookup the server is asked for; its answer goes to [`ConfigurationNavigation::answered`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    id: u64,
    revision: u64,
}

/// How to answer a navigation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Not a configuration key's declaration: ask the server as usual, nothing is shared.
    Direct,
    /// Ask the server for the declaration at `offset` (the key's start) and hand the answer to
    /// [`ConfigurationNavigation::answered`]; the request is still wanted while
    /// [`ConfigurationNavigation::wanted`] says so.
    Ask { ticket: Ticket, offset: u32 },
    /// The same key's lookup is in flight: the answer of `ticket` is this request's answer too.
    Wait { ticket: Ticket },
    /// The key's answer, still valid.
    Ready(Vec<Location>),
}

type Key = (DocId, u32, u32);

#[derive(Debug, Clone)]
enum Answer {
    Pending(Ticket),
    Done(Vec<Location>),
}

#[derive(Debug, Clone)]
struct Entry {
    key: Key,
    text: Rope,
    path: String,
    at: u64,
    size: usize,
    /// Cleared when the server reports progress after the lookup started: its answer is used but
    /// not kept.
    retain: bool,
    answer: Answer,
}

/// The cache of every configuration editor.
#[derive(Debug, Clone, Default)]
pub struct ConfigurationNavigation {
    /// Least recently used first.
    entries: Vec<Entry>,
    /// Lookups in flight: their key and the text they were asked for.
    inflight: HashMap<u64, (Key, Rope)>,
    revision: u64,
    next: u64,
}

impl ConfigurationNavigation {
    /// Bumped by every invalidation: probe results remembered elsewhere are stale when it moved.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Something the answers depend on changed: they are all dropped, and lookups in flight are
    /// cancelled when they answer.
    pub fn invalidate(&mut self) {
        self.revision += 1;
        self.entries.clear();
    }

    /// The server refreshed or settled after indexing, which may be the progress of a lookup in
    /// flight: that lookup is not cancelled (its click still navigates), only not kept; answers
    /// already kept are dropped.
    pub fn refresh(&mut self) {
        for entry in &mut self.entries {
            entry.retain = false;
        }
        self.entries.retain(|entry| entry.size == 0);
    }

    /// The backend events that change what configuration keys resolve to.
    pub fn ide_event(&mut self, event: &IdeEvent) {
        match event {
            IdeEvent::FsChanged { .. }
            | IdeEvent::ExcludedDirsChanged { .. }
            | IdeEvent::DocPatched { .. }
            | IdeEvent::DocLanguage { .. }
            | IdeEvent::WorkspaceEditApplied { .. }
            | IdeEvent::LanguageServerRemoved { .. } => self.invalidate(),
            IdeEvent::LanguageServer {
                status: ServerStatus::Starting | ServerStatus::Stopped | ServerStatus::Crashed,
                ..
            } => self.invalidate(),
            IdeEvent::LspRefresh { .. } => self.refresh(),
            _ => {}
        }
    }

    /// Any open editor's document changed (before the edit reaches the backend), a document was
    /// closed or saved (closing an unsaved Java buffer restores its declarations on disk), or the
    /// project root changed.
    pub fn document_changed(&mut self) {
        self.invalidate();
    }

    /// Whether a non-empty answer for the key at `offset` is kept, current and fresh: a remembered
    /// probe of that key may still underline it.
    pub fn has_navigation(&self, doc: &OpenDoc<'_>, offset: u32, now: u64) -> bool {
        let Some(word) = navigation_word(doc.text, offset, true) else { return false };
        self.entries.iter().any(|entry| {
            entry.key == (doc.doc, word.start, word.end)
                && entry.size > 0
                && entry.retain
                && entry.path == doc.path
                && entry.text == *doc.text
                && now.saturating_sub(entry.at) < CONFIGURATION_NAVIGATION_MAX_AGE_MS
        })
    }

    /// A navigation of `kind` at `offset` of `doc`, at time `now` in milliseconds.
    pub fn lookup(&mut self, doc: &OpenDoc<'_>, offset: u32, kind: GotoKind, now: u64) -> Lookup {
        let OpenDoc { doc, path, language, text } = *doc;
        let configuration = is_configuration_language(language);
        let Some(word) = navigation_word(text, offset, configuration).filter(|_| configuration) else {
            return Lookup::Direct;
        };
        if kind != GotoKind::Definition {
            return Lookup::Direct;
        }
        let key = (doc, word.start, word.end);
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let entry = &self.entries[index];
            let max_age = if entry.size > 0 { CONFIGURATION_NAVIGATION_MAX_AGE_MS } else { PENDING_SHARE_MS };
            if entry.path == path && entry.text == *text && now.saturating_sub(entry.at) < max_age {
                let entry = self.entries.remove(index);
                let found = match &entry.answer {
                    Answer::Pending(ticket) => Lookup::Wait { ticket: *ticket },
                    Answer::Done(locations) => Lookup::Ready(locations.clone()),
                };
                self.entries.push(entry);
                return found;
            }
            self.entries.remove(index);
        }
        self.next += 1;
        let ticket = Ticket { id: self.next, revision: self.revision };
        self.entries.push(Entry {
            key,
            text: text.clone(),
            path: path.to_owned(),
            at: now,
            size: 0,
            retain: true,
            answer: Answer::Pending(ticket),
        });
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
        self.inflight.insert(ticket.id, (key, text.clone()));
        Lookup::Ask { ticket, offset: word.start }
    }

    /// Whether the lookup of `ticket` is still wanted (nothing was invalidated since it started).
    pub fn wanted(&self, ticket: Ticket) -> bool {
        ticket.revision == self.revision
    }

    fn pending_index(&self, ticket: Ticket) -> Option<usize> {
        self.entries.iter().position(|entry| matches!(entry.answer, Answer::Pending(own) if own == ticket))
    }

    fn drop_pending(&mut self, ticket: Ticket) {
        if let Some(index) = self.pending_index(ticket) {
            self.entries.remove(index);
        }
    }

    /// The server's answer to `ticket`, with the document's text now; returns the answer of the
    /// lookup (and of every lookup waiting on `ticket`). An answer computed before an invalidation,
    /// or for a text that changed meanwhile, is `cancelled`. Every [`Lookup::Ask`] ends here, a
    /// request not sent (no longer [`ConfigurationNavigation::wanted`]) with a `cancelled` error.
    pub fn answered(
        &mut self,
        ticket: Ticket,
        result: Result<Vec<Location>, IdeError>,
        text: &Rope,
    ) -> Result<Vec<Location>, IdeError> {
        let asked = self.inflight.remove(&ticket.id);
        let current = asked.is_some_and(|(_, asked)| asked == *text) && self.wanted(ticket);
        let locations = match result {
            Ok(_) if !current => Err(IdeError::new(ErrorKind::Cancelled, "cancelled")),
            other => other,
        };
        let locations = match locations {
            Ok(locations) => locations,
            Err(error) => {
                self.drop_pending(ticket);
                return Err(error);
            }
        };
        if let Some(index) = self.pending_index(ticket) {
            if locations.is_empty() || !self.entries[index].retain {
                self.entries.remove(index);
            } else {
                let entry = &mut self.entries[index];
                entry.size = locations.len();
                entry.answer = Answer::Done(locations.clone());
            }
        }
        let mut size: usize = self.entries.iter().map(|entry| entry.size).sum();
        while size > MAX_LOCATIONS && !self.entries.is_empty() {
            size -= self.entries.remove(0).size;
        }
        Ok(locations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_lsp::availability::{ServerSpells, SettleListener};
    use ide_core::event::LspRefreshKind;
    use ide_core::{IpcPath, Pos, Range as TextRange, ServerId};

    fn target() -> Location {
        Location { path: "/p/Config.java".into(), range: TextRange::new(Pos::new(3, 4), Pos::new(3, 9)), uri: None }
    }

    /// An open editor of the spec.
    struct Editor {
        doc: DocId,
        path: String,
        language: &'static str,
        text: Rope,
    }

    fn editor(doc: u32, language: &'static str, text: &str) -> Editor {
        Editor { doc: DocId(doc), path: format!("/p/{doc}.{language}"), language, text: Rope::from_str(text) }
    }

    fn yaml(doc: u32) -> Editor {
        editor(doc, "yaml", "server:\n  port: 8080\n")
    }

    /// The cache and the server, which answers `answers` in turn (then the target) and counts
    /// the requests it got.
    struct Server {
        cache: ConfigurationNavigation,
        asked: usize,
        answers: Vec<Result<Vec<Location>, IdeError>>,
        now: u64,
    }

    impl Server {
        fn new() -> Self {
            Self { cache: ConfigurationNavigation::default(), asked: 0, answers: Vec::new(), now: 1_000_000 }
        }

        fn lookup(&mut self, editor: &Editor, offset: u32, kind: GotoKind) -> Lookup {
            let doc = OpenDoc { doc: editor.doc, path: &editor.path, language: editor.language, text: &editor.text };
            self.cache.lookup(&doc, offset, kind, self.now)
        }

        fn reply(&mut self) -> Result<Vec<Location>, IdeError> {
            self.asked += 1;
            if self.answers.is_empty() { Ok(vec![target()]) } else { self.answers.remove(0) }
        }

        /// A navigation request carried out to its answer.
        fn navigate(&mut self, editor: &Editor, offset: u32, kind: GotoKind) -> Result<Vec<Location>, IdeError> {
            match self.lookup(editor, offset, kind) {
                Lookup::Direct => self.reply(),
                Lookup::Ask { ticket, .. } => {
                    let reply = self.reply();
                    self.cache.answered(ticket, reply, &editor.text)
                }
                Lookup::Ready(locations) => Ok(locations),
                Lookup::Wait { .. } => panic!("nothing is in flight"),
            }
        }

        fn definition(&mut self, editor: &Editor, offset: u32) -> Result<Vec<Location>, IdeError> {
            self.navigate(editor, offset, GotoKind::Definition)
        }

        fn ask(&mut self, editor: &Editor, offset: u32) -> Ticket {
            let Lookup::Ask { ticket, .. } = self.lookup(editor, offset, GotoKind::Definition) else {
                panic!("the server is asked")
            };
            self.asked += 1;
            ticket
        }
    }

    #[test]
    fn names_include_separators_but_exclude_syntax_and_adjacent_values() {
        for language in ["yaml", "properties"] {
            let view = editor(1, language, "  \"spring.datasource.driver-class-name\": example.Driver");
            for offset in [3, 9, 20, 26, 32, 35] {
                let range = navigation_word(&view.text, offset, true).expect("a name");
                assert_eq!(buffer::slice(&view.text, range.start, range.end), "spring.datasource.driver-class-name");
            }
            assert_eq!(navigation_word(&view.text, 1, true), None, "{language}");
            assert_eq!(navigation_word(&view.text, 39, true), None, "{language}");
        }
    }

    #[test]
    fn shares_an_in_flight_hover_lookup_with_a_click_anywhere_in_the_same_key() {
        let mut server = Server::new();
        let view = yaml(1);
        let hover = server.ask(&view, 10);
        assert_eq!(server.lookup(&view, 12, GotoKind::Definition), Lookup::Wait { ticket: hover });
        let answer = server.cache.answered(hover, Ok(vec![target()]), &view.text);
        assert_eq!(answer.ok(), Some(vec![target()]));
        assert_eq!(server.definition(&view, 11).ok(), Some(vec![target()]));
        assert_eq!(server.asked, 1);
    }

    #[test]
    fn reuses_results_when_the_same_document_is_shown_again() {
        let mut server = Server::new();
        assert!(server.definition(&yaml(1), 10).is_ok());
        assert!(server.definition(&yaml(1), 11).is_ok());
        assert_eq!(server.asked, 1);
    }

    #[test]
    fn keeps_different_keys_and_documents_separate() {
        let mut server = Server::new();
        let view = yaml(1);
        assert!(server.definition(&view, 1).is_ok());
        assert!(server.definition(&view, 10).is_ok());
        assert!(server.definition(&yaml(2), 10).is_ok());
        assert_eq!(server.asked, 3);
    }

    #[test]
    fn leaves_native_navigation_uncached() {
        for language in ["java", "typescript"] {
            let mut server = Server::new();
            let view = editor(1, language, "value");
            assert_eq!(server.lookup(&view, 1, GotoKind::Definition), Lookup::Direct);
            assert!(server.definition(&view, 1).is_ok());
            assert!(server.definition(&view, 1).is_ok());
            assert_eq!(server.asked, 2, "{language}");
        }
    }

    #[test]
    fn does_not_mix_definition_with_other_navigation_kinds() {
        let mut server = Server::new();
        let view = yaml(1);
        assert!(server.definition(&view, 10).is_ok());
        assert!(server.navigate(&view, 10, GotoKind::TypeDefinition).is_ok());
        assert_eq!(server.asked, 2);
    }

    #[test]
    fn invalidates_on_edits_to_another_open_java_document_before_its_ipc_sync() {
        let mut server = Server::new();
        let view = yaml(1);
        assert!(server.definition(&view, 10).is_ok());
        // The Java editor's document changed; the backend has not seen the edit yet.
        server.cache.document_changed();
        assert!(server.definition(&view, 10).is_ok());
        assert_eq!(server.asked, 2);
    }

    #[test]
    fn invalidates_on_type() {
        let path = || vec![IpcPath::from("/p/Config.java")];
        let events = [
            IdeEvent::FsChanged { created: vec![], modified: path(), removed: vec![] },
            IdeEvent::FsChanged { created: vec![], modified: vec![], removed: path() },
            IdeEvent::DocPatched { doc: DocId(2), base_version: 1, version: 2, changes: vec![], workspace_edit: false },
            IdeEvent::LanguageServer {
                server: ServerId(1),
                name: "jdtls".into(),
                status: ServerStatus::Starting,
                message: None,
            },
            IdeEvent::LspRefresh { server: ServerId(1), kind: LspRefreshKind::CodeLens },
            IdeEvent::ExcludedDirsChanged { dirs: vec![IpcPath::from("/p/target")] },
        ];
        for event in events {
            let mut server = Server::new();
            let view = yaml(1);
            assert!(server.definition(&view, 10).is_ok());
            server.cache.ide_event(&event);
            assert!(server.definition(&view, 10).is_ok());
            assert_eq!(server.asked, 2, "{event:?}");
        }
    }

    #[test]
    fn does_not_invalidate_just_because_navigation_opened_a_clean_target() {
        let mut server = Server::new();
        let view = yaml(1);
        assert!(server.definition(&view, 10).is_ok());
        // Opening the target's editor is no event for the cache.
        assert!(server.definition(&view, 10).is_ok());
        assert_eq!(server.asked, 1);
        // Closing it is: an unsaved buffer's declarations go back to the file's.
        server.cache.document_changed();
        assert!(server.definition(&view, 10).is_ok());
        assert_eq!(server.asked, 2);
    }

    #[test]
    fn discards_in_flight_results_when_dependencies_change() {
        let mut server = Server::new();
        let view = yaml(1);
        let ticket = server.ask(&view, 10);
        server.cache.invalidate();
        assert!(!server.cache.wanted(ticket));
        let answer = server.cache.answered(ticket, Ok(vec![target()]), &view.text);
        assert_eq!(answer.map_err(|error| error.kind).err(), Some(ErrorKind::Cancelled));
        assert!(server.definition(&view, 10).is_ok());
        assert_eq!(server.asked, 2);
    }

    #[test]
    fn does_not_swallow_an_in_flight_click_on_its_own_server_event() {
        for event in ["settled", "refresh"] {
            let mut server = Server::new();
            let view = yaml(1);
            let ticket = server.ask(&view, 10);
            if event == "settled" {
                let mut spells = ServerSpells::default();
                let mut listener = SettleListener::default();
                spells.status(ServerId(1), ServerStatus::Busy, server.now);
                listener.status(ServerId(1), ServerStatus::Busy);
                let settled = spells.status(ServerId(1), ServerStatus::Ready, server.now + 1600).expect("it settled");
                listener.status(ServerId(1), ServerStatus::Ready);
                assert!(listener.settled(&settled, server.now + 1600));
                server.cache.refresh();
            } else {
                server.cache.ide_event(&IdeEvent::LspRefresh { server: ServerId(1), kind: LspRefreshKind::CodeLens });
            }
            let answer = server.cache.answered(ticket, Ok(vec![target()]), &view.text);
            assert_eq!(answer.ok(), Some(vec![target()]), "{event}");
            assert!(server.definition(&view, 10).is_ok());
            assert_eq!(server.asked, 2, "{event}");
        }
    }

    #[test]
    fn does_not_retain_errors_or_missing_targets_during_import() {
        let mut server = Server::new();
        let view = yaml(1);
        server.answers = vec![Err(IdeError::new(ErrorKind::Protocol, "timeout")), Ok(Vec::new())];
        assert_eq!(server.definition(&view, 10).map_err(|error| error.message).err().as_deref(), Some("timeout"));
        assert_eq!(server.definition(&view, 10).ok(), Some(Vec::new()));
        assert_eq!(server.definition(&view, 10).ok(), Some(vec![target()]));
        assert_eq!(server.asked, 3);
    }

    #[test]
    fn expires_results_when_a_server_did_not_report_a_change() {
        let mut server = Server::new();
        let view = yaml(1);
        assert!(server.definition(&view, 10).is_ok());
        server.now += 30_001;
        assert!(server.definition(&view, 10).is_ok());
        assert_eq!(server.asked, 2);
    }

    #[test]
    fn bounds_retained_entries() {
        let mut server = Server::new();
        let text: String = (0..130).map(|i| format!("key{i}=x\n")).collect();
        let view = editor(1, "properties", &text);
        for line in 0..130 {
            assert!(server.definition(&view, buffer::line_range(&view.text, line).start).is_ok());
        }
        assert!(server.definition(&view, 0).is_ok());
        assert_eq!(server.asked, 131);
    }

    #[test]
    fn bounds_retained_locations_even_for_large_multi_target_answers() {
        let mut server = Server::new();
        let view = yaml(1);
        server.answers = vec![Ok(vec![target(); 4097])];
        assert!(server.definition(&view, 10).is_ok());
        assert!(server.definition(&view, 10).is_ok());
        assert_eq!(server.asked, 2);
    }
}
