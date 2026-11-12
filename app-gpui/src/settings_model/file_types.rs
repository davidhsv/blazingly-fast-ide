//! Settings › Editor › File Types: the user's file name and hashbang patterns per language
//! (`files.fileTypes`) and the per-file Override File Type (`files.fileTypeOverrides`, project
//! scope). The backend's registry detects languages with them; here is the page's model and the
//! name matching the UI repeats for file icons.

use ide_core::settings::FileTypeAssociation;
use std::collections::BTreeMap;

/// The settings key of the associations.
pub const FILE_TYPES_KEY: &str = "files.fileTypes";
/// The settings key of the per-file overrides.
pub const FILE_TYPE_OVERRIDES_KEY: &str = "files.fileTypeOverrides";

/// The user's patterns, by language id.
pub type FileTypeAssociations = BTreeMap<String, FileTypeAssociation>;

/// What the file type model reads of a language definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguagePatterns<'a> {
    pub id: &'a str,
    /// Without the dot.
    pub extensions: &'a [String],
    /// Exact file names and file name patterns.
    pub filenames: &'a [String],
}

/// A file name pattern of a language, and whether its definition brings it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePattern {
    pub pattern: String,
    pub builtin: bool,
}

/// The file name patterns of a language definition: `*.ext` and its file names.
pub fn builtin_patterns(lang: LanguagePatterns<'_>) -> Vec<String> {
    lang.extensions.iter().map(|extension| format!("*.{extension}")).chain(lang.filenames.iter().cloned()).collect()
}

/// The patterns in effect for a language: its definition's minus the removed ones, plus the added.
pub fn effective_patterns(lang: LanguagePatterns<'_>, assoc: &FileTypeAssociations) -> Vec<EffectivePattern> {
    let (added, removed): (&[String], &[String]) = match assoc.get(lang.id) {
        Some(own) => (&own.patterns, &own.removed_patterns),
        None => (&[], &[]),
    };
    let removed: Vec<String> = removed.iter().map(|pattern| pattern.to_lowercase()).collect();
    let mut out: Vec<EffectivePattern> = builtin_patterns(lang)
        .into_iter()
        .filter(|pattern| !removed.contains(&pattern.to_lowercase()))
        .map(|pattern| EffectivePattern { pattern, builtin: true })
        .collect();
    for pattern in added {
        if !out.iter().any(|listed| listed.pattern == *pattern) {
            out.push(EffectivePattern { pattern: pattern.clone(), builtin: false });
        }
    }
    out
}

/// `assoc` with the entry of `id` changed by `change`; an entry left without anything is dropped.
fn with_assoc(
    assoc: &FileTypeAssociations,
    id: &str,
    change: impl FnOnce(&mut FileTypeAssociation),
) -> FileTypeAssociations {
    let mut next = assoc.clone();
    let entry = next.entry(id.to_owned()).or_default();
    change(entry);
    if entry.patterns.is_empty() && entry.removed_patterns.is_empty() && entry.hashbangs.is_empty() {
        next.remove(id);
    }
    next
}

/// Adds `pattern` to the language `id`. As in IDEA a pattern belongs to one file type: it is
/// taken away from any other language that has it, built-in or added.
pub fn add_pattern(
    assoc: &FileTypeAssociations,
    langs: &[LanguagePatterns<'_>],
    id: &str,
    pattern: &str,
) -> FileTypeAssociations {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return assoc.clone();
    }
    let mut next = assoc.clone();
    for &lang in langs.iter().filter(|lang| lang.id != id) {
        if effective_patterns(lang, &next).iter().any(|listed| listed.pattern == pattern) {
            next = remove_pattern(&next, lang, pattern);
        }
    }
    let own = langs.iter().find(|lang| lang.id == id);
    let builtin = own.is_some_and(|&lang| builtin_patterns(lang).iter().any(|p| p == pattern));
    with_assoc(&next, id, |entry| {
        if builtin {
            // Adding back a removed built-in pattern restores it.
            entry.removed_patterns.retain(|removed| removed != pattern);
        } else if !entry.patterns.iter().any(|added| added == pattern) {
            entry.patterns.push(pattern.to_owned());
        }
    })
}

