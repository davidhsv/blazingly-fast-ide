//! Submenu aiming: while the pointer heads for an open submenu it may cross other menu items without
//! switching to them.

/// A pointer position, in the same coordinate space as the submenu's rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuPointer {
    pub x: f64,
    pub y: f64,
}

/// The edges of an open submenu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubmenuRect {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

/// How far above and below the submenu's near edge the triangle reaches.
const TOLERANCE: f64 = 8.0;

/// Whether this move stays in the triangle leading to the submenu's near edge.
pub fn is_submenu_aim(previous: Option<MenuPointer>, current: MenuPointer, rect: SubmenuRect) -> bool {
    let Some(previous) = previous else { return false };
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return false;
    }
    let edge = if previous.x < rect.left { rect.left } else { rect.right };
    let distance = edge - previous.x;
    if distance == 0.0 || distance.is_nan() || (rect.left..=rect.right).contains(&previous.x) {
        return false;
    }
    let progress = (current.x - previous.x) / distance;
    if progress <= 0.0 || progress > 1.0 {
        return false;
    }
    let top = previous.y + (rect.top - TOLERANCE - previous.y) * progress;
    let bottom = previous.y + (rect.bottom + TOLERANCE - previous.y) * progress;
    (top..=bottom).contains(&current.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: SubmenuRect = SubmenuRect { left: 182.0, right: 362.0, top: 6.0, bottom: 150.0 };

    fn at(x: f64, y: f64) -> MenuPointer {
        MenuPointer { x, y }
    }

    fn aim(previous: (f64, f64), current: (f64, f64), rect: SubmenuRect) -> bool {
        is_submenu_aim(Some(at(previous.0, previous.1)), at(current.0, current.1), rect)
    }

    // src/ui/menuAim.test.ts › isSubmenuAim › accepts diagonal movement toward either near corner and the gap
    #[test]
    fn accepts_diagonal_movement_toward_either_near_corner_and_the_gap() {
        assert!(aim((50.0, 20.0), (100.0, 45.0), RIGHT));
        assert!(aim((50.0, 80.0), (100.0, 55.0), RIGHT));
        assert!(aim((170.0, 40.0), (181.0, 60.0), RIGHT));
        assert!(aim((170.0, 40.0), (182.0, 155.0), RIGHT));
    }

    // src/ui/menuAim.test.ts › isSubmenuAim › rejects movement away, outside the triangle, or beyond the submenu
    #[test]
    fn rejects_movement_away_outside_the_triangle_or_beyond_the_submenu() {
        assert!(!aim((100.0, 45.0), (60.0, 45.0), RIGHT));
        assert!(!aim((50.0, 20.0), (60.0, 100.0), RIGHT));
        assert!(!aim((50.0, 20.0), (50.0, 35.0), RIGHT));
        assert!(!aim((50.0, 20.0), (400.0, 45.0), RIGHT));
    }

    // src/ui/menuAim.test.ts › isSubmenuAim › uses the right edge of a submenu flipped to the left
    #[test]
    fn uses_the_right_edge_of_a_submenu_flipped_to_the_left() {
        let left = SubmenuRect { left: 6.0, right: 180.0, top: 6.0, bottom: 150.0 };
        assert!(aim((300.0, 20.0), (250.0, 45.0), left));
        assert!(!aim((250.0, 45.0), (300.0, 45.0), left));
    }

    // src/ui/menuAim.test.ts › isSubmenuAim › rejects a first move, a stationary pointer and degenerate triangles
    #[test]
    fn rejects_a_first_move_a_stationary_pointer_and_degenerate_triangles() {
        assert!(!is_submenu_aim(None, at(100.0, 45.0), RIGHT));
        assert!(!aim((50.0, 20.0), (50.0, 20.0), RIGHT));
        assert!(!aim((182.0, 20.0), (182.0, 45.0), RIGHT));
        assert!(!aim((200.0, 20.0), (210.0, 45.0), RIGHT));
        assert!(!aim((50.0, 20.0), (100.0, 45.0), SubmenuRect { bottom: 6.0, ..RIGHT }));
    }
}
