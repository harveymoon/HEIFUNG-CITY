//! [r15-water] The water's edge drawn where the sim puts it.
//!
//! The sim's water is exact (`Landscape::is_water`: a river's centreline and
//! width, a lake's or the sea's outline), but the renderer draws the ground
//! as a height field of 8 m cells (`terrain::LandscapeTiles`) and the water
//! as one flat sheet at `WATER_LEVEL` under it. Where the bank stands high
//! (a town raised to its bridges' decks, a quay street on fill) a cell
//! running from the bank down to the bed rose over the sheet metres into the
//! water: a 14 m stream through Harvey's city was drawn as a strip of brown
//! bed colour, or not at all, though the sim still called it a river.
//!
//! The sim now keeps its free ground under the water
//! (`nd_sim::landscape::keep_water_open`); here every stretch of shore where
//! the drawn ground still stands over the water is cut out of the ground
//! mesh, a strip [`STRIP`] metres into the water (the whole of a narrow
//! river), and drawn as it would be built: a stone quay wall from the bank
//! down into the water where the bank is high, a short earth bank where it
//! is low, and the bed below.

use crate::buildings::parts::{mix, mul, rgba};
use crate::lot_ground::TerrainProbe;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::landscape::{Landscape, WATER_LEVEL};
use nd_sim::World;

/// How far into the water the cut reaches from the shore. Longer than an
/// 8 m cell's diagonal, so no cell left beyond it touches the land.
pub const STRIP: f32 = 12.0;
/// Longest piece of shore cut or drawn as one.
pub const PIECE: f32 = 6.0;
/// A bank standing this far over the water gets a quay wall, not an earth bank.
pub const WALL_FROM: f32 = 1.4;
/// The bed along a cut shore, and a wall's foot.
pub const EDGE_BED: f32 = WATER_LEVEL - 1.5;

/// One piece of shore: `a` to `b` on the waterline, `na`, `nb` pointing
/// into the water, `width` how far the strip reaches.
#[derive(Clone, Copy, Debug)]
pub struct ShorePiece {
    pub a: Vec2,
    pub b: Vec2,
    pub na: Vec2,
    pub nb: Vec2,
    pub width: f32,
}

impl ShorePiece {
    pub fn at(&self, s: f32, t: f32) -> Vec2 {
        self.a.lerp(self.b, s) + self.na.lerp(self.nb, s).normalize_or_zero() * t
    }

    /// The strip as two triangles (each convex, as `holes` needs).
    pub fn triangles(&self) -> [Vec<Vec2>; 2] {
        let (c, d) = (self.b + self.nb * self.width, self.a + self.na * self.width);
        [vec![self.a, self.b, c], vec![self.a, c, d]]
    }
}

