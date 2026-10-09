//! Solids from lot-local outlines: extrusions, lofts, bands, ground plates
//! and decals on faces.

#[allow(unused_imports)]
use super::*;

// ── solids ───────────────────────────────────────────────────────────────────

pub fn extrude(g: &mut Geometry, f: &Frame, s: Surface, poly: &[Vec2], y0: f32, y1: f32, color: [f32; 3]) {
    if poly.len() < 3 {
        return;
    }
    g.part(s).frustum(&f.poly3(poly, y0), &f.poly3(poly, y1), rgba(color));
}

/// Solid between two polygons with the same vertex count.
pub fn loft(g: &mut Geometry, f: &Frame, s: Surface, bottom: &[Vec2], top: &[Vec2], y0: f32, y1: f32, color: [f32; 3]) {
    if bottom.len() < 3 || bottom.len() != top.len() {
        return;
    }
    g.part(s).frustum(&f.poly3(bottom, y0), &f.poly3(top, y1), rgba(color));
}

/// The sides of a solid between two polygons, without its caps: a floor
/// band proud of a shaft, whose caps would lie in the shaft's own.
#[allow(clippy::too_many_arguments)]
pub fn band(g: &mut Geometry, f: &Frame, s: Surface, bottom: &[Vec2], top: &[Vec2], y0: f32, y1: f32, color: [f32; 3]) {
    if bottom.len() < 3 || bottom.len() != top.len() {
        return;
    }
    g.part(s).band(&f.poly3(bottom, y0), &f.poly3(top, y1), rgba(color));
}

pub fn ground(g: &mut Geometry, f: &Frame, s: Surface, poly: &[Vec2], y: f32, color: [f32; 3]) {
    let world: Vec<Vec2> = poly.iter().map(|p| f.to_world(p.x, p.y)).collect();
    g.part(s).ground_polygon(&world, y, rgba(color));
}

/// The parcel inset by `inset`, floored with `s` at ground level: the yard
/// nearly every industrial, transport and civic style starts from.
pub fn yard(g: &mut Geometry, c: &Ctx, inset: f32, s: Surface, color: [f32; 3]) -> Vec<Vec2> {
    let poly = offset_poly(&c.lot_poly, inset);
    ground(g, &c.frame, s, &poly, 0.03, color);
    poly
}

/// One quad on a face: `w` along it, `h` high, centred `t` from the middle
/// and `d` proud of the wall, facing the way the face does. Two triangles,
/// four vertices, for every pane, frame, sign letter, LED cell and stain
/// that a closed box used to spend twelve on.
#[allow(clippy::too_many_arguments)]
pub fn decal(g: &mut Geometry, f: &Frame, s: Surface, face: &Face, t: f32, y: f32, d: f32, w: f32, h: f32, color: [f32; 3]) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let centre = f.at2(face.at(t, d), y);
    let along = f.dir3(face.tangent);
    let out = f.dir3(face.out);
    g.part(s).decal(centre, along, Vec3::Y, w, h, out, rgba(color));
}

/// Low wall along every edge of a footprint, on the inside of the edge.
pub fn parapet(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, h: f32, thick: f32, s: Surface, color: [f32; 3]) {
    for face in c.faces(poly) {
        face.bx(g, &c.frame, s, 0.0, y + h * 0.5, -thick * 0.5, Vec3::new(face.len + thick, h, thick), color);
    }
}

/// Band that stands proud of every edge (floor plates, string courses).
pub fn cornice(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, h: f32, proud: f32, s: Surface, color: [f32; 3]) {
    for face in c.faces(poly) {
        face.bx(g, &c.frame, s, 0.0, y, proud * 0.5, Vec3::new(face.len + proud * 2.0, h, proud + 0.02), color);
    }
}

/// Columns spaced along the edges of a footprint, slightly inside it.
#[allow(clippy::too_many_arguments)]
pub fn pilotis(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y0: f32, y1: f32, spacing: f32, size: f32, s: Surface, color: [f32; 3]) {
    for face in c.faces(poly) {
        if face.len < size * 2.5 {
            continue;
        }
        let n = (face.len / spacing).floor().max(1.0) as i32;
        for k in 0..=n {
            let t = -face.len * 0.5 + k as f32 * (face.len / n as f32);
            let t = t.clamp(-face.len * 0.5 + size, face.len * 0.5 - size);
            face.bx(g, &c.frame, s, t, (y0 + y1) * 0.5, -size * 0.6, Vec3::new(size, y1 - y0, size), color);
        }
    }
}

/// Steps around a footprint.
pub fn plinth(g: &mut Geometry, c: &Ctx, poly: &[Vec2], steps: usize, rise: f32, tread: f32, s: Surface, color: [f32; 3]) {
    for k in 0..steps {
        let p = offset_poly(poly, -(tread * (steps - k) as f32));
        // Steps that would spill over the lot edge are dropped.
        let p = if c.all_inside(&p) { p } else { fit_inside(c, &p) };
        extrude(g, &c.frame, s, &p, 0.0, rise * (k + 1) as f32, color);
    }
}
