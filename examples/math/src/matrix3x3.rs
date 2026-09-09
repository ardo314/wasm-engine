use crate::{
    Component,
    bindings::exports::ardo314::math::{
        matrix3x3::Guest,
        types::{Matrix3x3, Vector3d},
    },
};

impl Guest for Component {
    fn identity() -> Matrix3x3 {
        Matrix3x3 {
            m00: 1.0,
            m10: 0.0,
            m20: 0.0,
            m01: 0.0,
            m11: 1.0,
            m21: 0.0,
            m02: 0.0,
            m12: 0.0,
            m22: 1.0,
        }
    }

    fn mul(lhs: Matrix3x3, rhs: Matrix3x3) -> Matrix3x3 {
        Matrix3x3 {
            m00: lhs.m00 * rhs.m00 + lhs.m01 * rhs.m10 + lhs.m02 * rhs.m20,
            m01: lhs.m00 * rhs.m01 + lhs.m01 * rhs.m11 + lhs.m02 * rhs.m21,
            m02: lhs.m00 * rhs.m02 + lhs.m01 * rhs.m12 + lhs.m02 * rhs.m22,
            m10: lhs.m10 * rhs.m00 + lhs.m11 * rhs.m10 + lhs.m12 * rhs.m20,
            m11: lhs.m10 * rhs.m01 + lhs.m11 * rhs.m11 + lhs.m12 * rhs.m21,
            m12: lhs.m10 * rhs.m02 + lhs.m11 * rhs.m12 + lhs.m12 * rhs.m22,
            m20: lhs.m20 * rhs.m00 + lhs.m21 * rhs.m10 + lhs.m22 * rhs.m20,
            m21: lhs.m20 * rhs.m01 + lhs.m21 * rhs.m11 + lhs.m22 * rhs.m21,
            m22: lhs.m20 * rhs.m02 + lhs.m21 * rhs.m12 + lhs.m22 * rhs.m22,
        }
    }

    fn mul_vector3d(lhs: Matrix3x3, rhs: Vector3d) -> Vector3d {
        (
            lhs.m00 * rhs.0 + lhs.m01 * rhs.1 + lhs.m02 * rhs.2,
            lhs.m10 * rhs.0 + lhs.m11 * rhs.1 + lhs.m12 * rhs.2,
            lhs.m20 * rhs.0 + lhs.m21 * rhs.1 + lhs.m22 * rhs.2,
        )
    }
}
