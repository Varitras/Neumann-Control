//! Where an element dragged in edit mode lands: above or below the fader by the pointer's
//! height, before the first other element of that area the pointer is not past yet (reading
//! order), else at the area's end. The strip draws that landing while the drag lasts.

use eframe::egui::{Pos2, Rect};
use neumann_control_core::settings::StripElement;

use crate::strip_layout::{Area, LayoutChange, StripLayout};

/// What a drag in edit mode carries: the element, and whose layout it belongs to
/// (`LayoutOwner::key`).
#[derive(Clone, Debug, PartialEq)]
pub struct Dragged {
    pub layout: String,
    pub element: StripElement,
}

/// One element as the strip last drew it.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub area: Area,
    pub element: StripElement,
    pub rect: Rect,
}

/// Where a strip last drew its elements, its fader and itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub slots: Vec<Slot>,
    pub fader: Rect,
    pub strip: Rect,
}

/// The change that puts `dragged` where the pointer is, counted in the stored list of the area
/// without `dragged`, so an element the speaker lacks keeps its place.
pub fn landing(
    geometry: &Geometry,
    layout: &StripLayout,
    dragged: &StripElement,
    pointer: Pos2,
) -> LayoutChange {
    let area = if pointer.y < geometry.fader.center().y {
        Area::Above
    } else {
        Area::Below
    };
    let next = geometry
        .slots
        .iter()
        .filter(|slot| slot.area == area && slot.element != *dragged)
        .find(|slot| comes_after(pointer, slot.rect))
        .map(|slot| &slot.element);
    let stored = match area {
        Area::Above => &layout.above,
        Area::Below => &layout.below,
    };
    let others: Vec<&StripElement> = stored
        .iter()
        .filter(|element| *element != dragged)
        .collect();
    let index = next
        .and_then(|next| others.iter().position(|element| *element == next))
        .unwrap_or(others.len());
    LayoutChange::MoveTo(dragged.clone(), area, index)
}

/// Whether an element at `rect` comes after the pointer in reading order: on a later line, or
/// on the pointer's line with its middle not left of the pointer.
fn comes_after(pointer: Pos2, rect: Rect) -> bool {
    pointer.y < rect.top() || (pointer.y <= rect.bottom() && pointer.x <= rect.center().x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use eframe::egui::{pos2, vec2};
    use neumann_control_core::ssc::wire::SscPath;

    fn path(text: &str) -> StripElement {
        StripElement::Path(SscPath::parse(text).unwrap())
    }

    fn at(x: f32, y: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(40.0, 20.0))
    }

    /// Delay and standby knobs side by side above a fader at y 100–200; mute and identify on
    /// one line below, EQ on the next.
    fn strip() -> (Geometry, StripLayout) {
        let slot = |area, element, rect| Slot {
            area,
            element,
            rect,
        };
        let geometry = Geometry {
            slots: vec![
                slot(Area::Above, path(paths::DELAY), at(0.0, 40.0)),
                slot(Area::Above, path(paths::STANDBY_TIME), at(60.0, 40.0)),
                slot(Area::Below, path(paths::MUTE), at(0.0, 220.0)),
                slot(Area::Below, path(paths::IDENTIFY), at(60.0, 220.0)),
                slot(Area::Below, StripElement::Eq, at(0.0, 250.0)),
            ],
            fader: Rect::from_min_max(pos2(0.0, 100.0), pos2(120.0, 200.0)),
            strip: Rect::from_min_max(pos2(0.0, 0.0), pos2(130.0, 300.0)),
        };
        let layout = StripLayout {
            above: vec![path(paths::DELAY), path(paths::STANDBY_TIME)],
            below: vec![path(paths::MUTE), path(paths::IDENTIFY), StripElement::Eq],
        };
        (geometry, layout)
    }

    #[test]
    fn let_go_on_an_element_lands_before_it() {
        let (geometry, layout) = strip();
        let on_mute = at(0.0, 220.0).center();
        assert_eq!(
            landing(&geometry, &layout, &path(paths::DELAY), on_mute),
            LayoutChange::MoveTo(path(paths::DELAY), Area::Below, 0)
        );
    }

    #[test]
    fn right_of_an_elements_middle_lands_after_it() {
        let (geometry, layout) = strip();
        let pointer = at(0.0, 220.0).center() + vec2(10.0, 0.0);
        assert_eq!(
            landing(&geometry, &layout, &path(paths::DELAY), pointer),
            LayoutChange::MoveTo(path(paths::DELAY), Area::Below, 1)
        );
    }

    #[test]
    fn past_the_last_element_lands_at_the_end() {
        let (geometry, layout) = strip();
        assert_eq!(
            landing(&geometry, &layout, &path(paths::DELAY), pos2(100.0, 290.0)),
            LayoutChange::MoveTo(path(paths::DELAY), Area::Below, 3)
        );
    }

    #[test]
    fn above_the_faders_middle_is_the_upper_area() {
        let (geometry, layout) = strip();
        let pointer = pos2(10.0, 120.0);
        assert_eq!(
            landing(&geometry, &layout, &StripElement::Eq, pointer),
            LayoutChange::MoveTo(StripElement::Eq, Area::Above, 2)
        );
        let before_standby = at(60.0, 40.0).center();
        assert_eq!(
            landing(&geometry, &layout, &path(paths::MUTE), before_standby),
            LayoutChange::MoveTo(path(paths::MUTE), Area::Above, 1)
        );
    }

    /// The dragged element is not a target of its own, and its own place does not count.
    #[test]
    fn the_dragged_element_is_left_out() {
        let (geometry, layout) = strip();
        let on_itself = at(60.0, 220.0).center();
        assert_eq!(
            landing(&geometry, &layout, &path(paths::IDENTIFY), on_itself),
            LayoutChange::MoveTo(path(paths::IDENTIFY), Area::Below, 1)
        );
    }

    /// An element the speaker lacks is not drawn but keeps its place in the stored list.
    #[test]
    fn an_absent_element_keeps_its_stored_place() {
        let (geometry, mut layout) = strip();
        layout.below.insert(1, path(paths::PHASE_INVERSION));
        let on_identify = at(60.0, 220.0).center();
        assert_eq!(
            landing(&geometry, &layout, &path(paths::DELAY), on_identify),
            LayoutChange::MoveTo(path(paths::DELAY), Area::Below, 2)
        );
    }
}
