//! Air transport buildings: the vertiport style (a fenced drone pad, a
//! terminal with a landing deck on its roof, a skyport with a control
//! tower) and the rooftop helideck a dear tower grows when the sim gives it
//! a pad (`nd_sim::air`). Every pad drawn is recorded in `Geometry::pads`
//! so the renderer can land aircraft on the real deck.

use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::Tier;
use rand::Rng;

const PAD_GREEN: [f32; 3] = [0.3, 2.4, 0.9];
const PAD_AMBER: [f32; 3] = [2.6, 1.3, 0.15];
const SOCK: [f32; 3] = [0.95, 0.35, 0.05];

/// Where a helideck fits on top of a building: the highest roof with a
/// clear, level disc of room. Returns the deck centre (world x, z), its
/// radius, the roof height under it and the deck height (on stilts clear of
/// the roof plant). Works on a 1 m height map of everything drawn so far:
/// flat, upward faces fill the cells they cover, everything else (walls,
/// masts, tanks) marks the cells of its vertices.
pub fn roof_site(g: &Geometry, on_lot: impl Fn(Vec2) -> bool) -> Option<(Vec2, f32, f32, f32)> {
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for (_, m) in g.iter() {
        for q in &m.positions {
            lo = lo.min(Vec2::new(q[0], q[2]));
            hi = hi.max(Vec2::new(q[0], q[2]));
        }
    }
    if lo.x > hi.x {
        return None;
    }
    let cell = 1.0f32;
    let nx = ((hi.x - lo.x) / cell).ceil() as usize + 1;
    let nz = ((hi.y - lo.y) / cell).ceil() as usize + 1;
    if nx * nz > 400_000 {
        return None;
    }
    let mut top = vec![0.0f32; nx * nz];
    let idx = |p: Vec2| -> (usize, usize) { (((p.x - lo.x) / cell).floor().clamp(0.0, (nx - 1) as f32) as usize, ((p.y - lo.y) / cell).floor().clamp(0.0, (nz - 1) as f32) as usize) };
    for (_, m) in g.iter() {
        for tri in m.indices.chunks_exact(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from_array(m.positions[i as usize]));
            let n = (b - a).cross(c - a);
            // Walls mark their corners; roofs, sloped or flat, cover their cells.
            let wall = n.length() < 1e-4 || n.normalize().y.abs() < 0.25;
            if wall {
                for v in [a, b, c] {
                    let (x, z) = idx(Vec2::new(v.x, v.z));
                    top[z * nx + x] = top[z * nx + x].max(v.y);
                }
                continue;
            }
            let (a2, b2, c2) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z), Vec2::new(c.x, c.z));
            let (x0, z0) = idx(a2.min(b2).min(c2));
            let (x1, z1) = idx(a2.max(b2).max(c2));
            let area = (b2 - a2).perp_dot(c2 - a2);
            if area.abs() < 1e-6 {
                continue;
            }
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let p = Vec2::new(lo.x + (x as f32 + 0.5) * cell, lo.y + (z as f32 + 0.5) * cell);
                    let w0 = (b2 - p).perp_dot(c2 - p) / area;
                    let w1 = (c2 - p).perp_dot(a2 - p) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 >= -0.01 && w1 >= -0.01 && w2 >= -0.01 {
                        let y = w0 * a.y + w1 * b.y + w2 * c.y;
                        top[z * nx + x] = top[z * nx + x].max(y);
                    }
                }
            }
        }
    }
    // Best disc per radius: a level roof under all of it (no overhang past
    // the edge), nothing much above it; the highest deck wins, a bigger
    // one when they are within a couple of metres.
    let mut best: Option<(f32, f32, Vec2, f32, f32)> = None;
    // First a deck over the roof; failing that one cantilevered off the
    // top of a slim tower, over the lot.
    for (r, overhang) in [(6.5f32, false), (5.0, false), (4.0, false), (3.2, false), (4.5, true), (3.6, true)] {
        if overhang && best.is_some() {
            break;
        }
        let k = r.ceil() as i32;
        for z in 0..nz as i32 {
            for x in 0..nx as i32 {
                let floor = top[z as usize * nx + x as usize];
                if floor < 6.0 || x - k < 0 || z - k < 0 || x + k >= nx as i32 || z + k >= nz as i32 {
                    continue;
                }
                let (mut mx, mut mn) = (f32::MIN, f32::MAX);
                for dz in -k..=k {
                    for dx in -k..=k {
                        if ((dx * dx + dz * dz) as f32) > r * r {
                            continue;
                        }
                        let h = top[(z + dz) as usize * nx + (x + dx) as usize];
                        mx = mx.max(h);
                        mn = mn.min(h);
                    }
                }
                if (!overhang && mn < floor - 2.5) || mx > floor + 6.0 {
                    continue;
                }
                let deck = (floor + 3.0).max(mx + 0.6);
                let centre = Vec2::new(lo.x + (x as f32 + 0.5) * cell, lo.y + (z as f32 + 0.5) * cell);
                if overhang && (0..8).any(|i| {
                    let a = i as f32 * std::f32::consts::FRAC_PI_4;
                    !on_lot(centre + Vec2::new(a.cos(), a.sin()) * (r + 0.6))
                }) {
                    continue;
                }
                let key = ((deck / 2.0).floor(), r);
                if best.is_none_or(|b| key > (b.0, b.1)) {
                    best = Some((key.0, r, centre, floor, deck));
                }
            }
        }
    }
    best.map(|(_, r, c, floor, deck)| (c, r - 0.3, floor, deck))
}

