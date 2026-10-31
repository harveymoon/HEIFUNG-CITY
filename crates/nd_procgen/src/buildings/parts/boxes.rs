//! [K34] Real boxes: a shipping container drawn from its record
//! (`nd_sim::containers::BoxFace`), in its line's livery, as worn as it
//! is (faded, rusting, dented, patched), with its number stencilled on
//! both sides and the door: owner code, serial and the boxed check digit.

use super::*;
use nd_sim::containers::BoxFace;
use nd_sim::rail::BoxKind;

/// [K34] A place in a depot's stacks for one real box: the middle of its
/// floor (world, metres), the way it lies (world yaw of its length), the
/// tier, and whether a forty-footer fits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSlot {
    pub at: Vec3,
    pub yaw: f32,
    pub tier: u8,
    pub forty: bool,
}

/// [K34] Hand the renderer a slot at lot-local `(u, v)`, `yaw` along it
/// (lot frame), tier `tier` (its floor `tier` boxes up from `y0`).
#[allow(clippy::too_many_arguments)]
pub fn box_slot(g: &mut Geometry, c: &Ctx, u: f32, v: f32, yaw: f32, y0: f32, tier: u8, forty: bool) {
    let f = &c.frame;
    let d = f.dir3(Vec2::new(yaw.cos(), yaw.sin()));
    g.box_slots.push(BoxSlot { at: f.at2(Vec2::new(u, v), y0 + tier as f32 * CONTAINER_H), yaw: d.z.atan2(d.x), tier, forty });
}

/// Rows of a 3 x 5 stencil glyph, top first; bit 2 is the left column.
fn glyph(c: u8) -> [u8; 5] {
    match c {
        b'0' => [7, 5, 5, 5, 7],
        b'1' => [2, 6, 2, 2, 7],
        b'2' => [7, 1, 7, 4, 7],
        b'3' => [7, 1, 3, 1, 7],
        b'4' => [5, 5, 7, 1, 1],
        b'5' => [7, 4, 7, 1, 7],
        b'6' => [7, 4, 7, 5, 7],
        b'7' => [7, 1, 2, 2, 2],
        b'8' => [7, 5, 7, 5, 7],
        b'9' => [7, 5, 7, 1, 7],
        b'A' => [2, 5, 7, 5, 5],
        b'B' => [6, 5, 6, 5, 6],
        b'C' => [3, 4, 4, 4, 3],
        b'D' => [6, 5, 5, 5, 6],
        b'E' => [7, 4, 6, 4, 7],
        b'F' => [7, 4, 6, 4, 4],
        b'G' => [3, 4, 5, 5, 3],
        b'H' => [5, 5, 7, 5, 5],
        b'I' => [7, 2, 2, 2, 7],
        b'J' => [1, 1, 1, 5, 2],
        b'K' => [5, 5, 6, 5, 5],
        b'L' => [4, 4, 4, 4, 7],
        b'M' => [5, 7, 7, 5, 5],
        b'N' => [6, 5, 5, 5, 5],
        b'O' => [2, 5, 5, 5, 2],
        b'P' => [6, 5, 6, 4, 4],
        b'Q' => [2, 5, 5, 6, 3],
        b'R' => [6, 5, 6, 5, 5],
        b'S' => [3, 4, 2, 1, 6],
        b'T' => [7, 2, 2, 2, 2],
        b'U' => [5, 5, 5, 5, 7],
        b'V' => [5, 5, 5, 5, 2],
        b'W' => [5, 5, 7, 7, 5],
        b'X' => [5, 5, 2, 5, 5],
        b'Y' => [5, 5, 2, 2, 2],
        b'Z' => [7, 1, 2, 4, 7],
        _ => [0; 5],
    }
}

/// A glyph as few rectangles as the stencil allows: runs along each row,
/// joined down the rows where they line up. (x0, y0, x1, y1) in cells,
/// y down from the top.
pub fn glyph_rects(c: u8) -> Vec<(u8, u8, u8, u8)> {
    let rows = glyph(c);
    let mut runs: Vec<(u8, u8, u8, u8)> = Vec::new();
    for (y, bits) in rows.iter().enumerate() {
        let mut x = 0u8;
        while x < 3 {
            if bits & (4 >> x) != 0 {
                let x0 = x;
                while x < 3 && bits & (4 >> x) != 0 {
                    x += 1;
                }
                // Extend the run above when it spans the same cells.
                let y = y as u8;
                match runs.iter_mut().find(|r| r.0 == x0 && r.2 == x && r.3 == y) {
                    Some(r) => r.3 = y + 1,
                    None => runs.push((x0, y, x, y + 1)),
                }
            } else {
                x += 1;
            }
        }
    }
    runs
}

