#[cfg(test)]
mod tests {
    use crate::{
        Component,
        bindings::exports::ardo314::math::{point3d::Guest, types::Point3d},
    };

    const EPSILON: f32 = 1e-6;

    fn assert_point_eq(a: Point3d, b: Point3d) {
        assert!(
            (a.0 - b.0).abs() < EPSILON,
            "x components differ: {} vs {}",
            a.0,
            b.0
        );
        assert!(
            (a.1 - b.1).abs() < EPSILON,
            "y components differ: {} vs {}",
            a.1,
            b.1
        );
        assert!(
            (a.2 - b.2).abs() < EPSILON,
            "z components differ: {} vs {}",
            a.2,
            b.2
        );
    }

    #[test]
    fn test_add_vector3d_basic() {
        let point = (1.0, 2.0, 3.0);
        let vector = (4.0, 5.0, 6.0);
        let result = <Component as Guest>::add_vector3d(point, vector);
        assert_point_eq(result, (5.0, 7.0, 9.0));
    }

    #[test]
    fn test_add_vector3d_zero_vector() {
        let point = (5.0, -3.0, 7.0);
        let zero_vector = (0.0, 0.0, 0.0);
        let result = <Component as Guest>::add_vector3d(point, zero_vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_add_vector3d_negative_components() {
        let point = (2.0, 3.0, 4.0);
        let vector = (-1.0, -2.0, -3.0);
        let result = <Component as Guest>::add_vector3d(point, vector);
        assert_point_eq(result, (1.0, 1.0, 1.0));
    }

    #[test]
    fn test_add_vector3d_origin() {
        let origin = (0.0, 0.0, 0.0);
        let vector = (7.0, -5.0, 2.0);
        let result = <Component as Guest>::add_vector3d(origin, vector);
        assert_point_eq(result, (7.0, -5.0, 2.0));
    }

    #[test]
    fn test_add_vector3d_fractional() {
        let point = (1.5, 2.25, 3.75);
        let vector = (0.75, -1.25, -0.5);
        let result = <Component as Guest>::add_vector3d(point, vector);
        assert_point_eq(result, (2.25, 1.0, 3.25));
    }

    #[test]
    fn test_sub_vector3d_basic() {
        let point = (5.0, 7.0, 9.0);
        let vector = (2.0, 3.0, 4.0);
        let result = <Component as Guest>::sub_vector3d(point, vector);
        assert_point_eq(result, (3.0, 4.0, 5.0));
    }

    #[test]
    fn test_sub_vector3d_zero_vector() {
        let point = (-2.0, 8.0, -1.0);
        let zero_vector = (0.0, 0.0, 0.0);
        let result = <Component as Guest>::sub_vector3d(point, zero_vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_sub_vector3d_negative_components() {
        let point = (1.0, 2.0, 3.0);
        let vector = (-3.0, -4.0, -5.0);
        let result = <Component as Guest>::sub_vector3d(point, vector);
        assert_point_eq(result, (4.0, 6.0, 8.0));
    }

    #[test]
    fn test_sub_vector3d_same_magnitude() {
        let point = (3.0, 4.0, 5.0);
        let vector = (3.0, 4.0, 5.0);
        let result = <Component as Guest>::sub_vector3d(point, vector);
        assert_point_eq(result, (0.0, 0.0, 0.0));
    }

    #[test]
    fn test_sub_vector3d_fractional() {
        let point = (2.75, 1.5, 4.25);
        let vector = (0.25, 2.5, 1.75);
        let result = <Component as Guest>::sub_vector3d(point, vector);
        assert_point_eq(result, (2.5, -1.0, 2.5));
    }

    #[test]
    fn test_add_sub_vector3d_inverse() {
        let point = (10.0, -5.0, 8.0);
        let vector = (3.0, 7.0, -2.0);

        // Adding and then subtracting the same vector should return to original point
        let added = <Component as Guest>::add_vector3d(point, vector);
        let result = <Component as Guest>::sub_vector3d(added, vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_sub_add_vector3d_inverse() {
        let point = (-1.0, 9.0, 3.5);
        let vector = (4.0, -2.0, 1.5);

        // Subtracting and then adding the same vector should return to original point
        let subtracted = <Component as Guest>::sub_vector3d(point, vector);
        let result = <Component as Guest>::add_vector3d(subtracted, vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_large_values() {
        let point = (1000000.0, -500000.0, 750000.0);
        let vector = (-999999.0, 500001.0, -749999.0);
        let result = <Component as Guest>::add_vector3d(point, vector);
        assert_point_eq(result, (1.0, 1.0, 1.0));
    }

    #[test]
    fn test_small_values() {
        let point = (0.0001, -0.0002, 0.0003);
        let vector = (0.0003, 0.0004, -0.0001);
        let result = <Component as Guest>::add_vector3d(point, vector);
        assert_point_eq(result, (0.0004, 0.0002, 0.0002));
    }

    #[test]
    fn test_translation_operations() {
        // Test common translation operations in 3D
        let origin = (0.0, 0.0, 0.0);

        // Translate along X-axis
        let right = (1.0, 0.0, 0.0);
        let translated_right = <Component as Guest>::add_vector3d(origin, right);
        assert_point_eq(translated_right, (1.0, 0.0, 0.0));

        // Translate along Y-axis
        let up = (0.0, 1.0, 0.0);
        let translated_up = <Component as Guest>::add_vector3d(origin, up);
        assert_point_eq(translated_up, (0.0, 1.0, 0.0));

        // Translate along Z-axis
        let forward = (0.0, 0.0, 1.0);
        let translated_forward = <Component as Guest>::add_vector3d(origin, forward);
        assert_point_eq(translated_forward, (0.0, 0.0, 1.0));

        // Translate diagonally in 3D space
        let diagonal = (1.0, 1.0, 1.0);
        let translated_diagonal = <Component as Guest>::add_vector3d(origin, diagonal);
        assert_point_eq(translated_diagonal, (1.0, 1.0, 1.0));
    }

    #[test]
    fn test_multiple_operations() {
        let start_point = (5.0, 3.0, -2.0);
        let vector1 = (2.0, -1.0, 4.0);
        let vector2 = (-3.0, 4.0, 1.0);

        // Apply multiple vector operations
        let intermediate = <Component as Guest>::add_vector3d(start_point, vector1);
        let result = <Component as Guest>::add_vector3d(intermediate, vector2);

        // Should be equivalent to adding the sum of vectors
        let vector_sum = (
            vector1.0 + vector2.0,
            vector1.1 + vector2.1,
            vector1.2 + vector2.2,
        );
        let expected = <Component as Guest>::add_vector3d(start_point, vector_sum);

        assert_point_eq(result, expected);
    }

    #[test]
    fn test_commutative_like_property() {
        // While point + vector != vector + point (types differ),
        // we can test that the displacement is consistent
        let point = (3.0, 4.0, 5.0);
        let vector = (1.0, 2.0, -1.0);

        let result1 = <Component as Guest>::add_vector3d(point, vector);
        let result2 = <Component as Guest>::sub_vector3d(point, (-vector.0, -vector.1, -vector.2));

        assert_point_eq(result1, result2);
    }

    #[test]
    fn test_zero_point_operations() {
        let zero_point = (0.0, 0.0, 0.0);
        let test_vector = (5.0, -3.0, 7.0);

        // Adding vector to origin should give the vector components as coordinates
        let result_add = <Component as Guest>::add_vector3d(zero_point, test_vector);
        assert_point_eq(result_add, test_vector);

        // Subtracting vector from origin should give negative vector components
        let result_sub = <Component as Guest>::sub_vector3d(zero_point, test_vector);
        assert_point_eq(result_sub, (-test_vector.0, -test_vector.1, -test_vector.2));
    }

    #[test]
    fn test_axis_aligned_operations() {
        // Test operations along each axis individually
        let point = (1.0, 2.0, 3.0);

        // X-axis movement
        let x_vector = (5.0, 0.0, 0.0);
        let x_result = <Component as Guest>::add_vector3d(point, x_vector);
        assert_point_eq(x_result, (6.0, 2.0, 3.0));

        // Y-axis movement
        let y_vector = (0.0, -3.0, 0.0);
        let y_result = <Component as Guest>::add_vector3d(point, y_vector);
        assert_point_eq(y_result, (1.0, -1.0, 3.0));

        // Z-axis movement
        let z_vector = (0.0, 0.0, 4.0);
        let z_result = <Component as Guest>::add_vector3d(point, z_vector);
        assert_point_eq(z_result, (1.0, 2.0, 7.0));
    }

    #[test]
    fn test_3d_cube_vertices() {
        // Test operations that create vertices of a unit cube
        let origin = (0.0, 0.0, 0.0);

        // Define vectors to each vertex of a unit cube
        let vertices = [
            (0.0, 0.0, 0.0), // origin
            (1.0, 0.0, 0.0), // +X
            (0.0, 1.0, 0.0), // +Y
            (0.0, 0.0, 1.0), // +Z
            (1.0, 1.0, 0.0), // +X+Y
            (1.0, 0.0, 1.0), // +X+Z
            (0.0, 1.0, 1.0), // +Y+Z
            (1.0, 1.0, 1.0), // +X+Y+Z
        ];

        for vertex in vertices {
            let result = <Component as Guest>::add_vector3d(origin, vertex);
            assert_point_eq(result, vertex);
        }
    }

    #[test]
    fn test_precision_edge_cases() {
        // Test very small differences to ensure epsilon comparison works
        let point = (1.0, 1.0, 1.0);
        let tiny_vector = (1e-7, -1e-7, 1e-7);
        let result = <Component as Guest>::add_vector3d(point, tiny_vector);

        // Should be very close to original point, within our epsilon
        assert_point_eq(result, (1.0, 1.0, 1.0));
    }
}
