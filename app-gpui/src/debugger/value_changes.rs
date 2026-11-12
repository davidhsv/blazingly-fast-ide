//! Values changed since the previous stop, which the variables tree paints in a distinct colour.
//!
//! Values are remembered per scope (a thread and a frame depth, say), so switching frames doesn't
//! compare unrelated locals.

use std::collections::HashMap;

/// How many scopes are remembered; the oldest goes first.
const MAX_SCOPES: usize = 64;

#[derive(Debug, Clone, Default)]
struct ScopeValues {
    epoch: u64,
    /// Values of the stop before the current one: the baseline.
    prev: HashMap<String, String>,
    /// Values seen in the current stop.
    cur: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ValueChanges {
    /// Scopes in the order they were first entered.
    scopes: Vec<(String, ScopeValues)>,
}

impl ValueChanges {
    fn scope(&self, scope: &str) -> Option<&ScopeValues> {
        self.scopes.iter().find(|(name, _)| name == scope).map(|(_, values)| values)
    }

    /// Starts `epoch` for `scope`: the values seen in the earlier epoch become the baseline.
    pub fn enter(&mut self, scope: &str, epoch: u64) {
        match self.scopes.iter_mut().find(|(name, _)| name == scope) {
            None => {
                if self.scopes.len() >= MAX_SCOPES {
                    self.scopes.remove(0);
                }
                self.scopes.push((scope.to_owned(), ScopeValues { epoch, ..Default::default() }));
            }
            Some((_, values)) if values.epoch != epoch => {
                let seen = std::mem::take(&mut values.cur);
                // A stop that showed nothing of this scope keeps the baseline it had.
                if !seen.is_empty() {
                    values.prev = seen;
                }
                values.epoch = epoch;
            }
            Some(_) => {}
        }
    }

    /// Remembers the value shown for `key` in the current epoch of `scope`.
    pub fn record(&mut self, scope: &str, key: &str, value: &str) {
        if let Some((_, values)) = self.scopes.iter_mut().find(|(name, _)| name == scope) {
            values.cur.insert(key.to_owned(), value.to_owned());
        }
    }

    /// Whether `value` differs from what `key` had at the previous stop. A value that was not
    /// there before is new, not changed.
    pub fn changed(&self, scope: &str, key: &str, value: &str) -> bool {
        self.scope(scope).and_then(|values| values.prev.get(key)).is_some_and(|prev| prev != value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_with_the_previous_stop_in_the_same_scope_only() {
        let mut c = ValueChanges::default();
        c.enter("t1:0", 1);
        c.record("t1:0", "local:i", "1");
        c.record("t1:0", "local:s", "'a'");
        assert!(!c.changed("t1:0", "local:i", "1"));

        c.enter("t1:0", 2);
        assert!(c.changed("t1:0", "local:i", "2"));
        assert!(!c.changed("t1:0", "local:s", "'a'"));
        // New variables aren't "changed".
        assert!(!c.changed("t1:0", "local:n", "0"));
        // Another frame depth has its own baseline.
        assert!(!c.changed("t1:1", "local:i", "2"));

        // Re-rendering in the same stop keeps the baseline.
        c.record("t1:0", "local:i", "2");
        c.enter("t1:0", 2);
        assert!(c.changed("t1:0", "local:i", "2"));
        c.enter("t1:0", 3);
        assert!(!c.changed("t1:0", "local:i", "2"));
    }

    // Not from a spec: the oldest scope is forgotten once more than the cap are remembered.
    #[test]
    fn forgets_the_oldest_scope_past_the_cap() {
        let mut c = ValueChanges::default();
        for i in 0..=MAX_SCOPES {
            let scope = format!("t{i}");
            c.enter(&scope, 1);
            c.record(&scope, "x", "1");
            c.enter(&scope, 2);
        }
        assert!(!c.changed("t0", "x", "2"));
        assert!(c.changed("t1", "x", "2"));
    }
}