/// Stencil `text` (ASCII; a space is a gap) as decals: centred on
/// `centre`, reading along `along`, `h` tall, on a face looking out along
/// `facing`. `drop` leaves a stroke out where it says so (a scuffed
/// number); returns the text's width.
#[allow(clippy::too_many_arguments)]
pub fn stencil(m: &mut MeshData, centre: Vec3, along: Vec3, facing: Vec3, h: f32, text: &[u8], color: [f32; 4], drop: impl Fn(usize) -> bool) -> f32 {
    let cell = h / 5.0;
    let advance = cell * 4.0;
    let width = advance * text.len() as f32 - cell;
    let mut k = 0usize;
    for (i, &ch) in text.iter().enumerate() {
        let x0 = -width * 0.5 + advance * i as f32;
        for (a, b, c2, d) in glyph_rects(ch) {
            k += 1;
            if drop(k) {
                continue;
            }
            let cx = x0 + (a as f32 + c2 as f32) * 0.5 * cell;
            let cy = h * 0.5 - (b as f32 + d as f32) * 0.5 * cell;
            m.decal(centre + along * cx + Vec3::Y * cy, along, Vec3::Y, (c2 - a) as f32 * cell, (d - b) as f32 * cell, facing, color);
        }
    }
    width
}

/// A thin frame round a cell of the stencil: the box round the check digit.
fn frame_box(m: &mut MeshData, centre: Vec3, along: Vec3, facing: Vec3, w: f32, h: f32, t: f32, color: [f32; 4]) {
    m.decal(centre + Vec3::Y * (h * 0.5), along, Vec3::Y, w + t, t, facing, color);
    m.decal(centre - Vec3::Y * (h * 0.5), along, Vec3::Y, w + t, t, facing, color);
    m.decal(centre + along * (w * 0.5), along, Vec3::Y, t, h, facing, color);
    m.decal(centre - along * (w * 0.5), along, Vec3::Y, t, h, facing, color);
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2]
}

/// [K34] The colour a box's number is stencilled in: its line's mark when
/// that reads on the paint, else white or black, faded with the paint.
pub fn number_colour(face: &BoxFace) -> [f32; 3] {
    let line = &SHIPPING_LINES[face.line as usize % SHIPPING_LINES.len()];
    let body = if face.kind == BoxKind::Reefer { [0.78, 0.78, 0.75] } else { line.body };
    let base = if face.marks.repainted {
        if lum(body) > 0.4 { [0.05, 0.05, 0.06] } else { [0.85, 0.85, 0.8] }
    } else if (lum(line.mark) - lum(body)).abs() > 0.25 {
        line.mark
    } else if lum(body) > 0.4 {
        [0.06, 0.06, 0.07]
    } else {
        [0.82, 0.82, 0.78]
    };
    mix(base, body, 0.35 * face.wear)
}

