use crate::{
    Component,
    bindings::exports::ardo314::math::{
        matrix4x4::Guest,
        types::{Matrix4x4, Vector4d},
    },
};

impl Guest for Component {
    fn identity() -> Matrix4x4 {
        Matrix4x4 {
            m00: 1.0,
            m10: 0.0,
            m20: 0.0,
            m30: 0.0,
            m01: 0.0,
            m11: 1.0,
            m21: 0.0,
            m31: 0.0,
            m02: 0.0,
            m12: 0.0,
            m22: 1.0,
            m32: 0.0,
            m03: 0.0,
            m13: 0.0,
            m23: 0.0,
            m33: 1.0,
        }
    }

    fn mul(lhs: Matrix4x4, rhs: Matrix4x4) -> Matrix4x4 {
        Matrix4x4 {
            m00: lhs.m00 * rhs.m00 + lhs.m01 * rhs.m10 + lhs.m02 * rhs.m20 + lhs.m03 * rhs.m30,
            m01: lhs.m00 * rhs.m01 + lhs.m01 * rhs.m11 + lhs.m02 * rhs.m21 + lhs.m03 * rhs.m31,
            m02: lhs.m00 * rhs.m02 + lhs.m01 * rhs.m12 + lhs.m02 * rhs.m22 + lhs.m03 * rhs.m32,
            m03: lhs.m00 * rhs.m03 + lhs.m01 * rhs.m13 + lhs.m02 * rhs.m23 + lhs.m03 * rhs.m33,
            m10: lhs.m10 * rhs.m00 + lhs.m11 * rhs.m10 + lhs.m12 * rhs.m20 + lhs.m13 * rhs.m30,
            m11: lhs.m10 * rhs.m01 + lhs.m11 * rhs.m11 + lhs.m12 * rhs.m21 + lhs.m13 * rhs.m31,
            m12: lhs.m10 * rhs.m02 + lhs.m11 * rhs.m12 + lhs.m12 * rhs.m22 + lhs.m13 * rhs.m32,
            m13: lhs.m10 * rhs.m03 + lhs.m11 * rhs.m13 + lhs.m12 * rhs.m23 + lhs.m13 * rhs.m33,
            m20: lhs.m20 * rhs.m00 + lhs.m21 * rhs.m10 + lhs.m22 * rhs.m20 + lhs.m23 * rhs.m30,
            m21: lhs.m20 * rhs.m01 + lhs.m21 * rhs.m11 + lhs.m22 * rhs.m21 + lhs.m23 * rhs.m31,
            m22: lhs.m20 * rhs.m02 + lhs.m21 * rhs.m12 + lhs.m22 * rhs.m22 + lhs.m23 * rhs.m32,
            m23: lhs.m20 * rhs.m03 + lhs.m21 * rhs.m13 + lhs.m22 * rhs.m23 + lhs.m23 * rhs.m33,
            m30: lhs.m30 * rhs.m00 + lhs.m31 * rhs.m10 + lhs.m32 * rhs.m20 + lhs.m33 * rhs.m30,
            m31: lhs.m30 * rhs.m01 + lhs.m31 * rhs.m11 + lhs.m32 * rhs.m21 + lhs.m33 * rhs.m31,
            m32: lhs.m30 * rhs.m02 + lhs.m31 * rhs.m12 + lhs.m32 * rhs.m22 + lhs.m33 * rhs.m32,
            m33: lhs.m30 * rhs.m03 + lhs.m31 * rhs.m13 + lhs.m32 * rhs.m23 + lhs.m33 * rhs.m33,
        }
    }

    fn mul_vector4d(lhs: Matrix4x4, rhs: Vector4d) -> Vector4d {
        (
            lhs.m00 * rhs.0 + lhs.m01 * rhs.1 + lhs.m02 * rhs.2 + lhs.m03 * rhs.3,
            lhs.m10 * rhs.0 + lhs.m11 * rhs.1 + lhs.m12 * rhs.2 + lhs.m13 * rhs.3,
            lhs.m20 * rhs.0 + lhs.m21 * rhs.1 + lhs.m22 * rhs.2 + lhs.m23 * rhs.3,
            lhs.m30 * rhs.0 + lhs.m31 * rhs.1 + lhs.m32 * rhs.2 + lhs.m33 * rhs.3,
        )
    }
}
