//! Settings › Plugins without the page: the installed list and its filter, catalog and marketplace
//! states, the install flow (a folder, its `plugin.toml`, or a package file unpacked first, then the
//! permission page), and how command failures are reported.
//!
//! [`InstallFlow`] calls nothing: each step returns the next [`InstallStep`] (a backend call or the
//! consent dialog), and the answer is passed to the step's method.

use super::consent::{Capability, capabilities_of};
use super::contribution::Contribution;
use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_host::plugins::{CatalogPlugin, Consent, InstallPreview, PluginInfo, PluginSource, PluginStatus};
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub const SETTINGS_PAGE_ID: &str = "plugins";

/// Numeric dotted-version order (`1.10.0` > `1.9.2`); pre-release and build suffixes are ignored.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn parts(v: &str) -> Vec<u64> {
        let core = v.split(['-', '+']).next().unwrap_or_default();
        core.split('.').map(|x| leading_number(x).unwrap_or(0)).collect()
    }
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        match x.get(i).copied().unwrap_or(0).cmp(&y.get(i).copied().unwrap_or(0)) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

/// The number a part starts with (`3rc` → 3), as `parseInt` reads it.
fn leading_number(part: &str) -> Option<u64> {
    let digits: String = part.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn matches(texts: &[&str], query: &str) -> bool {
    texts.iter().any(|text| text.to_lowercase().contains(query))
}

/// The installed plugins whose name, id, description, authors or contributions hold `query`.
pub fn filter_plugins<'a>(plugins: &'a [PluginInfo], query: &str) -> Vec<&'a PluginInfo> {
    let q = query.trim().to_lowercase();
    plugins
        .iter()
        .filter(|p| {
            if q.is_empty() {
                return true;
            }
            let mut texts = vec![p.name.as_str(), p.id.as_str(), p.description.as_str()];
            texts.extend(p.authors.iter().map(String::as_str));
            texts.extend(p.contributions.iter().map(|c| c.label.as_str()));
            matches(&texts, &q)
        })
        .collect()
}

/// The catalog packages whose name, id, description or authors hold `query`.
pub fn filter_catalog<'a>(catalog: &'a [CatalogPlugin], query: &str) -> Vec<&'a CatalogPlugin> {
    let q = query.trim().to_lowercase();
    catalog
        .iter()
        .filter(|p| {
            let mut texts = vec![p.name.as_str(), p.id.as_str(), p.description.as_str()];
            texts.extend(p.authors.iter().map(String::as_str));
            q.is_empty() || matches(&texts, &q)
        })
        .collect()
}

/// What the Available tab offers for a catalog package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogState {
    Install,
    Update,
    Installed,
    /// A development copy is loaded under the same id.
    Dev,
}

pub fn catalog_state(entry: &CatalogPlugin, installed: &[PluginInfo]) -> CatalogState {
    let Some(have) = installed.iter().find(|p| p.id == entry.id) else { return CatalogState::Install };
    if have.source != PluginSource::Installed {
        return CatalogState::Dev;
    }
    if compare_versions(&entry.version, &have.version) == Ordering::Greater {
        CatalogState::Update
    } else {
        CatalogState::Installed
    }
}

/// Whether a manifest's homepage is a web address to open (`http`/`https` with a host); anything
/// else is shown as plain text.
pub fn is_web_homepage(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && !host.is_empty()
        && !host.contains(char::is_whitespace)
}

/// The names Settings › Inspections lists a plugin's problems under.
pub fn producer_names(plugins: &[PluginInfo]) -> BTreeMap<String, String> {
    plugins
        .iter()
        .flat_map(|p| {
            [(format!("plugin:{}", p.id), p.name.clone()), (format!("plugin-provider:{}", p.id), p.name.clone())]
        })
        .collect()
}

/// The title of command `command_id` of `plugin_id`, else the id.
pub fn command_title(contributions: &[Contribution], plugin_id: &str, command_id: &str) -> String {
    contributions
        .iter()
        .find(|c| c.point == "commands" && c.plugin_id == plugin_id && c.key == command_id)
        .and_then(|c| c.text("title"))
        .unwrap_or(command_id)
        .to_owned()
}