/// Takes `pattern` away from `lang`: an added one is dropped, one of the definition is recorded
/// as removed.
pub fn remove_pattern(assoc: &FileTypeAssociations, lang: LanguagePatterns<'_>, pattern: &str) -> FileTypeAssociations {
    let builtin = builtin_patterns(lang).iter().any(|p| p == pattern);
    with_assoc(assoc, lang.id, |entry| {
        entry.patterns.retain(|added| added != pattern);
        if builtin && !entry.removed_patterns.iter().any(|removed| removed == pattern) {
            entry.removed_patterns.push(pattern.to_owned());
        }
    })
}

/// Sets the interpreters whose `#!` line selects the language `id`: trimmed, without blanks or
/// repeats.
pub fn set_hashbangs(assoc: &FileTypeAssociations, id: &str, hashbangs: &[String]) -> FileTypeAssociations {
    with_assoc(assoc, id, |entry| {
        entry.hashbangs.clear();
        for hashbang in hashbangs.iter().map(|hashbang| hashbang.trim()).filter(|hashbang| !hashbang.is_empty()) {
            if !entry.hashbangs.iter().any(|listed| listed == hashbang) {
                entry.hashbangs.push(hashbang.to_owned());
            }
        }
    })
}

/// Applies an edited pattern list of `lang` as additions and removals (a pattern moves between
/// languages).
pub fn apply_pattern_edit(
    assoc: &FileTypeAssociations,
    langs: &[LanguagePatterns<'_>],
    lang: LanguagePatterns<'_>,
    next: &[String],
) -> FileTypeAssociations {
    let before: Vec<String> = effective_patterns(lang, assoc).into_iter().map(|listed| listed.pattern).collect();
    let mut out = assoc.clone();
    for pattern in before.iter().filter(|pattern| !next.contains(pattern)) {
        out = remove_pattern(&out, lang, pattern);
    }
    for pattern in next.iter().filter(|pattern| !before.contains(pattern)) {
        out = add_pattern(&out, langs, lang.id, pattern);
    }
    out
}

/// `*` matches any run of characters, `?` one, as the backend's file name patterns.
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    let (pattern, name): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    // Where to resume after the last `*` when the characters after it stop matching.
    let mut star: Option<(usize, usize)> = None;
    let (mut p, mut n) = (0, 0);
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((star_p, star_n)) => {
                    star = Some((star_p, star_n + 1));
                    p = star_p + 1;
                    n = star_n + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

fn has_wildcard(pattern: &str) -> bool {
    pattern.contains(['*', '?'])
}

/// The language a user-added pattern assigns to `file_name`; `None` when none applies. An exact
/// name wins, then the longest pattern with wildcards, then the longest `*.ext`.
pub fn user_file_type<'a>(assoc: &'a FileTypeAssociations, file_name: &str) -> Option<&'a str> {
    let lower = file_name.to_lowercase();
    let mut glob: Option<(&str, usize)> = None;
    let mut ext: Option<(&str, usize)> = None;
    for (id, entry) in assoc {
        for pattern in &entry.patterns {
            let length = pattern.encode_utf16().count();
            // `*.ext`: a dot and more after the star, without further wildcards.
            let is_extension = |rest: &&str| rest.len() > 1 && rest.starts_with('.') && !has_wildcard(rest);
            let extension = pattern.strip_prefix('*').filter(is_extension);
            if !has_wildcard(pattern) {
                if pattern == file_name {
                    return Some(id.as_str());
                }
            } else if let Some(suffix) = extension {
                let suffix = suffix.to_lowercase();
                let longer = ext.is_none_or(|(_, best)| length - 1 > best);
                if lower.ends_with(&suffix) && lower.len() > suffix.len() && longer {
                    ext = Some((id.as_str(), length - 1));
                }
            } else if wildcard_match(pattern, file_name) && glob.is_none_or(|(_, best)| length > best) {
                glob = Some((id.as_str(), length));
            }
        }
    }
    glob.or(ext).map(|(id, _)| id)
}

/// A file name standing for a language when its icon is picked (`file.py`).
pub fn sample_file_name(lang: LanguagePatterns<'_>) -> Option<String> {
    match lang.extensions.first().filter(|extension| !extension.is_empty()) {
        Some(extension) => Some(format!("file.{extension}")),
        None => lang.filenames.iter().find(|name| !has_wildcard(name)).cloned(),
    }
}

