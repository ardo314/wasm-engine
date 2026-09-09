use crate::{
    Component,
    bindings::exports::ardo314::math::{
        pose3d::Guest,
        types::{Point3d, Pose3d, RotationVector},
    },
};

impl Guest for Component {
    fn position(p: Pose3d) -> Point3d {
        (p.0, p.1, p.2)
    }

    fn rotation(p: Pose3d) -> RotationVector {
        (p.3, p.4, p.5)
    }
}
