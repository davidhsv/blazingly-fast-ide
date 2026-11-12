//! Pointer-driven drag and drop between components: a press becomes a drag once the pointer
//! moved far enough, a ghost of the source follows it, and the drop target under the pointer is
//! told about the drag.
//!
//! The caller owns the pointer events and the hit test: it says where the pointer is and which
//! target (if any) lies under it, and carries out the effects it gets back.

/// Project files dragged from the Project view: the payload is the paths joined by `\n`,
/// folders ending with `/`.
pub const FILES_DRAG: &str = "application/x-openidea-files";

/// Movement before a press becomes a drag, in pixels.
pub const DRAG_THRESHOLD: f64 = 4.0;

/// What is dragged: targets match on `kind` (e.g. `application/x-openidea-editor-tab`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragData {
    pub kind: String,
    pub payload: String,
}

/// A rectangle by its edges; `right` and `bottom` are outside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edges {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// Whether the point is outside `viewport` (the window's content area): a tab drag released
/// there detaches into a new window instead of reordering.
pub fn is_outside_window(viewport: Edges, x: f64, y: f64) -> bool {
    x < viewport.left || x >= viewport.right || y < viewport.top || y >= viewport.bottom
}

/// A rectangle by its origin and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Where a drop lands on a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

/// Where a drop at (`x`, `y`) over `rect` lands: the nearest edge when within its outer quarter
/// (a tab dropped there splits off a new pane), else the middle.
pub fn drop_zone_at(x: f64, y: f64, rect: Bounds) -> DropZone {
    let fx = (x - rect.left) / rect.width.max(1.0);
    let fy = (y - rect.top) / rect.height.max(1.0);
    let edges = [(DropZone::Left, fx), (DropZone::Right, 1.0 - fx), (DropZone::Top, fy), (DropZone::Bottom, 1.0 - fy)];
    // The first of equally near edges wins.
    let nearest = edges.into_iter().min_by(|a, b| a.1.total_cmp(&b.1));
    nearest.filter(|(_, distance)| *distance < 0.25).map_or(DropZone::Center, |(edge, _)| edge)
}

/// What a drop target is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetEvent<T> {
    /// The pointer moved over the target.
    Over(T),
    /// The pointer left the target, or the drag ended elsewhere or was cancelled.
    Leave(T),
    Drop(T),
}

/// The target a drag is over. `T` identifies a registered drop target that accepts the drag;
/// the caller finds the innermost such target under the pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragSession<T> {
    current: Option<T>,
}

impl<T> Default for DragSession<T> {
    fn default() -> Self {
        Self { current: None }
    }
}

impl<T: Clone + PartialEq> DragSession<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the drag is over a target that accepts it.
    pub fn has_target(&self) -> bool {
        self.current.is_some()
    }

    fn enter(&mut self, target: Option<T>, events: &mut Vec<TargetEvent<T>>) {
        if self.current == target {
            return;
        }
        if let Some(left) = std::mem::replace(&mut self.current, target) {
            events.push(TargetEvent::Leave(left));
        }
    }

    /// The pointer moved; `under` is the accepting target below it.
    pub fn moved(&mut self, under: Option<T>) -> Vec<TargetEvent<T>> {
        let mut events = Vec::new();
        self.enter(under, &mut events);
        events.extend(self.current.clone().map(TargetEvent::Over));
        events
    }

    /// The pointer was released over `under`: the hovered target is left, then `under` gets the
    /// drop. Whether there was a target to drop on is whether the last event is a drop.
    pub fn dropped(&mut self, under: Option<T>) -> Vec<TargetEvent<T>> {
        let mut events = Vec::new();
        self.enter(None, &mut events);
        events.extend(under.map(TargetEvent::Drop));
        events
    }

    pub fn cancel(&mut self) -> Vec<TargetEvent<T>> {
        let mut events = Vec::new();
        self.enter(None, &mut events);
        events
    }
}

/// Where a drag was released.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerDrop {
    pub x: f64,
    pub y: f64,
    /// A registered target accepted the drop.
    pub accepted: bool,
}

/// What the owner of the pointer events carries out.
#[derive(Debug, Clone, PartialEq)]
pub enum DragEffect<T> {
    /// The press became a drag: show the ghost of the source.
    Started,
    /// Move the ghost by this much from where the source was; `no_target` marks it as not
    /// droppable here.
    Ghost {
        dx: f64,
        dy: f64,
        no_target: bool,
    },
    Target(TargetEvent<T>),
    /// The gesture is over: remove the ghost. `dragged` tells whether a drag happened (the
    /// click that follows is then swallowed); `drop` is set when the drag ended with a release
    /// rather than a cancel.
    Ended {
        dragged: bool,
        drop: Option<PointerDrop>,
    },
}

