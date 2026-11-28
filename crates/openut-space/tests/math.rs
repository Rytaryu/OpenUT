use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, PI};

use openut_space::{Mat3, Quat, Vec3};

const EPSILON: f64 = 1e-12;

fn assert_close(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} differs from {b}");
}

fn assert_vec(a: Vec3, b: Vec3) {
    assert_close(a.x, b.x, EPSILON);
    assert_close(a.y, b.y, EPSILON);
    assert_close(a.z, b.z, EPSILON);
}

fn assert_matrix(a: Mat3, b: Mat3, tolerance: f64) {
    for (actual, expected) in a
        .rows
        .into_iter()
        .flatten()
        .zip(b.rows.into_iter().flatten())
    {
        assert_close(actual, expected, tolerance);
    }
}

#[test]
fn vector_arithmetic_uses_all_three_axes() {
    let a = Vec3::new(3.0, 4.0, 12.0);
    let b = Vec3::new(1.0, -2.0, 2.0);
    assert_eq!(a + Vec3::ZERO, a);
    assert_eq!((a - b) * 2.0, Vec3::new(4.0, 12.0, 20.0));
    assert_eq!(a.dot(b), 19.0);
    assert_eq!(a.norm(), 13.0);
    assert!(Vec3::new(1e308, 1e308, 0.0).norm().is_finite());
}

#[test]
fn positive_y_quarter_turn_rotates_right_handed_and_inverts_with_conjugate() {
    let yaw = Quat::yaw(FRAC_PI_2);
    let input = Vec3::new(2.0, 3.0, 4.0);
    let rotated = Vec3::new(4.0, 3.0, -2.0);
    assert_vec(yaw.rotate(input), rotated);
    assert_vec(yaw.to_matrix() * input, rotated);
    assert_vec(yaw.conjugate().rotate(rotated), input);
    assert_matrix(
        yaw.to_matrix(),
        Mat3::new([[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]),
        EPSILON,
    );
}

#[test]
fn hamilton_product_applies_the_right_rotation_first() {
    let yaw = Quat::yaw(FRAC_PI_2);
    let pitch = Quat::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0, 0.0);
    let forward = Vec3::new(0.0, 0.0, 1.0);
    assert_vec((yaw * pitch).rotate(forward), Vec3::new(0.0, -1.0, 0.0));
    assert_vec((pitch * yaw).rotate(forward), Vec3::new(1.0, 0.0, 0.0));
    assert_matrix(
        (yaw * pitch).to_matrix(),
        yaw.to_matrix() * pitch.to_matrix(),
        EPSILON,
    );
}

#[test]
fn matrix_round_trip_is_stable_at_and_near_half_turns() {
    let small = 1e-14;
    for q in [
        Quat::new(0.0, 1.0, 0.0, 0.0),
        Quat::new(0.0, 0.0, 1.0, 0.0),
        Quat::new(0.0, 0.0, 0.0, 1.0),
        Quat::new(small, 0.3, -0.4, 0.5),
        Quat::yaw(PI - small),
    ] {
        let matrix = q.normalized().unwrap().to_matrix();
        assert_matrix(
            Quat::from_matrix(matrix).unwrap().to_matrix(),
            matrix,
            EPSILON,
        );
    }
}

#[test]
fn fixed_seed_rotations_round_trip_and_preserve_length() {
    let mut seed = 0xab4f_05b2_9337_1d83;
    let vector = Vec3::new(-123.0, 456.0, 789.0);
    for _ in 0..512 {
        let q = Quat::new(
            sample(&mut seed),
            sample(&mut seed),
            sample(&mut seed),
            sample(&mut seed),
        )
        .normalized()
        .unwrap();
        let m = q.to_matrix();
        let recovered = Quat::from_matrix(m).unwrap();
        assert_matrix(recovered.to_matrix(), m, EPSILON);
        assert_matrix(m.transpose() * m, Mat3::IDENTITY, EPSILON);
        assert_close(m.determinant(), 1.0, EPSILON);

        let rotated = q.rotate(vector);
        assert_vec(rotated, m * vector);
        assert_close(rotated.norm(), vector.norm(), EPSILON);
    }
}