/// The helideck a tower grows when the sim gives it a rooftop pad: a deck
/// on steel stilts clear of the roof plant, with the H, a lit rim, edge
/// lights and a windsock. False when the roof has no room.
pub fn rooftop_deck(g: &mut Geometry, c: &Ctx) -> bool {
    let Some((centre, r, roof, deck)) = roof_site(g, |p| c.inside_local(c.frame.to_local(p))) else { return false };
    let f = &c.frame;
    let pal = c.palette;
    let local = f.to_local(centre);
    let steel = mul(pal.steel, 0.55);
    for k in 0..4 {
        let a = std::f32::consts::FRAC_PI_4 + k as f32 * std::f32::consts::FRAC_PI_2;
        let p = local + Vec2::new(a.cos(), a.sin()) * r * 0.6;
        f.bx(g, Metal, p.x, (roof + deck) * 0.5, p.y, Vec3::new(0.35, deck - roof, 0.35), steel);
    }
    // Underside frame, then the pad itself.
    g.part(Metal).cylinder(centre, r * 0.75, r * 0.95, deck - 0.9, deck - 0.3, 12, rgba(steel));
    helipad(g, c, local, deck - 0.3, r);
    // Edge lights round the rim, green and amber.
    for k in 0..10 {
        let a = k as f32 * std::f32::consts::TAU / 10.0;
        let p = local + Vec2::new(a.cos(), a.sin()) * (r + 0.05);
        f.bx(g, Glow, p.x, deck + 0.12, p.y, Vec3::splat(0.22), if k % 2 == 0 { PAD_GREEN } else { PAD_AMBER });
    }
    // Windsock on a short pole at the rim.
    let w = local + Vec2::new(r * 0.8, 0.0);
    f.bx(g, Metal, w.x, deck + 1.2, w.y, Vec3::new(0.08, 2.4, 0.08), steel);
    f.bx(g, Tarp, w.x + 0.45, deck + 2.25, w.y, Vec3::new(0.9, 0.3, 0.3), SOCK);
    aviation_light_local(g, f, pal, w.x, deck + 2.5, w.y, 0.14);
    true
}

