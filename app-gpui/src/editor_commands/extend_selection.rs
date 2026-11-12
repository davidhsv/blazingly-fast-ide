//! Extend Selection's backend chain: the ranges the structure service answered around a position,
//! kept for the repeated presses of one extend.

/// The selection ranges the backend answered around `pos`, innermost first (UTF-16 offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendChain {
    /// The document's change counter when the chain was computed.
    pub counter: u64,
    /// The offset the backend was asked at.
    pub pos: u32,
    pub ranges: Vec<(u32, u32)>,
}

/// The chain of a previous extend answers again while the document is unchanged and the selection
/// `current` still covers its position: every node around the selection is around that position
/// too, so repeated presses need no backend round trip.
pub fn reusable_chain(chain: Option<&BackendChain>, counter: u64, current: (u32, u32)) -> Option<&[(u32, u32)]> {
    chain
        .filter(|chain| chain.counter == counter && chain.pos >= current.0 && chain.pos <= current.1)
        .map(|chain| chain.ranges.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> BackendChain {
        BackendChain { counter: 3, pos: 10, ranges: vec![(8, 14), (0, 40)] }
    }

    #[test]
    fn reuses_the_backend_chain_while_the_document_is_unchanged_and_the_selection_covers_its_position() {
        let chain = chain();
        assert_eq!(reusable_chain(Some(&chain), 3, (10, 10)), Some(chain.ranges.as_slice()));
        assert_eq!(reusable_chain(Some(&chain), 3, (8, 14)), Some(chain.ranges.as_slice()));
    }

    #[test]
    fn asks_again_after_an_edit_or_for_a_selection_elsewhere() {
        let chain = chain();
        assert_eq!(reusable_chain(Some(&chain), 4, (10, 10)), None);
        assert_eq!(reusable_chain(Some(&chain), 3, (11, 20)), None);
        assert_eq!(reusable_chain(None, 3, (10, 10)), None);
    }
}
