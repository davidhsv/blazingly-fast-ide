//! Contributions registered under an id: a later registration of the same id replaces the
//! earlier one, and giving a registration back removes only what that registration put there.

/// Handle of one [`Registry::register`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Registration(u64);

#[derive(Debug, Clone)]
struct Entry<T> {
    serial: u64,
    id: String,
    value: T,
}

/// Values by id, in the order their ids were first registered.
#[derive(Debug, Clone)]
pub struct Registry<T> {
    entries: Vec<Entry<T>>,
    next_serial: u64,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self { entries: Vec::new(), next_serial: 0 }
    }
}

impl<T> Registry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `value` under `id`, replacing a value of the same id (which keeps its place).
    pub fn register(&mut self, id: &str, value: T) -> Registration {
        self.next_serial += 1;
        let entry = Entry { serial: self.next_serial, id: id.to_owned(), value };
        match self.entries.iter().position(|existing| existing.id == entry.id) {
            Some(index) => self.entries[index] = entry,
            None => self.entries.push(entry),
        }
        Registration(self.next_serial)
    }

    /// Removes what `registration` registered, unless a later registration of the same id
    /// replaced it. Returns whether anything was removed.
    pub fn unregister(&mut self, registration: Registration) -> bool {
        let count = self.entries.len();
        self.entries.retain(|entry| entry.serial != registration.0);
        self.entries.len() != count
    }

    pub fn get(&self, id: &str) -> Option<&T> {
        self.entries.iter().find(|entry| entry.id == id).map(|entry| &entry.value)
    }

    /// The registered ids and values, in registration order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.entries.iter().map(|entry| (entry.id.as_str(), &entry.value))
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|entry| &entry.value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a replaced id keeps its place, and the replaced registration no longer
    // removes anything.
    #[test]
    fn a_later_registration_replaces_the_earlier_one_in_place() {
        let mut registry = Registry::new();
        let first = registry.register("a", 1);
        registry.register("b", 2);
        let second = registry.register("a", 3);
        assert_eq!(registry.iter().collect::<Vec<_>>(), [("a", &3), ("b", &2)]);
        assert!(!registry.unregister(first));
        assert_eq!(registry.get("a"), Some(&3));
        assert!(registry.unregister(second));
        assert_eq!(registry.values().copied().collect::<Vec<_>>(), [2]);
        assert_eq!((registry.len(), registry.is_empty()), (1, false));
    }
}
