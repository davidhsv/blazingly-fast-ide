//! Leases on backend resources that several editor resolutions may share (a database session,
//! a notebook kernel), held from the moment a resolver opens one until a tab owns it.
//!
//! A resource is identified by its incarnation id. Opening one takes a lease; a tab that is
//! admitted keeps the resource, an abandoned resolution gives its lease back, and the resource
//! is closed once nothing leases or owns it. The pool decides; the caller opens and closes and
//! reports back:
//!
//! 1. [`ProviderAcquisitions::begin_open`], then open the resource, then
//!    [`ProviderAcquisitions::opened`] or [`ProviderAcquisitions::open_failed`];
//! 2. [`ProviderAcquisitions::admit`] or [`ProviderAcquisitions::rollback`] with the lease;
//! 3. close what the pool asks to close and report [`ProviderAcquisitions::closed`].

use std::collections::HashMap;

/// One acquisition of a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lease(u64);

/// What giving a lease back came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Release {
    /// Nothing to close: the resource is still leased or owned, or the lease was settled before.
    Kept,
    /// Opens are in flight and one of them may lease this resource: decided when the last of
    /// them reports, in what [`ProviderAcquisitions::opened`] or
    /// [`ProviderAcquisitions::open_failed`] return.
    Deferred,
    /// Close the resource of this id, then report [`ProviderAcquisitions::closed`].
    Close(String),
    /// A close of the resource is already in flight.
    Closing,
}

#[derive(Debug, Clone, Default)]
struct Resource {
    leases: usize,
    closing: bool,
}

/// Whether an admitted session holds the resource of an id on its own account.
pub type Owned<'a> = &'a dyn Fn(&str) -> bool;

/// The leases on the resources of one provider.
#[derive(Debug, Clone, Default)]
pub struct ProviderAcquisitions {
    /// Opens in flight.
    pending: usize,
    /// Closes in flight.
    closing: usize,
    resources: HashMap<String, Resource>,
    /// Leases not settled yet, with the id of their resource.
    live: HashMap<u64, String>,
    /// Resources of abandoned leases whose fate waits for the opens in flight.
    deferred: Vec<String>,
    next_serial: u64,
}