/// Pad centres on a polygon (lot-local): as many as `want` fit on a grid
/// with room for a deck of radius `r` each.
/// Pad centres on `poly` with the radius each one fits: `r` on a grid, or
/// one smaller pad on the deepest point.
fn pad_spots(poly: &[Vec2], want: usize, r: f32) -> Vec<(Vec2, f32)> {
    let inner = offset_poly(poly, r + 0.4);
    if inner.len() < 3 {
        return Vec::new();
    }
    let (lo, hi) = bounds(&inner);
    let step = 2.0 * r + 1.6;
    let mut out = Vec::new();
    let mut v = lo.y;
    while v <= hi.y + 0.01 && out.len() < want {
        let mut u = lo.x;
        while u <= hi.x + 0.01 && out.len() < want {
            let p = Vec2::new(u, v);
            if nd_sim::zoning::point_in_polygon(p, &inner) && distance_to_boundary(p, poly) >= r + 0.3 {
                out.push((p, r));
            }
            u += step;
        }
        v += step;
    }
    if out.is_empty() {
        let p = interior_anchor(poly);
        let room = distance_to_boundary(p, poly);
        if room >= r * 0.8 {
            out.push((p, r.min(room - 0.1)));
        }
    }
    out
}

/// Charging bollard with a lit ring beside a pad.
fn charger(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32) {
    let f = &c.frame;
    f.bx(g, Panel, p.x, y + 0.6, p.y, Vec3::new(0.45, 1.2, 0.35), [0.82, 0.84, 0.86]);
    f.bx(g, Glow, p.x, y + 1.05, p.y - 0.18, Vec3::new(0.3, 0.12, 0.04), [0.3, 1.6, 2.6]);
}

