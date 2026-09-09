use crate::{
    Component,
    bindings::exports::ardo314::math::{types::Vector3d, vector3d::Guest},
};

impl Guest for Component {
    fn add(lhs: Vector3d, rhs: Vector3d) -> Vector3d {
        (lhs.0 + rhs.0, lhs.1 + rhs.1, lhs.2 + rhs.2)
    }

    fn sub(lhs: Vector3d, rhs: Vector3d) -> Vector3d {
        (lhs.0 - rhs.0, lhs.1 - rhs.1, lhs.2 - rhs.2)
    }

    fn dot(lhs: Vector3d, rhs: Vector3d) -> f32 {
        lhs.0 * rhs.0 + lhs.1 * rhs.1 + lhs.2 * rhs.2
    }

    fn add_f32(lhs: Vector3d, rhs: f32) -> Vector3d {
        (lhs.0 + rhs, lhs.1 + rhs, lhs.2 + rhs)
    }

    fn sub_f32(lhs: Vector3d, rhs: f32) -> Vector3d {
        (lhs.0 - rhs, lhs.1 - rhs, lhs.2 - rhs)
    }

    fn mul_f32(lhs: Vector3d, rhs: f32) -> Vector3d {
        (lhs.0 * rhs, lhs.1 * rhs, lhs.2 * rhs)
    }

    fn div_f32(lhs: Vector3d, rhs: f32) -> Vector3d {
        (lhs.0 / rhs, lhs.1 / rhs, lhs.2 / rhs)
    }

    fn neg(v: Vector3d) -> Vector3d {
        (-v.0, -v.1, -v.2)
    }

    fn sqr_length(v: Vector3d) -> f32 {
        Self::dot(v, v)
    }

    fn length(v: Vector3d) -> f32 {
        Self::sqr_length(v).sqrt()
    }

    fn normalize(v: Vector3d) -> Vector3d {
        let len = Self::length(v);
        if len > 0.0 {
            (v.0 / len, v.1 / len, v.2 / len)
        } else {
            (0.0, 0.0, 0.0)
        }
    }

    fn cross(lhs: Vector3d, rhs: Vector3d) -> Vector3d {
        (
            lhs.1 * rhs.2 - lhs.2 * rhs.1,
            lhs.2 * rhs.0 - lhs.0 * rhs.2,
            lhs.0 * rhs.1 - lhs.1 * rhs.0,
        )
    }
}
