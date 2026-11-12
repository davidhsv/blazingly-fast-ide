//! File editor providers: features that open some files in an editor of their own (an image
//! viewer, a database console) instead of the text editor, and wrappers that render around the
//! text editor of matching files (Markdown's editor and preview).

use super::registry::{Registration, Registry};

/// Where a provider that declares no order is asked.
pub const DEFAULT_PROVIDER_ORDER: i32 = 100;

/// A feature that may open a file in its own editor.
pub trait FileEditorProvider {
    /// What the editor area opens.
    type Input;
    /// Something the provider took hold of while resolving (a backend session, a lease) that
    /// the tab owns once it is admitted, and that has to be released otherwise.
    type Acquisition;

    /// The editor input for `path`, or `None` for a file it does not handle. What it acquires
    /// on the way is pushed to `acquired`, also when it fails afterwards.
    fn resolve(&self, path: &str, acquired: &mut Vec<Self::Acquisition>) -> Result<Option<Self::Input>, String>;
}

/// A provider that failed while resolving a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFailure {
    pub provider: String,
    pub message: String,
}

/// The editor a provider resolved for a file, with what it acquired for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEditorResolution<I, A> {
    pub input: I,
    /// Id of the provider that answered.
    pub provider: String,
    acquisitions: Vec<A>,
}

impl<I, A> FileEditorResolution<I, A> {
    /// The tab was opened: hands each acquisition to `commit`, in the order they were made, and
    /// returns the input.
    pub fn admit(self, commit: impl FnMut(A)) -> I {
        self.acquisitions.into_iter().for_each(commit);
        self.input
    }

    /// The tab was not opened after all: hands each acquisition to `release`.
    pub fn rollback(self, release: impl FnMut(A)) {
        self.acquisitions.into_iter().for_each(release);
    }
}

/// What asking the providers for a path came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEditorOutcome<I, A> {
    /// `None`: the text editor opens the file.
    pub resolution: Option<FileEditorResolution<I, A>>,
    /// The providers that failed before one answered; they are skipped, not fatal.
    pub failures: Vec<ProviderFailure>,
}

#[derive(Debug, Clone)]
struct Ordered<P> {
    order: i32,
    provider: P,
}

/// The registered file editor providers.
#[derive(Debug, Clone)]
pub struct FileEditorProviders<P> {
    providers: Registry<Ordered<P>>,
}

impl<P> Default for FileEditorProviders<P> {
    fn default() -> Self {
        Self { providers: Registry::new() }
    }
}

impl<P: FileEditorProvider> FileEditorProviders<P> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `provider`, replacing a provider of the same id. A lower `order` is asked
    /// first; `None` is [`DEFAULT_PROVIDER_ORDER`].
    pub fn register(&mut self, id: &str, order: Option<i32>, provider: P) -> Registration {
        self.providers.register(id, Ordered { order: order.unwrap_or(DEFAULT_PROVIDER_ORDER), provider })
    }

    /// Removes the provider `registration` registered, unless it was replaced since.
    pub fn unregister(&mut self, registration: Registration) -> bool {
        self.providers.unregister(registration)
    }

    /// Asks the providers in order for the editor of `path`. A provider that fails or declines
    /// has what it acquired handed to `release` before the next one is asked.
    pub fn resolve(
        &self,
        path: &str,
        mut release: impl FnMut(P::Acquisition),
    ) -> FileEditorOutcome<P::Input, P::Acquisition> {
        let mut providers: Vec<(&str, &Ordered<P>)> = self.providers.iter().collect();
        providers.sort_by_key(|(_, ordered)| ordered.order);
        let mut failures = Vec::new();
        for (id, ordered) in providers {
            let mut acquisitions = Vec::new();
            match ordered.provider.resolve(path, &mut acquisitions) {
                Ok(Some(input)) => {
                    let resolution = FileEditorResolution { input, provider: id.to_owned(), acquisitions };
                    return FileEditorOutcome { resolution: Some(resolution), failures };
                }
                Ok(None) => {}
                Err(message) => failures.push(ProviderFailure { provider: id.to_owned(), message }),
            }
            acquisitions.into_iter().for_each(&mut release);
        }
        FileEditorOutcome { resolution: None, failures }
    }
}

/// Something rendered around the text editor of the files it matches.
pub trait TextEditorWrapper {
    fn matches(&self, path: &str) -> bool;
}

/// The registered text editor wrappers.
#[derive(Debug, Clone)]
pub struct TextEditorWrappers<W> {
    wrappers: Registry<W>,
}

impl<W> Default for TextEditorWrappers<W> {
    fn default() -> Self {
        Self { wrappers: Registry::new() }
    }
}

