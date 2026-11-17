//! A start of a run that takes as long as a test wants: a test holds the start of a program (named by
//! its command, a stand-in in the test's own temporary folder) either before the process host is asked
//! ([`GatePoint::BeforeStart`], as a run target's validation keeps a start waiting) or after the host
//! started the process and before its answer reaches the window ([`GatePoint::AfterStart`], an answer
//! that arrives late). The test lets it go with [`StartGate::release`].
//!
//! The gate is keyed by command, so tests running side by side, each with programs in its own folder,
//! never hold each other's starts.

use futures::channel::oneshot;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

/// Where a held start waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatePoint {
    /// Before the process host is asked: no process exists yet.
    BeforeStart,
    /// After the process host started the process, before its answer is handed back.
    AfterStart,
}

/// What the start of one command reports to its test, and what it waits on.
struct Held {
    point: GatePoint,
    /// The start reached the point and waits there.
    reached: Arc<AtomicBool>,
    /// The process host answered the start.
    answered: Arc<AtomicBool>,
    /// Taken by the start when it reaches the point.
    release: Option<oneshot::Receiver<()>>,
}

static HELD: LazyLock<Mutex<HashMap<String, Held>>> = LazyLock::new(Mutex::default);

/// The held starts; a test that panicked while holding the lock leaves the map usable for the others.
fn held() -> MutexGuard<'static, HashMap<String, Held>> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A start a test holds. Dropping it lets the start go and forgets the command.
pub struct StartGate {
    command: String,
    reached: Arc<AtomicBool>,
    answered: Arc<AtomicBool>,
    release: Option<oneshot::Sender<()>>,
}

impl StartGate {
    /// Holds the next start of `command` at `point`.
    pub fn hold(command: &str, point: GatePoint) -> Self {
        let (release, waits) = oneshot::channel();
        let (reached, answered) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let entry =
            Held { point, reached: Arc::clone(&reached), answered: Arc::clone(&answered), release: Some(waits) };
        held().insert(command.to_owned(), entry);
        Self { command: command.to_owned(), reached, answered, release: Some(release) }
    }

    /// Whether the start waits at the gate's point.
    pub fn reached(&self) -> bool {
        self.reached.load(Ordering::SeqCst)
    }

    /// Whether the process host has answered the start (with a process or an error).
    pub fn answered(&self) -> bool {
        self.answered.load(Ordering::SeqCst)
    }

    /// Lets the held start go on.
    pub fn release(&mut self) {
        if let Some(release) = self.release.take() {
            // The start is gone only when its window closed; nothing waits then.
            let _ = release.send(());
        }
    }
}

impl Drop for StartGate {
    fn drop(&mut self) {
        self.release();
        held().remove(&self.command);
    }
}

/// Waits at `point` when a test holds the start of `command` there; returns at once otherwise.
async fn wait_at(command: &str, point: GatePoint) {
    let waits = {
        let mut held = held();
        let Some(entry) = held.get_mut(command).filter(|entry| entry.point == point) else { return };
        entry.reached.store(true, Ordering::SeqCst);
        entry.release.take()
    };
    if let Some(waits) = waits {
        // A dropped gate lets the start go as a release does.
        let _ = waits.await;
    }
}

/// The start of `command` (`start`, not yet polled), held where a test holds it: before `start` runs, or
/// with its answer, which is handed back once the test lets it go.
pub async fn gated<T>(command: String, start: impl Future<Output = T>) -> T {
    wait_at(&command, GatePoint::BeforeStart).await;
    let answer = start.await;
    if let Some(entry) = held().get(&command) {
        entry.answered.store(true, Ordering::SeqCst);
    }
    wait_at(&command, GatePoint::AfterStart).await;
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_start_waits_at_its_point_until_released_and_others_pass() {
        let mut gate = StartGate::hold("/held/program", GatePoint::BeforeStart);
        assert_eq!(futures::executor::block_on(gated("/other/program".to_owned(), async { 1 })), 1);
        assert!(!gate.reached(), "another program's start passes");
        let waiting =
            std::thread::spawn(|| futures::executor::block_on(gated("/held/program".to_owned(), async { 2 })));
        while !gate.reached() {
            std::thread::yield_now();
        }
        assert!(!waiting.is_finished() && !gate.answered(), "the start waits for the test before it runs");
        gate.release();
        assert_eq!(waiting.join().expect("the start goes on once released"), 2);
        assert!(gate.answered());
        drop(gate);
        assert!(!held().contains_key("/held/program"));
    }

    #[test]
    fn an_answer_held_after_the_start_is_handed_back_once_released() {
        let mut gate = StartGate::hold("/late/program", GatePoint::AfterStart);
        let waiting =
            std::thread::spawn(|| futures::executor::block_on(gated("/late/program".to_owned(), async { 3 })));
        while !gate.reached() {
            std::thread::yield_now();
        }
        assert!(gate.answered() && !waiting.is_finished(), "the process host answered; the answer waits");
        gate.release();
        assert_eq!(waiting.join().expect("the answer is handed back"), 3);
    }
}
