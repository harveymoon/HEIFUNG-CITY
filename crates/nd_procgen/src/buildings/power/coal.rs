//! A coal station composed of units, not stretched: boilers, stacks, ash filters, a turbine hall, cooling towers and the coal yard.

use super::*;

// ── coal: a station composed of units, not stretched ─────────────────────

/// [plant-scale] Near-black, sooty materials of the coal station.
pub(super) const SOOT_CONCRETE: [f32; 3] = [0.034, 0.032, 0.031];
pub(super) const BLACK_STEEL: [f32; 3] = [0.022, 0.023, 0.026];
pub(super) const COAL_BLACK: [f32; 3] = [0.012, 0.011, 0.013];
pub(super) const RUST_DARK: [f32; 3] = [0.075, 0.03, 0.017];
/// Firelight through the boiler house slits: a dim orange glow.
pub(super) const EMBER: [f32; 3] = [1.6, 0.42, 0.07];

/// Where a stack or a cooling tower lets out its plume, in world
/// coordinates with the height above the lot pad: the renderer starts the
/// smoke (or, from a cooling tower, the steam) there. The same layout the
/// mesh is drawn from ([`coal_vents`]), so every stack smokes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoalVent {
    pub top: Vec3,
    /// Radius of the flue, or of the cooling tower's mouth.
    pub r: f32,
    /// A cooling tower: white steam, not smoke.
    pub steam: bool,
}

/// A coal station's units in lot coordinates (u along the road, v inward):
/// worked out from the outline, the building's seed and its size alone, so
/// the mesh and the renderer's plumes agree.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CoalLayout {
    /// Boiler houses: centre, half size, height.
    pub boilers: Vec<(Vec2, Vec2, f32)>,
    /// Smokestacks: base, radius at the foot, height; the boiler it serves.
    pub stacks: Vec<(Vec2, f32, f32, Option<usize>)>,
    /// Ash filters (precipitators) beside the boilers: centre, half size, height.
    pub filters: Vec<(Vec2, Vec2, f32)>,
    pub turbine: Option<(Vec2, Vec2, f32)>,
    /// Cooling towers: base, radius at the foot, height.
    pub towers: Vec<(Vec2, f32, f32)>,
    /// Coal heaps: centre, half size, height.
    pub heaps: Vec<(Vec2, Vec2, f32)>,
    pub transformer: Option<Vec2>,
}

