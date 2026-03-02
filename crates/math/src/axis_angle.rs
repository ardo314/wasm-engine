use crate::{
    Component,
    bindings::exports::ardo314::math::{
        axis_angle::Guest,
        quaternion,
        types::{AxisAngle, Matrix3x3, Quaternion, RotationVector},
    },
};

impl Guest for Component {
    fn identity() -> AxisAngle {
        ((0.0, 0.0, 0.0), 0.0)
    }

    fn mul_f32(aa: AxisAngle, s: f32) -> AxisAngle {
        (aa.0, aa.1 * s)
    }

    fn div_f32(aa: AxisAngle, s: f32) -> AxisAngle {
        (aa.0, aa.1 / s)
    }

    fn to_rotation_vector(aa: AxisAngle) -> RotationVector {
        (aa.0.0 * aa.1, aa.0.1 * aa.1, aa.0.2 * aa.1)
    }

    fn to_quaternion(aa: AxisAngle) -> Quaternion {
        let half_angle = aa.1 / 2.0;
        let s = half_angle.sin();

        let x = aa.0.0 * s;
        let y = aa.0.1 * s;
        let z = aa.0.2 * s;
        let w = half_angle.cos();
        (x, y, z, w)
    }

    fn to_matrix3x3(aa: AxisAngle) -> Matrix3x3 {
        let q = Self::to_quaternion(aa);
        <Component as quaternion::Guest>::to_matrix3x3(q)
    }
}
