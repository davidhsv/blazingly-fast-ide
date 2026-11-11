//! The icon set: JetBrains new-UI ("expui") icons and OpenIDEA's own, as vendored by the webview
//! UI under `src/ui/expui` and `src/ui/openidea`, with their `_dark` and `@20x20` variants.
//! Names are the contract used by actions (`ActionMeta::icon`) and contributions.
//!
//! - Monochrome icons (only the standard greys `#6C707E` light / `#CED0D6` dark) are drawn in
//!   the caller's text colour, so selected and disabled states keep working.
//! - Coloured icons (file types, symbol nodes, run/debug, status…) keep their own palette: the
//!   light file in light themes, the `_dark` one otherwise.
//! - A size of 18 or more picks the `@20x20` variant when one exists (tool window stripes).

use gpui_kit::{AssetSource, SharedString};
use parking_lot::RwLock;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/icon_files.rs"));

const MONO_LIGHT: &str = "#6c707e";
const MONO_DARK: &str = "#ced0d6";
/// The `quote` icon is drawn by hand in the webview UI; this is the same path.
const QUOTE_PATH: &str = "openidea-generated/quote.svg";
const QUOTE_SVG: &[u8] = br##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M3 3v10M6.5 4.5H13M6.5 8H13M6.5 11.5H11" stroke="#6C707E" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

#[derive(Deserialize)]
struct Tables {
    icons: BTreeMap<String, String>,
    symbols: BTreeMap<String, String>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/gen/icons.json"))
            .expect("assets/gen/icons.json is generated and valid")
    })
}

fn files() -> &'static HashMap<&'static str, &'static [u8]> {
    static FILES: OnceLock<HashMap<&'static str, &'static [u8]>> = OnceLock::new();
    FILES.get_or_init(|| ICON_FILES.iter().copied().chain([(QUOTE_PATH, QUOTE_SVG)]).collect())
}

/// Icons supplied at run time (plugin icon themes), by name (`ext:<source>/<icon>`). The markup
/// was sanitised by the backend (`ide_host::plugins::sanitize_svg`).
fn external() -> &'static RwLock<HashMap<String, &'static [u8]>> {
    static EXTERNAL: OnceLock<RwLock<HashMap<String, &'static [u8]>>> = OnceLock::new();
    EXTERNAL.get_or_init(Default::default)
}

const EXTERNAL_PREFIX: &str = "ext:";

/// Registers icons by name (`ext:…`). An icon's bytes stay allocated for the life of the
/// process: assets are handed out as `'static`, and icon themes are a handful of small files.
pub fn register_external_icons(icons: impl IntoIterator<Item = (String, Vec<u8>)>) {
    let mut external = external().write();
    for (name, svg) in icons {
        if name.starts_with(EXTERNAL_PREFIX) {
            external.insert(name, Box::leak(svg.into_boxed_slice()));
        }
    }
    prepared().write().clear();
}

pub fn unregister_external_icons<'a>(names: impl IntoIterator<Item = &'a str>) {
    let mut external = external().write();
    for name in names {
        external.remove(name);
    }
    prepared().write().clear();
}

/// The table path of an icon name (`folder` → `nodes/folder`), `None` for an unknown name.
/// `symbol-<kind>` names a symbol node icon; `ext:` names are external icons.
pub fn icon_path(name: &str) -> Option<String> {
    if name.starts_with(EXTERNAL_PREFIX) {
        return external().read().contains_key(name).then(|| name.to_owned());
    }
    if let Some(kind) = name.strip_prefix("symbol-") {
        return tables().symbols.get(kind).cloned();
    }
    tables().icons.get(name).cloned()
}

/// Every built-in icon name.
pub fn icon_names() -> impl Iterator<Item = &'static str> {
    tables().icons.keys().map(String::as_str)
}

fn asset_path(path: &str) -> String {
    if path.starts_with("openidea/") { format!("{path}.svg") } else { format!("expui/{path}.svg") }
}

fn raw(path: &str) -> Option<&'static [u8]> {
    if path.starts_with(EXTERNAL_PREFIX) {
        return external().read().get(path).copied();
    }
    files().get(asset_path(path).as_str()).copied()
}

/// The hex colours a document uses, lowercased (`#abc`, `#aabbcc`, `#aabbccdd`).
fn colours(svg: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(svg);
    let mut found = Vec::new();
    let mut rest: &str = &text;
    while let Some(hash) = rest.find('#') {
        let after = &rest[hash + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_hexdigit).count();
        let boundary = after[digits..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if (3..=8).contains(&digits) && boundary {
            found.push(format!("#{}", after[..digits].to_ascii_lowercase()));
        }
        rest = &after[digits.min(after.len())..];
    }
    found
}

fn only(colours: &[String], expected: &str) -> bool {
    !colours.is_empty() && colours.iter().all(|c| c == expected)
}

/// What to draw for an icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconAsset {
    /// The asset to load (see [`IconAssets`]).
    pub asset: SharedString,
    /// Drawn in the text colour (a mask) rather than with its own colours.
    pub mono: bool,
}

/// Resolved assets by (table path, large, dark); `None` for a path with no file.
type Prepared = RwLock<HashMap<(String, bool, bool), Option<IconAsset>>>;

fn prepared() -> &'static Prepared {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    PREPARED.get_or_init(Default::default)
}