/// [plant-scale] The units of a coal station of size `scale` (share of the
/// nameplate) on a lot outline: one to four boiler houses, each with its
/// own smokestack of its own height and girth, a small start-up stack, a
/// tall common stack on the big ones, a turbine hall in front, ash filters,
/// one or two cooling towers on the biggest, and heaps of coal. A lot
/// bigger than the station needs is coal yard beyond its block.
pub(crate) fn coal_layout(poly: &[Vec2], seed: u64, scale: f32) -> CoalLayout {
    let mut rng = rng_for(seed, 0xC0A1);
    let s = scale.clamp(0.3, 3.0);
    let (lo, hi) = bounds(poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let lot_area = nd_sim::zoning::polygon_area(poly).abs();
    let plant_area = s * nd_sim::power::COAL_REFERENCE_AREA;
    // The station's block: the whole lot, or on a lot much bigger than it
    // needs a block at the front, the rest of the lot its coal yard.
    let roomy = lot_area > plant_area * 1.4;
    let (cw, cd) = if roomy {
        let cw = (plant_area * 1.3).sqrt().max(30.0).min(w);
        (cw, (plant_area * 1.3 / cw).max(26.0).min(d))
    } else {
        (w, d)
    };
    let cu = (lo.x + hi.x) * 0.5;
    // Fractions of the block: u from the left, v from the road.
    let at = |fu: f32, fv: f32| Vec2::new(cu + (fu - 0.5) * cw, lo.y + fv * cd);
    let mut pk = Packer::new(poly);
    let mut lay = CoalLayout::default();
    let units: usize = if s < 0.9 {
        1
    } else if s < 1.7 {
        2
    } else {
        3
    };
    // Bands from the road back: transformers and the turbine hall, the
    // boiler houses, their stacks; the coal yard down the left side.
    let yard_w = if units == 1 { 0.3 } else { 0.24 };
    let row = |k: usize| yard_w + (1.0 - yard_w) * (k as f32 + 0.5) / units as f32;
    let reach_b = (cw / units as f32).max(10.0);
    for k in 0..units {
        let h = Vec2::new(rng.random_range(3.6..4.4), rng.random_range(4.6..5.6));
        let bh = rng.random_range(26.0..36.0);
        let want = at(row(k), 0.56);
        let placed = pk.place(want, h, 1.5, 1.2, reach_b).map(|p| (p, h, bh)).or_else(|| pk.place(want, h * 0.75, 1.0, 1.0, reach_b).map(|p| (p, h * 0.75, bh * 0.8)));
        if let Some(b) = placed {
            lay.boilers.push(b);
        }
    }
    // Each boiler's own stack behind it: heights and girths differ.
    for (i, &(p, h, bh)) in lay.boilers.clone().iter().enumerate() {
        let r = rng.random_range(1.6..2.5);
        let sh = (bh + rng.random_range(20.0..44.0)).max(46.0);
        let want = p + Vec2::new(rng.random_range(-1.5..1.5), h.y + r + 3.0);
        if let Some(q) = pk.place(want, Vec2::splat(r * 1.2), 1.2, 1.0, 12.0) {
            lay.stacks.push((q, r, sh, Some(i)));
        }
    }
    // Turbine hall in front of the boilers.
    let hw = (3.5 + 6.5 * units as f32).min(cw * (1.0 - yard_w) * 0.45);
    let th = Vec2::new(hw, rng.random_range(4.0..5.0));
    let hall_h = rng.random_range(12.0..15.0);
    let hall_at = at(yard_w + (1.0 - yard_w) * 0.5, 0.22);
    if let Some(p) = pk.place(hall_at, th, 1.5, 1.2, cw * 0.4) {
        lay.turbine = Some((p, th, hall_h));
    } else if let Some(p) = pk.place(hall_at, th * 0.7, 1.0, 1.0, cw * 0.4) {
        lay.turbine = Some((p, th * 0.7, hall_h * 0.85));
    }
    // The tall common stack of a big station, at the back.
    if s >= 1.4 {
        let r = rng.random_range(3.0..3.5);
        if let Some(q) = pk.place(at(yard_w + (1.0 - yard_w) * 0.5, 0.92), Vec2::splat(r * 1.2), 1.5, 1.2, cw * 0.5) {
            lay.stacks.push((q, r, rng.random_range(96.0..112.0), None));
        }
    }
    // Cooling towers on the big stations, at the back of the coal yard
    // (as big as the ground there allows).
    let towers = if s >= 1.9 { 2 } else if s >= 1.3 { 1 } else { 0 };
    for k in 0..towers {
        let rb = rng.random_range(7.0..8.2);
        let want = at(0.12 + 0.3 * k as f32, 0.84);
        if let Some((q, rb)) = [1.0f32, 0.85, 0.72, 0.62].iter().find_map(|f| pk.place(want, Vec2::splat(rb * f), 1.0, 1.4, cw.max(cd) * 0.7).map(|q| (q, rb * f))) {
            lay.towers.push((q, rb, rb * rng.random_range(3.2..3.6)));
        }
    }
    // The start-up boiler's small stack by the turbine hall.
    let want = lay.turbine.map_or(at(0.9, 0.4), |(p, h, _)| p + Vec2::new(h.x + 2.5, h.y * 0.4));
    if let Some(q) = pk.place(want, Vec2::splat(1.3), 0.8, 0.8, cw.max(cd) * 0.6) {
        lay.stacks.push((q, 1.1, rng.random_range(24.0..32.0), None));
    }
    // Ash filters beside the boilers.
    for &(p, h, bh) in lay.boilers.clone().iter() {
        let fh = Vec2::new(2.2, 3.0);
        if let Some(q) = pk.place(p + Vec2::new(h.x + 3.6, -0.5), fh, 1.0, 0.8, 5.0) {
            lay.filters.push((q, fh, (bh * 0.42).max(10.0)));
        }
    }
    // Transformers by the road.
    lay.transformer = pk.place(at(0.88, 0.07), Vec2::new(5.3, 4.3), 0.6, 1.0, cw * 0.3);
    // Heaps of coal in the yard: more for a bigger station, and over all
    // the spare ground of a lot bigger than it needs.
    let spare = if roomy { ((lot_area - plant_area * 1.3) / 240.0) as usize } else { 0 };
    let heaps = (2 + (s * 1.5) as usize + spare).min(18);
    for k in 0..heaps {
        let hh = Vec2::new(rng.random_range(2.4..4.0), rng.random_range(4.0..7.5));
        let want = if k < 3 { at(yard_w * 0.45, 0.2 + 0.24 * k as f32) } else { Vec2::new(lo.x + w * rng.random_range(0.08..0.92), lo.y + d * rng.random_range(0.3..0.95)) };
        let placed = match pk.place(want, hh, 1.0, 1.6, w.max(d)) {
            Some(p) => Some((p, hh)),
            None => pk.place(want, hh * 0.6, 0.8, 1.2, w.max(d)).map(|p| (p, hh * 0.6)),
        };
        if let Some((p, hh)) = placed {
            lay.heaps.push((p, hh, (hh.x * 0.9).clamp(1.6, 4.2)));
        }
    }
    lay
}

/// [plant-scale] The plumes of a coal station on `lot`: every stack's top
/// (smoke) and every cooling tower's mouth (steam), for the renderer.
pub fn coal_vents(lot: &nd_sim::Lot, seed: u64, scale: f32) -> Vec<CoalVent> {
    let (frame, poly) = crate::buildings::Frame::lot_local(lot);
    let lay = coal_layout(&poly, seed, scale);
    let world = |p: Vec2| frame.to_world(p.x, p.y);
    let mut out: Vec<CoalVent> = lay
        .stacks
        .iter()
        .map(|&(p, r, h, _)| {
            let w = world(p);
            CoalVent { top: Vec3::new(w.x, h + 0.8, w.y), r: r * 0.75, steam: false }
        })
        .collect();
    out.extend(lay.towers.iter().map(|&(p, rb, th)| {
        let w = world(p);
        CoalVent { top: Vec3::new(w.x, th + 1.0, w.y), r: rb * 0.62, steam: true }
    }));
    out
}

/// An oval ring about `c` (lot-local) with half axes along u and v.
pub(super) fn oval(f: &crate::buildings::Frame, c: Vec2, h: Vec2, y: f32, n: usize) -> Vec<Vec3> {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            f.at(c.x + a.cos() * h.x, y, c.y + a.sin() * h.y)
        })
        .collect()
}

