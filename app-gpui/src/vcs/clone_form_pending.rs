//! Get from Version Control: the form of the Clone Repository dialog, drawn by
//! [`crate::vcs::git_form_dialog::GitFormDialog`].
//!
//! The URL, a recent URL to take instead, the directory (the parent folder joined with the name git
//! would give the clone, until the user types a directory of their own), Browse… for the parent
//! folder, and a shallow clone's depth. While the workbench clones, the form shows git's last
//! progress line and nothing can be pressed but Cancel; a failure shows in the form.

use crate::platform::i18n::I18n;
use crate::vcs::git_forms::{
    ButtonPlace, CANCEL, Choice, FormAnswer, FormButton, FormInput, FormRow, FormSpec, GitForm, OK,
};
use crate::vcs_ops::clone::{clone_dir_name, join_path, parse_clone_depth, validate_clone};

/// UI storage key of the folder the last clone went into.
pub const PARENT_KEY: &str = "openidea.vcs.cloneParent";
/// UI storage key of the URLs cloned last, newest first.
pub const RECENT_URLS_KEY: &str = "openidea.vcs.cloneRecentUrls";
/// How many cloned URLs are remembered.
const MAX_RECENT_URLS: usize = 10;

/// The id of the button that asks for the parent folder.
pub const BROWSE: &str = "browse";

/// What the Clone Repository dialog asks of the workbench. It holds the URL, which may carry
/// credentials, so it has no `Debug`.
#[derive(Clone)]
pub enum CloneAnswer {
    /// Browse…: the folder chooser, starting in `parent`.
    Browse { parent: String },
    /// Clone `url` into `dir`, `depth` commits deep when shallow.
    Clone { url: String, dir: String, depth: Option<u32> },
}

/// The URLs stored under [`RECENT_URLS_KEY`]; anything that is not a list of texts is none.
pub fn recent_urls(stored: Option<&str>) -> Vec<String> {
    let Some(serde_json::Value::Array(items)) = stored.and_then(|text| serde_json::from_str(text).ok()) else {
        return Vec::new();
    };
    items.into_iter().filter_map(|item| item.as_str().map(str::to_owned)).take(MAX_RECENT_URLS).collect()
}

/// The URLs to store once `url` was cloned: it first, the others after it, at most ten.
pub fn with_recent_url(stored: Option<&str>, url: &str) -> String {
    let mut urls = vec![url.to_owned()];
    urls.extend(recent_urls(stored).into_iter().filter(|known| known != url));
    urls.truncate(MAX_RECENT_URLS);
    serde_json::Value::from(urls).to_string()
}

pub struct CloneForm {
    url: String,
    /// The folder the clone goes into, unless a directory was typed.
    parent: String,
    /// A directory typed by hand: it no longer follows the URL.
    custom_dir: Option<String>,
    shallow: bool,
    depth: String,
    recent: Vec<String>,
    /// Git's last progress line while cloning (empty before the first one).
    progress: Option<String>,
    error: Option<String>,
}

impl CloneForm {
    /// The form on `url` (an action's argument, else empty), cloning into `parent`; `recent`: the
    /// URLs cloned last.
    pub fn new(url: String, parent: String, recent: Vec<String>) -> Self {
        Self {
            url,
            parent,
            custom_dir: None,
            shallow: false,
            depth: "1".to_owned(),
            recent,
            progress: None,
            error: None,
        }
    }

    /// The directory the clone goes into.
    pub fn dir(&self) -> String {
        self.custom_dir.clone().unwrap_or_else(|| join_path(&self.parent, &clone_dir_name(&self.url)))
    }

    fn busy(&self) -> bool {
        self.progress.is_some()
    }

    fn depth(&self) -> Option<u32> {
        if self.shallow { parse_clone_depth(&self.depth) } else { None }
    }

    /// Why the form cannot be submitted, in English; `None` when it can.
    fn invalid(&self) -> Option<&'static str> {
        validate_clone(&self.url, &self.dir())
            .or_else(|| (self.shallow && self.depth().is_none()).then_some("Enter a positive number of commits"))
    }
}

fn text(id: &'static str, value: String, placeholder: String, invalid: bool) -> FormRow {
    FormRow::Text { id, value, placeholder, mono: false, invalid }
}

fn button(id: &'static str, label: String, primary: bool, enabled: bool, place: ButtonPlace) -> FormButton {
    FormButton { id, label, primary, enabled, tooltip: None, place }
}

