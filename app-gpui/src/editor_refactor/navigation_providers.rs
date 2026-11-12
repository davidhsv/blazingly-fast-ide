//! Navigation contributed by plugins (a mapper XML's statement for a Java call, …): optional,
//! asynchronous providers asked in turn for the target at an offset. Answers are kept per
//! editor for its text and the providers' epoch, so that hover and click share one request and a
//! name's result serves every offset inside it; registering, removing or invalidating providers
//! (background indexing finished) drops them all.

use ide_core::Location;
use std::collections::{HashMap, VecDeque};

/// How many answers an editor keeps.
const KEPT: usize = 128;

/// A provider's answer: the text it covers and where it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationMatch {
    pub from: u32,
    pub to: u32,
    pub targets: Vec<Location>,
}

/// The registered providers, in registration order, and the epoch their answers belong to.
#[derive(Debug, Clone, Default)]
pub struct NavigationProviders {
    providers: Vec<u64>,
    next_id: u64,
    epoch: u64,
}

impl NavigationProviders {
    /// Registers a provider; returns its id, to unregister it with.
    pub fn register(&mut self) -> u64 {
        self.next_id += 1;
        self.providers.push(self.next_id);
        self.invalidate();
        self.next_id
    }

    pub fn unregister(&mut self, id: u64) {
        self.providers.retain(|provider| *provider != id);
        self.invalidate();
    }

    /// The providers' answers changed (a project or index change): every kept answer is
    /// dropped. The caller then tells the editors to refresh their link underlines.
    pub fn invalidate(&mut self) {
        self.epoch += 1;
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

/// What a lookup comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// No provider has a target here.
    None,
    Hit(NavigationMatch),
    /// A request for this offset is running: its [`Answered::Resolved`] answers this lookup too.
    Joined,
    /// Ask the provider `provider` for the target at `offset`; answer with
    /// [`ContributedNavigation::answered`].
    Ask {
        ticket: u64,
        provider: u64,
        offset: u32,
    },
}

/// What a provider's answer comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    /// Ask the next provider.
    Next(Lookup),
    /// The result for every lookup of `offset`.
    Resolved { offset: u32, result: Option<NavigationMatch> },
    /// A stale ticket.
    Ignored,
}

#[derive(Debug, Clone)]
struct Pending {
    offset: u32,
    provider_index: usize,
}

/// The contributed navigation of one editor.
#[derive(Debug, Clone, Default)]
pub struct ContributedNavigation {
    version: u64,
    epoch: u64,
    hits: VecDeque<NavigationMatch>,
    /// Offsets no provider answered for.
    misses: VecDeque<u32>,
    pending: HashMap<u64, Pending>,
    next_ticket: u64,
}

impl ContributedNavigation {
    fn reset_if_stale(&mut self, registry: &NavigationProviders, version: u64) {
        if self.version != version || self.epoch != registry.epoch {
            *self = Self { version, epoch: registry.epoch, next_ticket: self.next_ticket, ..Self::default() };
        }
    }

    fn ask(&mut self, registry: &NavigationProviders, offset: u32, provider_index: usize) -> Lookup {
        let Some(provider) = registry.providers.get(provider_index).copied() else { return Lookup::None };
        self.next_ticket += 1;
        self.pending.insert(self.next_ticket, Pending { offset, provider_index });
        Lookup::Ask { ticket: self.next_ticket, provider, offset }
    }

    /// The target at `offset` of the editor's text (version `version`).
    pub fn lookup(&mut self, registry: &NavigationProviders, version: u64, offset: u32) -> Lookup {
        if registry.providers.is_empty() {
            return Lookup::None;
        }
        self.reset_if_stale(registry, version);
        if let Some(hit) = self.hits.iter().find(|hit| hit.from <= offset && offset < hit.to) {
            return Lookup::Hit(hit.clone());
        }
        if self.misses.contains(&offset) {
            return Lookup::None;
        }
        if self.pending.values().any(|pending| pending.offset == offset) {
            return Lookup::Joined;
        }
        self.ask(registry, offset, 0)
    }