/// A smokestack: a tapering sooty shaft on a plinth, blackened at the top,
/// service galleries, and red aviation lights at the top (and half way up
/// on the tall ones).
pub(super) fn smokestack(g: &mut Geometry, f: &crate::buildings::Frame, p: Vec2, r: f32, h: f32, light: [f32; 3], tone: f32) {
    let w = f.to_world(p.x, p.y);
    let top_r = r * 0.72;
    g.part(Concrete).cylinder(w, r * 1.3, r * 1.18, 0.0, 2.5, 14, rgba(mul(SOOT_CONCRETE, 1.3)));
    g.part(Concrete).cylinder(w, r, top_r * 1.03, 2.5, h * 0.88, 14, rgba(mul(SOOT_CONCRETE, tone)));
    g.part(Matte).cylinder(w, top_r * 1.03, top_r, h * 0.88, h, 14, rgba(COAL_BLACK));
    g.part(Matte).cylinder(w, top_r * 0.86, top_r * 0.86, h, h + 0.5, 14, rgba([0.01, 0.01, 0.01]));
    let mut gallery = |y: f32| {
        let rr = r + (top_r - r) * (y / h) + 0.7;
        g.part(Matte).cylinder(w, rr, rr, y, y + 0.35, 14, rgba(BLACK_STEEL));
    };
    gallery(h - 4.0);
    if h > 60.0 {
        gallery(h * 0.55);
    }
    for k in 0..3 {
        let a = k as f32 / 3.0 * std::f32::consts::TAU + 0.4;
        let rr = top_r + 0.25;
        g.part(Glow).boxed(Vec3::new(w.x + a.cos() * rr, h - 1.2, w.y + a.sin() * rr), Vec3::splat(0.45), 0.0, rgba(light));
        if h > 60.0 {
            let rr = r + (top_r - r) * 0.55 + 0.9;
            g.part(Glow).boxed(Vec3::new(w.x + a.cos() * rr, h * 0.55 + 0.9, w.y + a.sin() * rr), Vec3::splat(0.4), 0.0, rgba(light));
        }
    }
}

