use crate::{
    Component,
    bindings::exports::ardo314::math::{
        axis_angle, quaternion,
        rotation_matrix3x3::Guest,
        types::{AxisAngle, Matrix3x3, Quaternion, RotationVector},
    },
};

impl Guest for Component {
    fn to_axis_angle(m: Matrix3x3) -> AxisAngle {
        let q = Self::to_quaternion(m);
        <Component as quaternion::Guest>::to_axis_angle(q)
    }

    fn to_rotation_vector(m: Matrix3x3) -> RotationVector {
        let aa = Self::to_axis_angle(m);
        <Component as axis_angle::Guest>::to_rotation_vector(aa)
    }

    fn to_quaternion(m: Matrix3x3) -> Quaternion {
        let tr = m.m00 + m.m11 + m.m22;

        if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0; // S=4*qw
            (
                (m.m21 - m.m12) / s,
                (m.m02 - m.m20) / s,
                (m.m10 - m.m01) / s,
                0.25 * s,
            )
        } else if (m.m00 > m.m11) & (m.m00 > m.m22) {
            let s = (1.0 + m.m00 - m.m11 - m.m22).sqrt() * 2.0; // S=4*qx 
            (
                0.25 * s,
                (m.m01 + m.m10) / s,
                (m.m02 + m.m20) / s,
                (m.m21 - m.m12) / s,
            )
        } else if m.m11 > m.m22 {
            let s = (1.0 + m.m11 - m.m00 - m.m22).sqrt() * 2.0; // S=4*qy
            (
                (m.m01 + m.m10) / s,
                0.25 * s,
                (m.m12 + m.m21) / s,
                (m.m02 - m.m20) / s,
            )
        } else {
            let s = (1.0 + m.m22 - m.m00 - m.m11).sqrt() * 2.0; // S=4*qz
            (
                (m.m02 + m.m20) / s,
                (m.m12 + m.m21) / s,
                0.25 * s,
                (m.m10 - m.m01) / s,
            )
        }
    }
}
