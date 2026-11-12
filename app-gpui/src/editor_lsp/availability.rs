//! When background features may ask a language server, and when to ask again.
//!
//! A feature that got an `unavailable` answer for a document leaves it alone for a while, so
//! typing in a file without a server (or with a server lacking the feature) does not send a
//! request per keystroke. Each feature has its own back-off: a server lacking one capability must
//! not switch off the others. All of them end when any server becomes ready.
//!
//! Answers given while a server was starting, loading the project or indexing may be partial:
//! [`ServerSpells`] follows every server's busy spells and [`SettleListener`] tells a feature when
//! to ask again. Times are milliseconds passed in by the caller.

use ide_core::event::ServerStatus;
use ide_core::{DocId, ServerId};
use std::collections::HashMap;

/// How long a document is left alone after an `unavailable` answer.
pub const DEFAULT_BACKOFF_MS: u64 = 15_000;

/// The back-off of one feature.
#[derive(Debug, Clone)]
pub struct FeatureBackoff {
    ms: u64,
    until: HashMap<DocId, u64>,
}

impl Default for FeatureBackoff {
    fn default() -> Self {
        Self::new(DEFAULT_BACKOFF_MS)
    }
}

impl FeatureBackoff {
    pub fn new(ms: u64) -> Self {
        Self { ms, until: HashMap::new() }
    }

    /// Stops asking about `doc` for a while.
    pub fn mark(&mut self, doc: DocId, now: u64) {
        self.until.insert(doc, now + self.ms);
    }

    /// Whether `doc` is still being left alone.
    pub fn active(&mut self, doc: DocId, now: u64) -> bool {
        match self.until.get(&doc) {
            None => false,
            Some(&until) if until > now => true,
            Some(_) => {
                self.until.remove(&doc);
                false
            }
        }
    }

    pub fn clear(&mut self) {
        self.until.clear();
    }
}

/// The background features that back off on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LspFeature {
    Completion,
    InlineCompletion,
    Intentions,
    AutoImport,
    InlayHints,
    CodeLens,
    ConfigurationNavigation,
}

/// The back-offs of every feature.
#[derive(Debug, Clone, Default)]
pub struct FeatureBackoffs {
    features: HashMap<LspFeature, FeatureBackoff>,
}

impl FeatureBackoffs {
    pub fn feature(&mut self, feature: LspFeature) -> &mut FeatureBackoff {
        self.features.entry(feature).or_default()
    }

    /// A server changed status: once one is ready, every feature may ask again.
    pub fn server_status(&mut self, status: ServerStatus) {
        if status == ServerStatus::Ready {
            self.features.values_mut().for_each(FeatureBackoff::clear);
        }
    }
}

/// A busy spell at least this long is indexing or a build, not the server answering a request.
const LONG_BUSY_MS: u64 = 1500;
/// A busy spell starting this soon after a feature asked again may be that feature's own requests.
const ECHO_MS: i64 = 2000;
/// A busy spell starting this soon after a start is the server loading the project.
const LOADING_AFTER_START_MS: u64 = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spell {
    Starting,
    BusySince(u64),
}

/// A server that became ready after starting or after a busy spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settled {
    pub server: ServerId,
    /// When the spell began; `None` when the server was starting.
    pub since: Option<u64>,
    /// The spell began right after a start: the project was loading.
    pub loading: bool,
}

/// Every server's spells, followed from the start of the app, so that an editor opened while
/// its server starts or loads still learns when that ends.
#[derive(Debug, Clone, Default)]
pub struct ServerSpells {
    spells: HashMap<ServerId, Spell>,
    /// When servers became ready after starting, until their next spell ends.
    ready_after_start: HashMap<ServerId, u64>,
}

