use crate::{
    Component,
    bindings::exports::ardo314::math::{rotation_matrix2x2::Guest, types::Matrix2x2},
};

impl Guest for Component {
    fn to_angle(m: Matrix2x2) -> f32 {
        m.m10.atan2(m.m00)
    }

    fn from_angle(a: f32) -> Matrix2x2 {
        let cos_a = a.cos();
        let sin_a = a.sin();

        Matrix2x2 {
            m00: cos_a,
            m10: sin_a,
            m01: -sin_a,
            m11: cos_a,
        }
    }
}
