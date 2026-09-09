use crate::{
    Component,
    bindings::exports::ardo314::math::{
        axis_angle, quaternion,
        rotation_vector::Guest,
        types::{AxisAngle, Matrix3x3, Quaternion, RotationVector},
    },
};

fn length(rv: RotationVector) -> f32 {
    (rv.0 * rv.0 + rv.1 * rv.1 + rv.2 * rv.2).sqrt()
}

impl Guest for Component {
    fn identity() -> RotationVector {
        (0.0, 0.0, 0.0)
    }

    fn mul_f32(rv: RotationVector, s: f32) -> RotationVector {
        (rv.0 * s, rv.1 * s, rv.2 * s)
    }

    fn div_f32(rv: RotationVector, s: f32) -> RotationVector {
        (rv.0 / s, rv.1 / s, rv.2 / s)
    }

    fn to_axis_angle(rv: RotationVector) -> AxisAngle {
        let angle = length(rv);
        let axis = if angle > 0.0 {
            Self::div_f32(rv, angle)
        } else {
            (0.0, 0.0, 0.0)
        };
        (axis, angle)
    }

    fn to_quaternion(rv: RotationVector) -> Quaternion {
        let aa = Self::to_axis_angle(rv);
        <Component as axis_angle::Guest>::to_quaternion(aa)
    }

    fn to_matrix3x3(rv: RotationVector) -> Matrix3x3 {
        let q = Self::to_quaternion(rv);
        <Component as quaternion::Guest>::to_matrix3x3(q)
    }
}
