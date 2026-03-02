#[cfg(test)]
mod tests {
    use crate::{
        Component,
        bindings::exports::ardo314::math::{rotation_matrix2x2::Guest, types::Matrix2x2},
    };

    const EPSILON: f32 = 1e-6;

    fn assert_angle_eq(a: f32, b: f32) {
        let diff = (a - b).abs();
        // Handle angle wraparound (2π periodicity)
        let wrapped_diff = diff.min((2.0 * std::f32::consts::PI - diff).abs());
        assert!(
            wrapped_diff < EPSILON,
            "Angles differ: {} vs {} (diff: {})",
            a,
            b,
            wrapped_diff
        );
    }

    fn assert_matrix_eq(a: Matrix2x2, b: Matrix2x2) {
        assert!(
            (a.m00 - b.m00).abs() < EPSILON,
            "m00 values differ: {} vs {}",
            a.m00,
            b.m00
        );
        assert!(
            (a.m01 - b.m01).abs() < EPSILON,
            "m01 values differ: {} vs {}",
            a.m01,
            b.m01
        );
        assert!(
            (a.m10 - b.m10).abs() < EPSILON,
            "m10 values differ: {} vs {}",
            a.m10,
            b.m10
        );
        assert!(
            (a.m11 - b.m11).abs() < EPSILON,
            "m11 values differ: {} vs {}",
            a.m11,
            b.m11
        );
    }

    #[test]
    fn test_from_angle_zero() {
        let angle = 0.0;
        let matrix = <Component as Guest>::from_angle(angle);

        // At 0 radians, should be identity matrix
        let expected = Matrix2x2 {
            m00: 1.0, // cos(0) = 1
            m01: 0.0, // -sin(0) = 0
            m10: 0.0, // sin(0) = 0
            m11: 1.0, // cos(0) = 1
        };
        assert_matrix_eq(matrix, expected);
    }

    #[test]
    fn test_from_angle_quarter_turn() {
        let angle = std::f32::consts::PI / 2.0; // 90 degrees
        let matrix = <Component as Guest>::from_angle(angle);

        let expected = Matrix2x2 {
            m00: 0.0,  // cos(π/2) ≈ 0
            m01: -1.0, // -sin(π/2) = -1
            m10: 1.0,  // sin(π/2) = 1
            m11: 0.0,  // cos(π/2) ≈ 0
        };
        assert_matrix_eq(matrix, expected);
    }

    #[test]
    fn test_from_angle_half_turn() {
        let angle = std::f32::consts::PI; // 180 degrees
        let matrix = <Component as Guest>::from_angle(angle);

        let expected = Matrix2x2 {
            m00: -1.0, // cos(π) = -1
            m01: 0.0,  // -sin(π) ≈ 0
            m10: 0.0,  // sin(π) ≈ 0
            m11: -1.0, // cos(π) = -1
        };
        assert_matrix_eq(matrix, expected);
    }

    #[test]
    fn test_from_angle_three_quarter_turn() {
        let angle = 3.0 * std::f32::consts::PI / 2.0; // 270 degrees
        let matrix = <Component as Guest>::from_angle(angle);

        let expected = Matrix2x2 {
            m00: 0.0,  // cos(3π/2) ≈ 0
            m01: 1.0,  // -sin(3π/2) = 1
            m10: -1.0, // sin(3π/2) = -1
            m11: 0.0,  // cos(3π/2) ≈ 0
        };
        assert_matrix_eq(matrix, expected);
    }

    #[test]
    fn test_from_angle_full_turn() {
        let angle = 2.0 * std::f32::consts::PI; // 360 degrees
        let matrix = <Component as Guest>::from_angle(angle);

        // Should be same as identity (0 degrees)
        let expected = Matrix2x2 {
            m00: 1.0, // cos(2π) = 1
            m01: 0.0, // -sin(2π) = 0
            m10: 0.0, // sin(2π) = 0
            m11: 1.0, // cos(2π) = 1
        };
        assert_matrix_eq(matrix, expected);
    }

    #[test]
    fn test_from_angle_negative() {
        let angle = -std::f32::consts::PI / 4.0; // -45 degrees
        let matrix = <Component as Guest>::from_angle(angle);

        let sqrt2_over_2 = std::f32::consts::SQRT_2 / 2.0;
        let expected = Matrix2x2 {
            m00: sqrt2_over_2,  // cos(-π/4) = √2/2
            m01: sqrt2_over_2,  // -sin(-π/4) = √2/2
            m10: -sqrt2_over_2, // sin(-π/4) = -√2/2
            m11: sqrt2_over_2,  // cos(-π/4) = √2/2
        };
        assert_matrix_eq(matrix, expected);
    }

    #[test]
    fn test_to_angle_identity() {
        let matrix = Matrix2x2 {
            m00: 1.0,
            m01: 0.0,
            m10: 0.0,
            m11: 1.0,
        };
        let angle = <Component as Guest>::to_angle(matrix);
        assert_angle_eq(angle, 0.0);
    }

    #[test]
    fn test_to_angle_quarter_turn() {
        let matrix = Matrix2x2 {
            m00: 0.0,
            m01: -1.0,
            m10: 1.0,
            m11: 0.0,
        };
        let angle = <Component as Guest>::to_angle(matrix);
        assert_angle_eq(angle, std::f32::consts::PI / 2.0);
    }

    #[test]
    fn test_to_angle_half_turn() {
        let matrix = Matrix2x2 {
            m00: -1.0,
            m01: 0.0,
            m10: 0.0,
            m11: -1.0,
        };
        let angle = <Component as Guest>::to_angle(matrix);
        assert_angle_eq(angle, std::f32::consts::PI);
    }

