#[cfg(test)]
mod tests {
    use crate::{
        Component,
        bindings::exports::ardo314::math::{point2d::Guest, types::Point2d},
    };

    const EPSILON: f32 = 1e-6;

    fn assert_point_eq(a: Point2d, b: Point2d) {
        assert!(
            (a.0 - b.0).abs() < EPSILON,
            "x <Component as Guest>s differ: {} vs {}",
            a.0,
            b.0
        );
        assert!(
            (a.1 - b.1).abs() < EPSILON,
            "y <Component as Guest>s differ: {} vs {}",
            a.1,
            b.1
        );
    }

    #[test]
    fn test_add_vector2d_basic() {
        let point = (1.0, 2.0);
        let vector = (3.0, 4.0);
        let result = <Component as Guest>::add_vector2d(point, vector);
        assert_point_eq(result, (4.0, 6.0));
    }

    #[test]
    fn test_add_vector2d_zero_vector() {
        let point = (5.0, -3.0);
        let zero_vector = (0.0, 0.0);
        let result = <Component as Guest>::add_vector2d(point, zero_vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_add_vector2d_negative_components() {
        let point = (2.0, 3.0);
        let vector = (-1.0, -2.0);
        let result = <Component as Guest>::add_vector2d(point, vector);
        assert_point_eq(result, (1.0, 1.0));
    }

    #[test]
    fn test_add_vector2d_origin() {
        let origin = (0.0, 0.0);
        let vector = (7.0, -5.0);
        let result = <Component as Guest>::add_vector2d(origin, vector);
        assert_point_eq(result, (7.0, -5.0));
    }

    #[test]
    fn test_add_vector2d_fractional() {
        let point = (1.5, 2.25);
        let vector = (0.75, -1.25);
        let result = <Component as Guest>::add_vector2d(point, vector);
        assert_point_eq(result, (2.25, 1.0));
    }

    #[test]
    fn test_sub_vector2d_basic() {
        let point = (5.0, 7.0);
        let vector = (2.0, 3.0);
        let result = <Component as Guest>::sub_vector2d(point, vector);
        assert_point_eq(result, (3.0, 4.0));
    }

    #[test]
    fn test_sub_vector2d_zero_vector() {
        let point = (-2.0, 8.0);
        let zero_vector = (0.0, 0.0);
        let result = <Component as Guest>::sub_vector2d(point, zero_vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_sub_vector2d_negative_components() {
        let point = (1.0, 2.0);
        let vector = (-3.0, -4.0);
        let result = <Component as Guest>::sub_vector2d(point, vector);
        assert_point_eq(result, (4.0, 6.0));
    }

    #[test]
    fn test_sub_vector2d_same_magnitude() {
        let point = (3.0, 4.0);
        let vector = (3.0, 4.0);
        let result = <Component as Guest>::sub_vector2d(point, vector);
        assert_point_eq(result, (0.0, 0.0));
    }

    #[test]
    fn test_sub_vector2d_fractional() {
        let point = (2.75, 1.5);
        let vector = (0.25, 2.5);
        let result = <Component as Guest>::sub_vector2d(point, vector);
        assert_point_eq(result, (2.5, -1.0));
    }

    #[test]
    fn test_add_sub_vector2d_inverse() {
        let point = (10.0, -5.0);
        let vector = (3.0, 7.0);

        // Adding and then subtracting the same vector should return to original point
        let added = <Component as Guest>::add_vector2d(point, vector);
        let result = <Component as Guest>::sub_vector2d(added, vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_sub_add_vector2d_inverse() {
        let point = (-1.0, 9.0);
        let vector = (4.0, -2.0);

        // Subtracting and then adding the same vector should return to original point
        let subtracted = <Component as Guest>::sub_vector2d(point, vector);
        let result = <Component as Guest>::add_vector2d(subtracted, vector);
        assert_point_eq(result, point);
    }

    #[test]
    fn test_large_values() {
        let point = (1000000.0, -500000.0);
        let vector = (-999999.0, 500001.0);
        let result = <Component as Guest>::add_vector2d(point, vector);
        assert_point_eq(result, (1.0, 1.0));
    }

    #[test]
    fn test_small_values() {
        let point = (0.0001, -0.0002);
        let vector = (0.0003, 0.0004);
        let result = <Component as Guest>::add_vector2d(point, vector);
        assert_point_eq(result, (0.0004, 0.0002));
    }

    #[test]
    fn test_translation_operations() {
        // Test common translation operations
        let origin = (0.0, 0.0);

        // Translate right
        let right = (1.0, 0.0);
        let translated_right = <Component as Guest>::add_vector2d(origin, right);
        assert_point_eq(translated_right, (1.0, 0.0));

        // Translate up
        let up = (0.0, 1.0);
        let translated_up = <Component as Guest>::add_vector2d(origin, up);
        assert_point_eq(translated_up, (0.0, 1.0));

        // Translate diagonally
        let diagonal = (1.0, 1.0);
        let translated_diagonal = <Component as Guest>::add_vector2d(origin, diagonal);
        assert_point_eq(translated_diagonal, (1.0, 1.0));
    }

    #[test]
    fn test_multiple_operations() {
        let start_point = (5.0, 3.0);
        let vector1 = (2.0, -1.0);
        let vector2 = (-3.0, 4.0);

        // Apply multiple vector operations
        let intermediate = <Component as Guest>::add_vector2d(start_point, vector1);
        let result = <Component as Guest>::add_vector2d(intermediate, vector2);

        // Should be equivalent to adding the sum of vectors
        let vector_sum = (vector1.0 + vector2.0, vector1.1 + vector2.1);
        let expected = <Component as Guest>::add_vector2d(start_point, vector_sum);

        assert_point_eq(result, expected);
    }

    #[test]
    fn test_commutative_like_property() {
        // While point + vector != vector + point (types differ),
        // we can test that the displacement is consistent
        let point = (3.0, 4.0);
        let vector = (1.0, 2.0);

        let result1 = <Component as Guest>::add_vector2d(point, vector);
        let result2 = <Component as Guest>::sub_vector2d(point, (-vector.0, -vector.1));

        assert_point_eq(result1, result2);
    }

    #[test]
    fn test_zero_point_operations() {
        let zero_point = (0.0, 0.0);
        let test_vector = (5.0, -3.0);

        // Adding vector to origin should give the vector <Component as Guest>s as coordinates
        let result_add = <Component as Guest>::add_vector2d(zero_point, test_vector);
        assert_point_eq(result_add, test_vector);

        // Subtracting vector from origin should give negative vector <Component as Guest>s
        let result_sub = <Component as Guest>::sub_vector2d(zero_point, test_vector);
        assert_point_eq(result_sub, (-test_vector.0, -test_vector.1));
    }
}
