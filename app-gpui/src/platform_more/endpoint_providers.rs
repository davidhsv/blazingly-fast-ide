//! Endpoint providers: framework support that lists a project's request handlers for Search
//! Everywhere and the Endpoints view.

use super::registry::{Registration, Registry};
use crate::everywhere::routes::EndpointEntry;

/// Something that knows the endpoints of a project.
pub trait EndpointProvider {
    /// The endpoints of the project at `root`; `Err` when they cannot be told now.
    fn endpoints(&self, root: &str) -> Result<Vec<EndpointEntry>, String>;
}

/// The registered endpoint providers.
#[derive(Debug, Clone)]
pub struct EndpointProviders<P> {
    providers: Registry<P>,
    revision: u64,
}

impl<P> Default for EndpointProviders<P> {
    fn default() -> Self {
        Self { providers: Registry::new(), revision: 0 }
    }
}

impl<P: EndpointProvider> EndpointProviders<P> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `provider`, replacing a provider of the same id.
    pub fn register(&mut self, id: &str, provider: P) -> Registration {
        self.revision += 1;
        self.providers.register(id, provider)
    }

    /// Removes the provider `registration` registered, unless it was replaced since.
    pub fn unregister(&mut self, registration: Registration) -> bool {
        let removed = self.providers.unregister(registration);
        if removed {
            self.revision += 1;
        }
        removed
    }

    /// A provider's endpoints may have changed: what lists them asks again.
    pub fn endpoints_changed(&mut self) {
        self.revision += 1;
    }

    /// Grows whenever a list of endpoints taken earlier may be out of date.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The endpoints every provider reports for `root`, in registration order. Without a project
    /// no provider is asked; a provider that fails contributes nothing.
    pub fn search(&self, root: Option<&str>) -> Vec<EndpointEntry> {
        let Some(root) = root.filter(|root| !root.is_empty()) else {
            return Vec::new();
        };
        self.providers.values().flat_map(|provider| provider.endpoints(root).unwrap_or_default()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Fixed {
        entries: Vec<EndpointEntry>,
        asked: Rc<RefCell<Vec<String>>>,
    }

    impl EndpointProvider for Fixed {
        fn endpoints(&self, root: &str) -> Result<Vec<EndpointEntry>, String> {
            self.asked.borrow_mut().push(root.to_owned());
            Ok(self.entries.clone())
        }
    }

    fn provider(entries: &[EndpointEntry]) -> (Fixed, Rc<RefCell<Vec<String>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        (Fixed { entries: entries.to_vec(), asked: asked.clone() }, asked)
    }

    fn endpoint() -> EndpointEntry {
        EndpointEntry {
            method: "GET".to_owned(),
            url: "/menu/list".to_owned(),
            controller: "MenuController".to_owned(),
            method_name: "listMenus".to_owned(),
            path: "/p/MenuController.java".to_owned(),
            rel: "MenuController.java".to_owned(),
            line: 4,
            col: 11,
            ..EndpointEntry::default()
        }
    }

    // collects endpoints from all providers for the requested root
    #[test]
    fn collects_endpoints_from_all_providers_for_the_requested_root() {
        let other = EndpointEntry { url: "/other".to_owned(), ..endpoint() };
        let (first, first_asked) = provider(&[endpoint()]);
        let (second, second_asked) = provider(std::slice::from_ref(&other));
        let mut providers = EndpointProviders::new();
        let registrations = [providers.register("test.first", first), providers.register("test.second", second)];
        assert!(providers.search(None).is_empty());
        assert!(first_asked.borrow().is_empty());
        assert!(second_asked.borrow().is_empty());
        assert_eq!(providers.search(Some("/p")), [endpoint(), other]);
        assert_eq!(*first_asked.borrow(), ["/p"]);
        assert_eq!(*second_asked.borrow(), ["/p"]);
        for registration in registrations {
            assert!(providers.unregister(registration));
        }
        assert!(providers.search(Some("/p")).is_empty());
    }

    // keeps a replacement provider registered when its predecessor is disposed
    #[test]
    fn keeps_a_replacement_provider_registered_when_its_predecessor_is_disposed() {
        let mut providers = EndpointProviders::new();
        let old = providers.register("test.replaced", provider(&[]).0);
        let new = providers.register("test.replaced", provider(&[endpoint()]).0);
        assert!(!providers.unregister(old));
        assert_eq!(providers.search(Some("/p")), [endpoint()]);
        assert!(!providers.unregister(old));
        assert!(providers.unregister(new));
        assert!(providers.search(Some("/p")).is_empty());
    }

    // Not from a spec: a provider that fails contributes nothing, and every change of the
    // providers moves the revision.
    #[test]
    fn a_failing_provider_contributes_nothing_and_changes_move_the_revision() {
        struct Failing;
        impl EndpointProvider for Failing {
            fn endpoints(&self, _root: &str) -> Result<Vec<EndpointEntry>, String> {
                Err("no backend".to_owned())
            }
        }
        let mut failing = EndpointProviders::new();
        let registration = failing.register("test.failing", Failing);
        assert!(failing.search(Some("/p")).is_empty());
        assert_eq!(failing.revision(), 1);
        failing.endpoints_changed();
        assert!(failing.unregister(registration));
        assert!(!failing.unregister(registration));
        assert_eq!(failing.revision(), 3);
    }
}