impl<W: TextEditorWrapper> TextEditorWrappers<W> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `wrapper`, replacing a wrapper of the same id.
    pub fn register(&mut self, id: &str, wrapper: W) -> Registration {
        self.wrappers.register(id, wrapper)
    }

    /// Removes the wrapper `registration` registered, unless it was replaced since.
    pub fn unregister(&mut self, registration: Registration) -> bool {
        self.wrappers.unregister(registration)
    }

    pub fn len(&self) -> usize {
        self.wrappers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.wrappers.is_empty()
    }

    /// The first registered wrapper that matches `path`, with its id.
    pub fn wrapper_for(&self, path: &str) -> Option<(&str, &W)> {
        self.wrappers.iter().find(|(_, wrapper)| wrapper.matches(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Input {
        kind: &'static str,
        key: String,
    }

    type Resolve = Box<dyn Fn(&str, &mut Vec<&'static str>) -> Result<Option<Input>, String>>;

    struct Provider(Resolve);

    impl FileEditorProvider for Provider {
        type Input = Input;
        type Acquisition = &'static str;

        fn resolve(&self, path: &str, acquired: &mut Vec<&'static str>) -> Result<Option<Input>, String> {
            (self.0)(path, acquired)
        }
    }

    fn input(kind: &'static str, key: &str) -> Input {
        Input { kind, key: key.to_owned() }
    }

    // file editor providers › asks providers in order; failures fall through; disposal removes them
    #[test]
    fn asks_providers_in_order_failures_fall_through_disposal_removes_them() {
        let mut providers = FileEditorProviders::new();
        let registrations = [
            providers.register(
                "late",
                Some(50),
                Provider(Box::new(|path, _| Ok(Some(input("late", &format!("late:{path}")))))),
            ),
            providers.register("broken", Some(5), Provider(Box::new(|_, _| Err("no backend".to_owned())))),
            providers.register(
                "png",
                Some(10),
                Provider(Box::new(|path, _| Ok(path.ends_with(".png").then(|| input("image", path))))),
            ),
        ];
        let kind = |path: &str| {
            let outcome = providers.resolve(path, |_| {});
            assert_eq!(
                outcome.failures,
                [ProviderFailure { provider: "broken".to_owned(), message: "no backend".to_owned() }]
            );
            outcome.resolution.map(|resolution| resolution.input.kind)
        };
        assert_eq!(kind("/a.png"), Some("image"));
        assert_eq!(kind("/a.txt"), Some("late"));
        for registration in registrations {
            assert!(providers.unregister(registration));
        }
        assert_eq!(providers.resolve("/a.png", |_| {}).resolution, None);
    }

    // file editor providers › releases a failed provider's acquisition before trying the next provider
    #[test]
    fn releases_a_failed_providers_acquisition_before_trying_the_next_provider() {
        let live = Rc::new(RefCell::new(BTreeSet::from(["failed", "winner"])));
        let at_winner = Rc::new(RefCell::new(BTreeSet::new()));
        let mut providers = FileEditorProviders::new();
        providers.register(
            "acquired-failure",
            Some(1),
            Provider(Box::new(|_, acquired| {
                acquired.push("failed");
                Err("probe failed after acquisition".to_owned())
            })),
        );
        let (seen_live, seen_at_winner) = (live.clone(), at_winner.clone());
        providers.register(
            "acquired-winner",
            Some(2),
            Provider(Box::new(move |_, acquired| {
                *seen_at_winner.borrow_mut() = seen_live.borrow().clone();
                acquired.push("winner");
                Ok(Some(input("viewer", "winner")))
            })),
        );
        let release = |acquisition: &'static str| {
            live.borrow_mut().remove(acquisition);
        };
        let outcome = providers.resolve("/a", release);
        assert_eq!(*at_winner.borrow(), BTreeSet::from(["winner"]));
        assert_eq!(*live.borrow(), BTreeSet::from(["winner"]));
        let resolution = outcome.resolution.expect("the second provider answers");
        assert_eq!((resolution.provider.as_str(), resolution.input.kind), ("acquired-winner", "viewer"));
        resolution.rollback(release);
        assert!(live.borrow().is_empty());
    }

    // file editor providers › finds the text editor wrapper of a path
    #[test]
    fn finds_the_text_editor_wrapper_of_a_path() {
        struct Suffix(&'static str);
        impl TextEditorWrapper for Suffix {
            fn matches(&self, path: &str) -> bool {
                path.ends_with(self.0)
            }
        }
        let mut wrappers = TextEditorWrappers::new();
        let registration = wrappers.register("md", Suffix(".md"));
        assert_eq!(wrappers.wrapper_for("/r/README.md").map(|(id, _)| id), Some("md"));
        assert!(wrappers.wrapper_for("/r/a.ts").is_none());
        assert!(wrappers.unregister(registration));
        assert_eq!((wrappers.len(), wrappers.is_empty()), (0, true));
    }

    // Not from a spec: admitting hands every acquisition over in order and yields the input.
    #[test]
    fn admitting_commits_the_acquisitions_in_order() {
        let mut providers = FileEditorProviders::new();
        providers.register(
            "two",
            None,
            Provider(Box::new(|_, acquired| {
                acquired.extend(["session", "lease"]);
                Ok(Some(input("viewer", "two")))
            })),
        );
        let resolution = providers.resolve("/a", |_| {}).resolution.expect("the provider answers");
        let mut committed = Vec::new();
        assert_eq!(resolution.admit(|acquisition| committed.push(acquisition)), input("viewer", "two"));
        assert_eq!(committed, ["session", "lease"]);
    }
}
