use super::*;

/// [sweep-visual] `clear_along` follows an arm past its first piece to
/// where it stops overlapping a neighbour at a sharp angle, and leaves
/// an arm that never overlaps at its start.
#[test]
fn clear_along_finds_where_a_sharp_arm_parts_from_its_neighbour() {
    // The neighbour: a street along +x, half width 5.
    let street = vec![(vec![Vec2::ZERO, Vec2::new(200.0, 0.0)], 5.0)];
    // A ramp leaving at 15 degrees in short pieces, half width 3.
    let d = Vec2::from_angle(15f32.to_radians());
    let ramp: Vec<Vec2> = (0..10).map(|k| d * (k as f32 * 6.0)).collect();
    let s = clear_along(&ramp, 4.0, 50.0, &[-3.0, 0.0, 3.0], &street);
    // Its near edge (3 m in) clears the street's 5 m once sin(15)*s - 3 cos(15) >= 5.
    let want = (5.0 + 3.0 * 15f32.to_radians().cos()) / 15f32.to_radians().sin();
    assert!((s - want).abs() < 0.6, "{s} vs {want}");
    assert!(s > 6.0, "past the first piece");
    // Square to it: clear at once.
    let square: Vec<Vec2> = (0..10).map(|k| Vec2::new(0.0, 6.0 + k as f32 * 6.0)).collect();
    assert_eq!(clear_along(&square, 4.0, 50.0, &[-3.0, 0.0, 3.0], &street), 4.0);
}
