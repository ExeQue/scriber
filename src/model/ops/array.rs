//! Array: where the copies of a rectangular or polar array go.

/// Offsets of the copies in a `rows` × `columns` grid, `dx` mm between columns and `dy` between rows
/// (the original is the cell at [0, 0] and is not included).
pub fn grid_offsets(rows: u32, columns: u32, dx: f64, dy: f64) -> Vec<[f64; 2]> {
    (0..rows).flat_map(|r| (0..columns).map(move |c| [c as f64 * dx, r as f64 * dy])).skip(1).collect()
}

/// Rotation (degrees) of each copy when `count` items in total (original included) are spread over
/// `angle` degrees. A full circle spaces them evenly, so the last copy does not land on the original.
pub fn polar_angles(count: u32, angle: f64) -> Vec<f64> {
    let full = (angle.abs() - 360.0).abs() < 1e-9;
    let step = if full || count < 2 { angle / count.max(1) as f64 } else { angle / (count - 1) as f64 };
    (1..count).map(|i| step * i as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_and_polar_positions() {
        assert_eq!(grid_offsets(2, 3, 10.0, 5.0), vec![[10.0, 0.0], [20.0, 0.0], [0.0, 5.0], [10.0, 5.0], [20.0, 5.0]]);
        assert_eq!(polar_angles(4, 360.0), vec![90.0, 180.0, 270.0]);
        assert_eq!(polar_angles(3, 90.0), vec![45.0, 90.0]);
        assert_eq!(polar_angles(3, -360.0), vec![-120.0, -240.0]);
    }
}