    #[test]
    fn test_to_angle_negative_quarter_turn() {
        let matrix = Matrix2x2 {
            m00: 0.0,
            m01: 1.0,
            m10: -1.0,
            m11: 0.0,
        };
        let angle = <Component as Guest>::to_angle(matrix);
        assert_angle_eq(angle, -std::f32::consts::PI / 2.0);
    }

    #[test]
    fn test_round_trip_from_to_angle() {
        let test_angles = [
            0.0,
            std::f32::consts::PI / 6.0,  // 30 degrees
            std::f32::consts::PI / 4.0,  // 45 degrees
            std::f32::consts::PI / 3.0,  // 60 degrees
            std::f32::consts::PI / 2.0,  // 90 degrees
            std::f32::consts::PI,        // 180 degrees
            -std::f32::consts::PI / 4.0, // -45 degrees
            -std::f32::consts::PI / 2.0, // -90 degrees
        ];

        for &angle in &test_angles {
            let matrix = <Component as Guest>::from_angle(angle);
            let recovered_angle = <Component as Guest>::to_angle(matrix);
            assert_angle_eq(recovered_angle, angle);
        }
    }

    #[test]
    fn test_round_trip_to_from_angle() {
        // Test with various rotation matrices
        let test_matrices = [
            // Identity
            Matrix2x2 {
                m00: 1.0,
                m01: 0.0,
                m10: 0.0,
                m11: 1.0,
            },
            // 45 degree rotation
            Matrix2x2 {
                m00: std::f32::consts::SQRT_2 / 2.0,
                m01: -std::f32::consts::SQRT_2 / 2.0,
                m10: std::f32::consts::SQRT_2 / 2.0,
                m11: std::f32::consts::SQRT_2 / 2.0,
            },
            // 90 degree rotation
            Matrix2x2 {
                m00: 0.0,
                m01: -1.0,
                m10: 1.0,
                m11: 0.0,
            },
        ];

        for &matrix in &test_matrices {
            let angle = <Component as Guest>::to_angle(matrix);
            let recovered_matrix = <Component as Guest>::from_angle(angle);
            assert_matrix_eq(recovered_matrix, matrix);
        }
    }

    #[test]
    fn test_matrix_properties() {
        let angles = [
            0.0,
            std::f32::consts::PI / 6.0,
            std::f32::consts::PI / 4.0,
            std::f32::consts::PI / 3.0,
            std::f32::consts::PI / 2.0,
        ];

        for &angle in &angles {
            let matrix = <Component as Guest>::from_angle(angle);

            // Test that determinant is 1 (proper rotation)
            let det = matrix.m00 * matrix.m11 - matrix.m01 * matrix.m10;
            assert!(
                (det - 1.0).abs() < EPSILON,
                "Determinant should be 1, got {}",
                det
            );

            // Test that columns are orthonormal
            // Column 1: (m00, m10)
            // Column 2: (m01, m11)
            let col1_length_sq = matrix.m00 * matrix.m00 + matrix.m10 * matrix.m10;
            let col2_length_sq = matrix.m01 * matrix.m01 + matrix.m11 * matrix.m11;
            let dot_product = matrix.m00 * matrix.m01 + matrix.m10 * matrix.m11;

            assert!(
                (col1_length_sq - 1.0).abs() < EPSILON,
                "First column should have unit length, got {}",
                col1_length_sq.sqrt()
            );
            assert!(
                (col2_length_sq - 1.0).abs() < EPSILON,
                "Second column should have unit length, got {}",
                col2_length_sq.sqrt()
            );
            assert!(
                dot_product.abs() < EPSILON,
                "Columns should be orthogonal, dot product: {}",
                dot_product
            );
        }
    }

    #[test]
    fn test_angle_wraparound() {
        // Test that angles beyond 2π work correctly
        let angle_2pi_plus = 2.0 * std::f32::consts::PI + std::f32::consts::PI / 4.0;
        let angle_normal = std::f32::consts::PI / 4.0;

        let matrix_wrapped = <Component as Guest>::from_angle(angle_2pi_plus);
        let matrix_normal = <Component as Guest>::from_angle(angle_normal);

        assert_matrix_eq(matrix_wrapped, matrix_normal);
    }

    #[test]
    fn test_small_angles() {
        let small_angle = 1e-6;
        let matrix = <Component as Guest>::from_angle(small_angle);
        let recovered_angle = <Component as Guest>::to_angle(matrix);

        assert_angle_eq(recovered_angle, small_angle);
    }

    #[test]
    fn test_large_angles() {
        let large_angle = 10.0 * std::f32::consts::PI; // 5 full rotations
        let matrix = <Component as Guest>::from_angle(large_angle);
        let recovered_angle = <Component as Guest>::to_angle(matrix);

        // The recovered angle should be equivalent to the original modulo 2π
        let normalized_large = large_angle % (2.0 * std::f32::consts::PI);
        assert_angle_eq(recovered_angle, normalized_large);
    }

    #[test]
    fn test_precision_edge_cases() {
        // Test angles that might cause precision issues
        let edge_angles = [
            std::f32::consts::PI - EPSILON,
            std::f32::consts::PI + EPSILON,
            -std::f32::consts::PI + EPSILON,
            -std::f32::consts::PI - EPSILON,
        ];

        for &angle in &edge_angles {
            let matrix = <Component as Guest>::from_angle(angle);
            let recovered_angle = <Component as Guest>::to_angle(matrix);

            // Due to atan2 range [-π, π], we need to handle wraparound carefully
            let normalized_angle = if angle > std::f32::consts::PI {
                angle - 2.0 * std::f32::consts::PI
            } else if angle < -std::f32::consts::PI {
                angle + 2.0 * std::f32::consts::PI
            } else {
                angle
            };

            assert_angle_eq(recovered_angle, normalized_angle);
        }
    }
}