/// A drone pad (low), a vertiport (middle) or a skyport (high): pads on
/// the yard behind a fence, or on the roof deck of a glass terminal, with
/// chargers, a lit sign and, at a skyport, a control tower. The deck height
/// is `nd_sim::air::vertiport_deck` so the sim and the drawing agree.
pub fn vertiport(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.4, Paving, mul(pal.paving, 0.85));
    let want = nd_sim::air::vertiport_slots(c.tier) as usize;
    let deck = nd_sim::air::vertiport_deck(c.tier);
    let neon = pick(rng, &pal.sign_colors);
    if c.tier == Tier::Low {
        // Pads on the yard, a ticket kiosk by the gate, a fence round it.
        let mut r = 4.2;
        let mut spots = pad_spots(&lot, want, r);
        if spots.is_empty() {
            r = 3.0;
            spots = pad_spots(&lot, want, r);
        }
        for &(p, r) in &spots {
            helipad(g, c, p, 0.05, r);
            charger(g, c, p + Vec2::new(r + 0.3, 0.0), 0.0);
        }
        let front = c.front_face(&lot);
        let k = front.at(front.len * 0.3, 2.2);
        if c.inside_local(k) && distance_to_boundary(k, &c.lot_poly) > 1.6 && spots.iter().all(|(p, _)| p.distance(k) > r + 1.8) {
            f.bx(g, Glass, k.x, 1.3, k.y, Vec3::new(2.4, 2.6, 1.8), pal.glass_dark);
            f.bx(g, Panel, k.x, 2.7, k.y, Vec3::new(2.8, 0.25, 2.2), [0.85, 0.86, 0.88]);
            f.bx(g, Glow, k.x, 2.95, k.y - 0.95, Vec3::new(2.2, 0.35, 0.06), neon);
        }
        fence_lot(g, c, &lot, 2.2, false);
        floodlight(g, c, front.at(-front.len * 0.4, 1.0), 6.0);
        return;
    }
    // The terminal: a glass box up to the deck, the deck on top.
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let main = fit_inside(c, &rect((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5 + d * 0.04, w * 0.46, d * 0.42));
    if main.len() < 3 {
        return;
    }
    let base_col = [0.78, 0.8, 0.83];
    extrude(g, f, Panel, &main, 0.0, 0.8, mul(base_col, 0.8));
    extrude(g, f, Glass, &offset_poly(&main, 0.3), 0.8, deck - 0.6, pal.glass_dark);
    // Floor bands across the glass and the deck slab.
    let fh = 3.6;
    let mut y = fh;
    while y < deck - 1.0 {
        extrude(g, f, Panel, &main, y - 0.25, y + 0.1, base_col);
        y += fh;
    }
    extrude(g, f, Concrete, &main, deck - 0.6, deck, mul(pal.concrete(1), 0.7));
    for face in c.faces(&main) {
        if face.len > 5.0 {
            face.decal(g, f, Glow, 0.0, deck - 0.9, 0.06, face.len * 0.92, 0.12, mul(neon, 0.8));
        }
    }
    let front = c.front_face(&main);
    entrance(g, c, rng, &front, 0.0, 3.2, 2.8);
    sign(g, c, &front, deck - 2.0, neon);
    // Pads on the deck, chargers beside them, a lit rail round the edge.
    let r = if c.tier == Tier::High { 4.6 } else { 4.2 };
    let spots = {
        let s = pad_spots(&main, want, r);
        if s.is_empty() { pad_spots(&main, want, 3.0) } else { s }
    };
    for &(p, r) in &spots {
        helipad(g, c, p, deck - 0.05, r);
        let q = p + Vec2::new(0.0, r + 0.35);
        if nd_sim::zoning::point_in_polygon(q, &main) {
            charger(g, c, q, deck);
        }
    }
    let rail = offset_poly(&main, 0.15);
    for face in c.faces(&rail) {
        if face.len > 1.0 {
            face.bx(g, f, Glass, 0.0, deck + 0.55, 0.0, Vec3::new(face.len, 1.1, 0.06), pal.glass_dark);
            face.decal(g, f, Glow, 0.0, deck + 1.12, 0.05, face.len * 0.96, 0.05, PAD_GREEN);
        }
    }
    // Lift core and stair, at the back.
    let core = fit_inside(c, &rect(lo.x + w * 0.5 + w * 0.36, hi.y - d * 0.12, 1.6, 1.6));
    if core.len() >= 3 && inradius(&core) > 0.8 && spots.iter().all(|(p, _)| p.distance(poly_center(&core)) > r + 1.8) {
        extrude(g, f, Panel, &core, 0.0, deck + 3.0, mul(base_col, 0.9));
        extrude(g, f, Glow, &offset_poly(&core, 0.02), deck + 2.6, deck + 2.8, mul(neon, 0.7));
    }
    if c.tier == Tier::High {
        // Control tower: a slim shaft with a glass cab and a beacon.
        let t = fit_inside(c, &rect(lo.x + w * 0.1, hi.y - d * 0.12, 1.8, 1.8));
        if t.len() >= 3 && inradius(&t) > 0.9 && spots.iter().all(|(p, _)| p.distance(poly_center(&t)) > r + 2.2) {
            let top = deck + 14.0;
            extrude(g, f, Concrete, &t, 0.0, top, mul(pal.concrete(0), 0.9));
            let cab = fit_inside(c, &offset_poly(&t, -0.9));
            extrude(g, f, Glass, &cab, top, top + 3.0, [0.1, 0.16, 0.2]);
            extrude(g, f, Glow, &offset_poly(&cab, 0.02), top + 1.4, top + 1.6, PAD_GREEN);
            extrude(g, f, Panel, &cab, top + 3.0, top + 3.4, [0.85, 0.86, 0.88]);
            let tc = poly_center(&t);
            aviation_light_local(g, f, pal, tc.x, top + 4.0, tc.y, 0.4);
        }
    }
    floodlight(g, c, front.at(-front.len * 0.45, -1.2), 6.0);
}

/// A lit name board across a face.
fn sign(g: &mut Geometry, c: &Ctx, face: &super::Face, y: f32, color: [f32; 3]) {
    let f = &c.frame;
    let w = (face.len * 0.5).min(9.0);
    face.bx(g, f, Matte, 0.0, y, 0.2, Vec3::new(w, 1.0, 0.2), DARK);
    face.decal(g, f, Glow, 0.0, y, 0.32, w * 0.9, 0.55, color);
}