/// A boiler house: a tall black steel box, ribbed, braced, its slits lit
/// orange by the fires inside, soot running down from the roof.
pub(super) fn boiler_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, h: Vec2, bh: f32) {
    let f = &c.frame;
    let body = rect(p.x, p.y, h.x, h.y);
    let tone = rng.random_range(0.85..1.2);
    extrude(g, f, Matte, &body, 0.0, bh, mul(BLACK_STEEL, tone));
    extrude(g, f, Concrete, &rect(p.x, p.y, h.x + 0.3, h.y + 0.3), 0.0, 3.0, mul(SOOT_CONCRETE, 1.2));
    // Penthouse and the fans on the roof.
    let ph = rect(p.x + rng.random_range(-1.0..1.0), p.y, h.x * 0.6, h.y * 0.5);
    extrude(g, f, Matte, &ph, bh, bh + 3.5, mul(BLACK_STEEL, 0.8));
    for k in 0..2 {
        let q = Vec2::new(p.x + (k as f32 - 0.5) * h.x * 0.9, p.y - h.y * 0.6);
        vent_stack(g, f, q, bh, 0.5, 2.5, mul(BLACK_STEEL, 0.7));
    }
    for face in c.faces(&body) {
        // Ribs.
        let n = (face.len / 2.6).floor() as i32;
        for k in 1..n {
            let t = -face.len * 0.5 + k as f32 * face.len / n as f32;
            face.bx(g, f, Metal, t, bh * 0.5, 0.12, Vec3::new(0.22, bh, 0.25), mul(BLACK_STEEL, 0.6));
        }
        // Firelight slits at odd heights.
        let slits = rng.random_range(2..5);
        for _ in 0..slits {
            let t = rng.random_range(-face.len * 0.4..face.len * 0.4);
            let sh = rng.random_range(2.5..6.0);
            let y = rng.random_range(5.0..(bh - sh).max(6.0));
            face.decal(g, f, Glow, t, y, 0.28, rng.random_range(0.35..0.6), sh, mul(EMBER, rng.random_range(0.55..1.0)));
        }
        // The control room's low strip of light.
        if face.is_front {
            face.decal(g, f, Glow, 0.0, 4.2, 0.28, face.len * 0.7, 0.5, mul(EMBER, 0.45));
        }
        grime(g, c, rng, &face, 0.0, bh, 3.0);
        // Cross bracing up the side walls.
        if face.out.y.abs() < 0.5 {
            let panels = ((bh / 9.0).floor() as i32).max(1);
            for k in 0..panels {
                let (y0, y1) = (3.0 + k as f32 * (bh - 3.0) / panels as f32, 3.0 + (k + 1) as f32 * (bh - 3.0) / panels as f32);
                let (a, b) = (face.at(-face.len * 0.45, 0.35), face.at(face.len * 0.45, 0.35));
                g.part(Metal).beam(f.at(a.x, y0, a.y), f.at(b.x, y1, b.y), 0.3, rgba(RUST_DARK));
                g.part(Metal).beam(f.at(b.x, y0, b.y), f.at(a.x, y1, a.y), 0.3, rgba(RUST_DARK));
            }
        }
    }
}

