//! [r17-logos] A stroke font for wordmarks and ads: every glyph is a few
//! polylines on a 4 x 6 grid (x right, y up, the baseline at 0), drawn as
//! tubes of light by `logo::stroke`. Capitals and digits only (lower case
//! is drawn as capitals); anything unknown is a gap.

use glam::Vec2;

/// Glyph cell width, height and the advance between letters (grid units).
pub const GLYPH_W: f32 = 4.0;
pub const GLYPH_H: f32 = 6.0;
pub const ADVANCE: f32 = 5.6;
pub const SPACE: f32 = 3.6;

/// The polylines of a glyph as text: points "x,y" separated by spaces,
/// polylines by " | ".
fn source(c: char) -> Option<&'static str> {
    Some(match c.to_ascii_uppercase() {
        'A' => "0,0 0,4 2,6 4,4 4,0 | 0,2.6 4,2.6",
        'B' => "0,3 3,3 4,2 4,1 3,0 0,0 0,6 3,6 4,5 4,4 3,3",
        'C' => "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1",
        'D' => "0,0 0,6 2,6 4,4 4,2 2,0 0,0",
        'E' => "4,6 0,6 0,0 4,0 | 0,3 3,3",
        'F' => "4,6 0,6 0,0 | 0,3 3,3",
        'G' => "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,3 2,3",
        'H' => "0,0 0,6 | 4,0 4,6 | 0,3 4,3",
        'I' => "2,0 2,6 | 1,6 3,6 | 1,0 3,0",
        'J' => "1,6 4,6 | 3,6 3,1 2,0 1,0 0,1",
        'K' => "0,0 0,6 | 4,6 0,2 | 1.3,3.3 4,0",
        'L' => "0,6 0,0 4,0",
        'M' => "0,0 0,6 2,3 4,6 4,0",
        'N' => "0,0 0,6 4,0 4,6",
        'O' => "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0",
        'P' => "0,0 0,6 3,6 4,5 4,4 3,3 0,3",
        'Q' => "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0 | 2.4,1.6 4,0",
        'R' => "0,0 0,6 3,6 4,5 4,4 3,3 0,3 | 2,3 4,0",
        'S' => "4,5 3,6 1,6 0,5 0,4 1,3 3,3 4,2 4,1 3,0 1,0 0,1",
        'T' => "0,6 4,6 | 2,6 2,0",
        'U' => "0,6 0,1 1,0 3,0 4,1 4,6",
        'V' => "0,6 2,0 4,6",
        'W' => "0,6 1,0 2,4 3,0 4,6",
        'X' => "0,0 4,6 | 0,6 4,0",
        'Y' => "0,6 2,3 4,6 | 2,3 2,0",
        'Z' => "0,6 4,6 0,0 4,0",
        '0' => "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0 | 0.6,1 3.4,5",
        '1' => "1,5 2,6 2,0 | 1,0 3,0",
        '2' => "0,5 1,6 3,6 4,5 4,4 0,0 4,0",
        '3' => "0,5 1,6 3,6 4,5 4,4 3,3 1.5,3 | 3,3 4,2 4,1 3,0 1,0 0,1",
        '4' => "3,0 3,6 0,2 4,2",
        '5' => "4,6 0,6 0,3 3,3 4,2 4,1 3,0 0,0",
        '6' => "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,2 3,3 0,3",
        '7' => "0,6 4,6 1,0",
        '8' => "1,3 0,4 0,5 1,6 3,6 4,5 4,4 3,3 1,3 0,2 0,1 1,0 3,0 4,1 4,2 3,3",
        '9' => "4,3 1,3 0,4 0,5 1,6 3,6 4,5 4,1 3,0 0,0",
        '&' => "4,0 1,4 1,5 2,6 3,5 3,4 0,1.5 0,1 1,0 2,0 4,2",
        '-' => "1,3 3,3",
        '+' => "2,1 2,5 | 0,3 4,3",
        '.' => "1.6,0 2.4,0",
        ',' => "2.2,0.6 1.6,-0.8",
        '\'' => "2,6 2,4.6",
        '!' => "2,6 2,2 | 1.6,0 2.4,0",
        '?' => "0,5 1,6 3,6 4,5 4,4 2,3 2,1.8 | 1.6,0 2.4,0",
        '#' => "1.2,0 1.8,6 | 2.4,0 3,6 | 0,2 4,2 | 0,4 4,4",
        '/' => "0,0 4,6",
        '$' => "4,5 3,5.6 1,5.6 0,4.7 1,3 3,3 4,1.4 3,0.4 1,0.4 0,1 | 2,6.4 2,-0.4",
        '%' => "0,0 4,6 | 0,5 0.8,5.8 | 3.2,0.2 4,1",
        ':' => "2,4 2,4.6 | 2,0 2,0.6",
        '*' => "2,1 2,5 | 0.4,2 3.6,4 | 0.4,4 3.6,2",
        '@' => "3,2 3,4 1,4 1,2 3,2 4,3 3.4,5.4 2,6 0.6,5.4 0,3 0.6,0.6 2,0 3.6,0.6",
        _ => return None,
    })
}

/// The polylines of `c` (grid units), or `None` for a gap.
pub fn glyph(c: char) -> Option<Vec<Vec<Vec2>>> {
    let s = source(c)?;
    Some(
        s.split('|')
            .map(|line| {
                line.split_whitespace()
                    .filter_map(|p| {
                        let (x, y) = p.split_once(',')?;
                        Some(Vec2::new(x.parse().ok()?, y.parse().ok()?))
                    })
                    .collect()
            })
            .filter(|l: &Vec<Vec2>| l.len() >= 2)
            .collect(),
    )
}

/// Width of `text` in grid units (from the first glyph's left to the last
/// glyph's right).
pub fn text_width(text: &str) -> f32 {
    let mut w: f32 = 0.0;
    let mut any = false;
    for ch in text.chars() {
        if ch == ' ' {
            w += SPACE;
        } else {
            w += ADVANCE;
            any = true;
        }
    }
    if any {
        w - (ADVANCE - GLYPH_W)
    } else {
        0.0
    }
}

/// The polylines of `text` in grid units, left to right from x = 0, with a
/// slant (x moved by `slant` per unit of height).
pub fn text_lines(text: &str, slant: f32) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    let mut x = 0.0;
    for ch in text.chars() {
        if ch == ' ' {
            x += SPACE;
            continue;
        }
        if let Some(lines) = glyph(ch) {
            for l in lines {
                out.push(l.into_iter().map(|p| Vec2::new(x + p.x + p.y * slant, p.y)).collect());
            }
        }
        x += ADVANCE;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_letter_and_digit_has_strokes_on_the_grid() {
        for c in ('A'..='Z').chain('0'..='9') {
            let g = glyph(c).unwrap_or_else(|| panic!("no glyph for {c}"));
            assert!(!g.is_empty(), "{c}");
            for l in &g {
                assert!(l.len() >= 2, "{c}");
                for p in l {
                    assert!((-0.1..=GLYPH_W + 0.1).contains(&p.x) && (-1.0..=GLYPH_H + 0.5).contains(&p.y), "{c} {p}");
                }
            }
        }
        assert_eq!(glyph('a'), glyph('A'));
        assert!(glyph('~').is_none());
    }

    #[test]
    fn widths_add_up() {
        assert_eq!(text_width(""), 0.0);
        assert_eq!(text_width("A"), GLYPH_W);
        assert!((text_width("AB") - (ADVANCE + GLYPH_W)).abs() < 1e-5);
        assert!(text_width("A B") > text_width("AB"));
    }
}
