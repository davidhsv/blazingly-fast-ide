//! Trackpad page swipes (Back / Forward) against horizontal scrolling: a two-finger swipe that
//! starts over content which scrolls sideways belongs to that content for the whole gesture,
//! even when it starts at an edge. Which action a swipe runs is the keymap's
//! ([`crate::platform::dispatcher::Dispatcher::swipe`]).

/// One element of the chain from what is under the pointer up to the window.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SwipeRegion {
    /// A terminal: the application in it consumes wheel input itself (alternate-screen
    /// programs included).
    pub terminal: bool,
    /// The element handles the wheel on its own account (image panning, a diff's divider).
    pub handles_wheel: bool,
    /// The element scrolls what overflows it sideways. Content that is merely clipped does not
    /// count.
    pub scrolls_horizontally: bool,
    /// Width of the content.
    pub content_width: f64,
    /// Width of the element.
    pub width: f64,
}

/// Whether the element under the pointer, or one around it, keeps horizontal wheel input:
/// `chain` lists them innermost first.
pub fn owns_horizontal_scroll(chain: &[SwipeRegion]) -> bool {
    chain.iter().any(|region| {
        region.terminal || region.handles_wheel || (region.scrolls_horizontally && region.content_width > region.width)
    })
}

/// Whether a swipe that may begin over `chain` is tracked as a page swipe. `None`: nothing of
/// the window is under the pointer.
pub fn tracks_page_swipe(chain: Option<&[SwipeRegion]>) -> bool {
    chain.is_some_and(|chain| !owns_horizontal_scroll(chain))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: SwipeRegion = SwipeRegion {
        terminal: false,
        handles_wheel: false,
        scrolls_horizontally: false,
        content_width: 0.0,
        width: 0.0,
    };
    const WIDE: SwipeRegion = SwipeRegion { content_width: 500.0, width: 100.0, ..PLAIN };

    // native page swipes › only starts native tracking for the requesting window and a non-scrolling target
    #[test]
    fn only_starts_native_tracking_for_a_non_scrolling_target() {
        let in_terminal = [PLAIN, SwipeRegion { terminal: true, ..PLAIN }];
        assert!(!tracks_page_swipe(Some(&in_terminal)));
        let on_a_button = [PLAIN, PLAIN];
        assert!(tracks_page_swipe(Some(&on_a_button)));
        assert!(!tracks_page_swipe(None));
    }

    // native page swipes › preserves horizontal %s scrolling even at the edge
    #[test]
    fn preserves_horizontal_scrolling_even_at_the_edge() {
        let scroller = SwipeRegion { scrolls_horizontally: true, ..WIDE };
        assert!(owns_horizontal_scroll(&[PLAIN, scroller]));
    }

    // native page swipes › does not classify clipped content or vertical-only content as horizontal scrolling
    #[test]
    fn does_not_classify_clipped_content_or_vertical_only_content_as_horizontal_scrolling() {
        assert!(!owns_horizontal_scroll(&[WIDE]));
        assert!(!owns_horizontal_scroll(&[SwipeRegion { scrolls_horizontally: true, ..PLAIN }]));
        assert!(!owns_horizontal_scroll(&[]));
    }

    // native page swipes › preserves custom wheel handlers such as image panning and diff dividers
    #[test]
    fn preserves_custom_wheel_handlers_such_as_image_panning_and_diff_dividers() {
        assert!(owns_horizontal_scroll(&[PLAIN, SwipeRegion { handles_wheel: true, ..PLAIN }]));
    }

    // native page swipes › leaves terminal wheel handling to the terminal application
    #[test]
    fn leaves_terminal_wheel_handling_to_the_terminal_application() {
        assert!(owns_horizontal_scroll(&[PLAIN, SwipeRegion { terminal: true, ..PLAIN }]));
    }
}
