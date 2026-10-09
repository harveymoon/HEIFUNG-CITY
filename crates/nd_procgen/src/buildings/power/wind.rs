//! Wind farms: helical turbines in staggered rows.

use super::*;

/// Height of a wind turbine's mast: the rotor sits on top of it.
pub const TURBINE_MAST: f32 = 18.0;
/// Height of the helical rotor.
pub const TURBINE_ROTOR_H: f32 = 11.0;
/// Radius the helical blades sweep.
pub const TURBINE_RADIUS: f32 = 3.0;
/// Spacing between turbines in a farm: about five rotor diameters along a
/// row, rows staggered.
pub const TURBINE_SPACING: f32 = 15.0;
/// Free ground between a blade tip and the lot edge.
pub const TURBINE_CLEARANCE: f32 = 2.0;

/// One helical vertical-axis turbine of a wind farm: where it stands (world
/// x/z), the height its rotor starts at above the lot pad, and the rotor's
/// starting angle. The rotor turns about the vertical through `pos`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turbine {
    pub pos: Vec2,
    pub hub: f32,
    pub phase: f32,
}

pub(super) const GRAPHITE: [f32; 3] = [0.075, 0.08, 0.085];
pub(super) const GUNMETAL: [f32; 3] = [0.16, 0.17, 0.18];
pub(super) const MATTE_BLACK: [f32; 3] = [0.025, 0.025, 0.03];

/// Turbine sites in lot coordinates with a tiny height jitter and a rotor
/// phase: staggered rows `TURBINE_SPACING` apart, centred on the parcel,
/// keeping every blade tip `TURBINE_CLEARANCE` inside the lot and the front
/// strip free for the cabin. A lot too small for a row still gets one
/// turbine where there is the most room, if anywhere.
pub(super) fn wind_sites(poly: &[Vec2], seed: u64) -> Vec<(Vec2, f32, f32)> {
    let need = TURBINE_RADIUS + TURBINE_CLEARANCE;
    let (lo, hi) = bounds(poly);
    let ok = |p: Vec2| nd_sim::zoning::point_in_polygon(p, poly) && distance_to_boundary(p, poly) >= need && p.y > lo.y + 5.0;
    let row = TURBINE_SPACING * 0.866;
    let rows = (((hi.y - lo.y) - 2.0 * need) / row).floor().max(0.0) as i32 + 1;
    let cols = (((hi.x - lo.x) - 2.0 * need) / TURBINE_SPACING).floor().max(0.0) as i32 + 1;
    let y0 = (lo.y + hi.y) * 0.5 - (rows - 1) as f32 * row * 0.5;
    let x0 = (lo.x + hi.x) * 0.5 - (cols - 1) as f32 * TURBINE_SPACING * 0.5;
    let mut rng = nd_sim::rng_for(seed, 71);
    let mut out: Vec<(Vec2, f32, f32)> = Vec::new();
    for r in 0..rows {
        let shift = if r % 2 == 1 { TURBINE_SPACING * 0.5 } else { 0.0 };
        for k in -1..=cols {
            let p = Vec2::new(x0 + k as f32 * TURBINE_SPACING + shift, y0 + r as f32 * row);
            // Tiny jitter only: a farm reads as one make of turbine.
            let p = p + Vec2::new(rng.random_range(-0.4..0.4), rng.random_range(-0.4..0.4));
            let dh = rng.random_range(-0.3f32..0.3);
            let phase = rng.random_range(0.0..std::f32::consts::TAU);
            if ok(p) && out.iter().all(|(q, _, _)| q.distance(p) >= TURBINE_SPACING * 0.95) {
                out.push((p, dh, phase));
            }
        }
    }
    if out.is_empty() {
        let mut best: Option<(f32, Vec2)> = None;
        let mut v = lo.y;
        while v <= hi.y {
            let mut u = lo.x;
            while u <= hi.x {
                let p = Vec2::new(u, v);
                if ok(p) {
                    let d = distance_to_boundary(p, poly);
                    if best.is_none_or(|(bd, _)| d > bd) {
                        best = Some((d, p));
                    }
                }
                u += 1.0;
            }
            v += 1.0;
        }
        if let Some((_, p)) = best {
            out.push((p, 0.0, rng.random_range(0.0..std::f32::consts::TAU)));
        }
    }
    out.truncate(48);
    out
}

/// The turbines of a wind farm on `lot`, in world coordinates. Depends only
/// on the parcel, the seed and the field it is part of, so the renderer finds the masts the generator
/// drew and puts a spinning rotor on each.
pub fn wind_turbines(lot: &nd_sim::Lot, seed: u64, field: Option<&PlantField>) -> Vec<Turbine> {
    if let Some(field) = field {
        return field.wind_sites().into_iter().map(|(p, dh, phase)| Turbine { pos: p, hub: TURBINE_MAST + dh, phase }).collect();
    }
    let (frame, poly) = crate::buildings::Frame::lot_local(lot);
    wind_sites(&poly, seed).into_iter().map(|(p, dh, phase)| Turbine { pos: frame.to_world(p.x, p.y), hub: TURBINE_MAST + dh, phase }).collect()
}