impl ServerSpells {
    /// Records a status change; returns the spell that ended, to hand to every [`SettleListener`].
    pub fn status(&mut self, server: ServerId, status: ServerStatus, now: u64) -> Option<Settled> {
        match status {
            ServerStatus::Starting => {
                self.spells.insert(server, Spell::Starting);
                self.ready_after_start.remove(&server);
                None
            }
            ServerStatus::Busy => {
                self.spells.entry(server).or_insert(Spell::BusySince(now));
                None
            }
            ServerStatus::Ready => {
                let spell = self.spells.remove(&server)?;
                let started_at = self.ready_after_start.remove(&server);
                Some(match spell {
                    Spell::Starting => {
                        self.ready_after_start.insert(server, now);
                        Settled { server, since: None, loading: started_at.is_some() }
                    }
                    Spell::BusySince(since) => Settled {
                        server,
                        since: Some(since),
                        loading: started_at.is_some_and(|at| since.saturating_sub(at) < LOADING_AFTER_START_MS),
                    },
                })
            }
            _ => {
                self.spells.remove(&server);
                self.ready_after_start.remove(&server);
                None
            }
        }
    }
}

/// One feature's view of the spells: whether a settled server means its earlier answers may have
/// been incomplete.
///
/// Short busy spells are ignored: a server reports progress for the very requests a feature
/// sends, and asking again on each of them would never stop. For the same reason a long spell
/// that began right after the feature asked again re-runs it only once in a row. The spell right
/// after a start counts whatever its length: it is the project loading, and answers given
/// meanwhile are partial.
#[derive(Debug, Clone, Default)]
pub struct SettleListener {
    /// Per server: when the feature last asked again, and whether that was an echo.
    fired: HashMap<ServerId, (u64, bool)>,
}

impl SettleListener {
    /// Call for every status change, before [`SettleListener::settled`].
    pub fn status(&mut self, server: ServerId, status: ServerStatus) {
        if status == ServerStatus::Starting {
            self.fired.remove(&server);
        }
    }

    /// Whether the feature should ask again now that `settled` ended.
    pub fn settled(&mut self, settled: &Settled, now: u64) -> bool {
        if !settled.loading && settled.since.is_some_and(|since| now.saturating_sub(since) < LONG_BUSY_MS) {
            return false;
        }
        let last = self.fired.get(&settled.server).copied();
        let echo = last.is_some_and(|(at, _)| match settled.since {
            None => true,
            Some(since) => (since as i64) - (at as i64) < ECHO_MS,
        });
        if echo && last.is_some_and(|(_, echoed)| echoed) {
            return false;
        }
        self.fired.insert(settled.server, (now, echo));
        true
    }
}

/// Whether data fetched earlier may be stale after a status change of `server`: it has just
/// (re)started, so requests made while it was down came back empty.
#[derive(Debug, Clone, Default)]
pub struct RestartWatch {
    starting: Vec<ServerId>,
}