fn prepare(path: &str, large: bool, dark: bool) -> Option<IconAsset> {
    let key = (path.to_owned(), large, dark);
    if let Some(hit) = prepared().read().get(&key) {
        return hit.clone();
    }
    let asset = (|| {
        if path == tables().icons.get("quote")?.as_str() || path == QUOTE_PATH {
            return Some(IconAsset { asset: QUOTE_PATH.into(), mono: true });
        }
        let sized = format!("{path}@20x20");
        let base = if large && raw(&sized).is_some() { sized.as_str() } else { path };
        let light = raw(base)?;
        let dark_path = format!("{base}_dark");
        let dark_source = raw(&dark_path);
        // Breakpoint icons keep their colours even when they happen to be grey.
        let keep_colours = path.starts_with("breakpoints/");
        let mono = !keep_colours
            && only(&colours(light), MONO_LIGHT)
            && dark_source.is_none_or(|d| only(&colours(d), MONO_DARK));
        let chosen = if !mono && dark && dark_source.is_some() { dark_path.as_str() } else { base };
        let asset = if chosen.starts_with(EXTERNAL_PREFIX) { chosen.to_owned() } else { asset_path(chosen) };
        Some(IconAsset { asset: asset.into(), mono })
    })();
    prepared().write().insert(key, asset.clone());
    asset
}

/// The asset for icon `name` at `size` pixels in a dark or light theme. Unknown names draw the
/// generic file icon.
pub fn icon_asset(name: &str, size: f32, dark: bool) -> IconAsset {
    let large = size >= 18.0;
    icon_path(name)
        .and_then(|path| prepare(&path, large, dark))
        .or_else(|| prepare(tables().icons.get("file")?, large, dark))
        .unwrap_or(IconAsset { asset: QUOTE_PATH.into(), mono: true })
}

/// Serves the embedded icons, external icons and, for the widget library's own glyphs, `fallback`.
pub struct IconAssets<F> {
    pub fallback: F,
}

impl<F: AssetSource> AssetSource for IconAssets<F> {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = files().get(path).copied().or_else(|| external().read().get(path).copied()) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        self.fallback.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut listed: Vec<SharedString> =
            files().keys().filter(|key| key.starts_with(path)).map(|key| SharedString::from(*key)).collect();
        listed.sort();
        listed.extend(self.fallback.list(path)?);
        Ok(listed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_name_resolves_to_an_embedded_file() {
        assert!(icon_names().count() > 400);
        for name in icon_names() {
            for (size, dark) in [(16.0, false), (16.0, true), (20.0, false), (20.0, true)] {
                let asset = icon_asset(name, size, dark);
                assert!(
                    files().contains_key(asset.asset.as_ref()),
                    "{name} at {size} ({dark}): {} is not embedded",
                    asset.asset
                );
            }
        }
        for kind in tables().symbols.keys() {
            assert!(icon_path(&format!("symbol-{kind}")).is_some(), "symbol-{kind}");
        }
    }

    #[test]
    fn picks_variants_like_the_webview_icon_component() {
        // A grey action icon is monochrome: one asset for both themes, drawn in the text colour.
        let search = icon_asset("search", 16.0, true);
        assert!(search.mono, "{search:?}");
        assert_eq!(search, icon_asset("search", 16.0, false));
        // A file-type icon keeps its colours and has a dark variant.
        let light = icon_asset("file-json", 16.0, false);
        let dark = icon_asset("file-json", 16.0, true);
        assert!(!light.mono && !dark.mono);
        assert_eq!(light.asset.as_ref(), "expui/fileTypes/json.svg");
        assert_eq!(dark.asset.as_ref(), "expui/fileTypes/json_dark.svg");
        // Tool window stripes ask for 20px and get the @20x20 drawing.
        assert!(
            icon_asset("toolwindow-project", 20.0, false).asset.contains("@20x20"),
            "{:?}",
            icon_asset("toolwindow-project", 20.0, false)
        );
        assert!(!icon_asset("toolwindow-project", 16.0, false).asset.contains("@20x20"));
        // Unknown names draw the generic file icon.
        assert_eq!(icon_asset("no-such-icon", 16.0, false), icon_asset("file", 16.0, false));
        assert!(icon_asset("quote", 16.0, false).mono);
    }

    #[test]
    fn finds_hex_colours() {
        assert_eq!(
            colours(br##"<path fill="#6C707E" stroke="#fff"/><use href="#clip0"/><x fill="#12"/>"##),
            ["#6c707e", "#fff"]
        );
        assert!(only(&["#6c707e".to_owned()], MONO_LIGHT) && !only(&[], MONO_LIGHT));
    }

    #[test]
    fn serves_embedded_and_external_icons_then_the_fallback() {
        let assets = IconAssets { fallback: () };
        assert!(assets.load("expui/nodes/folder.svg").unwrap().is_some());
        assert!(assets.load("nope.svg").unwrap().is_none());
        assert!(assets.list("openidea/vcs/").unwrap().iter().any(|p| p.as_ref() == "openidea/vcs/stash.svg"));
        register_external_icons([
            ("ext:theme/rust".to_owned(), b"<svg/>".to_vec()),
            ("not-external".to_owned(), vec![]),
        ]);
        assert_eq!(icon_path("ext:theme/rust").as_deref(), Some("ext:theme/rust"));
        assert_eq!(icon_asset("ext:theme/rust", 16.0, false).asset.as_ref(), "ext:theme/rust");
        assert!(assets.load("ext:theme/rust").unwrap().is_some());
        unregister_external_icons(["ext:theme/rust"]);
        assert_eq!(icon_path("ext:theme/rust"), None);
    }
}
