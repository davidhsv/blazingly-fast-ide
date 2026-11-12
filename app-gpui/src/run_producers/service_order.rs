//! The display order of the Services tool window's run configurations, kept per project and
//! independent of the detected or saved configurations themselves.

use crate::run::store::StorageWrite;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// The ids of each kind's configurations, in display order.
pub type ServiceOrder = BTreeMap<String, Vec<String>>;

/// Storage key prefix; the project's location follows.
pub const STORAGE_KEY: &str = "openidea.services.runOrder:";

/// The stored order: malformed records are dropped, and so are ids that are empty or repeated.
pub fn parse_service_order(stored: Option<&str>) -> ServiceOrder {
    let Some(Value::Object(kinds)) = stored.and_then(|stored| serde_json::from_str(stored).ok()) else {
        return ServiceOrder::new();
    };
    let ids_of = |ids: &Vec<Value>| {
        let mut out: Vec<String> = Vec::new();
        for id in ids.iter().filter_map(Value::as_str).filter(|id| !id.is_empty()) {
            if !out.iter().any(|known| known == id) {
                out.push(id.to_string());
            }
        }
        out
    };
    kinds.iter().filter_map(|(kind, ids)| Some((kind.clone(), ids_of(ids.as_array()?)))).collect()
}

/// `entries` in the saved order of their ids; the ones it does not list follow, in their own order.
pub fn order_services<T: Clone>(entries: &[T], saved: &[String], id: impl Fn(&T) -> &str) -> Vec<T> {
    let rank = |entry: &T| saved.iter().position(|known| known == id(entry)).unwrap_or(saved.len());
    let mut ordered = entries.to_vec();
    ordered.sort_by_key(rank);
    ordered
}

/// Where a dragged service is dropped, relative to its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPosition {
    Before,
    After,
}

/// The saved order after moving `source` next to `target`. `ids`: the services shown now; saved ids
/// that are not shown (temporarily hidden) keep their place.
pub fn move_service_order(
    saved: &[String],
    ids: &[String],
    source: &str,
    target: &str,
    position: DropPosition,
) -> Vec<String> {
    let shown = |id: &str| ids.iter().any(|known| known == id);
    if source == target || !shown(source) || !shown(target) {
        return saved.to_vec();
    }
    let mut order: Vec<String> = Vec::new();
    for id in saved.iter().chain(ids).filter(|id| *id != source) {
        if !order.contains(id) {
            order.push(id.clone());
        }
    }
    let at = order.iter().position(|id| id == target).map_or(0, |at| at + usize::from(position == DropPosition::After));
    order.insert(at, source.to_string());
    order
}

/// The service orders of this window: the one its view shows, and those not written to storage yet.
#[derive(Clone, Debug, Default)]
pub struct ServiceOrders {
    /// Orders saved here whose write has not been confirmed, by project location.
    unsaved: HashMap<String, ServiceOrder>,
    /// The project the view shows, with its order.
    shown: Option<(String, ServiceOrder)>,
}

impl ServiceOrders {
    /// The order of the project at `location`; `stored` is what storage holds for it (none when
    /// nothing is stored or storage cannot be read: the default order then).
    pub fn read(&self, location: &str, stored: Option<&str>) -> ServiceOrder {
        self.unsaved.get(location).cloned().unwrap_or_else(|| parse_service_order(stored))
    }

    /// The order the view of `location` shows.
    pub fn current(&self, location: &str, stored: Option<&str>) -> ServiceOrder {
        match &self.shown {
            Some((shown, order)) if shown == location => order.clone(),
            _ => self.read(location, stored),
        }
    }

    /// The view shows the project at `location` (none: no project), reading its order again.
    pub fn show(&mut self, location: Option<&str>, stored: Option<&str>) {
        self.shown = location.map(|location| (location.to_string(), self.read(location, stored)));
    }

    /// Saves the order of the project at `location`: the view shows it at once, and it stays in
    /// memory until the returned write is confirmed with [`Self::written`].
    pub fn save(&mut self, location: &str, order: ServiceOrder) -> StorageWrite {
        let value = serde_json::to_string(&order).ok();
        self.unsaved.insert(location.to_string(), order.clone());
        self.shown = Some((location.to_string(), order));
        StorageWrite { key: format!("{STORAGE_KEY}{location}"), value }
    }

    /// The write of [`Self::save`] reached storage.
    pub fn written(&mut self, location: &str) {
        self.unsaved.remove(location);
    }