/// The settings key of a file's override: project-relative inside the project, else absolute.
pub fn override_key<'p>(path: &'p str, root: Option<&str>) -> &'p str {
    let inside = root.filter(|root| !root.is_empty()).and_then(|root| path.strip_prefix(root));
    inside.and_then(|rest| rest.strip_prefix('/')).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::schema::parse_settings;
    use serde_json::json;

    struct Lang {
        id: &'static str,
        extensions: Vec<String>,
        filenames: Vec<String>,
    }

    impl Lang {
        fn new(id: &'static str, extensions: &[&str], filenames: &[&str]) -> Self {
            let owned = |list: &[&str]| list.iter().map(|&item| item.to_owned()).collect();
            Self { id, extensions: owned(extensions), filenames: owned(filenames) }
        }

        fn patterns(&self) -> LanguagePatterns<'_> {
            LanguagePatterns { id: self.id, extensions: &self.extensions, filenames: &self.filenames }
        }
    }

    fn langs() -> [Lang; 3] {
        [
            Lang::new("python", &["py", "pyi"], &["SConstruct"]),
            Lang::new("groovy", &["groovy"], &["Jenkinsfile"]),
            Lang::new("shellscript", &["sh"], &[]),
        ]
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|&item| item.to_owned()).collect()
    }

    fn entry(patterns: &[&str], removed: &[&str]) -> FileTypeAssociation {
        FileTypeAssociation { patterns: strings(patterns), removed_patterns: strings(removed), hashbangs: Vec::new() }
    }

    fn shown(lang: LanguagePatterns<'_>, assoc: &FileTypeAssociations) -> Vec<String> {
        effective_patterns(lang, assoc).into_iter().map(|listed| listed.pattern).collect()
    }

    // file type associations › adds, removes and restores patterns of a language
    #[test]
    fn adds_removes_and_restores_patterns_of_a_language() {
        let langs = langs();
        let all: Vec<LanguagePatterns<'_>> = langs.iter().map(Lang::patterns).collect();
        let python = all[0];
        let edited = strings(&["*.py", "*.pyi", "SConstruct", "*.foo"]);
        let mut a = apply_pattern_edit(&FileTypeAssociations::new(), &all, python, &edited);
        assert_eq!(a, FileTypeAssociations::from([("python".to_owned(), entry(&["*.foo"], &[]))]));
        a = apply_pattern_edit(&a, &all, python, &strings(&["*.py", "*.foo"]));
        assert_eq!(a.get("python"), Some(&entry(&["*.foo"], &["*.pyi", "SConstruct"])));
        assert_eq!(shown(python, &a), ["*.py", "*.foo"]);
        // Adding back a built-in pattern restores it; removing the last change drops the entry.
        a = apply_pattern_edit(&a, &all, python, &strings(&["*.py", "*.pyi", "SConstruct"]));
        assert!(a.is_empty());
    }

    // file type associations › moves a pattern from the language that had it
    #[test]
    fn moves_a_pattern_from_the_language_that_had_it() {
        let langs = langs();
        let all: Vec<LanguagePatterns<'_>> = langs.iter().map(Lang::patterns).collect();
        let (groovy, shell) = (all[1], all[2]);
        let a = apply_pattern_edit(&FileTypeAssociations::new(), &all, shell, &strings(&["*.sh", "Jenkinsfile"]));
        assert_eq!(a.get("shellscript").map(|entry| entry.patterns.clone()), Some(strings(&["Jenkinsfile"])));
        assert_eq!(a.get("groovy").map(|entry| entry.removed_patterns.clone()), Some(strings(&["Jenkinsfile"])));
        assert_eq!(shown(groovy, &a), ["*.groovy"]);
        assert_eq!(remove_pattern(&a, shell, "Jenkinsfile").get("shellscript"), None);
    }

    // file type associations › matches user patterns for icons
    #[test]
    fn matches_user_patterns_for_icons() {
        let a = FileTypeAssociations::from([
            ("python".to_owned(), entry(&["*.foo", "build_*.cfg"], &[])),
            ("shellscript".to_owned(), entry(&["Jenkinsfile"], &[])),
        ]);
        assert_eq!(user_file_type(&a, "x.FOO"), Some("python"));
        assert_eq!(user_file_type(&a, "build_dev.cfg"), Some("python"));
        assert_eq!(user_file_type(&a, "Jenkinsfile"), Some("shellscript"));
        assert_eq!(user_file_type(&a, "x.py"), None);
        assert!(wildcard_match("file?.txt", "file1.txt"));
        assert!(!wildcard_match("file?.txt", "file.txt"));
    }

    // file type associations › validates stored associations and keeps them in the settings
    //
    // The backend keeps the associations as stored: patterns are not trimmed or de-duplicated,
    // content roots keep their repeats, and an entry of the wrong type is refused.
    #[test]
    fn keeps_stored_associations_in_the_settings() {
        let stored = json!({
            "files": {
                "fileTypes": { "go": { "hashbangs": ["gorun"] } },
                "fileTypeOverrides": { "a/b.txt": "markdown" },
            },
            "projectStructure": { "contentRoots": ["/data/shared", "/data/shared"], "excludeFiles": "*.log;tmp*" },
        });
        let s = parse_settings(&stored).expect("the backend takes the value");
        assert_eq!(s.files.file_types.get("go").map(|go| go.hashbangs.clone()), Some(strings(&["gorun"])));
        assert_eq!(s.files.file_types.get("go").map(|go| go.patterns.len()), Some(0));
        assert_eq!(s.files.file_type_overrides, BTreeMap::from([("a/b.txt".to_owned(), "markdown".to_owned())]));
        assert_eq!(s.project_structure.content_roots, ["/data/shared", "/data/shared"]);
        assert_eq!(s.project_structure.exclude_files, "*.log;tmp*");
        let padded = json!({ "files": { "fileTypes": { "python": { "patterns": [" *.foo ", "*.foo"] } } } });
        let python = parse_settings(&padded).map(|s| s.files.file_types["python"].patterns.clone());
        assert_eq!(python, Ok(strings(&[" *.foo ", "*.foo"])));
        for malformed in [
            json!({ "fileTypes": { "python": { "patterns": [3] } } }),
            json!({ "fileTypes": { "x": 1 } }),
            json!({ "fileTypeOverrides": { "bad": 1 } }),
        ] {
            assert!(parse_settings(&json!({ "files": malformed })).is_err());
        }
    }

    // file type associations › keys overrides by project-relative path inside the project
    #[test]
    fn keys_overrides_by_project_relative_path_inside_the_project() {
        assert_eq!(override_key("/p/src/a.txt", Some("/p")), "src/a.txt");
        assert_eq!(override_key("/elsewhere/a.txt", Some("/p")), "/elsewhere/a.txt");
        assert_eq!(override_key("/p2/a.txt", Some("/p")), "/p2/a.txt");
    }

    // Not from a spec: wildcards backtrack, the longest matching pattern decides a file's type,
    // hashbangs are tidied and a language is pictured by its first extension.
    #[test]
    fn wildcards_backtrack_and_the_longest_pattern_wins() {
        assert!(wildcard_match("*.tar.*", "a.tar.b.tar.gz"));
        assert!(wildcard_match("a*b*c", "aXbXbc"));
        assert!(!wildcard_match("a*b", "ab c"));
        assert!(wildcard_match("*", ""));
        assert!(!wildcard_match("?", ""));
        let a = FileTypeAssociations::from([
            ("a".to_owned(), entry(&["*.gz", "*"], &[])),
            ("b".to_owned(), entry(&["*.tar.gz"], &[])),
            ("c".to_owned(), entry(&["Docker*"], &[])),
        ]);
        assert_eq!(user_file_type(&a, "x.tar.gz"), Some("a"));
        assert_eq!(user_file_type(&a, "Dockerfile"), Some("c"));
        let no_glob = FileTypeAssociations::from([
            ("a".to_owned(), entry(&["*.gz"], &[])),
            ("b".to_owned(), entry(&["*.tar.gz"], &[])),
        ]);
        assert_eq!(user_file_type(&no_glob, "x.tar.gz"), Some("b"));
        assert_eq!(user_file_type(&no_glob, ".gz"), None);
        let tidy = set_hashbangs(&FileTypeAssociations::new(), "go", &strings(&[" gorun ", "", "gorun"]));
        assert_eq!(tidy.get("go").map(|go| go.hashbangs.clone()), Some(strings(&["gorun"])));
        assert!(set_hashbangs(&tidy, "go", &[]).is_empty());
        let langs = langs();
        assert_eq!(sample_file_name(langs[0].patterns()).as_deref(), Some("file.py"));
        let named = Lang::new("docker", &[], &["Dockerfile.*", "Dockerfile"]);
        assert_eq!(sample_file_name(named.patterns()).as_deref(), Some("Dockerfile"));
        assert_eq!(override_key("/p/a.txt", None), "/p/a.txt");
    }
}