impl RestartWatch {
    pub fn status(&mut self, server: ServerId, status: ServerStatus) -> bool {
        match status {
            ServerStatus::Starting => {
                if !self.starting.contains(&server) {
                    self.starting.push(server);
                }
                false
            }
            ServerStatus::Ready => {
                let before = self.starting.len();
                self.starting.retain(|other| *other != server);
                self.starting.len() != before
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The servers' spells and the calls of the features listening, with a clock.
    #[derive(Default)]
    struct World {
        now: u64,
        spells: ServerSpells,
        listeners: Vec<(SettleListener, u32)>,
    }

    impl World {
        fn listen(&mut self) -> usize {
            self.listeners.push((SettleListener::default(), 0));
            self.listeners.len() - 1
        }

        fn status(&mut self, status: ServerStatus, server: u32) {
            let server = ServerId(server);
            let settled = self.spells.status(server, status, self.now);
            for (listener, calls) in &mut self.listeners {
                listener.status(server, status);
                if let Some(settled) = &settled
                    && listener.settled(settled, self.now)
                {
                    *calls += 1;
                }
            }
        }

        fn calls(&self, listener: usize) -> u32 {
            self.listeners[listener].1
        }
    }

    #[test]
    fn re_runs_a_callback_that_keeps_the_server_busy_only_once() {
        let mut world = World::default();
        let fn_ = world.listen();
        world.status(ServerStatus::Starting, 1);
        world.status(ServerStatus::Ready, 1);
        assert_eq!(world.calls(fn_), 1);
        for _ in 0..2 {
            world.status(ServerStatus::Busy, 1);
            world.now += 2000;
            world.status(ServerStatus::Ready, 1);
        }
        assert_eq!(world.calls(fn_), 2);
        // Indexing that starts later is a new settle.
        world.now += 10_000;
        world.status(ServerStatus::Busy, 1);
        world.now += 5000;
        world.status(ServerStatus::Ready, 1);
        assert_eq!(world.calls(fn_), 3);
    }

    #[test]
    fn ignores_short_busy_spells() {
        let mut world = World::default();
        let fn_ = world.listen();
        world.status(ServerStatus::Busy, 2);
        world.now += 100;
        world.status(ServerStatus::Ready, 2);
        assert_eq!(world.calls(fn_), 0);
    }

    #[test]
    fn re_runs_after_the_project_loads_right_after_a_start_however_briefly() {
        let mut world = World::default();
        let fn_ = world.listen();
        world.status(ServerStatus::Starting, 3);
        world.status(ServerStatus::Ready, 3);
        assert_eq!(world.calls(fn_), 1);
        // A small project loads in about a second; answers given meanwhile were partial.
        world.status(ServerStatus::Busy, 3);
        world.now += 1000;
        world.status(ServerStatus::Ready, 3);
        assert_eq!(world.calls(fn_), 2);
        // Later short spells are requests again.
        world.status(ServerStatus::Busy, 3);
        world.now += 100;
        world.status(ServerStatus::Ready, 3);
        assert_eq!(world.calls(fn_), 2);
    }

    #[test]
    fn tells_a_callback_added_while_the_project_loads_that_it_has() {
        let mut world = World::default();
        world.listen();
        world.status(ServerStatus::Starting, 4);
        world.status(ServerStatus::Ready, 4);
        world.status(ServerStatus::Busy, 4);
        // An editor restored at launch opens now.
        let fn_ = world.listen();
        world.now += 800;
        world.status(ServerStatus::Ready, 4);
        assert_eq!(world.calls(fn_), 1);
    }

    #[test]
    fn ignores_a_short_spell_that_begins_well_after_a_start() {
        let mut world = World::default();
        let fn_ = world.listen();
        world.status(ServerStatus::Starting, 5);
        world.status(ServerStatus::Ready, 5);
        world.now += 10_000;
        world.status(ServerStatus::Busy, 5);
        world.now += 100;
        world.status(ServerStatus::Ready, 5);
        assert_eq!(world.calls(fn_), 1);
    }

    #[test]
    fn leaves_a_document_alone_until_the_back_off_expires() {
        let mut backoff = FeatureBackoff::new(1000);
        assert!(!backoff.active(DocId(1), 0));
        backoff.mark(DocId(1), 0);
        assert!(backoff.active(DocId(1), 0));
        assert!(!backoff.active(DocId(2), 0));
        assert!(!backoff.active(DocId(1), 1001));
    }

    #[test]
    fn keeps_features_independent_one_feature_backing_off_does_not_stop_another() {
        let mut backoffs = FeatureBackoffs::default();
        backoffs.feature(LspFeature::InlayHints).mark(DocId(1), 0);
        assert!(backoffs.feature(LspFeature::InlayHints).active(DocId(1), 0));
        assert!(!backoffs.feature(LspFeature::Completion).active(DocId(1), 0));
    }

    #[test]
    fn is_cleared_for_every_feature_when_a_server_becomes_ready() {
        let mut backoffs = FeatureBackoffs::default();
        backoffs.feature(LspFeature::Completion).mark(DocId(1), 0);
        backoffs.feature(LspFeature::CodeLens).mark(DocId(2), 0);
        backoffs.server_status(ServerStatus::Starting);
        assert!(backoffs.feature(LspFeature::Completion).active(DocId(1), 0));
        backoffs.server_status(ServerStatus::Ready);
        assert!(!backoffs.feature(LspFeature::Completion).active(DocId(1), 0));
        assert!(!backoffs.feature(LspFeature::CodeLens).active(DocId(2), 0));
    }
}
