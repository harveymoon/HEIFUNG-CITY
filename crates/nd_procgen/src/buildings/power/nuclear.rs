//! A nuclear station: containments, turbine halls, cooling towers and a switchyard.

use super::*;

/// [plant-scale] A nuclear station's units in lot coordinates: one or two
/// reactors (a domed containment with its turbine and service hall), two to
/// four cooling towers, and a switchyard, laid out at their own sizes on
/// the lot rather than stretched across it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NuclearLayout {
    /// Containments: centre, radius.
    pub reactors: Vec<(Vec2, f32)>,
    /// Service and turbine halls: centre, half size, height.
    pub halls: Vec<(Vec2, Vec2, f32)>,
    /// Cooling towers: base, radius at the foot.
    pub towers: Vec<(Vec2, f32)>,
    pub switchyard: Option<Vec2>,
    pub gate: Option<Vec2>,
}

pub(crate) fn nuclear_layout(poly: &[Vec2], seed: u64, scale: f32) -> NuclearLayout {
    let mut rng = rng_for(seed, 0x0A70);
    let s = scale.clamp(0.3, 3.0);
    let (lo, hi) = bounds(poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let at = |fu: f32, fv: f32| Vec2::new(lo.x + fu * w, lo.y + fv * d);
    let reach = w.max(d);
    let mut pk = Packer::new(poly);
    let mut lay = NuclearLayout::default();
    let reactors = if s >= 1.6 { 2 } else { 1 };
    let r = (8.0 + 3.0 * s.min(1.6)).min(w.min(d) * 0.22).max(5.0);
    for k in 0..reactors {
        let want = at(0.3 + 0.32 * k as f32, 0.62);
        if let Some(p) = pk.place(want, Vec2::splat(r), 2.0, 2.0, reach * 0.5).or_else(|| pk.place(want, Vec2::splat(r * 0.7), 1.5, 1.5, reach * 0.5)) {
            let rr = pk.taken.last().map_or(r, |(a, b)| (b.x - a.x) * 0.5);
            lay.reactors.push((p, rr));
            let hh = Vec2::new(rng.random_range(6.0..8.0), rng.random_range(5.0..7.0));
            if let Some(q) = pk.place(p + Vec2::new(rr + hh.x + 3.0, 0.0), hh, 1.5, 1.5, reach * 0.4) {
                lay.halls.push((q, hh, rng.random_range(14.0..18.0)));
            }
        }
    }
    let towers = 2 + usize::from(s >= 1.2) + usize::from(s >= 1.9);
    let rb = (7.5 + 2.0 * s.min(1.5)).min(w.min(d) * 0.2).max(3.0);
    for k in 0..towers {
        let fu = (k as f32 + 0.5) / towers as f32;
        if let Some(q) = pk.place(at(fu, 0.2), Vec2::splat(rb), 1.5, 2.0, reach) {
            lay.towers.push((q, rb));
        }
    }
    lay.switchyard = pk.place(at(0.9, 0.9), Vec2::new(6.3, 5.3), 0.8, 1.0, reach * 0.5);
    lay.gate = pk.place(at(0.75, 0.05), Vec2::new(2.0, 1.6), 1.5, 0.5, reach * 0.3);
    lay
}

pub fn nuclear(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.4));
    let (lo, hi) = bounds(&c.lot_poly);
    let concrete = mul(pal.concrete(0), 1.15);
    let lay = nuclear_layout(&c.lot_poly, c.seed, c.plant_scale);
    let panel = mul(pick(rng, &pal.panel), 0.95);
    // Containment: a cylinder with a dome.
    for &(cc, r) in &lay.reactors {
        let w = f.to_world(cc.x, cc.y);
        // The dome stands on the drum: the caps between them, and between
        // the dome's rings, are hidden and left out.
        g.part(Concrete).cylinder_capped(w, r, r, 0.0, r * 1.8, 20, true, false, rgba(concrete));
        let mut prev = MeshData::ngon(w, r, 20, 0.0, r * 1.8);
        for k in 1..=5 {
            let t = k as f32 / 5.0;
            let a = t * std::f32::consts::FRAC_PI_2;
            // The last ring keeps a small radius so no side quad collapses.
            let ring = MeshData::ngon(w, (r * a.cos()).max(r * 0.05), 20, 0.0, r * 1.8 + r * 0.9 * a.sin());
            g.part(Concrete).frustum_capped(&prev, &ring, false, k == 5, rgba(mul(concrete, 1.0 - 0.05 * k as f32)));
            prev = ring;
        }
        aviation_light(g, pal, Vec3::new(w.x, r * 2.75, w.y), 0.4);
    }
    // Turbine and reactor service halls.
    for &(p, h, hh) in &lay.halls {
        let hall = rect(p.x, p.y, h.x, h.y);
        extrude(g, f, Panel, &hall, 0.0, hh, panel);
        cornice(g, c, &hall, hh - 0.5, 1.0, 0.3, Panel, mul(panel, 0.8));
        for face in c.faces(&hall) {
            if face.len > 6.0 {
                band_windows(g, c, rng, &face, hh * 0.62, 1, 4.0);
            }
        }
    }
    // Cooling towers: hyperboloids from stacked frustums, with a steam plume.
    for &(p, rb) in &lay.towers {
        let w = f.to_world(p.x, p.y);
        let profile = [(0.0f32, 1.0f32), (0.3, 0.78), (0.6, 0.62), (0.8, 0.6), (1.0, 0.68)];
        let th = rb * 3.6;
        let last = profile.len() - 2;
        for (k, seg) in profile.windows(2).enumerate() {
            let a = MeshData::ngon(w, rb * seg[0].1, 18, 0.0, th * seg[0].0);
            let b = MeshData::ngon(w, rb * seg[1].1, 18, 0.0, th * seg[1].0);
            g.part(Concrete).frustum_capped(&a, &b, k == 0, k == last, rgba(mul(concrete, 0.95)));
        }
        for s in 0..4 {
            let y = th + 1.0 + s as f32 * 4.0;
            g.part(Matte).cylinder(w + Vec2::new(s as f32 * 0.5, 0.0), rb * 0.5 + s as f32 * 0.4, rb * 0.55 + s as f32 * 0.4, y, y + 3.5, 10, rgba([0.9, 0.9, 0.92]));
        }
    }
    // Switchyard, double fence, floodlights, gatehouse.
    if let Some(sy) = lay.switchyard {
        let pen = rect(sy.x, sy.y, 6.0, 5.0);
        ground(g, f, Dirt, &pen, 0.05, pal.gravel);
        fence_poly(g, c, &pen, 2.2, None, true);
        for k in 0..4 {
            let p = sy + Vec2::new((k as f32 - 1.5) * 3.0, 0.0);
            f.bx(g, Metal, p.x, 1.3, p.y, Vec3::new(1.8, 2.6, 1.3), mul(pal.metal_paint(1), 0.9));
            f.bx(g, Matte, p.x, 4.0, p.y, Vec3::new(0.15, 2.6, 0.15), [0.6, 0.6, 0.55]);
        }
        f.bx(g, Matte, sy.x, 5.3, sy.y, Vec3::new(11.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
        f.bx(g, Matte, sy.x, 5.3, sy.y + 1.2, Vec3::new(11.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 3.0, true);
    fence_lot(g, c, &offset_poly(&yard, 1.5), 2.4, true);
    for p in [front.at(-front.len * 0.42, -2.0), front.at(front.len * 0.42, -2.0), Vec2::new(lo.x + 3.0, hi.y - 3.0), Vec2::new(hi.x - 3.0, lo.y + 3.0)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 12.0);
        }
    }
    if let Some(gate) = lay.gate {
        let house = rect(gate.x, gate.y, 2.0, 1.6);
        extrude(g, f, Panel, &house, 0.0, 3.0, mul(panel, 0.9));
        curtain_lights(g, c, rng, &house, 1.0, 1, 1.6);
    }
    bollards(g, c, &front, 6, -1.4);
    sign_board(g, c, rng, &front, 0.0, 3.2, (front.len * 0.4).min(10.0), 1.0);
}
