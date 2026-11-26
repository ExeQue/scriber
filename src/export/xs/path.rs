//! Contours as an xTool `dPath`: absolute `M L C Z`, arcs as cubic Béziers (xTool writes `C`, never `A`).

use crate::model::Contour;
use crate::model::geom::arc_params;
use std::fmt::Write;

/// A number rounded to 1e-6 mm, without "-0".
fn n(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}

/// The contours as one path string (several contours = a compound path).
pub fn dpath(contours: &[Contour]) -> String {
    let mut d = String::new();
    for c in contours {
        let v = &c.vertices;
        let Some(first) = v.first() else { continue };
        if !d.is_empty() {
            d.push(' ');
        }
        write!(d, "M{} {}", n(first.x), n(first.y)).unwrap();
        let segs = if c.closed { v.len() } else { v.len().saturating_sub(1) };
        for i in 0..segs {
            let (a, b) = (v[i], v[(i + 1) % v.len()]);
            if a.bulge.abs() < 1e-12 {
                write!(d, "L{} {}", n(b.x), n(b.y)).unwrap();
                continue;
            }
            let (c, r, start, sweep) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
            // At most 90° per cubic: the error stays below 0.03 % of the radius.
            let k = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            let step = sweep / k as f64;
            let h = 4.0 / 3.0 * (step / 4.0).tan() * r;
            for j in 0..k {
                let (a0, a1) = (start + step * j as f64, start + step * (j + 1) as f64);
                let (s0, c0, s1, c1) = (a0.sin(), a0.cos(), a1.sin(), a1.cos());
                let p0 = [c[0] + r * c0, c[1] + r * s0];
                // The last end point is the exact vertex, so the contour closes without a gap.
                let p3 = if j + 1 == k { [b.x, b.y] } else { [c[0] + r * c1, c[1] + r * s1] };
                let (p1, p2) = ([p0[0] - h * s0, p0[1] + h * c0], [p3[0] + h * s1, p3[1] - h * c1]);
                write!(d, "C{} {} {} {} {} {}", n(p1[0]), n(p1[1]), n(p2[0]), n(p2[1]), n(p3[0]), n(p3[1])).unwrap();
            }
        }
        if c.closed {
            d.push('Z');
        }
    }
    d
}
