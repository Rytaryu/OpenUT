use std::f64::consts::FRAC_PI_2;

use openut_space::{AxisGroup, AxisMap, BodyRotation, Handedness, Mat3, Quat, SignedAxis, Vec3};

const IDENTITY: Mat3 = Mat3::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const POINT: Vec3 = Vec3::new(2.0, -3.0, 5.0);

const SOURCE_UP_AXES: [(SignedAxis, usize, f64); 6] = [
    (SignedAxis::PosX, 0, 1.0),
    (SignedAxis::NegX, 0, -1.0),
    (SignedAxis::PosY, 1, 1.0),
    (SignedAxis::NegY, 1, -1.0),
    (SignedAxis::PosZ, 2, 1.0),
    (SignedAxis::NegZ, 2, -1.0),
];

fn sample_rotation() -> Quat {
    let unit = 30.0_f64.sqrt().recip();
    Quat::new(unit, 2.0 * unit, 3.0 * unit, 4.0 * unit)
}

fn assert_matrix_close(actual: Mat3, expected: Mat3) {
    for (actual_row, expected_row) in actual.rows.into_iter().zip(expected.rows) {
        for (value, target) in actual_row.into_iter().zip(expected_row) {
            assert!((value - target).abs() < 1e-12, "{actual:?} != {expected:?}");
        }
    }
}

#[test]
fn all_axis_maps_are_distinct_and_orthogonal() {
    let mut matrices = Vec::new();
    for map in AxisMap::all() {
        let matrix = map.matrix();
        assert!(!matrices.contains(&matrix), "duplicate map {map}");
        matrices.push(matrix);
        assert_eq!(matrix * matrix.transpose(), IDENTITY, "{map}");
        assert_eq!(matrix.determinant().abs(), 1.0, "{map}");
    }
    assert_eq!(matrices.len(), 48);
}

#[test]
fn axis_maps_apply_and_reverse_positions_and_proper_rotations() {
    let orientation = sample_rotation();
    let rotation = orientation.to_matrix();
    assert_eq!(AxisMap::IDENTITY.apply_position(POINT), POINT);
    assert_matrix_close(
        AxisMap::IDENTITY.apply_rotation(orientation).to_matrix(),
        rotation,
    );

    for map in AxisMap::all() {
        let matrix = map.matrix();
        let mapped_position = map.apply_position(POINT);
        let mapped_orientation = map.apply_rotation(orientation);
        let mapped_rotation = mapped_orientation.to_matrix();
        assert_eq!(mapped_position, matrix * POINT, "{map}");
        assert_matrix_close(mapped_rotation, matrix * rotation * matrix.transpose());
        assert!((mapped_rotation.determinant() - 1.0).abs() < 1e-12, "{map}");

        let inverse = map.inverse();
        assert_eq!(inverse.matrix(), matrix.transpose(), "{map}");
        assert_eq!(inverse.apply_position(mapped_position), POINT, "{map}");
        assert_matrix_close(
            inverse.apply_rotation(mapped_orientation).to_matrix(),
            rotation,
        );
    }

    let reflection: AxisMap = "-x+y+z".parse().unwrap();
    let reflected_yaw = reflection.apply_rotation(Quat::yaw(FRAC_PI_2));
    assert_matrix_close(reflected_yaw.to_matrix(), Quat::yaw(-FRAC_PI_2).to_matrix());
}

#[test]
fn axis_strings_are_canonical_and_require_each_lowercase_axis_once() {
    let coordinate_change: AxisMap = "-z+y+x".parse().unwrap();
    assert_eq!(
        coordinate_change.apply_position(POINT),
        Vec3::new(-5.0, -3.0, 2.0)
    );
    assert_eq!(coordinate_change.to_string(), "-z+y+x");
    assert_eq!(AxisMap::IDENTITY.to_string(), "+x+y+z");
    for map in AxisMap::all() {
        let text = map.to_string();
        assert_eq!(text.len(), 6);
        assert_eq!(text.parse::<AxisMap>(), Ok(map), "{text}");
    }

    for malformed in [
        "", "x+y+z", "+x+y", "+x+y+zz", "+x+y+z ", "+x+y+z\n", " +x+y+z", "+X+y+z", "−x+y+z",
        "+w+y+z", "+x+x+z", "+x+y-y", "+x-x+z",
    ] {
        assert!(malformed.parse::<AxisMap>().is_err(), "{malformed:?}");
        assert!(malformed.parse::<BodyRotation>().is_err(), "{malformed:?}");
    }
}