/// Every line of shore on the map with its pieces, in order.
pub fn shore_lines(land: &Landscape) -> Vec<Vec<ShorePiece>> {
    let mut out = Vec::new();
    if land.is_flat() {
        return out;
    }
    // Clear of the map's edge, where the ground tiles meet the far ground
    // vertex for vertex (a cut there would add vertices on the seam).
    let half = land.land_half() - STRIP - 12.0;
    let inside = |p: Vec2| p.x.abs() < half && p.y.abs() < half;
    // Points along a line with a normal each (averaged at the corners),
    // split into pieces no longer than `PIECE`.
    let pieces = |pts: &[Vec2], ns: &[Vec2], width: &dyn Fn(usize) -> f32| -> Vec<ShorePiece> {
        let mut v = Vec::new();
        for k in 0..pts.len().saturating_sub(1) {
            let (a, b) = (pts[k], pts[k + 1]);
            let m = (a.distance(b) / PIECE).ceil().max(1.0) as usize;
            for i in 0..m {
                let (s0, s1) = (i as f32 / m as f32, (i + 1) as f32 / m as f32);
                let (pa, pb) = (a.lerp(b, s0), a.lerp(b, s1));
                if !inside(pa) || !inside(pb) {
                    continue;
                }
                let na = ns[k].lerp(ns[k + 1], s0).normalize_or_zero();
                let nb = ns[k].lerp(ns[k + 1], s1).normalize_or_zero();
                v.push(ShorePiece { a: pa, b: pb, na, nb, width: width(k) });
            }
        }
        v
    };
    let normals = |pts: &[Vec2], closed: bool| -> Vec<Vec2> {
        let n = pts.len();
        (0..n)
            .map(|i| {
                let prev = if i > 0 { pts[i - 1] } else if closed { pts[n - 2] } else { pts[i] };
                let next = if i + 1 < n { pts[i + 1] } else if closed { pts[1] } else { pts[i] };
                let t = (next - prev).normalize_or_zero();
                Vec2::new(-t.y, t.x)
            })
            .collect()
    };
    for r in &land.rivers {
        if r.points.len() < 2 {
            continue;
        }
        let hw = r.width * 0.5;
        let ns = normals(&r.points, false);
        let w = STRIP.min(hw);
        for side in [1.0f32, -1.0] {
            let bank: Vec<Vec2> = r.points.iter().zip(&ns).map(|(p, n)| *p + *n * hw * side).collect();
            let inward: Vec<Vec2> = ns.iter().map(|n| -*n * side).collect();
            out.push(pieces(&bank, &inward, &|_| w));
        }
    }
    let mut rings: Vec<Vec<Vec2>> = Vec::new();
    for l in &land.lakes {
        let mut ring = l.outline.clone();
        if let Some(&f) = ring.first() {
            ring.push(f);
        }
        rings.push(ring);
    }
    if let Some(sea) = land.sea_now() {
        rings.extend(sea.shores());
    }
    for line in rings {
        if line.len() < 2 {
            continue;
        }
        let closed = line.len() > 2 && line[0].distance(line[line.len() - 1]) < 0.01;
        let mut ns = normals(&line, closed);
        // Which side is the water: most of the line's midpoints say.
        let wet = line.windows(2).zip(ns.iter()).filter(|(w, n)| land.is_water((w[0] + w[1]) * 0.5 + **n * 2.0)).count();
        if wet * 2 < line.len() - 1 {
            for n in ns.iter_mut() {
                *n = -*n;
            }
        }
        out.push(pieces(&line, &ns, &|_| STRIP));
    }
    out
}

/// The pieces of shore whose drawn ground stands over the water anywhere
/// in their strip, with a piece either side so a cut run ends in water.
pub fn cut_pieces(world: &World, probe: &TerrainProbe) -> Vec<ShorePiece> {
    let land = &world.landscape;
    if !world.relief || land.is_flat() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in shore_lines(land) {
        let covered: Vec<bool> = line
            .iter()
            .map(|pc| {
                [0.1f32, 0.5, 0.9].iter().any(|&s| {
                    [0.3f32, 1.5, 0.35 * pc.width, 0.7 * pc.width, pc.width - 0.2].iter().any(|&t| {
                        let p = pc.at(s, t);
                        probe.height(p) > WATER_LEVEL - 0.05 && land.is_water(p)
                    })
                })
            })
            .collect();
        for k in 0..line.len() {
            if covered[k] || (k > 0 && covered[k - 1]) || covered.get(k + 1).copied().unwrap_or(false) {
                out.push(line[k]);
            }
        }
    }
    out
}

/// The holes the cut shore leaves in the ground mesh.
pub fn water_holes(world: &World) -> Vec<Vec<Vec2>> {
    water_holes_with(world, &TerrainProbe::new(world))
}

pub fn water_holes_with(world: &World, probe: &TerrainProbe) -> Vec<Vec<Vec2>> {
    cut_pieces(world, probe).iter().flat_map(|p| p.triangles()).collect()
}

/// Is the bank of a cut piece high enough for a quay wall: the highest the
/// drawn ground stands along its shore line.
pub fn walled(probe: &TerrainProbe, pc: &ShorePiece) -> bool {
    probe.breaks(pc.a, pc.b).into_iter().map(|t| probe.height(pc.a.lerp(pc.b, t))).fold(f32::MIN, f32::max) > WALL_FROM
}

