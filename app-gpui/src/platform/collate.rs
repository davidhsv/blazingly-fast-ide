//! Name ordering as the webview's `localeCompare` / `Intl.Collator` do it: by the Unicode
//! Collation Algorithm, not by code points. Punctuation sorts before digits and letters, accents
//! and case are secondary, and digit runs can compare as numbers.
//!
//! One implementation for every list the UI sorts (tool windows, tabs, the project tree, branch
//! popups, themes), so that the same names come out in the same order everywhere.

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use std::cmp::Ordering;
use std::sync::OnceLock;

/// What makes two names differ, as `Intl.Collator`'s `sensitivity` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    /// `localeCompare`'s default: letters, then accents, then case all count.
    Variant,
    /// `sensitivity: "base"`: only different letters differ; `a`, `A` and `á` are equal.
    Base,
}

fn collator(numeric: bool, sensitivity: Sensitivity) -> &'static CollatorBorrowed<'static> {
    static COLLATORS: [OnceLock<CollatorBorrowed<'static>>; 4] = [const { OnceLock::new() }; 4];
    let index = usize::from(numeric) * 2 + usize::from(sensitivity == Sensitivity::Base);
    COLLATORS[index].get_or_init(|| {
        let mut preferences = CollatorPreferences::default();
        preferences.numeric_ordering =
            Some(if numeric { CollationNumericOrdering::True } else { CollationNumericOrdering::False });
        let mut options = CollatorOptions::default();
        options.strength = Some(match sensitivity {
            Sensitivity::Variant => Strength::Tertiary,
            Sensitivity::Base => Strength::Primary,
        });
        // The root collation is compiled into the binary: building a collator for it cannot fail.
        Collator::try_new(preferences, options).expect("the root collation data is compiled in")
    })
}

/// `a.localeCompare(b)`: the default collation.
pub fn compare(a: &str, b: &str) -> Ordering {
    collator(false, Sensitivity::Variant).compare(a, b)
}

/// `Intl.Collator(undefined, { numeric, sensitivity }).compare(a, b)`.
pub fn compare_with(a: &str, b: &str, numeric: bool, sensitivity: Sensitivity) -> Ordering {
    collator(numeric, sensitivity).compare(a, b)
}

/// `a.localeCompare(b, undefined, { numeric: true })`: digit runs compare as numbers, `v2`
/// before `v10`; accents and case still count.
pub fn compare_numeric(a: &str, b: &str) -> Ordering {
    collator(true, Sensitivity::Variant).compare(a, b)
}

/// The order of JavaScript's `<` on strings: by UTF-16 code units. The tie-break the webview
/// applies where it needs a total order.
pub fn compare_code_units(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Natural, case-insensitive order (`numeric: true, sensitivity: "base"`): `file2` before
/// `file10`, `readme` equal to `README`. What file lists use.
pub fn compare_natural(a: &str, b: &str) -> Ordering {
    collator(true, Sensitivity::Base).compare(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Table {
        names: Vec<String>,
        default: Vec<usize>,
        base: Vec<usize>,
        numeric_base: Vec<usize>,
        numeric: Vec<usize>,
        default_signs: Vec<String>,
        base_signs: Vec<String>,
        numeric_base_signs: Vec<String>,
        numeric_signs: Vec<String>,
    }

    /// Every option set orders the reference names exactly as Node's `Intl.Collator("en")` does
    /// (`assets/gen/collation.json`, written by `docs/gpui-port/gen-data.mjs`): the sorted list,
    /// and the sign of every pair, so names held equal are told from names in order.
    #[test]
    fn orders_names_as_intl_collator_does() {
        let table: Table =
            serde_json::from_str(include_str!("../../assets/gen/collation.json")).expect("the table parses");
        assert!(table.names.len() > 60);
        for (label, expected, signs, numeric, sensitivity) in [
            ("default", &table.default, &table.default_signs, false, Sensitivity::Variant),
            ("base", &table.base, &table.base_signs, false, Sensitivity::Base),
            ("numericBase", &table.numeric_base, &table.numeric_base_signs, true, Sensitivity::Base),
            ("numeric", &table.numeric, &table.numeric_signs, true, Sensitivity::Variant),
        ] {
            assert_eq!(signs.len(), table.names.len(), "{label}");
            for (a, row) in table.names.iter().zip(signs) {
                let ours: String = table
                    .names
                    .iter()
                    .map(|b| match compare_with(a, b, numeric, sensitivity) {
                        Ordering::Equal => '=',
                        Ordering::Less => '<',
                        Ordering::Greater => '>',
                    })
                    .collect();
                assert_eq!(&ours, row, "{label}: {a:?} against every name");
            }
            let mut order: Vec<usize> = (0..table.names.len()).collect();
            // Stable, like `Array.prototype.sort`: names held equal keep their order.
            order.sort_by(|&a, &b| compare_with(&table.names[a], &table.names[b], numeric, sensitivity));
            let names = |order: &[usize]| order.iter().map(|&i| table.names[i].as_str()).collect::<Vec<_>>();
            assert_eq!(names(&order), names(expected), "{label}");
        }
    }

    #[test]
    fn the_three_entry_points_agree_with_their_options() {
        assert_eq!(compare("a", "B"), Ordering::Less);
        assert_eq!(compare("main", "Main"), Ordering::Less);
        assert_eq!(compare("_x", "1"), Ordering::Less);
        assert_eq!(compare_natural("file2", "file10"), Ordering::Less);
        assert_eq!(compare_natural("README", "readme"), Ordering::Equal);
        assert_eq!(compare_with("résumé", "resume", false, Sensitivity::Base), Ordering::Equal);
        assert_eq!(compare("file2", "file10"), Ordering::Greater);
    }
}
