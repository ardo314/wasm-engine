use crate::{
    Component,
    bindings::exports::ardo314::math::{types::Vector2d, vector2d::Guest},
};

impl Guest for Component {
    fn add(lhs: Vector2d, rhs: Vector2d) -> Vector2d {
        (lhs.0 + rhs.0, lhs.1 + rhs.1)
    }

    fn sub(lhs: Vector2d, rhs: Vector2d) -> Vector2d {
        (lhs.0 - rhs.0, lhs.1 - rhs.1)
    }

    fn dot(lhs: Vector2d, rhs: Vector2d) -> f32 {
        lhs.0 * rhs.0 + lhs.1 * rhs.1
    }

    fn add_f32(lhs: Vector2d, rhs: f32) -> Vector2d {
        (lhs.0 + rhs, lhs.1 + rhs)
    }

    fn sub_f32(lhs: Vector2d, rhs: f32) -> Vector2d {
        (lhs.0 - rhs, lhs.1 - rhs)
    }

    fn mul_f32(lhs: Vector2d, rhs: f32) -> Vector2d {
        (lhs.0 * rhs, lhs.1 * rhs)
    }

    fn div_f32(lhs: Vector2d, rhs: f32) -> Vector2d {
        (lhs.0 / rhs, lhs.1 / rhs)
    }

    fn neg(v: Vector2d) -> Vector2d {
        (-v.0, -v.1)
    }

    fn sqr_length(v: Vector2d) -> f32 {
        Self::dot(v, v)
    }

    fn length(v: Vector2d) -> f32 {
        Self::sqr_length(v).sqrt()
    }

    fn normalize(v: Vector2d) -> Vector2d {
        let len = Self::length(v);
        if len > 0.0 {
            (v.0 / len, v.1 / len)
        } else {
            (0.0, 0.0)
        }
    }
}