/// A helical (Gorlov-type) vertical-axis rotor standing on the origin: a
/// shaft, three graphite blades twisted a third of a turn about it, struts
/// at the bottom, middle and top, a red aviation light on the cap and small
/// cyan tip lights that trace the spin at night. Starts at `y = 0`; the
/// caller lifts it onto the mast and turns it about the vertical.
pub fn wind_rotor(phase: f32) -> Geometry {
    let mut g = Geometry::default();
    let h = TURBINE_ROTOR_H;
    let r = TURBINE_RADIUS;
    let tau = std::f32::consts::TAU;
    g.part(Metal).cylinder(Vec2::ZERO, 0.32, 0.26, 0.0, h + 0.6, 10, rgba(GUNMETAL));
    g.part(Matte).cylinder(Vec2::ZERO, 0.55, 0.45, h + 0.6, h + 1.1, 10, rgba(MATTE_BLACK));
    let steps = 14;
    let twist = tau / 3.0;
    let chord = 0.55;
    let thick = 0.09;
    for b in 0..3 {
        let a0 = phase + b as f32 * tau / 3.0;
        let angle = |t: f32| a0 + twist * t;
        let point = |t: f32| {
            let a = angle(t);
            Vec3::new(a.cos() * r, 0.4 + t * (h - 0.8), a.sin() * r)
        };
        let rings: Vec<Vec<Vec3>> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                let a = angle(t);
                let c = point(t);
                let radial = Vec3::new(a.cos(), 0.0, a.sin());
                let tangent = Vec3::new(-a.sin(), 0.0, a.cos());
                [(1.0f32, 0.0f32), (0.35, 1.0), (-0.45, 1.0), (-1.0, 0.0), (-0.45, -1.0), (0.35, -1.0)]
                    .iter()
                    .map(|(u, v)| c + tangent * (u * chord) + radial * (v * thick))
                    .collect()
            })
            .collect();
        g.part(Matte).strip(&rings, rgba(GRAPHITE));
        for t in [0.0f32, 0.5, 1.0] {
            let p = point(t);
            g.part(Metal).beam(Vec3::new(0.0, p.y, 0.0), p, 0.14, rgba(GUNMETAL));
        }
        let tip = point(1.0) + Vec3::Y * 0.15;
        g.part(Glow).boxed(tip, Vec3::splat(0.16), 0.0, rgba([0.25, 1.4, 1.8]));
    }
    g.part(Glow).boxed(Vec3::new(0.0, h + 1.3, 0.0), Vec3::splat(0.35), 0.0, [2.4, 0.15, 0.1, 1.0]);
    g
}

/// A wind farm: helical vertical-axis turbines in staggered rows on dark
/// masts, a cabin at the gate and a fence. With `Ctx::animated_rotors` the
/// rotors are left to the renderer, which spins them.
pub fn wind(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = match c.field {
        Some(field) => field_yard(g, c, field, Foliage, mul(pal.foliage_dry, 1.1), 1.6),
        None => yard(g, c, 0.5, Foliage, mul(pal.foliage_dry, 1.1)),
    };
    weeds(g, c, rng, &yard, 10);
    let toward_front = f.dir3(Vec2::new(0.0, -1.0));
    let sites: Vec<(Vec2, f32, f32)> = match c.field {
        Some(field) => field.wind_sites(),
        None => wind_sites(&c.lot_poly, c.seed).into_iter().map(|(p, dh, phase)| (f.to_world(p.x, p.y), dh, phase)).collect(),
    };
    for (w, dh, phase) in sites {
        let hub = TURBINE_MAST + dh;
        // Footing, a tapered gunmetal mast, a bearing housing on top.
        g.part(Concrete).cylinder(w, 2.2, 2.2, 0.0, 0.5, 12, rgba(mul(pal.concrete(0), 0.8)));
        g.part(Metal).cylinder(w, 0.95, 0.55, 0.5, hub - 0.6, 12, rgba(GUNMETAL));
        g.part(Matte).cylinder(w, 0.75, 0.6, hub - 0.6, hub, 12, rgba(MATTE_BLACK));
        // Service door facing the gate, with a small warm lamp over it.
        let door = Vec3::new(w.x, 1.4, w.y) + toward_front * 0.97;
        g.part(Matte).decal(door, f.dir3(Vec2::X), Vec3::Y, 0.7, 1.8, toward_front, rgba(MATTE_BLACK));
        g.part(Glow).boxed(door + Vec3::Y * 1.2 + toward_front * 0.1, Vec3::new(0.25, 0.08, 0.12), f.yaw(), rgba(mul(pal.street_lamp, 0.8)));
        if !c.animated_rotors {
            let rotor = wind_rotor(phase);
            for (s, m) in rotor.iter() {
                let mut m = m.clone();
                for pos in &mut m.positions {
                    pos[0] += w.x;
                    pos[1] += hub;
                    pos[2] += w.y;
                }
                g.part(s).append(&m);
            }
        }
    }
    let front = c.front_face(&yard);
    let cab = front.at(front.len * 0.3, -3.0);
    if c.inside_local(cab) {
        let cb = fit_inside(c, &rect(cab.x, cab.y, 2.0, 1.6));
        extrude(g, f, Panel, &cb, 0.0, 2.6, mul(GUNMETAL, 1.4));
    }
    if c.field.is_none() {
        fence_lot(g, c, &yard, 1.6, false);
    }
}