/// The ground as drawn with the shore cut: the terrain mesh, and inside a
/// cut strip the bank or wall and the bed drawn there (for probes and
/// tests: is the sim's water drawn as water?).
pub struct DrawnGround<'a> {
    pub probe: &'a TerrainProbe<'a>,
    cut: Vec<(ShorePiece, bool)>,
    bins: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

impl<'a> DrawnGround<'a> {
    pub fn new(world: &World, probe: &'a TerrainProbe<'a>) -> Self {
        let cut: Vec<(ShorePiece, bool)> = cut_pieces(world, probe).into_iter().map(|pc| (pc, walled(probe, &pc))).collect();
        let mut bins: std::collections::HashMap<(i32, i32), Vec<u32>> = Default::default();
        for (k, (pc, _)) in cut.iter().enumerate() {
            let [t0, t1] = pc.triangles();
            let (lo, hi) = nd_sim::geom::bounds(&[t0, t1].concat());
            for j in (lo.y / 16.0).floor() as i32..=(hi.y / 16.0).floor() as i32 {
                for i in (lo.x / 16.0).floor() as i32..=(hi.x / 16.0).floor() as i32 {
                    bins.entry((i, j)).or_default().push(k as u32);
                }
            }
        }
        Self { probe, cut, bins }
    }

    /// How many pieces of shore are cut.
    pub fn cut_len(&self) -> usize {
        self.cut.len()
    }

    /// The drawn ground's height at `p`.
    pub fn height(&self, p: Vec2) -> f32 {
        let key = ((p.x / 16.0).floor() as i32, (p.y / 16.0).floor() as i32);
        if let Some(v) = self.bins.get(&key) {
            for &k in v {
                let (pc, walled) = &self.cut[k as usize];
                let [t0, t1] = pc.triangles();
                if !nd_sim::geom::point_in_polygon(p, &t0) && !nd_sim::geom::point_in_polygon(p, &t1) {
                    continue;
                }
                let (s, on) = nd_sim::geom::project_on_segment(p, pc.a, pc.b);
                let t = (p - on).length();
                let _ = s;
                if *walled || t >= 1.0 {
                    return bed_at(t);
                }
                let h = self.probe.height(on).max(EDGE_BED);
                return h + (EDGE_BED - h) * t;
            }
        }
        self.probe.height(p)
    }
}

/// The natural bed `t` metres out from the shore (as the landscape carves
/// it), at least [`EDGE_BED`] deep.
fn bed_at(t: f32) -> f32 {
    (WATER_LEVEL - 1.2 - t.min(10.0) * 0.35).min(EDGE_BED)
}

/// The cut shore drawn: quay walls, earth banks and the bed (absolute
/// heights), into `g`.
pub fn water_edges(world: &World, palette: &Palette, g: &mut Geometry) {
    let probe = TerrainProbe::new(world);
    water_edges_with(world, palette, &probe, &cut_pieces(world, &probe), g);
}