    /// Storage changed under `key` (`None`: it was cleared), here or in another window: the view of
    /// that project reads its order again, from `stored`.
    pub fn storage_changed(&mut self, key: Option<&str>, stored: Option<&str>) {
        let Some((location, _)) = &self.shown else { return };
        let location = location.clone();
        if key.is_none_or(|key| key.strip_prefix(STORAGE_KEY) == Some(location.as_str())) {
            self.unsaved.remove(&location);
            self.show(Some(&location), stored);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn order(kinds: &[(&str, &[&str])]) -> ServiceOrder {
        kinds.iter().map(|(kind, own)| (kind.to_string(), ids(own))).collect()
    }

    /// What storage holds after the writes made so far.
    #[derive(Default)]
    struct Storage(BTreeMap<String, String>);

    impl Storage {
        fn write(&mut self, write: StorageWrite) {
            match write.value {
                Some(value) => self.0.insert(write.key, value),
                None => self.0.remove(&write.key),
            };
        }

        fn of(&self, location: &str) -> Option<&str> {
            self.0.get(&format!("{STORAGE_KEY}{location}")).map(String::as_str)
        }
    }

    #[test]
    fn orders_known_ids_appends_new_ones_and_leaves_configurations_untouched() {
        let entries = ["gateway", "portal", "new"];
        let ordered = order_services(&entries, &ids(&["gone", "portal", "gateway"]), |entry| *entry);
        assert_eq!(ordered, ["portal", "gateway", "new"]);
        assert_eq!(entries, ["gateway", "portal", "new"]);
    }

    #[test]
    fn moves_before_or_after_the_target_while_retaining_temporarily_hidden_ids() {
        let saved = ids(&["a", "hidden", "b"]);
        let moved = move_service_order(&saved, &ids(&["a", "b", "new"]), "b", "a", DropPosition::Before);
        assert_eq!(moved, ["b", "a", "hidden", "new"]);
        let moved = move_service_order(&[], &ids(&["a", "b", "c"]), "a", "b", DropPosition::After);
        assert_eq!(moved, ["b", "a", "c"]);
    }

    #[test]
    fn ignores_missing_self_and_stale_drop_targets() {
        for (source, target) in [("a", "a"), ("gone", "b"), ("a", "gone")] {
            let moved = move_service_order(&ids(&["b", "a"]), &ids(&["a", "b"]), source, target, DropPosition::Before);
            assert_eq!(moved, ["b", "a"], "{source} onto {target}");
        }
    }

    #[test]
    fn isolates_project_locations_and_normalizes_malformed_records_and_duplicate_ids() {
        let mut storage = Storage::default();
        let key = format!("{STORAGE_KEY}/app");
        storage.0.insert(key.clone(), r#"{"npm":["b","b",4,"","a"],"python":null}"#.to_string());
        let orders = ServiceOrders::default();
        assert_eq!(orders.read("/app", storage.of("/app")), order(&[("npm", &["b", "a"])]));
        assert!(orders.read("ssh://other/app", storage.of("ssh://other/app")).is_empty());
        for malformed in ["broken", "[]"] {
            storage.0.insert(key.clone(), malformed.to_string());
            assert!(orders.read("/app", storage.of("/app")).is_empty(), "{malformed}");
        }
    }

    #[test]
    fn continues_with_default_order_when_storage_cannot_be_read() {
        assert!(ServiceOrders::default().read("/denied", None).is_empty());
    }

    #[test]
    fn updates_mounted_views_survives_remounts_and_reads_changes_from_other_windows() {
        let (mut storage, mut orders) = (Storage::default(), ServiceOrders::default());
        orders.show(Some("/persist"), storage.of("/persist"));
        let saved = order(&[("npm", &["b", "a"])]);
        storage.write(orders.save("/persist", saved.clone()));
        orders.written("/persist");
        assert_eq!(orders.current("/persist", storage.of("/persist")), saved);
        // Another view of the project, opened later, reads what was stored.
        let mut next = ServiceOrders::default();
        next.show(Some("/persist"), storage.of("/persist"));
        assert_eq!(next.current("/persist", storage.of("/persist")), saved);
        // Another window stores a different order.
        let key = format!("{STORAGE_KEY}/persist");
        storage.0.insert(key.clone(), r#"{"npm":["a","b"]}"#.to_string());
        next.storage_changed(Some(&key), storage.of("/persist"));
        assert_eq!(next.current("/persist", None), order(&[("npm", &["a", "b"])]));
        storage.0.clear();
        next.storage_changed(None, storage.of("/persist"));
        assert!(next.current("/persist", None).is_empty());
    }

    #[test]
    fn switches_projects_without_leaking_the_previous_projects_order() {
        let (mut storage, mut orders) = (Storage::default(), ServiceOrders::default());
        let (one, two) = (order(&[("npm", &["b", "a"])]), order(&[("python", &["y", "x"])]));
        for (location, saved) in [("/one", &one), ("/two", &two)] {
            storage.write(orders.save(location, saved.clone()));
            orders.written(location);
        }
        orders.show(Some("/one"), storage.of("/one"));
        assert_eq!(orders.current("/one", storage.of("/one")), one);
        orders.show(Some("/two"), storage.of("/two"));
        assert_eq!(orders.current("/two", storage.of("/two")), two);
    }

    #[test]
    fn keeps_the_in_memory_order_when_writing_storage_fails() {
        let mut orders = ServiceOrders::default();
        orders.show(Some("/unavailable"), None);
        let saved = order(&[("npm", &["b", "a"])]);
        // The write never reaches storage.
        let write = orders.save("/unavailable", saved.clone());
        assert_eq!(write.key, format!("{STORAGE_KEY}/unavailable"));
        assert_eq!(orders.current("/unavailable", None), saved);
        // A view opened again still shows it.
        orders.show(None, None);
        orders.show(Some("/unavailable"), None);
        assert_eq!(orders.current("/unavailable", None), saved);
    }
}