/// A primary-button press that may become a drag.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerDrag<T> {
    start: (f64, f64),
    dragging: bool,
    /// The gesture ended: later events of the same press are ignored.
    finished: bool,
    session: DragSession<T>,
}

impl<T: Clone + PartialEq> PointerDrag<T> {
    /// A press at (`x`, `y`). `None` when it cannot start a drag: another button, or Alt is
    /// required for drags ("Drag-and-drop with Alt pressed only") and is not held.
    pub fn press(primary_button: bool, x: f64, y: f64, alt: bool, requires_alt: bool) -> Option<Self> {
        (primary_button && (alt || !requires_alt)).then(|| Self {
            start: (x, y),
            dragging: false,
            finished: false,
            session: DragSession::new(),
        })
    }

    /// Whether the press became a drag.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    fn end(&mut self, dragged: bool, drop: Option<PointerDrop>, effects: &mut Vec<DragEffect<T>>) {
        self.finished = true;
        effects.push(DragEffect::Ended { dragged, drop });
    }

    /// The pointer moved to (`x`, `y`) with the primary button held or not; `under` is the
    /// accepting drop target below it.
    pub fn moved(&mut self, primary_held: bool, x: f64, y: f64, under: Option<T>) -> Vec<DragEffect<T>> {
        let mut effects = Vec::new();
        if self.finished {
            return effects;
        }
        if !self.dragging {
            // A move without the button before any drag is a press whose release was missed:
            // it must not turn into a drag that nothing ends.
            if !primary_held {
                effects.extend(self.session.cancel().into_iter().map(DragEffect::Target));
                self.end(false, None, &mut effects);
                return effects;
            }
            if (x - self.start.0).hypot(y - self.start.1) < DRAG_THRESHOLD {
                return effects;
            }
            self.dragging = true;
            effects.push(DragEffect::Started);
        }
        // Late moves after the release may still precede it: they neither move nor cancel the drop.
        if !primary_held {
            return effects;
        }
        let events = self.session.moved(under);
        effects.push(DragEffect::Ghost {
            dx: x - self.start.0,
            dy: y - self.start.1,
            no_target: !self.session.has_target(),
        });
        effects.extend(events.into_iter().map(DragEffect::Target));
        effects
    }

    /// The button was released at (`x`, `y`) over the accepting target `under`.
    pub fn released(&mut self, x: f64, y: f64, under: Option<T>) -> Vec<DragEffect<T>> {
        let mut effects = Vec::new();
        if self.finished {
            return effects;
        }
        if !self.dragging {
            self.end(false, None, &mut effects);
            return effects;
        }
        let accepted = under.is_some();
        effects.extend(self.session.dropped(under).into_iter().map(DragEffect::Target));
        self.end(true, Some(PointerDrop { x, y, accepted }), &mut effects);
        effects
    }

    /// Escape was pressed. Cancels a drag in progress; a press that is not a drag yet keeps the
    /// key (nothing is returned).
    pub fn escape(&mut self) -> Vec<DragEffect<T>> {
        if self.dragging { self.cancel() } else { Vec::new() }
    }

    /// The pointer was taken away (or the owner gives the gesture up).
    pub fn cancel(&mut self) -> Vec<DragEffect<T>> {
        let mut effects = Vec::new();
        if self.finished {
            return effects;
        }
        effects.extend(self.session.cancel().into_iter().map(DragEffect::Target));
        let dragged = self.dragging;
        self.end(dragged, None, &mut effects);
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press() -> PointerDrag<&'static str> {
        PointerDrag::press(true, 0.0, 0.0, false, false).expect("a primary press starts a gesture")
    }

    fn ended(dragged: bool, drop: Option<PointerDrop>) -> DragEffect<&'static str> {
        DragEffect::Ended { dragged, drop }
    }

    // startPointerDrag with a reordered macOS tap › never starts a drag from moves with no button held
    #[test]
    fn never_starts_a_drag_from_moves_with_no_button_held() {
        let mut drag = press();
        assert_eq!(drag.moved(false, 40.0, 0.0, None), [ended(false, None)]);
        assert!(!drag.is_dragging());
        // The gesture ended once; the release that follows adds nothing.
        assert!(drag.released(40.0, 0.0, None).is_empty());
    }

    // startPointerDrag with a reordered macOS tap › drags and drops while the button is held
    #[test]
    fn drags_and_drops_while_the_button_is_held() {
        let mut drag = press();
        let effects = drag.moved(true, 40.0, 0.0, None);
        assert_eq!(effects, [DragEffect::Started, DragEffect::Ghost { dx: 40.0, dy: 0.0, no_target: true }]);
        let drop = PointerDrop { x: 40.0, y: 0.0, accepted: false };
        assert_eq!(drag.released(40.0, 0.0, None), [ended(true, Some(drop))]);
    }

    // startPointerDrag with a reordered macOS tap › keeps the drop when late moves arrive after the button was
    // released
    #[test]
    fn keeps_the_drop_when_late_moves_arrive_after_the_button_was_released() {
        let mut drag = press();
        assert_eq!(drag.moved(true, 40.0, 0.0, None).first(), Some(&DragEffect::Started));
        assert!(drag.moved(false, 60.0, 0.0, None).is_empty());
        let drop = PointerDrop { x: 60.0, y: 0.0, accepted: false };
        assert_eq!(drag.released(60.0, 0.0, None), [ended(true, Some(drop))]);
    }

    const VIEWPORT: Edges = Edges { left: 0.0, top: 0.0, right: 1280.0, bottom: 800.0 };

    // dropZoneAt › picks the nearest edge within its outer quarter, else the middle
    #[test]
    fn picks_the_nearest_edge_within_its_outer_quarter_else_the_middle() {
        let rect = Bounds { left: 100.0, top: 100.0, width: 400.0, height: 200.0 };
        assert_eq!(drop_zone_at(300.0, 200.0, rect), DropZone::Center);
        assert_eq!(drop_zone_at(110.0, 200.0, rect), DropZone::Left);
        assert_eq!(drop_zone_at(490.0, 200.0, rect), DropZone::Right);
        assert_eq!(drop_zone_at(300.0, 110.0, rect), DropZone::Top);
        assert_eq!(drop_zone_at(300.0, 290.0, rect), DropZone::Bottom);
    }

    // isOutsideWindow › treats the viewport interior as inside
    #[test]
    fn treats_the_viewport_interior_as_inside() {
        assert!(!is_outside_window(VIEWPORT, 640.0, 400.0));
        assert!(!is_outside_window(VIEWPORT, 0.0, 0.0));
        assert!(!is_outside_window(VIEWPORT, 1279.0, 799.0));
    }

    // isOutsideWindow › treats every side of the viewport as outside
    #[test]
    fn treats_every_side_of_the_viewport_as_outside() {
        assert!(is_outside_window(VIEWPORT, -1.0, 400.0));
        assert!(is_outside_window(VIEWPORT, 1280.0, 400.0));
        assert!(is_outside_window(VIEWPORT, 640.0, -1.0));
        assert!(is_outside_window(VIEWPORT, 640.0, 800.0));
    }

    // Not from a spec: a target is told when the drag is over it, leaves it and drops on it;
    // Escape cancels only a drag; a press below the threshold or without Alt where it is
    // required is not a drag.
    #[test]
    fn tells_targets_about_the_drag_and_cancels_on_escape() {
        let mut drag = press();
        assert!(drag.moved(true, 3.0, 0.0, Some("tabs")).is_empty());
        assert!(drag.escape().is_empty());
        let over = drag.moved(true, 10.0, 0.0, Some("tabs"));
        assert_eq!(
            over,
            [
                DragEffect::Started,
                DragEffect::Ghost { dx: 10.0, dy: 0.0, no_target: false },
                DragEffect::Target(TargetEvent::Over("tabs")),
            ]
        );
        let moved_on = drag.moved(true, 20.0, 0.0, Some("tree"));
        assert_eq!(
            moved_on[1..],
            [DragEffect::Target(TargetEvent::Leave("tabs")), DragEffect::Target(TargetEvent::Over("tree"))]
        );
        let drop = PointerDrop { x: 20.0, y: 0.0, accepted: true };
        assert_eq!(
            drag.released(20.0, 0.0, Some("tree")),
            [
                DragEffect::Target(TargetEvent::Leave("tree")),
                DragEffect::Target(TargetEvent::Drop("tree")),
                ended(true, Some(drop)),
            ]
        );

        let mut cancelled = press();
        assert_eq!(cancelled.moved(true, 10.0, 0.0, Some("tabs")).len(), 3);
        assert_eq!(cancelled.escape(), [DragEffect::Target(TargetEvent::Leave("tabs")), ended(true, None)]);
        assert!(cancelled.cancel().is_empty());

        assert!(PointerDrag::<&str>::press(false, 0.0, 0.0, false, false).is_none());
        assert!(PointerDrag::<&str>::press(true, 0.0, 0.0, false, true).is_none());
        assert!(PointerDrag::<&str>::press(true, 0.0, 0.0, true, true).is_some());
    }
}