pub fn water_edges_with(world: &World, palette: &Palette, probe: &TerrainProbe, cut: &[ShorePiece], g: &mut Geometry) {
    let stone = mix(palette.stone, palette.concrete(0), 0.35);
    let wet = mul(mix(stone, [0.16, 0.2, 0.17], 0.55), 0.55);
    culverts(&world.landscape, probe, stone, wet, g);
    let earth = mul(mix(palette.ground, [0.3, 0.26, 0.2], 0.5), 0.8);
    let mud = mul(mix(palette.ground, [0.25, 0.22, 0.16], 0.6), 0.6);
    let v = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    for pc in cut {
        let into = pc.na.lerp(pc.nb, 0.5).normalize_or_zero();
        let into3 = Vec3::new(into.x, 0.0, into.y);
        // The shore line as the cut ground meets it: the drawn height at
        // every place the mesh bends along it.
        let ts = probe.breaks(pc.a, pc.b);
        let top: Vec<(Vec2, f32)> = ts.iter().map(|&t| {
            let p = pc.a.lerp(pc.b, t);
            (p, probe.height(p))
        }).collect();
        let high = top.iter().map(|x| x.1).fold(f32::MIN, f32::max);
        let walled = high > WALL_FROM;
        // The bed: from the foot of the bank (at the line for a wall, a
        // metre out for an earth bank) to the strip's far edge, where it
        // meets the ground left beyond (or the other bank's strip).
        let foot = if walled { 0.0 } else { 1.0 };
        let far = |s: f32| {
            let p = pc.at(s, pc.width);
            (p, probe.height(p).min(bed_at(pc.width)))
        };
        let (fa, fb) = (far(0.0), far(1.0));
        let (ma, mb) = (pc.at(0.0, foot), pc.at(1.0, foot));
        let bed = g.part(Surface::Dirt);
        bed.quad_facing([v(ma, EDGE_BED), v(mb, EDGE_BED), v(fb.0, fb.1), v(fa.0, fa.1)], Vec3::Y, rgba(mud));
        if walled {
            // A quay wall: stone from under the water to a coping over the
            // bank, dark and wet below the waterline.
            // [r17-fixes] (photo 2026-11-24 F5) A level coping along the
            // piece, at its highest bank: following the 8 m ground cells'
            // heights the wall's top ran up and down in a saw-tooth under
            // the lots above.
            let level = high.max(0.6) + 0.2;
            let wall = g.part(Surface::Stone);
            for w in top.windows(2) {
                let ((p0, _), (p1, _)) = (w[0], w[1]);
                let (t0, t1) = (level, level);
                wall.quad_facing([v(p0, EDGE_BED), v(p1, EDGE_BED), v(p1, 0.35), v(p0, 0.35)], into3, rgba(wet));
                wall.quad_facing([v(p0, 0.35), v(p1, 0.35), v(p1, t1), v(p0, t0)], into3, rgba(stone));
                // The coping, half a metre back over the bank.
                let back = -into * 0.5;
                wall.quad_facing([v(p0, t0), v(p1, t1), v(p1 + back, t1), v(p0 + back, t0)], Vec3::Y, rgba(mul(stone, 1.1)));
            }
        } else {
            // An earth bank shelving into the water.
            let bank = g.part(Surface::Dirt);
            for w in top.windows(2) {
                let ((p0, h0), (p1, h1)) = (w[0], w[1]);
                let (q0, q1) = (p0 + into * foot, p1 + into * foot);
                if h0.max(h1) <= EDGE_BED {
                    continue;
                }
                bank.quad_facing([v(p0, h0.max(EDGE_BED)), v(p1, h1.max(EDGE_BED)), v(q1, EDGE_BED), v(q0, EDGE_BED)], Vec3::Y, rgba(earth));
            }
        }
    }
}

