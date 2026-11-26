//! Reading what was typed on the command line: points, numbers and option keywords.

use crate::model::Point;

/// One answer to a prompt.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// Enter (or Space, or right-click) on an empty line.
    Empty,
    Point(Point),
    /// A bare number: a distance, radius, angle, factor or count, depending on the prompt.
    Number(f64),
    /// An option keyword or command name, upper-cased.
    Word(String),
}

/// Parse one line. `last` is the last point given, for relative input (`@dx,dy`, `@dist<angle`).
///
/// Absolute and relative coordinates are in model millimetres (y down, as in the status bar).
/// Polar angles follow AutoCAD: 0° points right and positive angles turn counter-clockwise on screen.
pub fn parse(text: &str, last: Option<Point>) -> Result<Entry, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Entry::Empty);
    }
    if let Some(n) = number(text) {
        return Ok(Entry::Number(n));
    }
    if text.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok(Entry::Word(text.to_ascii_uppercase()));
    }
    let (origin, rest) = match text.strip_prefix('@') {
        Some(rest) => (last.ok_or("There is no last point yet.")?, rest),
        None => ([0.0, 0.0], text),
    };
    let invalid = || format!("\"{text}\" is not a point. Use x,y or @dx,dy or @distance<angle.");
    let offset = if let Some((d, a)) = rest.split_once('<') {
        let (d, a) = (number(d).ok_or_else(invalid)?, number(a).ok_or_else(invalid)?.to_radians());
        [d * a.cos(), -d * a.sin()]
    } else if let Some((x, y)) = rest.split_once(',') {
        [number(x).ok_or_else(invalid)?, number(y).ok_or_else(invalid)?]
    } else {
        return Err(invalid());
    };
    Ok(Entry::Point([origin[0] + offset[0], origin[1] + offset[1]]))
}

/// The point `dist` mm from `from`, towards `pointer` (to the right when there is no pointer).
pub fn toward(from: Point, pointer: Option<Point>, dist: f64) -> Point {
    let (dx, dy) = pointer.map_or((1.0, 0.0), |p| (p[0] - from[0], p[1] - from[1]));
    let len = dx.hypot(dy);
    let (ux, uy) = if len < 1e-9 { (1.0, 0.0) } else { (dx / len, dy / len) };
    [from[0] + ux * dist, from[1] + uy * dist]
}

/// Whether a typed word picks `option` (any leading part of it, as AutoCAD does: "D" = "Diameter").
pub fn is_option(word: &str, option: &str) -> bool {
    !word.is_empty() && option.to_ascii_uppercase().starts_with(word)
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(text: &str, last: Option<Point>) -> Point {
        match parse(text, last) {
            Ok(Entry::Point(p)) => p,
            other => panic!("{text}: {other:?}"),
        }
    }

    fn close(a: Point, b: Point) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn absolute_and_relative() {
        assert_eq!(point("10,20", None), [10.0, 20.0]);
        assert_eq!(point(" -1.5 , 2 ", None), [-1.5, 2.0]);
        assert_eq!(point("@5,-5", Some([10.0, 10.0])), [15.0, 5.0]);
    }

    #[test]
    fn polar_is_counter_clockwise_on_screen() {
        let last = Some([10.0, 10.0]);
        assert!(close(point("@10<0", last), [20.0, 10.0]));
        // 90° goes up on screen, which is -y in the model.
        assert!(close(point("@10<90", last), [10.0, 0.0]));
        assert!(close(point("@10<180", last), [0.0, 10.0]));
        assert!(close(point("10<270", None), [0.0, 10.0]));
    }

    #[test]
    fn numbers_words_and_empty() {
        assert_eq!(parse("  ", None), Ok(Entry::Empty));
        assert_eq!(parse("12.5", None), Ok(Entry::Number(12.5)));
        assert_eq!(parse("line", None), Ok(Entry::Word("LINE".into())));
        assert!(is_option("D", "Diameter") && is_option("DIA", "Diameter") && !is_option("X", "Diameter"));
    }

    #[test]
    fn bare_distance_follows_the_pointer() {
        assert!(close(toward([0.0, 0.0], Some([3.0, 4.0]), 10.0), [6.0, 8.0]));
        assert!(close(toward([1.0, 1.0], None, 2.0), [3.0, 1.0]));
        assert!(close(toward([1.0, 1.0], Some([1.0, 1.0]), 2.0), [3.0, 1.0]));
    }

    #[test]
    fn bad_input() {
        assert!(parse("@1,1", None).is_err());
        assert!(parse("1,2,3", None).is_err());
        assert!(parse("1;2", None).is_err());
        assert!(parse("@a<5", Some([0.0, 0.0])).is_err());
        assert!(parse("10,x", None).is_err());
        assert!(parse("inf,1", None).is_err());
        assert!(matches!(parse("nan", None), Ok(Entry::Word(_))));
    }
}