/// A cooling tower: a sooty concrete hyperboloid, darker at the lip.
pub(super) fn cooling_tower(g: &mut Geometry, f: &crate::buildings::Frame, p: Vec2, rb: f32, th: f32) {
    let w = f.to_world(p.x, p.y);
    let profile = [(0.0f32, 1.0f32), (0.12, 0.9), (0.3, 0.77), (0.55, 0.64), (0.78, 0.6), (0.92, 0.63), (1.0, 0.66)];
    let shade = |t: f32| mul(SOOT_CONCRETE, 2.6 - 1.6 * t * t);
    for seg in profile.windows(2) {
        let a = MeshData::ngon(w, rb * seg[0].1, 24, 0.0, th * seg[0].0);
        let b = MeshData::ngon(w, rb * seg[1].1, 24, 0.0, th * seg[1].0);
        g.part(Concrete).frustum(&a, &b, rgba(shade(seg[1].0)));
    }
    // The dark mouth and the shadowed ring of legs at the foot.
    g.part(Matte).cylinder(w, rb * 0.62, rb * 0.62, th - 0.2, th + 0.05, 24, rgba([0.012, 0.012, 0.014]));
    g.part(Matte).cylinder(w, rb * 1.02, rb * 1.01, 0.0, 1.6, 24, rgba([0.02, 0.02, 0.022]));
}

pub fn coal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let lay = coal_layout(&c.lot_poly, c.seed, c.plant_scale);
    // Ground black with coal dust.
    let yard = yard(g, c, 0.5, Dirt, [0.03, 0.028, 0.027]);
    coal_heaps(g, c, rng, &lay);
    for &(p, h, bh) in &lay.boilers {
        boiler_house(g, c, rng, p, h, bh);
    }
    stacks_and_flues(g, c, &lay);
    ash_filters(g, c, &lay);
    turbine_hall(g, c, rng, &lay);
    conveyors(g, c, &lay);
    for &(p, rb, th) in &lay.towers {
        cooling_tower(g, f, p, rb, th);
    }
    transformer_yard(g, c, &lay);
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.6, true);
    // Sodium floodlights round the yard.
    let (lo, hi) = bounds(&c.lot_poly);
    for p in [front.at(-front.len * 0.4, -1.5), front.at(front.len * 0.4, -1.5), Vec2::new(lo.x + 3.0, hi.y - 3.0), Vec2::new(hi.x - 3.0, hi.y - 3.0)] {
        floodlight(g, c, p, 11.0);
    }
    sign_board(g, c, rng, &front, 0.0, 3.0, (front.len * 0.4).min(10.0), 0.9);
}

/// Coal heaps: long black windrows on their own dust.
fn coal_heaps(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, lay: &CoalLayout) {
    let f = &c.frame;
    // Coal heaps: long black windrows on their own dust.
    for &(p, h, hh) in &lay.heaps {
        let dust: Vec<Vec2> = (0..16)
            .map(|i| {
                let a = i as f32 / 16.0 * std::f32::consts::TAU;
                p + Vec2::new(a.cos() * (h.x + 1.2), a.sin() * (h.y + 1.2))
            })
            .collect();
        if poly_inside(c, &dust) {
            ground(g, f, Dirt, &dust, 0.06, [0.035, 0.032, 0.03]);
        }
        let base = oval(f, p, h, 0.0, 14);
        let top = oval(f, p, h * Vec2::new(0.22, 0.7), hh, 14);
        g.part(Matte).frustum(&base, &top, rgba(mul(COAL_BLACK, rng.random_range(0.8..1.3))));
    }
}

