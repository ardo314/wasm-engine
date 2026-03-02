use crate::{
    Component,
    bindings::exports::ardo314::math::{
        point2d::Guest,
        types::{Point2d, Vector2d},
    },
};

impl Guest for Component {
    fn add_vector2d(p: Point2d, v: Vector2d) -> Point2d {
        (p.0 + v.0, p.1 + v.1)
    }

    fn sub_vector2d(p: Point2d, v: Vector2d) -> Point2d {
        (p.0 - v.0, p.1 - v.1)
    }
}
