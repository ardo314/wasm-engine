use crate::{
    Component,
    bindings::exports::ardo314::math::{
        pose2d::Guest,
        types::{Point2d, Pose2d},
    },
};

impl Guest for Component {
    fn position(p: Pose2d) -> Point2d {
        (p.0, p.1)
    }

    fn rotation(p: Pose2d) -> f32 {
        p.2
    }
}
