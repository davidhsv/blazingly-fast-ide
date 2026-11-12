//! Language server first, the syntax tree's structure service second: an editor feature asks the
//! server and switches to the structure service only when the server is not usable (`NotReady`:
//! no server, not running, starting), fails (`Error`), answers nothing (`Empty`, a refactoring
//! over code with syntax errors) or, opt-in, is too slow (`Slow`). Cancellation is never a reason
//! to fall back, and when the fallback fails too the server's outcome stands (its error, or its
//! empty answer).

use crate::editor_lsp::lsp::{is_cancelled, is_unavailable};
use ide_core::IdeError;

/// When the structure service answers instead of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackWhen {
    NotReady,
    Error,
    Empty,
    Slow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackOptions {
    pub when: Vec<FallbackWhen>,
    /// With [`FallbackWhen::Slow`]: ask the fallback once the server has not answered within
    /// this time.
    pub slow_ms: u64,
    /// Skip the server (a back-off after recent `unavailable` errors).
    pub skip_lsp: bool,
}

impl Default for FallbackOptions {
    fn default() -> Self {
        Self {
            when: vec![FallbackWhen::NotReady, FallbackWhen::Error, FallbackWhen::Empty],
            slow_ms: 1500,
            skip_lsp: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Lsp,
    Syntax,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer<T> {
    pub value: T,
    pub source: Source,
}

#[derive(Debug, Clone)]
pub enum FallbackStep<T> {
    /// Ask the server ([`WithFallback::lsp_answered`]); with `slow_timer`, start a timer of that
    /// many milliseconds and call [`WithFallback::slow_elapsed`] when it fires.
    AskLsp { slow_timer: Option<u64> },
    /// Ask the structure service ([`WithFallback::fallback_answered`]).
    AskFallback,
    /// The outcome; later answers change nothing.
    Done(Result<Answer<T>, IdeError>),
    /// Nothing to do until the next answer.
    Wait,
}

/// One feature request with its fallback.
pub struct WithFallback<T> {
    options: FallbackOptions,
    is_empty: Box<dyn Fn(&T) -> bool>,
    /// The server's outcome while the fallback it called for runs.
    lsp: Option<Result<T, IdeError>>,
    /// The fallback is running.
    asked: bool,
    /// The fallback answered nothing usable.
    fallback_failed: bool,
    done: bool,
}

impl<T> WithFallback<T> {
    /// Starts; `is_empty` tells an empty answer (an empty list, `None`).
    pub fn start(options: FallbackOptions, is_empty: impl Fn(&T) -> bool + 'static) -> (Self, FallbackStep<T>) {
        let step = if options.skip_lsp {
            FallbackStep::AskFallback
        } else {
            let slow = options.when.contains(&FallbackWhen::Slow).then_some(options.slow_ms);
            FallbackStep::AskLsp { slow_timer: slow }
        };
        let asked = options.skip_lsp;
        let flow =
            Self { options, is_empty: Box::new(is_empty), lsp: None, asked, fallback_failed: false, done: false };
        (flow, step)
    }

    fn finish(&mut self, outcome: Result<Answer<T>, IdeError>) -> FallbackStep<T> {
        self.done = true;
        FallbackStep::Done(outcome)
    }

    fn lsp_outcome(outcome: Result<T, IdeError>) -> Result<Answer<T>, IdeError> {
        outcome.map(|value| Answer { value, source: Source::Lsp })
    }

    /// The server's outcome stands unless the fallback answers.
    fn needs_fallback(&mut self, outcome: Result<T, IdeError>) -> FallbackStep<T> {
        if self.fallback_failed {
            return self.finish(Self::lsp_outcome(outcome));
        }
        self.lsp = Some(outcome);
        if self.asked {
            return FallbackStep::Wait;
        }
        self.asked = true;
        FallbackStep::AskFallback
    }

    pub fn lsp_answered(&mut self, result: Result<T, IdeError>) -> FallbackStep<T> {
        if self.done || self.options.skip_lsp {
            return FallbackStep::Wait;
        }
        match result {
            Err(error) => {
                let failure = if is_unavailable(&error) { FallbackWhen::NotReady } else { FallbackWhen::Error };
                if is_cancelled(&error) || !self.options.when.contains(&failure) {
                    return self.finish(Err(error));
                }
                self.needs_fallback(Err(error))
            }
            Ok(value) if self.options.when.contains(&FallbackWhen::Empty) && (self.is_empty)(&value) => {
                self.needs_fallback(Ok(value))
            }
            Ok(value) => self.finish(Self::lsp_outcome(Ok(value))),
        }
    }

    /// The slow timer fired: the fallback is asked while the server keeps going.
    pub fn slow_elapsed(&mut self) -> FallbackStep<T> {
        if self.done || self.asked || self.lsp.is_some() {
            return FallbackStep::Wait;
        }
        self.asked = true;
        FallbackStep::AskFallback
    }

    pub fn fallback_answered(&mut self, result: Result<T, IdeError>) -> FallbackStep<T> {
        if self.done {
            return FallbackStep::Wait;
        }
        self.asked = false;
        if self.options.skip_lsp {
            return self.finish(result.map(|value| Answer { value, source: Source::Syntax }));
        }
        match result {
            Ok(value) if !(self.is_empty)(&value) => self.finish(Ok(Answer { value, source: Source::Syntax })),
            Err(error) if is_cancelled(&error) && self.lsp.is_some() => self.finish(Err(error)),
            _ => {
                self.fallback_failed = true;
                match self.lsp.take() {
                    Some(outcome) => self.finish(Self::lsp_outcome(outcome)),
                    // The slow fallback found nothing: the server's answer is awaited.
                    None => FallbackStep::Wait,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{cancelled, unavailable};
    use super::*;
    use ide_core::error::ErrorKind;

    fn protocol() -> IdeError {
        IdeError::new(ErrorKind::Protocol, "boom")
    }

    type Outcome<T> = Result<Answer<T>, IdeError>;

    /// Runs a request whose server and fallback answer at once (the slow timer never fires).
    fn run<T: 'static>(
        options: FallbackOptions,
        is_empty: impl Fn(&T) -> bool + 'static,
        lsp: Result<T, IdeError>,
        fallback: Result<T, IdeError>,
    ) -> (Outcome<T>, bool) {
        let (mut flow, step) = WithFallback::start(options, is_empty);
        let (mut lsp, mut fallback) = (Some(lsp), Some(fallback));
        let mut asked_fallback = false;
        let mut step = match step {
            FallbackStep::AskLsp { .. } => flow.lsp_answered(lsp.take().expect("asked once")),
            other => other,
        };
        loop {
            step = match step {
                FallbackStep::AskFallback => {
                    asked_fallback = true;
                    flow.fallback_answered(fallback.take().expect("asked once"))
                }
                FallbackStep::Done(outcome) => return (outcome, asked_fallback),
                FallbackStep::Wait | FallbackStep::AskLsp { .. } => panic!("the flow is stuck"),
            };
        }
    }

    fn list(lsp: Result<Vec<i32>, IdeError>, fallback: Result<Vec<i32>, IdeError>) -> Outcome<Vec<i32>> {
        run(FallbackOptions::default(), Vec::is_empty, lsp, fallback).0
    }

    fn answer<T>(value: T, source: Source) -> Answer<T> {
        Answer { value, source }
    }

    #[test]
    fn prefers_the_language_server() {
        assert_eq!(list(Ok(vec![1]), Ok(vec![2])).ok(), Some(answer(vec![1], Source::Lsp)));
    }

    #[test]
    fn falls_back_when_the_server_is_not_ready_fails_or_answers_nothing() {
        assert_eq!(list(Err(unavailable()), Ok(vec![2])).ok(), Some(answer(vec![2], Source::Syntax)));
        assert_eq!(list(Err(protocol()), Ok(vec![2])).ok(), Some(answer(vec![2], Source::Syntax)));
        assert_eq!(list(Ok(Vec::new()), Ok(vec![2])).ok(), Some(answer(vec![2], Source::Syntax)));
        let (outcome, _) = run(FallbackOptions::default(), Option::is_none, Ok(None), Ok(Some("x")));
        assert_eq!(outcome.ok(), Some(answer(Some("x"), Source::Syntax)));
    }

    #[test]
    fn honours_the_when_conditions() {
        let not_ready = FallbackOptions { when: vec![FallbackWhen::NotReady], ..FallbackOptions::default() };
        let (outcome, _) = run(not_ready, Vec::<i32>::is_empty, Err(protocol()), Ok(vec![2]));
        assert_eq!(outcome.err().map(|error| error.message), Some("boom".to_owned()));
        let failures =
            FallbackOptions { when: vec![FallbackWhen::NotReady, FallbackWhen::Error], ..FallbackOptions::default() };
        let (outcome, _) = run(failures, Vec::<i32>::is_empty, Ok(Vec::new()), Ok(vec![2]));
        assert_eq!(outcome.ok(), Some(answer(Vec::new(), Source::Lsp)));
        let items = |items: Vec<i32>| (items, ());
        let (outcome, _) = run(
            FallbackOptions::default(),
            |value: &(Vec<i32>, ())| value.0.is_empty(),
            Ok(items(Vec::new())),
            Ok(items(vec![1])),
        );
        assert_eq!(outcome.ok(), Some(answer(items(vec![1]), Source::Syntax)));
    }

    #[test]
    fn never_falls_back_on_cancellation_and_keeps_the_server_outcome_when_the_fallback_fails() {
        let (outcome, asked) = run(FallbackOptions::default(), Vec::<i32>::is_empty, Err(cancelled()), Ok(vec![2]));
        assert!(outcome.is_err_and(|error| error.kind == ErrorKind::Cancelled));
        assert!(!asked);
        let outcome = list(Err(unavailable()), Err(unavailable()));
        assert_eq!(outcome.err().map(|error| error.message), Some("no server".to_owned()));
        assert_eq!(list(Ok(Vec::new()), Ok(Vec::new())).ok(), Some(answer(Vec::new(), Source::Lsp)));
        assert_eq!(list(Ok(Vec::new()), Err(protocol())).ok(), Some(answer(Vec::new(), Source::Lsp)));
    }

    #[test]
    fn answers_from_the_fallback_when_the_server_is_slow() {
        let slow = || FallbackOptions { when: vec![FallbackWhen::Slow], slow_ms: 10, ..FallbackOptions::default() };
        let (mut flow, step) = WithFallback::start(slow(), Vec::<i32>::is_empty);
        assert!(matches!(step, FallbackStep::AskLsp { slow_timer: Some(10) }));
        assert!(matches!(flow.slow_elapsed(), FallbackStep::AskFallback));
        let FallbackStep::Done(Ok(found)) = flow.fallback_answered(Ok(vec![2])) else {
            panic!("expected the fallback")
        };
        assert_eq!(found, answer(vec![2], Source::Syntax));
        assert!(matches!(flow.lsp_answered(Ok(vec![1])), FallbackStep::Wait));
        // An empty fallback waits for the server.
        let (mut flow, _) = WithFallback::start(slow(), Vec::<i32>::is_empty);
        flow.slow_elapsed();
        assert!(matches!(flow.fallback_answered(Ok(Vec::new())), FallbackStep::Wait));
        let FallbackStep::Done(Ok(found)) = flow.lsp_answered(Ok(vec![1])) else { panic!("expected the server") };
        assert_eq!(found, answer(vec![1], Source::Lsp));
        // A server faster than the timer answers.
        let (mut flow, _) = WithFallback::start(slow(), Vec::<i32>::is_empty);
        let FallbackStep::Done(Ok(found)) = flow.lsp_answered(Ok(vec![1])) else { panic!("expected the server") };
        assert_eq!(found, answer(vec![1], Source::Lsp));
        assert!(matches!(flow.slow_elapsed(), FallbackStep::Wait));
    }

    #[test]
    fn can_skip_the_server() {
        let skip = FallbackOptions { skip_lsp: true, ..FallbackOptions::default() };
        let (mut flow, step) = WithFallback::start(skip, Vec::<i32>::is_empty);
        assert!(matches!(step, FallbackStep::AskFallback));
        let FallbackStep::Done(Ok(found)) = flow.fallback_answered(Ok(vec![2])) else {
            panic!("expected the fallback")
        };
        assert_eq!(found, answer(vec![2], Source::Syntax));
    }
}