#[test]
fn each_group_has_four_members_and_one_canonical_representative() {
    let maps: Vec<_> = AxisMap::all().collect();
    for (up, source_axis, sign) in SOURCE_UP_AXES {
        let mut source_up = [0.0; 3];
        source_up[source_axis] = sign;
        let source_up = Vec3::new(source_up[0], source_up[1], source_up[2]);
        let first_horizontal = if source_axis == 0 { Y } else { X };
        for (handedness, determinant) in
            [(Handedness::Preserving, 1.0), (Handedness::Reversing, -1.0)]
        {
            let group = AxisGroup { up, handedness };
            let members: Vec<_> = maps
                .iter()
                .copied()
                .filter(|map| map.group() == group)
                .collect();
            assert_eq!(members.len(), 4, "{group:?}");
            for map in &members {
                assert_eq!(map.apply_position(source_up), Y, "{map}");
                assert_eq!(map.matrix().determinant(), determinant, "{map}");
            }

            let canonical: Vec<_> = members
                .into_iter()
                .filter(|map| map.apply_position(first_horizontal) == X)
                .collect();
            assert_eq!(canonical, [group.representative()], "{group:?}");
        }
    }
}

#[test]
fn groups_match_exactly_when_the_relative_map_is_a_y_quarter_turn() {
    let quarter_turns = [
        IDENTITY,
        Mat3::new([[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]),
        Mat3::new([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]]),
        Mat3::new([[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
    ];
    let maps: Vec<_> = AxisMap::all().collect();
    for first in &maps {
        for second in &maps {
            let relative = second.matrix() * first.matrix().transpose();
            assert_eq!(
                first.group() == second.group(),
                quarter_turns.contains(&relative),
                "{first} and {second}"
            );
        }
    }
}

#[test]
fn negating_a_map_reverses_the_up_axis_and_handedness() {
    let maps: Vec<_> = AxisMap::all().collect();
    for map in &maps {
        let negated_matrix = Mat3::new(map.matrix().rows.map(|row| row.map(|value| -value)));
        let negated = maps
            .iter()
            .find(|map| map.matrix() == negated_matrix)
            .unwrap();
        let group = map.group();
        let up = match group.up {
            SignedAxis::PosX => SignedAxis::NegX,
            SignedAxis::NegX => SignedAxis::PosX,
            SignedAxis::PosY => SignedAxis::NegY,
            SignedAxis::NegY => SignedAxis::PosY,
            SignedAxis::PosZ => SignedAxis::NegZ,
            SignedAxis::NegZ => SignedAxis::PosZ,
        };
        let handedness = match group.handedness {
            Handedness::Preserving => Handedness::Reversing,
            Handedness::Reversing => Handedness::Preserving,
        };
        assert_eq!(negated.group(), AxisGroup { up, handedness }, "{map}");
        let orientation = sample_rotation();
        assert_matrix_close(
            negated.apply_rotation(orientation).to_matrix(),
            map.apply_rotation(orientation).to_matrix(),
        );
    }
}

#[test]
fn body_rotations_include_exactly_the_proper_maps_and_apply_on_the_right() {
    let orientation = sample_rotation();
    let rotation = orientation.to_matrix();
    let identity = BodyRotation::default();
    assert_eq!(identity.to_string(), "+x+y+z");
    assert_eq!(identity.matrix(), IDENTITY);
    assert_matrix_close(identity.apply_rotation(orientation).to_matrix(), rotation);

    let mut matrices = Vec::new();
    for body in BodyRotation::all() {
        let matrix = body.matrix();
        assert!(
            !matrices.contains(&matrix),
            "duplicate body rotation {body}"
        );
        matrices.push(matrix);
        assert_eq!(matrix.determinant(), 1.0, "{body}");
        assert_eq!(body.to_string().parse::<BodyRotation>(), Ok(body));
        assert_matrix_close(
            body.apply_rotation(orientation).to_matrix(),
            rotation * matrix,
        );
    }
    assert_eq!(matrices.len(), 24);

    for map in AxisMap::all() {
        let parsed = map.to_string().parse::<BodyRotation>();
        let matrix = map.matrix();
        if matrix.determinant() == 1.0 {
            assert_eq!(parsed.unwrap().matrix(), matrix, "{map}");
            assert!(matrices.contains(&matrix), "{map}");
        } else {
            assert!(parsed.is_err(), "accepted reflection {map}");
        }
    }
}

#[test]
fn body_rotation_changes_only_orientation_after_axis_mapping() {
    let axes: AxisMap = "-z+y+x".parse().unwrap();
    let body: BodyRotation = "-z+y+x".parse().unwrap();
    let position = axes.apply_position(POINT);
    let orientation = axes.apply_rotation(Quat::yaw(FRAC_PI_2));
    let corrected = (position, body.apply_rotation(orientation));
    assert_eq!(corrected.0, Vec3::new(-5.0, -3.0, 2.0));
    assert_matrix_close(corrected.1.to_matrix(), IDENTITY);
}