/// The smokestacks, and the flue duct from each boiler to its own on a trestle.
fn stacks_and_flues(g: &mut Geometry, c: &Ctx, lay: &CoalLayout) {
    let f = &c.frame;
    let pal = c.palette;
    // Stacks, and the flue duct from each boiler to its own, on a trestle.
    for &(q, r, h, unit) in &lay.stacks {
        let tone = 0.9 + 0.3 * ((q.x * 7.3 + q.y * 3.1).sin() * 0.5 + 0.5);
        smokestack(g, f, q, r, h, pal.aviation_light, tone);
        if let Some(&(p, bh_half, bh)) = unit.and_then(|i| lay.boilers.get(i)) {
            let y = (bh * 0.62).min(h * 0.4);
            let from = Vec2::new(crate::mesh::clamp_safe(q.x, p.x - bh_half.x + 1.0, p.x + bh_half.x - 1.0), p.y + bh_half.y);
            let a = f.at(from.x, y, from.y);
            let b = f.at(q.x, y, q.y);
            g.part(Metal).tube(a, b, 1.3, 10, rgba(mul(BLACK_STEEL, 0.9)));
            let m = a.lerp(b, 0.5);
            g.part(Matte).boxed(Vec3::new(m.x, y * 0.5, m.z), Vec3::new(0.6, y, 0.6), f.yaw(), rgba(RUST_DARK));
        }
    }
}

/// Ash filters (precipitators): boxes on legs with hoppers under them.
fn ash_filters(g: &mut Geometry, c: &Ctx, lay: &CoalLayout) {
    let f = &c.frame;
    // Ash filters: boxes on legs with hoppers under them.
    for &(p, h, fh) in &lay.filters {
        extrude(g, f, Metal, &rect(p.x, p.y, h.x, h.y), 4.0, 4.0 + fh, mul(BLACK_STEEL, 1.15));
        for (du, dv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            f.bx(g, Matte, p.x + du * (h.x - 0.4), 2.0, p.y + dv * (h.y - 0.4), Vec3::new(0.45, 4.0, 0.45), RUST_DARK);
        }
        let w = f.to_world(p.x, p.y);
        g.part(Metal).cylinder(w, 0.4, h.x * 0.8, 1.6, 4.0, 4, rgba(mul(BLACK_STEEL, 0.8)));
    }
}

/// The turbine hall, its glowing roof monitor, and the pipe racks back to the boilers.
fn turbine_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, lay: &CoalLayout) {
    let f = &c.frame;
    let pal = c.palette;
    // Turbine hall: long and low, a monitor along the roof glowing dimly.
    if let Some((p, h, th)) = lay.turbine {
        let hall = rect(p.x, p.y, h.x, h.y);
        extrude(g, f, Concrete, &hall, 0.0, th, mul(SOOT_CONCRETE, 1.1));
        cornice(g, c, &hall, th - 0.4, 0.8, 0.25, Metal, mul(BLACK_STEEL, 0.7));
        let monitor = rect(p.x, p.y, h.x * 0.85, h.y * 0.22);
        extrude(g, f, Metal, &monitor, th, th + 1.8, mul(BLACK_STEEL, 0.9));
        for face in c.faces(&monitor) {
            if face.len > 4.0 {
                face.decal(g, f, Glow, 0.0, th + 0.9, 0.05, face.len * 0.9, 0.7, mul(EMBER, 0.35));
            }
        }
        for face in c.faces(&hall) {
            if face.len > 6.0 {
                face.decal(g, f, Glow, 0.0, th * 0.62, 0.06, face.len * 0.8, 0.5, mul(EMBER, 0.3));
                grime(g, c, rng, &face, 0.0, th, 2.0);
            }
        }
        // Pipe racks from the hall back to each boiler.
        for &(b, bh_half, _) in &lay.boilers {
            for (k, y) in [6.0f32, 7.2].iter().enumerate() {
                let du = (k as f32 - 0.5) * 1.4;
                let u = crate::mesh::clamp_safe(b.x + du, p.x - h.x + 1.0, p.x + h.x - 1.0);
                let pts = [Vec3::new(u, *y, p.y + h.y), Vec3::new(u, *y, (p.y + h.y + b.y - bh_half.y) * 0.5), Vec3::new(b.x + du, *y, b.y - bh_half.y)];
                if pts[2].z > pts[0].z {
                    pipe_run(g, f, &pts, 0.45, mul(pal.steel, 0.35));
                }
            }
        }
    }
}

