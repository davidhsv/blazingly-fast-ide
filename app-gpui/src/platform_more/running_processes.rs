//! Processes a closing window or project would terminate. What owns child processes (run and
//! debug sessions) publishes them here, so that the close guard can ask first.

use super::registry::{Registration, Registry};
use std::collections::HashSet;

/// A live process, by the configuration or session name the prompt shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub name: String,
}

/// Something that runs processes in the window.
pub trait RunningProcessesProvider {
    /// The processes it is running right now.
    fn running(&self) -> Vec<RunningProcess>;

    /// Stops them before a close: gracefully first, the whole set again when `kill` asks for
    /// them to be killed. A provider whose processes end with it has nothing to do.
    fn stop(&self, _kill: bool) {}
}

/// The registered providers of running processes.
#[derive(Debug, Clone)]
pub struct RunningProcessesProviders<P> {
    providers: Registry<P>,
}

impl<P> Default for RunningProcessesProviders<P> {
    fn default() -> Self {
        Self { providers: Registry::new() }
    }
}

impl<P: RunningProcessesProvider> RunningProcessesProviders<P> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `provider`, replacing a provider of the same id.
    pub fn register(&mut self, id: &str, provider: P) -> Registration {
        self.providers.register(id, provider)
    }

    /// Removes the provider `registration` registered, unless it was replaced since.
    pub fn unregister(&mut self, registration: Registration) -> bool {
        self.providers.unregister(registration)
    }

    /// The live processes of every provider, one entry per name, nameless ones left out.
    pub fn running(&self) -> Vec<RunningProcess> {
        let mut seen = HashSet::new();
        self.providers
            .values()
            .flat_map(RunningProcessesProvider::running)
            .filter(|process| !process.name.is_empty() && seen.insert(process.name.clone()))
            .collect()
    }

    /// Asks every provider to stop its processes: gracefully, or killed when `kill` is set.
    pub fn stop(&self, kill: bool) {
        for provider in self.providers.values() {
            provider.stop(kill);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Default)]
    struct Calls {
        running: Cell<u32>,
        stops: RefCell<Vec<bool>>,
    }

    struct Fixed {
        names: Vec<&'static str>,
        calls: Rc<Calls>,
    }

    impl RunningProcessesProvider for Fixed {
        fn running(&self) -> Vec<RunningProcess> {
            self.calls.running.set(self.calls.running.get() + 1);
            self.names.iter().map(|&name| process(name)).collect()
        }

        fn stop(&self, kill: bool) {
            self.calls.stops.borrow_mut().push(kill);
        }
    }

    fn process(name: &str) -> RunningProcess {
        RunningProcess { name: name.to_owned() }
    }

    fn provider(names: &[&'static str]) -> (Fixed, Rc<Calls>) {
        let calls = Rc::new(Calls::default());
        (Fixed { names: names.to_vec(), calls: calls.clone() }, calls)
    }

    // collects live processes from every provider, one entry per name
    #[test]
    fn collects_live_processes_from_every_provider_one_entry_per_name() {
        let (first, first_calls) = provider(&["app", "worker"]);
        let (second, second_calls) = provider(&["app", "docs"]);
        let mut providers = RunningProcessesProviders::new();
        let registrations = [providers.register("test.first", first), providers.register("test.second", second)];
        assert_eq!(providers.running(), [process("app"), process("worker"), process("docs")]);
        assert_eq!(first_calls.running.get(), 1);
        assert_eq!(second_calls.running.get(), 1);
        for registration in registrations {
            assert!(providers.unregister(registration));
        }
        assert!(providers.running().is_empty());
    }

    // keeps a replacement provider registered when its predecessor is disposed
    #[test]
    fn keeps_a_replacement_provider_registered_when_its_predecessor_is_disposed() {
        let mut providers = RunningProcessesProviders::new();
        let old = providers.register("test.replaced", provider(&[]).0);
        let new = providers.register("test.replaced", provider(&["app"]).0);
        assert!(!providers.unregister(old));
        assert_eq!(providers.running(), [process("app")]);
        assert!(!providers.unregister(old));
        assert!(providers.unregister(new));
        assert!(providers.running().is_empty());
    }

    // asks every provider to stop, gracefully and then killed
    #[test]
    fn asks_every_provider_to_stop_gracefully_and_then_killed() {
        let (stoppable, calls) = provider(&["app"]);
        let mut providers = RunningProcessesProviders::new();
        providers.register("test.stop", stoppable);
        providers.stop(false);
        assert_eq!(calls.stops.borrow().last(), Some(&false));
        providers.stop(true);
        assert_eq!(calls.stops.borrow().last(), Some(&true));
        assert_eq!(calls.stops.borrow().len(), 2);
    }
}