fn sample(seed: &mut u64) -> f64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let high_bits = (*seed >> 32) as u32;
    f64::from(high_bits) / f64::from(u32::MAX) * 2.0 - 1.0
}

#[test]
fn normalizing_accepts_any_finite_nonzero_scale() {
    let a = Quat::new(f64::MAX, 0.0, f64::MAX, 0.0);
    let tiny = f64::from_bits(1);
    let b = Quat::new(tiny, 0.0, tiny, 0.0);
    let expected = Quat::yaw(FRAC_PI_2).to_matrix();
    assert_matrix(a.normalized().unwrap().to_matrix(), expected, EPSILON);
    assert_matrix(b.normalized().unwrap().to_matrix(), expected, EPSILON);
    assert!(Quat::new(0.0, 0.0, 0.0, 0.0).normalized().is_none());
    assert!(
        Quat::new(1.0, f64::INFINITY, 0.0, 0.0)
            .normalized()
            .is_none()
    );
    assert!(Quat::new(1.0, 0.0, f64::NAN, 0.0).normalized().is_none());
}

#[test]
fn matrix_conversion_accepts_roundoff_and_rejects_nonrotations() {
    let expected = Quat::yaw(FRAC_PI_3).to_matrix();
    let rounded = Mat3::new(expected.rows.map(|row| row.map(|v| f64::from(v as f32))));
    assert_matrix(
        Quat::from_matrix(rounded).unwrap().to_matrix(),
        expected,
        1e-7,
    );
    let reflection = Mat3::new([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let shear = Mat3::new([[1.0, 0.1, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let singular = Mat3::new([[0.0; 3]; 3]);
    let nonfinite = Mat3::new([[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    for matrix in [reflection, shear, singular, nonfinite] {
        assert!(Quat::from_matrix(matrix).is_none(), "{matrix:?}");
    }
}

#[test]
fn slerp_takes_shortest_path_even_with_opposite_endpoint_sign() {
    let start = Quat::IDENTITY;
    let yaw = Quat::yaw(FRAC_PI_2);
    let negated = Quat::new(-yaw.w * 2.0, -yaw.x * 2.0, -yaw.y * 2.0, -yaw.z * 2.0);
    assert_matrix(
        start.slerp(negated, 0.5).unwrap().to_matrix(),
        Quat::yaw(FRAC_PI_4).to_matrix(),
        EPSILON,
    );
    assert_matrix(
        start.slerp(negated, 0.0).unwrap().to_matrix(),
        Mat3::IDENTITY,
        EPSILON,
    );
    assert_matrix(
        start.slerp(negated, 1.0).unwrap().to_matrix(),
        yaw.to_matrix(),
        EPSILON,
    );
}

#[test]
fn slerp_handles_same_orientation_and_small_angles_without_cancellation() {
    let yaw = Quat::yaw(FRAC_PI_3);
    let opposite_sign = Quat::new(-yaw.w, -yaw.x, -yaw.y, -yaw.z);
    assert_matrix(
        yaw.slerp(opposite_sign, 0.5).unwrap().to_matrix(),
        yaw.to_matrix(),
        EPSILON,
    );
    let start = Quat::IDENTITY;
    let end = Quat::yaw(1e-8);
    assert_matrix(
        start.slerp(end, 0.5).unwrap().to_matrix(),
        Quat::yaw(5e-9).to_matrix(),
        EPSILON,
    );
    assert_matrix(
        start.slerp(Quat::yaw(0.05), 0.25).unwrap().to_matrix(),
        Quat::yaw(0.0125).to_matrix(),
        EPSILON,
    );
    assert!(start.slerp(Quat::new(0.0, 0.0, 0.0, 0.0), 0.5).is_none());
    for invalid in [-0.1, 1.1, f64::NAN] {
        assert!(start.slerp(end, invalid).is_none());
    }
}
