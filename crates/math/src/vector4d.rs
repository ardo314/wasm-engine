use crate::{
    Component,
    bindings::exports::ardo314::math::{types::Vector4d, vector4d::Guest},
};

impl Guest for Component {
    fn add(lhs: Vector4d, rhs: Vector4d) -> Vector4d {
        (lhs.0 + rhs.0, lhs.1 + rhs.1, lhs.2 + rhs.2, lhs.3 + rhs.3)
    }

    fn sub(lhs: Vector4d, rhs: Vector4d) -> Vector4d {
        (lhs.0 - rhs.0, lhs.1 - rhs.1, lhs.2 - rhs.2, lhs.3 - rhs.3)
    }

    fn dot(lhs: Vector4d, rhs: Vector4d) -> f32 {
        lhs.0 * rhs.0 + lhs.1 * rhs.1 + lhs.2 * rhs.2 + lhs.3 * rhs.3
    }

    fn add_f32(lhs: Vector4d, rhs: f32) -> Vector4d {
        (lhs.0 + rhs, lhs.1 + rhs, lhs.2 + rhs, lhs.3 + rhs)
    }

    fn sub_f32(lhs: Vector4d, rhs: f32) -> Vector4d {
        (lhs.0 - rhs, lhs.1 - rhs, lhs.2 - rhs, lhs.3 - rhs)
    }

    fn mul_f32(lhs: Vector4d, rhs: f32) -> Vector4d {
        (lhs.0 * rhs, lhs.1 * rhs, lhs.2 * rhs, lhs.3 * rhs)
    }

    fn div_f32(lhs: Vector4d, rhs: f32) -> Vector4d {
        (lhs.0 / rhs, lhs.1 / rhs, lhs.2 / rhs, lhs.3 / rhs)
    }

    fn neg(v: Vector4d) -> Vector4d {
        (-v.0, -v.1, -v.2, -v.3)
    }

    fn sqr_length(v: Vector4d) -> f32 {
        Self::dot(v, v)
    }

    fn length(v: Vector4d) -> f32 {
        Self::sqr_length(v).sqrt()
    }

    fn normalize(v: Vector4d) -> Vector4d {
        let len = Self::length(v);
        if len > 0.0 {
            (v.0 / len, v.1 / len, v.2 / len, v.3 / len)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    }
}