impl GitForm for CloneForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let busy = self.busy();
        let git = Choice { value: "git".to_owned(), label: "Git".to_owned(), detail: None };
        let mut rows = vec![
            FormRow::Label(i18n.tx("Version control:")),
            FormRow::Select {
                id: "vcs",
                value: Some("git".to_owned()),
                placeholder: String::new(),
                options: vec![git],
            },
            FormRow::Label(i18n.tx("URL:")),
            text("url", self.url.clone(), "https://github.com/user/repository.git".to_owned(), false),
        ];
        if !self.recent.is_empty() {
            let options =
                self.recent.iter().map(|url| Choice { value: url.clone(), label: url.clone(), detail: None }).collect();
            rows.push(FormRow::Select {
                id: "recent",
                value: None,
                placeholder: i18n.tx("Recent repository URLs"),
                options,
            });
        }
        rows.extend([
            FormRow::Label(i18n.tx("Directory:")),
            text("dir", self.dir(), String::new(), false),
            FormRow::Check {
                id: "shallow",
                label: i18n.tx("Shallow clone with a history truncated to"),
                on: self.shallow,
                enabled: !busy,
            },
            text("depth", self.depth.clone(), String::new(), self.shallow && self.depth().is_none()),
            FormRow::Hint(i18n.tx("commits")),
        ]);
        let invalid = self.invalid();
        if let Some(progress) = &self.progress {
            let line = if progress.is_empty() { i18n.tx("Cloning…") } else { progress.clone() };
            rows.push(FormRow::Hint(line));
        } else if let Some(error) = &self.error {
            rows.push(FormRow::Error(error.clone()));
        } else if let Some(invalid) = invalid.filter(|_| !self.url.trim().is_empty()) {
            rows.push(FormRow::Hint(i18n.tx(invalid)));
        }
        FormSpec {
            title: i18n.tx("Clone Repository"),
            width: 620.0,
            rows,
            buttons: vec![
                button(BROWSE, i18n.tx("Browse…"), false, !busy, ButtonPlace::FooterStart),
                button(OK, i18n.tx("Clone"), true, invalid.is_none() && !busy, ButtonPlace::Footer),
                button(CANCEL, i18n.tx("Cancel"), false, true, ButtonPlace::Footer),
            ],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        if self.busy() {
            return;
        }
        match (id, input) {
            ("url", FormInput::Text(url)) | ("recent", FormInput::Choose(url)) => self.url = url,
            ("dir", FormInput::Text(dir)) => self.custom_dir = Some(dir),
            // The folder Browse… chose: the directory follows the URL in it again.
            ("parent", FormInput::Text(parent)) => {
                self.parent = parent;
                self.custom_dir = None;
            }
            ("shallow", FormInput::Toggle) => self.shallow = !self.shallow,
            ("depth", FormInput::Text(depth)) => self.depth = depth,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        if self.busy() {
            return None;
        }
        match button {
            BROWSE => Some(FormAnswer::Clone(CloneAnswer::Browse { parent: self.parent.clone() })),
            OK if self.invalid().is_none() => Some(FormAnswer::Clone(CloneAnswer::Clone {
                url: self.url.trim().to_owned(),
                dir: self.dir().trim().to_owned(),
                depth: self.depth(),
            })),
            _ => None,
        }
    }

    fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    fn set_progress(&mut self, progress: Option<String>) {
        if progress.is_some() {
            self.error = None;
        }
        self.progress = progress;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::git_forms::can_press;

    fn rows(form: &CloneForm) -> Vec<FormRow> {
        form.spec(&mut I18n::default()).rows
    }

    fn dir_row(form: &CloneForm) -> Option<String> {
        rows(form).into_iter().find_map(|row| match row {
            FormRow::Text { id: "dir", value, .. } => Some(value),
            _ => None,
        })
    }

    // Not from a spec: the directory follows the URL in the parent folder until one is typed, and
    // Browse… puts the URL's folder in the parent chosen again.
    #[test]
    fn the_directory_follows_the_url_until_it_is_typed() {
        let mut form = CloneForm::new(String::new(), "/dev".to_owned(), Vec::new());
        form.edit("url", FormInput::Text("https://h/o/repo.git".to_owned()));
        assert_eq!(dir_row(&form).as_deref(), Some("/dev/repo"));
        form.edit("dir", FormInput::Text("/elsewhere/r".to_owned()));
        form.edit("url", FormInput::Text("https://h/o/other.git".to_owned()));
        assert_eq!(dir_row(&form).as_deref(), Some("/elsewhere/r"), "a typed directory stays");
        form.edit("parent", FormInput::Text("/picked".to_owned()));
        assert_eq!(dir_row(&form).as_deref(), Some("/picked/other"));
    }

    // Not from a spec: Clone needs a valid form, says why it is not once a URL is typed, and a
    // shallow clone needs a positive depth.
    #[test]
    fn clone_answers_only_a_valid_form_and_says_what_is_missing() {
        let mut i18n = I18n::default();
        let mut form = CloneForm::new(String::new(), String::new(), Vec::new());
        assert!(!can_press(&form.spec(&mut i18n), OK));
        let feedback = |form: &CloneForm| rows(form).into_iter().filter(|row| matches!(row, FormRow::Error(_))).count();
        assert_eq!(feedback(&form), 0, "an empty form shows no problem");
        assert!(
            !rows(&form).contains(&FormRow::Hint("Enter a repository URL".to_owned())),
            "not before a URL is typed"
        );
        form.edit("url", FormInput::Text("https://h/o/repo.git".to_owned()));
        assert!(rows(&form).contains(&FormRow::Hint("The directory must be an absolute path".to_owned())));
        form.edit("parent", FormInput::Text("/dev".to_owned()));
        assert!(can_press(&form.spec(&mut i18n), OK));
        form.edit("shallow", FormInput::Toggle);
        form.edit("depth", FormInput::Text("0".to_owned()));
        assert!(rows(&form).contains(&FormRow::Hint("Enter a positive number of commits".to_owned())));
        assert!(form.press(OK, &mut i18n).is_none());
        form.edit("depth", FormInput::Text("3".to_owned()));
        let Some(FormAnswer::Clone(CloneAnswer::Clone { url, dir, depth })) = form.press(OK, &mut i18n) else {
            panic!("Clone answers")
        };
        assert_eq!((url.as_str(), dir.as_str(), depth), ("https://h/o/repo.git", "/dev/repo", Some(3)));
    }

    // Not from a spec: while cloning only Cancel can be pressed and the progress shows; an error
    // shows once it ends.
    #[test]
    fn while_cloning_only_cancel_can_be_pressed() {
        let mut i18n = I18n::default();
        let mut form = CloneForm::new("/srv/a.git".to_owned(), "/dev".to_owned(), Vec::new());
        form.set_progress(Some(String::new()));
        let spec = form.spec(&mut i18n);
        assert!(!can_press(&spec, OK) && !can_press(&spec, BROWSE) && can_press(&spec, CANCEL));
        assert!(spec.rows.contains(&FormRow::Hint("Cloning…".to_owned())));
        form.set_progress(Some("Receiving objects:  50% (1/2)".to_owned()));
        assert!(rows(&form).contains(&FormRow::Hint("Receiving objects:  50% (1/2)".to_owned())));
        form.edit("url", FormInput::Text("/srv/b.git".to_owned()));
        assert_eq!(dir_row(&form).as_deref(), Some("/dev/a"), "nothing changes while cloning");
        form.set_progress(None);
        form.set_error(Some("fatal: repository '/srv/a.git' does not exist".to_owned()));
        assert!(rows(&form).contains(&FormRow::Error("fatal: repository '/srv/a.git' does not exist".to_owned())));
        assert!(can_press(&form.spec(&mut i18n), OK));
    }

    // Not from a spec: a recent URL chosen is the URL; the URLs are stored newest first, at most ten.
    #[test]
    fn remembers_the_urls_cloned_newest_first() {
        let stored = with_recent_url(None, "/srv/a.git");
        let stored = with_recent_url(Some(&stored), "/srv/b.git");
        let stored = with_recent_url(Some(&stored), "/srv/a.git");
        assert_eq!(recent_urls(Some(&stored)), ["/srv/a.git", "/srv/b.git"]);
        let many = (0..12)
            .fold(None, |stored: Option<String>, n| Some(with_recent_url(stored.as_deref(), &format!("/srv/{n}.git"))));
        assert_eq!(recent_urls(many.as_deref()).len(), 10);
        assert!(recent_urls(Some("{")).is_empty() && recent_urls(Some("[1, \"x\"]")) == ["x"]);

        let mut form = CloneForm::new(String::new(), "/dev".to_owned(), recent_urls(Some(&stored)));
        form.edit("recent", FormInput::Choose("/srv/b.git".to_owned()));
        assert_eq!(dir_row(&form).as_deref(), Some("/dev/b"));
    }
}