impl ProviderAcquisitions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an open may start now. While a close is in flight it may not (the open could
    /// be handed the incarnation that is going away): ask again after
    /// [`ProviderAcquisitions::closed`]. `true` counts the open as in flight.
    pub fn begin_open(&mut self) -> bool {
        if self.closing > 0 {
            return false;
        }
        self.pending += 1;
        true
    }

    /// An open that [`ProviderAcquisitions::begin_open`] allowed produced the resource `id`.
    /// Returns its lease and the resources to close now.
    pub fn opened(&mut self, id: &str, owned: Owned<'_>) -> (Lease, Vec<String>) {
        self.resources.entry(id.to_owned()).or_default().leases += 1;
        self.next_serial += 1;
        self.live.insert(self.next_serial, id.to_owned());
        (Lease(self.next_serial), self.open_finished(owned))
    }

    /// An open that [`ProviderAcquisitions::begin_open`] allowed failed. Returns the resources
    /// to close now.
    pub fn open_failed(&mut self, owned: Owned<'_>) -> Vec<String> {
        self.open_finished(owned)
    }

    fn open_finished(&mut self, owned: Owned<'_>) -> Vec<String> {
        self.pending = self.pending.saturating_sub(1);
        if self.pending > 0 {
            return Vec::new();
        }
        let deferred = std::mem::take(&mut self.deferred);
        deferred
            .iter()
            .filter_map(|id| match self.settle(id, false, owned) {
                Release::Close(id) => Some(id),
                Release::Kept | Release::Deferred | Release::Closing => None,
            })
            .collect()
    }

    /// The id of the resource `lease` holds, until the lease is settled.
    pub fn resource(&self, lease: Lease) -> Option<&str> {
        self.live.get(&lease.0).map(String::as_str)
    }

    /// A tab took the resource over: the lease ends and the resource stays open.
    pub fn admit(&mut self, lease: Lease, owned: Owned<'_>) {
        if let Some(id) = self.end_lease(lease) {
            self.settle(&id, true, owned);
        }
    }

    /// The resolution was abandoned: the lease ends, and the resource is closed unless
    /// something else leases or owns it. Giving a lease back twice changes nothing.
    pub fn rollback(&mut self, lease: Lease, owned: Owned<'_>) -> Release {
        let Some(id) = self.end_lease(lease) else {
            return Release::Kept;
        };
        if self.pending > 0 {
            self.deferred.push(id);
            return Release::Deferred;
        }
        self.settle(&id, false, owned)
    }

    /// The close of `id` that the pool asked for has finished.
    pub fn closed(&mut self, id: &str) {
        if self.resources.get(id).is_some_and(|resource| resource.closing) {
            self.resources.remove(id);
            self.closing = self.closing.saturating_sub(1);
        }
    }

    fn end_lease(&mut self, lease: Lease) -> Option<String> {
        let id = self.live.remove(&lease.0)?;
        if let Some(resource) = self.resources.get_mut(&id) {
            resource.leases = resource.leases.saturating_sub(1);
        }
        Some(id)
    }

    /// Decides the fate of a resource one lease just left.
    fn settle(&mut self, id: &str, admitted: bool, owned: Owned<'_>) -> Release {
        if self.resources.get(id).is_some_and(|resource| resource.leases > 0) {
            return Release::Kept;
        }
        // A session that was admitted, this one or an earlier one, holds the resource by itself.
        if admitted || owned(id) {
            if self.resources.get(id).is_some_and(|resource| !resource.closing) {
                self.resources.remove(id);
            }
            return Release::Kept;
        }
        let resource = self.resources.entry(id.to_owned()).or_default();
        if resource.closing {
            return Release::Closing;
        }
        resource.closing = true;
        self.closing += 1;
        Release::Close(id.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Opens `id` at once and returns its lease; nothing may need closing.
    fn acquire(pool: &mut ProviderAcquisitions, id: &str, owned: Owned<'_>) -> Lease {
        assert!(pool.begin_open());
        let (lease, to_close) = pool.opened(id, owned);
        assert!(to_close.is_empty());
        lease
    }

    // provider incarnation ownership › does not close an admitted alias when another resolution is abandoned
    #[test]
    fn does_not_close_an_admitted_alias_when_another_resolution_is_abandoned() {
        let mut owned: HashSet<&str> = HashSet::new();
        let mut pool = ProviderAcquisitions::new();
        let first = acquire(&mut pool, "shared", &|id| owned.contains(id));
        // A second resolution starts opening the same backend and has not heard back yet.
        assert!(pool.begin_open());
        assert_eq!(pool.rollback(first, &|id| owned.contains(id)), Release::Deferred);
        let (second, to_close) = pool.opened("shared", &|id| owned.contains(id));
        // The unresolved acquisition now owns a lease, even before tab admission.
        assert!(to_close.is_empty());
        owned.insert("shared");
        pool.admit(second, &|id| owned.contains(id));
        assert!(pool.begin_open(), "no close is in flight");
    }

    // provider incarnation ownership › releases a shared backend once when every alias is abandoned
    #[test]
    fn releases_a_shared_backend_once_when_every_alias_is_abandoned() {
        let unowned: Owned<'_> = &|_| false;
        let mut pool = ProviderAcquisitions::new();
        let a = acquire(&mut pool, "shared", unowned);
        let b = acquire(&mut pool, "shared", unowned);
        let mut closed = Vec::new();
        for lease in [a, b, a] {
            if let Release::Close(id) = pool.rollback(lease, unowned) {
                pool.closed(&id);
                closed.push(id);
            }
        }
        assert_eq!(closed, ["shared"]);
    }

    // provider incarnation ownership › never releases an existing session or a replacement incarnation
    #[test]
    fn never_releases_an_existing_session_or_a_replacement_incarnation() {
        let mut owned = HashSet::from(["existing"]);
        let mut pool = ProviderAcquisitions::new();
        let existing = acquire(&mut pool, "existing", &|id| owned.contains(id));
        assert_eq!(pool.rollback(existing, &|id| owned.contains(id)), Release::Kept);
        let old = acquire(&mut pool, "old-project", &|id| owned.contains(id));
        owned.insert("new-project");
        let replacement = acquire(&mut pool, "new-project", &|id| owned.contains(id));
        pool.admit(replacement, &|id| owned.contains(id));
        assert_eq!(pool.rollback(old, &|id| owned.contains(id)), Release::Close("old-project".to_owned()));
        assert_eq!(owned, HashSet::from(["existing", "new-project"]));
    }

    // provider incarnation ownership › waits for abandoned cleanup before reopening the backend path
    #[test]
    fn waits_for_abandoned_cleanup_before_reopening_the_backend_path() {
        let unowned: Owned<'_> = &|_| false;
        let mut pool = ProviderAcquisitions::new();
        let old = acquire(&mut pool, "old", unowned);
        assert_eq!(pool.rollback(old, unowned), Release::Close("old".to_owned()));
        // The backend still serves the old incarnation while its close is in flight.
        assert!(!pool.begin_open());
        pool.closed("old");
        let fresh = acquire(&mut pool, "new", unowned);
        assert_eq!(pool.resource(fresh), Some("new"));
        pool.admit(fresh, unowned);
        assert_eq!(pool.resource(fresh), None);
    }

    // Not from a spec: an abandoned lease waits for every open in flight, a failed one included,
    // and a second abandoned alias joins the close that is already in flight.
    #[test]
    fn an_abandoned_lease_is_decided_when_the_last_open_reports() {
        let unowned: Owned<'_> = &|_| false;
        let mut pool = ProviderAcquisitions::new();
        let a = acquire(&mut pool, "shared", unowned);
        let b = acquire(&mut pool, "shared", unowned);
        assert!(pool.begin_open());
        assert!(pool.begin_open());
        assert_eq!(pool.rollback(a, unowned), Release::Deferred);
        assert_eq!(pool.rollback(b, unowned), Release::Deferred);
        assert!(pool.open_failed(unowned).is_empty());
        assert_eq!(pool.open_failed(unowned), ["shared"]);
        assert!(!pool.begin_open());
        pool.closed("shared");
        assert!(pool.begin_open());
    }
}
