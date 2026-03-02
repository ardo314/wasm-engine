use crate::{
    Component,
    bindings::exports::ardo314::math::{
        point3d::Guest,
        types::{Point3d, Vector3d},
    },
};

impl Guest for Component {
    fn add_vector3d(p: Point3d, v: Vector3d) -> Point3d {
        (p.0 + v.0, p.1 + v.1, p.2 + v.2)
    }

    fn sub_vector3d(p: Point3d, v: Vector3d) -> Point3d {
        (p.0 - v.0, p.1 - v.1, p.2 - v.2)
    }
}