/// The notification for a failed plugin command, unless the host already said why: the plugin
/// trapped (it is failed now) or it lacks a permission (a consent dialog opens).
pub fn command_failure(
    title: &str,
    error: &IdeError,
    status: Option<PluginStatus>,
    i18n: &mut I18n,
) -> Option<NotificationRequest> {
    if status == Some(PluginStatus::Failed) || error.kind == ErrorKind::PermissionDenied {
        return None;
    }
    let title = i18n.tx_built(&["", " failed"], &[Param::Text(title.to_owned())]);
    Some(NotificationRequest {
        group: Some("plugins".to_owned()),
        body: Some(error.message.clone()),
        ..NotificationRequest::new(NotificationLevel::Error, title)
    })
}

/// The installed (not development) plugins whose enablement differs from `enabled`.
pub fn installed_to_toggle(plugins: &[PluginInfo], enabled: bool) -> Vec<String> {
    plugins
        .iter()
        .filter(|p| p.source == PluginSource::Installed && p.enabled != enabled)
        .map(|p| p.id.clone())
        .collect()
}

/// The folder to install from for what the user picked, or the package file to unpack first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageSource {
    Folder(String),
    /// An `.oiplugin` or `.zip` file.
    Package(String),
}

pub fn package_source(picked: &str) -> PackageSource {
    let lower = picked.to_lowercase();
    if lower.ends_with(".oiplugin") || lower.ends_with(".zip") {
        return PackageSource::Package(picked.to_owned());
    }
    // A folder's manifest stands for the folder.
    const MANIFEST: &str = "plugin.toml";
    let cut = picked.len().saturating_sub(MANIFEST.len() + 1);
    if let Some(tail) = picked.get(cut..)
        && tail.len() == MANIFEST.len() + 1
        && (tail.starts_with('/') || tail.starts_with('\\'))
        && tail[1..].eq_ignore_ascii_case(MANIFEST)
    {
        return PackageSource::Folder(picked[..cut].to_owned());
    }
    PackageSource::Folder(picked.to_owned())
}

/// What a consent dialog shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentRequest {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    /// An install page (`true`) or a permission prompt for a loaded plugin.
    pub install: bool,
    pub capabilities: Vec<Capability>,
    pub unsigned: bool,
    pub signer_changed: bool,
    pub warnings: Vec<String>,
}

/// The consent dialog for a plugin waiting for permission.
pub fn grant_request(plugin: &PluginInfo) -> ConsentRequest {
    ConsentRequest {
        plugin_id: plugin.id.clone(),
        name: plugin.name.clone(),
        version: plugin.version.clone(),
        install: false,
        capabilities: capabilities_of(plugin),
        unsigned: plugin.source != PluginSource::Installed,
        signer_changed: false,
        warnings: plugin.warnings.clone(),
    }
}

/// The answer without a dialog: plugins asking for nothing are accepted.
pub fn consent_without_dialog(request: &ConsentRequest) -> Option<Consent> {
    request.capabilities.is_empty().then(Consent::default)
}

/// The next step of an install.
#[derive(Clone, Debug, PartialEq)]
pub enum InstallStep {
    /// Show the folder (or package file) picker; answer with [`InstallFlow::picked`].
    Pick { package_file: bool },
    /// `plugin_unpack_package`; answer with [`InstallFlow::unpacked`].
    Unpack(String),
    /// `plugin_preview_install`; answer with [`InstallFlow::previewed`].
    Preview(String),
    /// Show the install page; answer with [`InstallFlow::consented`].
    AskConsent(ConsentRequest),
    /// `plugin_install_dir(dir, Consent { optional, signer_change })`; answer with
    /// [`InstallFlow::installed`].
    Install { dir: String, optional: Vec<String>, signer_change: bool },
    /// Finished: notify (if any), list the plugins again, and adopt the new plugin's icon theme.
    Done { installed: Option<String>, notification: Option<NotificationRequest> },
}

