use crate::{
    Component,
    bindings::exports::ardo314::math::{
        axis_angle,
        quaternion::Guest,
        types::{AxisAngle, Matrix3x3, Quaternion, RotationVector, Vector3d},
    },
};

impl Guest for Component {
    fn identity() -> Quaternion {
        (0.0, 0.0, 0.0, 1.0)
    }

    fn to_rotation_vector(q: Quaternion) -> RotationVector {
        let aa = Self::to_axis_angle(q);
        <Component as axis_angle::Guest>::to_rotation_vector(aa)
    }

    fn to_axis_angle(q: Quaternion) -> AxisAngle {
        let mut axis: Vector3d = (0.0, 0.0, 0.0);
        let angle = 2.0 * q.3.acos();
        if 1.0 - (q.3 * q.3) < 0.000001 {
            axis.0 = q.0;
            axis.1 = q.1;
            axis.2 = q.2;
        } else {
            // http://www.euclideanspace.com/maths/geometry/rotations/conversions/quaternionToAngle/
            let s = (1.0 - (q.3 * q.3)).sqrt();
            axis.0 = q.0 / s;
            axis.1 = q.1 / s;
            axis.2 = q.2 / s;
        }
        (axis, angle)
    }

    fn to_matrix3x3(q: Quaternion) -> Matrix3x3 {
        let x = q.0;
        let y = q.1;
        let z = q.2;
        let w = q.3;
        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;

        Matrix3x3 {
            m00: 1.0 - (yy + zz),
            m10: xy + wz,
            m20: xz - wy,
            m01: xy - wz,
            m11: 1.0 - (xx + zz),
            m21: yz + wx,
            m02: xz + wy,
            m12: yz - wx,
            m22: 1.0 - (xx + yy),
        }
    }
}