/// Conveyor galleries from the coal yard up to the boiler houses.
fn conveyors(g: &mut Geometry, c: &Ctx, lay: &CoalLayout) {
    let f = &c.frame;
    // Conveyor galleries from the coal yard up to the boiler houses.
    for (i, &(b, bh_half, bh)) in lay.boilers.iter().enumerate() {
        let Some(&(hp, hh, _)) = lay.heaps.iter().take(3).min_by(|x, y| x.0.distance(b).total_cmp(&y.0.distance(b))) else { continue };
        let foot = hp + Vec2::new(hh.x + 1.5, 0.0);
        let side = Vec2::new(b.x - bh_half.x, b.y + (i as f32 - 0.5) * 2.0);
        if !c.inside_local(foot) || distance_to_boundary(foot, &c.lot_poly) < 2.3 || foot.distance(side) < 6.0 {
            continue;
        }
        let a = f.at(foot.x, 3.0, foot.y);
        let top = f.at(side.x, bh - 3.0, side.y);
        g.part(Metal).beam(a, top, 1.5, rgba(mul(RUST_DARK, 0.9)));
        f.bx(g, Metal, foot.x, 2.0, foot.y, Vec3::new(3.0, 4.0, 3.0), mul(BLACK_STEEL, 1.1));
        let n = ((a.distance(top) / 12.0).floor() as i32).max(1);
        for k in 1..=n {
            let m = a.lerp(top, k as f32 / (n + 1) as f32);
            g.part(Matte).boxed(Vec3::new(m.x, m.y * 0.5, m.z), Vec3::new(0.5, m.y, 0.5), f.yaw(), rgba(RUST_DARK));
        }
    }
}

/// The fenced transformer yard: insulators and bus bars.
fn transformer_yard(g: &mut Geometry, c: &Ctx, lay: &CoalLayout) {
    let f = &c.frame;
    let pal = c.palette;
    // Transformer yard: fenced, insulators, bus bars.
    if let Some(ty) = lay.transformer {
        let pen = rect(ty.x, ty.y, 5.0, 4.0);
        ground(g, f, Dirt, &pen, 0.05, pal.gravel);
        fence_poly(g, c, &pen, 2.0, None, true);
        for k in 0..3 {
            let p = ty + Vec2::new((k as f32 - 1.0) * 3.0, 0.0);
            f.bx(g, Metal, p.x, 1.2, p.y, Vec3::new(1.6, 2.4, 1.2), mul(pal.metal_paint(1), 0.5));
            f.bx(g, Matte, p.x, 3.3, p.y, Vec3::new(0.15, 1.8, 0.15), [0.45, 0.45, 0.42]);
        }
        f.bx(g, Matte, ty.x, 4.3, ty.y, Vec3::new(8.0, 0.08, 0.08), [0.4, 0.4, 0.4]);
    }
}