/// One install from disk or the marketplace.
#[derive(Debug)]
pub struct InstallFlow {
    package_file: bool,
    /// No notification on success (a batch of updates reports once).
    quiet: bool,
    dir: Option<String>,
}

impl InstallFlow {
    /// Starts with `path` when one is given, else with the picker.
    pub fn start(path: Option<&str>, package_file: bool, quiet: bool) -> (Self, InstallStep) {
        let mut flow = Self { package_file, quiet, dir: None };
        let step = match path {
            Some(path) => flow.after_pick(path),
            None => InstallStep::Pick { package_file },
        };
        (flow, step)
    }

    fn after_pick(&mut self, picked: &str) -> InstallStep {
        match package_source(picked) {
            PackageSource::Package(file) => InstallStep::Unpack(file),
            PackageSource::Folder(dir) => self.preview(dir),
        }
    }

    fn preview(&mut self, dir: String) -> InstallStep {
        self.dir = Some(dir.clone());
        InstallStep::Preview(dir)
    }

    fn failed(&self, title: &str, error: &str, i18n: &I18n) -> InstallStep {
        let notification = NotificationRequest {
            group: Some("plugins".to_owned()),
            body: Some(error.to_owned()),
            ..NotificationRequest::new(NotificationLevel::Error, i18n.tx(title))
        };
        InstallStep::Done { installed: None, notification: Some(notification) }
    }

    /// The picker's answer (`Ok(None)`: cancelled).
    pub fn picked(&mut self, picked: Result<Option<String>, String>, i18n: &I18n) -> InstallStep {
        match picked {
            Ok(Some(path)) => self.after_pick(&path),
            Ok(None) => InstallStep::Done { installed: None, notification: None },
            Err(error) => {
                let title =
                    if self.package_file { "Could not open the package" } else { "Could not open the file picker" };
                self.failed(title, &error, i18n)
            }
        }
    }

    pub fn unpacked(&mut self, dir: Result<String, String>, i18n: &I18n) -> InstallStep {
        match dir {
            Ok(dir) => self.preview(dir),
            Err(error) => self.failed("Could not open the package", &error, i18n),
        }
    }

    /// The install page's content: an update with nothing to confirm installs with its grants.
    pub fn previewed(&mut self, preview: Result<InstallPreview, String>, i18n: &I18n) -> InstallStep {
        let preview = match preview {
            Ok(preview) => preview,
            Err(error) => return self.failed("Could not install plugin", &error, i18n),
        };
        let capabilities: Vec<Capability> = serde_json::to_value(&preview.capabilities)
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if preview.needs_consent {
            return InstallStep::AskConsent(ConsentRequest {
                plugin_id: preview.id,
                name: preview.name,
                version: preview.version,
                install: true,
                capabilities,
                unsigned: preview.unsigned,
                signer_changed: preview.signer_changed,
                warnings: preview.warnings,
            });
        }
        let optional = capabilities.iter().filter(|c| c.optional && c.granted).map(|c| c.id.clone()).collect();
        self.consented(Some(Consent { optional, signer_change: false }))
    }

    /// The user's answer (`None`: cancelled).
    pub fn consented(&mut self, consent: Option<Consent>) -> InstallStep {
        match (consent, self.dir.clone()) {
            (Some(consent), Some(dir)) => {
                InstallStep::Install { dir, optional: consent.optional, signer_change: consent.signer_change }
            }
            _ => InstallStep::Done { installed: None, notification: None },
        }
    }

