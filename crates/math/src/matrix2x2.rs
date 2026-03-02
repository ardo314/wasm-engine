use crate::{
    Component,
    bindings::exports::ardo314::math::{
        matrix2x2::Guest,
        types::{Matrix2x2, Vector2d},
    },
};

impl Guest for Component {
    fn identity() -> Matrix2x2 {
        Matrix2x2 {
            m00: 1.0,
            m10: 0.0,
            m01: 0.0,
            m11: 1.0,
        }
    }

    fn mul(lhs: Matrix2x2, rhs: Matrix2x2) -> Matrix2x2 {
        Matrix2x2 {
            m00: lhs.m00 * rhs.m00 + lhs.m01 * rhs.m10,
            m01: lhs.m00 * rhs.m01 + lhs.m01 * rhs.m11,
            m10: lhs.m10 * rhs.m00 + lhs.m11 * rhs.m10,
            m11: lhs.m10 * rhs.m01 + lhs.m11 * rhs.m11,
        }
    }

    fn mul_vector2d(lhs: Matrix2x2, rhs: Vector2d) -> Vector2d {
        (
            lhs.m00 * rhs.0 + lhs.m01 * rhs.1,
            lhs.m10 * rhs.0 + lhs.m11 * rhs.1,
        )
    }
}