/// [r17-fixes] (photo 2026-11-24 F6) Where a stream starts in town (its
/// line ends with no water beyond: it comes from under the city) a culvert
/// mouth: a concrete headwall across the end with a dark culvert opening
/// at the waterline and a coping, so the stream runs out of a drain, not
/// out of a blank wall.
fn culverts(land: &Landscape, probe: &TerrainProbe, stone: [f32; 3], wet: [f32; 3], g: &mut Geometry) {
    if land.is_flat() {
        return;
    }
    let half = land.land_half() - STRIP - 12.0;
    let v = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    let dark = [0.012, 0.013, 0.014];
    for r in &land.rivers {
        let n = r.points.len();
        if n < 2 {
            continue;
        }
        for (end, prev) in [(r.points[0], r.points[1]), (r.points[n - 1], r.points[n - 2])] {
            let dir = (end - prev).normalize_or_zero();
            let hw = r.width * 0.5;
            // (The sim's river is round at its ends, `hw` past the last
            // point: look beyond that cap for more water.)
            if dir == Vec2::ZERO || end.x.abs() > half || end.y.abs() > half || land.is_water(end + dir * (hw + 2.0)) {
                continue;
            }
            // The headwall across the cap, half way into it.
            let end = end + dir * (hw * 0.5);
            let side = dir.perp();
            let (a, b) = (end - side * hw, end + side * hw);
            let into = Vec3::new(-dir.x, 0.0, -dir.y);
            let top = probe.height(end + dir * 1.5).max(0.6) + 0.25;
            let wall = g.part(Surface::Concrete);
            wall.quad_facing([v(b, EDGE_BED), v(a, EDGE_BED), v(a, 0.35), v(b, 0.35)], into, rgba(wet));
            wall.quad_facing([v(b, 0.35), v(a, 0.35), v(a, top), v(b, top)], into, rgba(stone));
            let back = dir * 0.6;
            wall.quad_facing([v(b, top), v(a, top), v(a + back, top), v(b + back, top)], Vec3::Y, rgba(mul(stone, 1.1)));
            // The opening, a box culvert's dark mouth just proud of the wall.
            let ow = (hw * 1.2).min(5.0);
            let oh = (top - 0.5).clamp(0.6, 2.2);
            let (oa, ob) = (end - side * ow * 0.5 - dir * 0.04, end + side * ow * 0.5 - dir * 0.04);
            g.part(Surface::Matte).quad_facing([v(ob, -0.3), v(oa, -0.3), v(oa, oh), v(ob, oh)], into, rgba(dark));
            // Its lintel.
            let (la, lb) = (oa - side * 0.3 - dir * 0.08, ob + side * 0.3 - dir * 0.08);
            g.part(Surface::Concrete).quad_facing([v(lb, oh), v(la, oh), v(la, oh + 0.35), v(lb, oh + 0.35)], into, rgba(mul(stone, 0.85)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shore_piece_is_two_triangles_into_the_water() {
        let pc = ShorePiece { a: Vec2::ZERO, b: Vec2::new(6.0, 0.0), na: Vec2::Y, nb: Vec2::Y, width: STRIP };
        let [t0, t1] = pc.triangles();
        assert_eq!(t0.len(), 3);
        assert!(t0.iter().chain(&t1).all(|p| p.y >= 0.0 && p.y <= STRIP));
        assert!((pc.at(0.5, 3.0) - Vec2::new(3.0, 3.0)).length() < 1e-4);
    }

    /// Every point the sim calls water is drawn as water: the ground as
    /// drawn (the terrain mesh, the cut shore's banks and bed) stands under
    /// the sheet there, but for the odd lot floor laid over the water.
    #[test]
    fn water_is_drawn_wherever_the_sim_says_water() {
        let (w, _, _) = crate::citygen::generate_world(crate::citygen::CityPreset::OldTown, 2);
        let land = &w.landscape;
        let probe = TerrainProbe::new(&w);
        let drawn = DrawnGround::new(&w, &probe);
        let lots: Vec<&Vec<Vec2>> = w.lots.values().map(|l| &l.polygon).collect();
        let (mut wet, mut covered, mut before) = (0, 0, 0);
        let half = land.land_half() - 10.0;
        let mut y = -half;
        while y < half {
            let mut x = -half;
            while x < half {
                let p = Vec2::new(x, y);
                x += 3.0;
                if !land.is_water(p) || lots.iter().any(|l| nd_sim::geom::point_in_polygon(p, l)) {
                    continue;
                }
                wet += 1;
                covered += (drawn.height(p) > WATER_LEVEL) as usize;
                before += (probe.height(p) > WATER_LEVEL) as usize;
            }
            y += 3.0;
        }
        println!("{wet} water points, {covered} drawn under ground ({before} with the shore uncut), {} pieces cut", drawn.cut_len());
        assert!(wet > 1000, "the old town has its river ({wet})");
        // A few points at the ends of cut runs and beside bridge abutments.
        assert!(covered * 200 <= wet, "{covered} of {wet} water points drawn under ground");
        assert!(covered * 10 <= before, "the cut shore draws the water ({covered} covered, {before} uncut)");
    }

    #[test]
    fn the_bed_deepens_out_from_the_shore() {
        assert!(bed_at(0.0) <= EDGE_BED);
        assert!(bed_at(10.0) < bed_at(2.0));
    }
}