    /// The installed plugin: a plugin that failed to start is reported even in a quiet install.
    pub fn installed(&mut self, info: Result<PluginInfo, String>, i18n: &mut I18n) -> InstallStep {
        let info = match info {
            Ok(info) => info,
            Err(error) => return self.failed("Could not install plugin", &error, i18n),
        };
        let failed = info.status == PluginStatus::Failed;
        let notification = (failed || !self.quiet).then(|| {
            let title = i18n.tx_built(
                &["Installed ", " ", ""],
                &[Param::Text(info.name.clone()), Param::Text(info.version.clone())],
            );
            let level = if failed { NotificationLevel::Warning } else { NotificationLevel::Info };
            let body = failed.then(|| info.error.clone().unwrap_or_else(|| i18n.tx("The plugin failed to start.")));
            NotificationRequest { group: Some("plugins".to_owned()), body, ..NotificationRequest::new(level, title) }
        });
        InstallStep::Done { installed: (!failed).then(|| info.id.clone()), notification }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_models::test_support::{capability, plugin};
    use serde_json::json;

    fn catalog(id: &str, version: &str, name: &str) -> CatalogPlugin {
        serde_json::from_value(json!({
            "id": id, "name": name, "version": version, "description": "", "authors": [], "homepage": null,
            "changeNotes": null, "icon": null, "path": format!("/catalog/{id}"), "hasCode": false,
            "capabilities": [], "contributions": [], "problem": null,
        }))
        .expect("a catalog entry")
    }

    fn preview(needs_consent: bool, capabilities: serde_json::Value) -> InstallPreview {
        serde_json::from_value(json!({
            "id": "hello", "name": "Hello", "version": "1.0.0", "description": "", "authors": [], "hasCode": true,
            "capabilities": capabilities, "contributions": [], "warnings": [], "replaces": null, "unsigned": true,
            "publisher": null, "signerChanged": false, "needsConsent": needs_consent,
        }))
        .expect("a preview")
    }

    #[test]
    fn compares_versions_numerically() {
        assert_eq!(compare_versions("1.10.0", "1.9.2"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.0-beta", "0.2.0"), Ordering::Less);
    }

    #[test]
    fn derives_install_update_installed_dev_states() {
        let entry = catalog("hello", "0.1.0", "Hello");
        assert_eq!(catalog_state(&entry, &[]), CatalogState::Install);
        assert_eq!(catalog_state(&entry, &[plugin("hello", json!({ "version": "0.0.9" }))]), CatalogState::Update);
        assert_eq!(catalog_state(&entry, &[plugin("hello", json!({ "version": "0.1.0" }))]), CatalogState::Installed);
        assert_eq!(catalog_state(&entry, &[plugin("hello", json!({ "source": "dev" }))]), CatalogState::Dev);
        let all = [entry, catalog("theme-harbor-night", "1.0.0", "Harbor Night")];
        let found: Vec<&str> = filter_catalog(&all, "harbor").iter().map(|c| c.id.as_str()).collect();
        assert_eq!(found, ["theme-harbor-night"]);
    }

    #[test]
    fn disables_and_re_enables_all_downloaded_plugins_leaving_dev_plugins_alone() {
        let plugins = [plugin("a", json!({})), plugin("b", json!({})), plugin("d", json!({ "source": "dev" }))];
        assert_eq!(installed_to_toggle(&plugins, false), ["a", "b"]);
        assert!(installed_to_toggle(&plugins, true).is_empty());
    }

    #[test]
    fn filters_by_name_id_author_and_contribution() {
        let list = [
            plugin("a", json!({ "name": "Alpha", "contributions": [{ "point": "commands", "label": "Count Words" }] })),
            plugin("b", json!({ "name": "Beta", "authors": ["Zed"] })),
        ];
        let ids = |query: &str| filter_plugins(&list, query).iter().map(|p| p.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids("count"), ["a"]);
        assert_eq!(ids("zed"), ["b"]);
        assert_eq!(ids(" ").len(), 2);
    }

    #[test]
    fn names_manual_and_provider_inspection_producers_after_their_plugin() {
        let names = producer_names(&[plugin("a", json!({ "name": "Alpha" }))]);
        assert_eq!(names.get("plugin:a").map(String::as_str), Some("Alpha"));
        assert_eq!(names.get("plugin-provider:a").map(String::as_str), Some("Alpha"));
    }

    #[test]
    fn reports_command_failures_but_not_twice_for_plugins_that_trapped() {
        let mut i18n = I18n::default();
        let decl = json!({ "id": "word-count", "title": "Hello: Word Count" });
        let contributions = [Contribution::new("hello", "commands", "word-count", decl)];
        let title = command_title(&contributions, "hello", "word-count");
        let error = IdeError::internal("boom");
        let note = command_failure(&title, &error, Some(PluginStatus::Active), &mut i18n).expect("reported");
        assert_eq!((note.level, note.title.as_str()), (NotificationLevel::Error, "Hello: Word Count failed"));
        let trapped = IdeError::internal("trapped");
        assert_eq!(command_failure(&title, &trapped, Some(PluginStatus::Failed), &mut i18n), None);
        let denied = IdeError::new(ErrorKind::PermissionDenied, "needs consent");
        assert_eq!(command_failure(&title, &denied, Some(PluginStatus::Active), &mut i18n), None);
        assert_eq!(command_title(&contributions, "hello", "missing"), "missing");
    }

    #[test]
    fn installs_from_a_folder_its_plugin_toml_or_a_package_file_unpacked_first() {
        let cases = [
            ("plugins/hello", None, "plugins/hello"),
            ("plugins/hello/plugin.toml", None, "plugins/hello"),
            ("C:\\plugins\\hello\\Plugin.TOML", None, "C:\\plugins\\hello"),
            ("/downloads/hello.zip", Some("/downloads/hello.zip"), "plugins/hello"),
            ("/downloads/hello-1.0.0.oiplugin", Some("/downloads/hello-1.0.0.oiplugin"), "plugins/hello"),
        ];
        let i18n = I18n::default();
        for (picked, unpacked, folder) in cases {
            let (mut flow, step) = InstallFlow::start(Some(picked), false, false);
            let step = match (step, unpacked) {
                (InstallStep::Unpack(file), Some(expected)) => {
                    assert_eq!(file, expected);
                    flow.unpacked(Ok("plugins/hello".to_owned()), &i18n)
                }
                (step, None) => step,
                (step, Some(_)) => panic!("{picked} is unpacked first, not {step:?}"),
            };
            assert_eq!(step, InstallStep::Preview(folder.to_owned()), "{picked}");
            let step = flow.previewed(Ok(preview(false, json!([]))), &i18n);
            assert!(matches!(step, InstallStep::Install { ref dir, .. } if dir == folder), "{picked}");
        }
    }

    #[test]
    fn asks_for_consent_before_installing_and_passes_the_answer_on() {
        let mut i18n = I18n::default();
        let (mut flow, _) = InstallFlow::start(Some("plugins/hello"), false, false);
        let step = flow.previewed(Ok(preview(true, json!([capability("editor.write", false)]))), &i18n);
        let InstallStep::AskConsent(request) = step else { panic!("the install page") };
        assert!(request.install && request.unsigned && request.plugin_id == "hello");
        assert!(request.capabilities.iter().any(|c| c.id == "editor.write"));
        let consent = Consent { optional: Vec::new(), signer_change: false };
        assert!(matches!(flow.consented(Some(consent)), InstallStep::Install { .. }));
        let done = flow.installed(Ok(plugin("hello", json!({}))), &mut i18n);
        let InstallStep::Done { installed, notification } = done else { panic!("done") };
        assert_eq!(installed.as_deref(), Some("hello"));
        assert_eq!(notification.map(|n| n.title).as_deref(), Some("Installed hello 1"));

        let (mut refused, _) = InstallFlow::start(Some("plugins/hello"), false, false);
        refused.previewed(Ok(preview(true, json!([]))), &i18n);
        assert_eq!(refused.consented(None), InstallStep::Done { installed: None, notification: None });
    }

    #[test]
    fn opens_a_web_homepage_and_keeps_an_unsupported_or_malformed_one_as_plain_text() {
        for homepage in ["https://example.com/plugin", "http://example.com/plugin", "HTTPS://example.com/plugin"] {
            assert!(is_web_homepage(homepage), "{homepage}");
        }
        let unsupported =
            ["not a URL", "https://", "/relative", "file:///tmp/plugin", "javascript:alert(1)", "mailto:a@b.com"];
        for homepage in unsupported {
            assert!(!is_web_homepage(homepage), "{homepage}");
        }
    }
}