    /// The provider answered `ticket`; `version` and `doc_len` are the editor's text now.
    pub fn answered(
        &mut self,
        registry: &NavigationProviders,
        ticket: u64,
        answer: Option<NavigationMatch>,
        version: u64,
        doc_len: u32,
    ) -> Answered {
        let Some(pending) = self.pending.remove(&ticket) else { return Answered::Ignored };
        let offset = pending.offset;
        if self.epoch != registry.epoch || self.version != version {
            return Answered::Resolved { offset, result: None };
        }
        if let Some(found) =
            answer.filter(|found| !found.targets.is_empty() && found.from < found.to && found.to <= doc_len)
        {
            self.hits.push_back(found.clone());
            if self.hits.len() > KEPT {
                self.hits.pop_front();
            }
            return Answered::Resolved { offset, result: Some(found) };
        }
        match self.ask(registry, offset, pending.provider_index + 1) {
            Lookup::None => {
                self.misses.push_back(offset);
                if self.misses.len() > KEPT {
                    self.misses.pop_front();
                }
                Answered::Resolved { offset, result: None }
            }
            next => Answered::Next(next),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::location;
    use super::*;
    use ide_core::{Pos, Range};

    fn hit() -> NavigationMatch {
        let range = Range::new(Pos::new(0, 0), Pos::new(0, 10));
        NavigationMatch { from: 0, to: 15, targets: vec![location("/src/UserMapper.java", range)] }
    }

    const LEN: u32 = 15;

    fn ticket(lookup: &Lookup) -> u64 {
        let Lookup::Ask { ticket, .. } = lookup else { panic!("expected a request, got {lookup:?}") };
        *ticket
    }

    #[test]
    fn shares_pending_hover_click_requests_and_whole_name_results() {
        let mut registry = NavigationProviders::default();
        registry.register();
        let mut editor = ContributedNavigation::default();
        let hover = editor.lookup(&registry, 1, 0);
        assert_eq!(editor.lookup(&registry, 1, 0), Lookup::Joined);
        let answered = editor.answered(&registry, ticket(&hover), Some(hit()), 1, LEN);
        assert_eq!(answered, Answered::Resolved { offset: 0, result: Some(hit()) });
        assert_eq!(editor.lookup(&registry, 1, 8), Lookup::Hit(hit()));
    }

    #[test]
    fn drops_results_after_edits_and_re_queries_the_new_document() {
        let mut registry = NavigationProviders::default();
        registry.register();
        let mut editor = ContributedNavigation::default();
        let request = editor.lookup(&registry, 1, 0);
        let answered = editor.answered(&registry, ticket(&request), Some(hit()), 2, LEN + 1);
        assert_eq!(answered, Answered::Resolved { offset: 0, result: None });
        assert!(matches!(editor.lookup(&registry, 2, 0), Lookup::Ask { .. }));
    }

    #[test]
    fn invalidates_misses_after_background_indexing_and_removes_disabled_providers() {
        let mut registry = NavigationProviders::default();
        let id = registry.register();
        let mut editor = ContributedNavigation::default();
        let first = editor.lookup(&registry, 1, 0);
        assert_eq!(
            editor.answered(&registry, ticket(&first), None, 1, LEN),
            Answered::Resolved { offset: 0, result: None }
        );
        // The miss is kept: the provider is not asked again.
        assert_eq!(editor.lookup(&registry, 1, 0), Lookup::None);
        registry.invalidate();
        let again = editor.lookup(&registry, 1, 0);
        assert_eq!(
            editor.answered(&registry, ticket(&again), Some(hit()), 1, LEN),
            Answered::Resolved { offset: 0, result: Some(hit()) }
        );
        registry.unregister(id);
        assert_eq!(editor.lookup(&registry, 1, 0), Lookup::None);
    }

    #[test]
    fn drops_old_results_after_project_provider_invalidation() {
        let mut registry = NavigationProviders::default();
        registry.register();
        let mut editor = ContributedNavigation::default();
        let request = editor.lookup(&registry, 1, 0);
        registry.invalidate();
        assert_eq!(
            editor.answered(&registry, ticket(&request), Some(hit()), 1, LEN),
            Answered::Resolved { offset: 0, result: None }
        );
    }
}
