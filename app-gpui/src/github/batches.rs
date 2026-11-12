//! Incrementally loaded lists (IntelliJ's `BatchesLoader` / `IncrementallyComputedValue`): pages
//! are appended as they arrive, so choosers show the first users while the rest load. A list loads
//! once and stays cached until [`BatchesLoader::reset`].
//!
//! The loader does no I/O: [`BatchesLoader::load`] hands out the page to fetch as a
//! [`PageTicket`], and [`BatchesLoader::page_loaded`] takes the answer and hands out the next one.
//! An answer for a ticket of before a reset is dropped.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchesState<T> {
    pub items: Vec<T>,
    /// A page is being fetched.
    pub loading: bool,
    /// Every page arrived.
    pub done: bool,
    pub error: Option<String>,
}

impl<T> Default for BatchesState<T> {
    fn default() -> Self {
        Self { items: Vec::new(), loading: false, done: false, error: None }
    }
}

/// A page to fetch: `cursor` of the load `generation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageTicket<C> {
    pub generation: u64,
    pub cursor: C,
}

/// One fetched page and the cursor of the next (`None`: the last page).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page<T, C> {
    pub items: Vec<T>,
    pub next: Option<C>,
}

/// What [`BatchesLoader::load`] asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadStep<C> {
    /// Fetch this page, then call [`BatchesLoader::page_loaded`].
    Fetch(PageTicket<C>),
    /// A load is already running: wait for it.
    Running,
    /// Every page is loaded: the items are there.
    Loaded,
}

/// What follows an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageOutcome<C> {
    Fetch(PageTicket<C>),
    /// The list is complete.
    Done,
    /// The page failed; the items loaded so far stay, the next [`BatchesLoader::load`] starts over.
    Failed(String),
    /// The answer is for a load a reset made void.
    Stale,
}

/// A loader fetching pages from `first` until a page has no next cursor.
#[derive(Clone, Debug)]
pub struct BatchesLoader<T, C> {
    first: C,
    state: BatchesState<T>,
    generation: u64,
    /// A load runs or has completed (the cached result); cleared by a failure and a reset.
    started: bool,
}

impl<T: Clone, C: Clone> BatchesLoader<T, C> {
    pub fn new(first: C) -> Self {
        Self { first, state: BatchesState::default(), generation: 0, started: false }
    }

    pub fn state(&self) -> &BatchesState<T> {
        &self.state
    }

    /// Starts loading unless loaded or loading.
    pub fn load(&mut self) -> LoadStep<C> {
        if self.started {
            return if self.state.done { LoadStep::Loaded } else { LoadStep::Running };
        }
        self.started = true;
        self.generation += 1;
        self.state = BatchesState { loading: true, ..BatchesState::default() };
        LoadStep::Fetch(PageTicket { generation: self.generation, cursor: self.first.clone() })
    }

    /// Takes the answer for `ticket`.
    pub fn page_loaded(&mut self, ticket: &PageTicket<C>, answer: Result<Page<T, C>, String>) -> PageOutcome<C> {
        if ticket.generation != self.generation || !self.started {
            return PageOutcome::Stale;
        }
        match answer {
            Ok(page) => {
                self.state.items.extend(page.items);
                match page.next {
                    Some(cursor) => PageOutcome::Fetch(PageTicket { generation: self.generation, cursor }),
                    None => {
                        self.state.loading = false;
                        self.state.done = true;
                        PageOutcome::Done
                    }
                }
            }
            Err(error) => {
                self.state.loading = false;
                self.state.error = Some(error.clone());
                self.started = false;
                PageOutcome::Failed(error)
            }
        }
    }

    /// Forgets the items; the next [`BatchesLoader::load`] starts over.
    pub fn reset(&mut self) {
        self.generation += 1;
        self.started = false;
        self.state = BatchesState::default();
    }
}

/// Merges several loaders' states (potential reviewers: teams + users), keeping their order.
pub fn merge_batches<T: Clone>(states: &[&BatchesState<T>]) -> BatchesState<T> {
    BatchesState {
        items: states.iter().flat_map(|s| s.items.iter().cloned()).collect(),
        loading: states.iter().any(|s| s.loading),
        done: states.iter().all(|s| s.done),
        error: states.iter().find_map(|s| s.error.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(cursor: u32) -> Page<u32, u32> {
        if cursor == 0 { Page { items: vec![1, 2], next: Some(1) } } else { Page { items: vec![3], next: None } }
    }

    /// Runs a load to its end, answering with `fetch`; the number of pages asked for and the outcome.
    fn run(
        loader: &mut BatchesLoader<u32, u32>,
        mut fetch: impl FnMut(u32) -> Result<Page<u32, u32>, String>,
    ) -> (usize, PageOutcome<u32>) {
        let LoadStep::Fetch(mut ticket) = loader.load() else { return (0, PageOutcome::Done) };
        let mut calls = 0;
        loop {
            calls += 1;
            match loader.page_loaded(&ticket, fetch(ticket.cursor)) {
                PageOutcome::Fetch(next) => ticket = next,
                other => return (calls, other),
            }
        }
    }

    #[test]
    fn appends_pages_as_they_arrive_and_loads_once() {
        let mut loader = BatchesLoader::new(0);
        let (calls, outcome) = run(&mut loader, |p| Ok(pages(p)));
        assert_eq!((calls, outcome), (2, PageOutcome::Done));
        assert_eq!(loader.state().items, [1, 2, 3]);
        assert!(!loader.state().loading && loader.state().done);
        assert_eq!(loader.load(), LoadStep::Loaded);
        loader.reset();
        assert!(loader.state().items.is_empty());
    }

    #[test]
    fn keeps_what_loaded_before_a_failure_and_retries_on_the_next_load() {
        let mut loader = BatchesLoader::new(0);
        let (_, outcome) = run(&mut loader, |p| if p == 1 { Err("boom".into()) } else { Ok(pages(p)) });
        assert_eq!(outcome, PageOutcome::Failed("boom".into()));
        assert_eq!(
            loader.state(),
            &BatchesState { items: vec![1, 2], loading: false, done: false, error: Some("boom".into()) }
        );
        run(&mut loader, |p| Ok(pages(p)));
        assert_eq!(loader.state().items, [1, 2, 3]);
        let a = BatchesState { items: vec![1], loading: false, done: true, error: None };
        let b = BatchesState { items: vec![2], loading: true, done: false, error: None };
        assert_eq!(
            merge_batches(&[&a, &b]),
            BatchesState { items: vec![1, 2], loading: true, done: false, error: None }
        );
        // A reset makes the answers of the load before it void.
        loader.reset();
        let LoadStep::Fetch(old) = loader.load() else { panic!("a reset loader fetches") };
        loader.reset();
        assert_eq!(loader.page_loaded(&old, Ok(pages(0))), PageOutcome::Stale);
    }
}