/// [K34] The number on a box drawn at `(u, v)` on its frame, `yaw` along
/// it, floor at `y0`: on both long sides (top right as you face it, the
/// check digit boxed, the size and type code under it) and in two lines
/// on the right-hand door leaf. A scuffed number loses strokes; one
/// painted over sits on a fresh patch of paint.
#[allow(clippy::too_many_arguments)]
pub fn box_number(m: &mut MeshData, f: &Frame, u: f32, v: f32, yaw: f32, y0: f32, face: &BoxFace, door: f32) {
    let len = if face.forty { CONTAINER_40 } else { CONTAINER_20 };
    let (hl, hw) = (len * 0.5, CONTAINER_W * 0.5);
    let (s, co) = yaw.sin_cos();
    let at = |lu: f32, y: f32, lv: f32| f.at2(Vec2::new(u + lu * co - lv * s, v + lu * s + lv * co), y);
    let long = f.dir3(Vec2::new(co, s));
    let across = f.dir3(Vec2::new(-s, co));
    let col = rgba(number_colour(face));
    let scuffed = face.marks.scuffed;
    let seed = face.seed;
    let drop = move |k: usize| scuffed && nd_sim::hash::unit(nd_sim::hash::mix(seed, k as u64)) < 0.3;
    let text = face.number;
    let line = &SHIPPING_LINES[face.line as usize % SHIPPING_LINES.len()];
    let patch = mul(if face.kind == BoxKind::Reefer { [0.78, 0.78, 0.75] } else { line.body }, 0.82);
    let iso: &[u8] = match (face.forty, face.kind) {
        (true, BoxKind::Reefer) => b"45R1",
        (false, BoxKind::Reefer) => b"22R1",
        (_, BoxKind::Tank) => b"22T6",
        (true, BoxKind::OpenTop) => b"42U1",
        (false, BoxKind::OpenTop) => b"22U1",
        (true, _) => b"45G1",
        (false, _) => b"22G1",
    };
    // One line: owner code, serial, check digit.
    let mut full: Vec<u8> = text[..4].to_vec();
    full.push(b' ');
    full.extend_from_slice(&text[4..10]);
    full.push(b' ');
    full.push(text[10]);
    let h = 0.24;
    let cell = h / 5.0;
    let adv = cell * 4.0;
    let width = adv * full.len() as f32 - cell;
    let top = y0 + CONTAINER_H - 0.42;
    for side in [-1.0f32, 1.0] {
        let out = across * side;
        // Facing the side, its right is the reading direction: looking
        // along -out with y up, right is (-out) x y.
        let read = (-out).cross(Vec3::Y).normalize_or_zero();
        let sgn = read.dot(long).signum();
        let mid = sgn * (hl - 0.4 - width * 0.5);
        let c = at(mid, top, side * (hw + 0.035));
        if face.marks.repainted {
            m.decal(at(mid, top - h * 0.5, side * (hw + 0.031)), read, Vec3::Y, width + 0.35, h * 2.9, out, rgba(patch));
        }
        stencil(m, c, read, out, h, &full, col, drop);
        // The check digit in its box.
        let cd = c + read * (width * 0.5 - cell * 1.5);
        frame_box(m, cd, read, out, cell * 4.6, h * 1.4, 0.022, col);
        // The size and type code under the owner code.
        let iw = adv * 0.7 * iso.len() as f32;
        stencil(m, c - read * (width * 0.5 - iw * 0.5) - Vec3::Y * (h * 1.7), read, out, h * 0.7, iso, col, |k| drop(k + 80));
    }
    // The door end: two lines on the right-hand leaf.
    let out = long * door;
    let read = (-out).cross(Vec3::Y).normalize_or_zero();
    let sgn = read.dot(across).signum();
    let dh = 0.17;
    let c = at(door * (hl + 0.05), top - 0.05, sgn * hw * 0.48);
    if face.marks.repainted {
        m.decal(at(door * (hl + 0.045), top - 0.2, sgn * hw * 0.48), read, Vec3::Y, 1.05, 0.62, out, rgba(patch));
    }
    stencil(m, c, read, out, dh, &text[..4], col, |k| drop(k + 120));
    let mut second: Vec<u8> = text[4..10].to_vec();
    second.push(b' ');
    second.push(text[10]);
    stencil(m, c - Vec3::Y * (dh * 1.6), read, out, dh, &second, col, |k| drop(k + 160));
}

/// [K34] The marks a box's wear and luck put on its paint, for drawing.
pub fn box_life(face: &BoxFace) -> BoxLife {
    BoxLife { wear: face.wear, patches: face.marks.patches, dents: face.marks.dents, door: if face.seed & 1 == 0 { 1.0 } else { -1.0 } }
}

/// [K34] A real box at `(u, v)` on the lot (`yaw` along it, floor at
/// `y0`), as its record says, its number on when `numbered`.
#[allow(clippy::too_many_arguments)]
pub fn real_box(g: &mut Geometry, c: &Ctx, u: f32, v: f32, yaw: f32, y0: f32, face: &BoxFace, numbered: bool) {
    let mut rng = nd_sim::rng_for(face.seed, 0xB0C5);
    let line = &SHIPPING_LINES[face.line as usize % SHIPPING_LINES.len()];
    let look = match face.kind {
        BoxKind::Dry => BoxLook::Dry,
        BoxKind::Reefer => BoxLook::Reefer,
        BoxKind::Tank => BoxLook::Tank,
        BoxKind::OpenTop => BoxLook::OpenTop,
    };
    let len = if face.forty && face.kind != BoxKind::Tank { CONTAINER_40 } else { CONTAINER_20 };
    let life = box_life(face);
    container_core(g, c, &mut rng, u, v, yaw, len, CONTAINER_W, CONTAINER_H, y0, Livery::Line(line), look, Some(life));
    if numbered && face.kind != BoxKind::Tank {
        box_number(g.part(Matte), &c.frame, u, v, yaw, y0, face, life.door);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stencil_glyph_is_a_few_rectangles_covering_its_cells() {
        for c in b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ" {
            let rects = glyph_rects(*c);
            assert!(!rects.is_empty() && rects.len() <= 7, "{} in {} rects", *c as char, rects.len());
            let cells: usize = rects.iter().map(|r| ((r.2 - r.0) * (r.3 - r.1)) as usize).sum();
            let lit: usize = glyph(*c).iter().map(|b| b.count_ones() as usize).sum();
            assert_eq!(cells, lit, "{} covers its lit cells once", *c as char);
        }
    }
}
