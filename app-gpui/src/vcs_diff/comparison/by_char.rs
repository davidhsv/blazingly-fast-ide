//! Character-level comparison of punctuation, as the IntelliJ Platform does it
//! (`ByCharRt.comparePunctuation`; Apache-2.0, Copyright JetBrains s.r.o. and contributors): only
//! punctuation is matched, everything else stays changed.

use super::iterables::{ChangeBuilder, DiffIterable, diff_sequences, unchanged};
use super::lcs::DiffTooBig;
use super::text::is_punctuation;

/// The changes between two texts when only their punctuation characters can match.
pub(crate) fn compare_punctuation(text1: &[u16], text2: &[u16]) -> Result<DiffIterable, DiffTooBig> {
    let (codes1, offsets1) = punctuation(text1);
    let (codes2, offsets2) = punctuation(text2);
    let changes = diff_sequences(&codes1, &codes2)?;
    let mut builder = ChangeBuilder::new(text1.len(), text2.len());
    for r in unchanged(&changes) {
        for (index1, index2) in r.pairs() {
            builder.mark_equal(offsets1[index1], offsets2[index2]);
        }
    }
    Ok(builder.finish())
}

/// The punctuation characters of a text and their offsets.
fn punctuation(text: &[u16]) -> (Vec<u16>, Vec<usize>) {
    text.iter().enumerate().filter(|(_, unit)| is_punctuation(u32::from(**unit))).map(|(i, unit)| (*unit, i)).unzip()
}

#[cfg(test)]
mod tests {
    use super::super::lcs::Range;
    use super::*;

    // Not from a spec: matched punctuation splits the texts, whatever stands between it.
    #[test]
    fn matches_punctuation_only() {
        let units = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        let changes = compare_punctuation(&units("a(b)"), &units("(xy)")).expect("short texts are compared");
        assert_eq!(changes.changes, [Range::new(0, 1, 0, 0), Range::new(2, 3, 1, 3)]);
    }
}
