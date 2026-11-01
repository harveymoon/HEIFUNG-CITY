//! [tools] Footbridges over the street (`nd_sim::walk::Footbridge`): a
//! walkway deck on two columns at `level` above the pavement, glass
//! balustrades with steel rails and a neon strip along each edge, and a
//! flight of stairs from each end down to the pavement, with stringers and
//! a handrail. Built from the walk graph, so what walkers climb is what is
//! drawn.

use crate::color::{mul, rgba};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::walk::{Stair, StairKind};
use nd_sim::World;

/// Rise of one step, metres.
const STEP_RISE: f32 = 0.3;
/// Depth of the walkway deck.
const DECK_DEPTH: f32 = 0.45;
/// Balustrade height.
const RAIL: f32 = 1.1;

/// Every footbridge of the walk graph.
pub fn footbridges(world: &World, palette: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let concrete = mul(palette.concrete(0), 0.9);
    let ground = |p: Vec2| world.surface_height(p);
    // [walk-crossings] The walkways: each run of decks joined end to end
    // (footbridges round a junction and the corner decks between them) is
    // one walkway, drawn along its line with mitred corners, so the ring
    // reads as one continuous deck with unbroken rails (G14) rather than
    // slabs crossing at every corner.
    let mut run_z: std::collections::HashMap<(u32, u32), f32> = std::collections::HashMap::new();
    for (pts, closed) in deck_runs(world) {
        let w = nd_sim::junctions::FOOTBRIDGE_WIDTH;
        let z = pts.iter().map(|&p| ground(p)).fold(f32::MIN, f32::max) + nd_sim::junctions::FOOTBRIDGE_LEVEL;
        walkway(&mut g, &pts, closed, z, w, palette);
        for p in &pts {
            run_z.insert((p.x.to_bits(), p.y.to_bits()), z);
        }
    }
    for fb in &world.walk.footbridges {
        let w = fb.width;
        let [t0, t1] = fb.top;
        // The stairs climb to the deck of the walkway the bridge is part of.
        let z = [t0, t1].iter().filter_map(|p| run_z.get(&(p.x.to_bits(), p.y.to_bits())).copied()).fold(ground(t0).max(ground(t1)) + fb.level, f32::max);
        // Stairs from each end down to its foot on the pavement.
        for i in 0..2 {
            let (top, foot) = (fb.top[i], fb.foot[i]);
            let st = &fb.stairs[i];
            let gz = ground(foot);
            match st.kind {
                // [r5-stairs] Down and back in two flights round a landing.
                StairKind::Switchback => switchback(&mut g, st, z, gz, fb.level, palette),
                // [r5-stairs] Round a column in a steel cage.
                StairKind::Spiral => spiral(&mut g, st, z, gz, palette),
                StairKind::Straight | StairKind::None => {
                    let rise = z - gz;
                    let run = (foot - top).length();
                    if run < 0.5 || rise < 0.5 {
                        continue;
                    }
                    let along = (foot - top) / run;
                    let yaw = along.y.atan2(along.x);
                    flight(&mut g, top, z, foot, gz, w, DECK_DEPTH, palette);
                    // A column under the stair head, on the pavement.
                    let col = top + along * 0.6;
                    let cz = ground(col);
                    g.part(Surface::Concrete).boxed(Vec3::new(col.x, (z - DECK_DEPTH + cz) * 0.5, col.y), Vec3::new(0.7, z - DECK_DEPTH - cz, 0.7), yaw, rgba(mul(concrete, 0.8)));
                }
            }
        }
    }
    // [pt-walk] Corner decks joining two footbridges' ends: a column at
    // the middle of each (the deck itself is part of its walkway, above).
    let level = nd_sim::junctions::FOOTBRIDGE_LEVEL;
    for &[p0, p1] in &world.walk.footbridge_links {
        let z = ground(p0).max(ground(p1)) + level;
        let across = (p1 - p0).normalize_or_zero();
        let mid = (p0 + p1) * 0.5;
        let cz = ground(mid);
        g.part(Surface::Concrete).boxed(Vec3::new(mid.x, (z - DECK_DEPTH + cz) * 0.5, mid.y), Vec3::new(0.7, z - DECK_DEPTH - cz, 0.7), across.y.atan2(across.x), rgba(mul(concrete, 0.8)));
    }
    g
}

/// One flight of stairs from `a` (its top step's tread at `za`) down to
/// `b` on `zb`, `w` wide: the steps, the stringer slab under them (from
/// `head` under the top), and a handrail, posts and a neon strip each side.
#[allow(clippy::too_many_arguments)]
fn flight(g: &mut Geometry, a: Vec2, za: f32, b: Vec2, zb: f32, w: f32, head: f32, palette: &Palette) {
    let concrete = mul(palette.concrete(0), 0.9);
    let rise = za - zb;
    let run = (b - a).length();
    if run < 0.2 || rise < 0.2 {
        return;
    }
    let along = (b - a) / run;
    let yaw = along.y.atan2(along.x);
    let steps = (rise / STEP_RISE).ceil().max(1.0) as usize;
    let tread = run / steps as f32;
    for k in 0..steps {
        // Step k counted down from the top.
        let c = a + along * (tread * (k as f32 + 0.5));
        let h = za - rise * (k as f32 + 1.0) / steps as f32;
        g.part(Surface::Concrete).boxed(Vec3::new(c.x, h + STEP_RISE * 0.5 - 0.02, c.y), Vec3::new(tread + 0.02, STEP_RISE, w - 0.2), yaw, rgba(mul(concrete, 1.05)));
    }
    // The stringer slab under the steps.
    let top3 = Vec3::new(a.x, za - head, a.y);
    let foot3 = Vec3::new(b.x, zb, b.y);
    g.part(Surface::Concrete).slab(top3, foot3, w, 0.35, rgba(concrete));
    let sd = Vec3::new(-along.y, 0.0, along.x) * (w * 0.5 - 0.05);
    for s in [1.0f32, -1.0] {
        let o = sd * s;
        g.part(Surface::Metal).beam(top3 + o + Vec3::Y * (RAIL + head), foot3 + o + Vec3::Y * RAIL, 0.07, rgba(palette.steel));
        g.part(Surface::Glow).beam(top3 + o + Vec3::Y * 0.05, foot3 + o + Vec3::Y * 0.05, 0.05, rgba(palette.footbridge_neon));
        for k in 0..=4 {
            let p = top3.lerp(foot3, k as f32 / 4.0) + o;
            g.part(Surface::Metal).beam(p, p + Vec3::Y * RAIL, 0.05, rgba(palette.steel));
        }
    }
}

/// [r5-stairs] A switchback (`StairKind::Switchback`): the first flight
/// down the road-side half of the pavement from the deck's edge, a landing
/// on steel posts, and the second flight back down the inner half to its
/// foot beside the deck's end. `z` is the deck's height, `gz` the foot's
/// ground, `level` the bridge's height above the street.
fn switchback(g: &mut Geometry, st: &Stair, z: f32, gz: f32, level: f32, palette: &Palette) {
    use nd_sim::walk::{SWITCHBACK_FLIGHT, SWITCHBACK_GAP, SWITCHBACK_LANDING};
    let p = &st.path;
    if p.len() < 7 {
        return;
    }
    let at = |l: f32| gz + (z - gz) * (l / level.max(0.1));
    let zm = at(p[2].1);
    let w = SWITCHBACK_FLIGHT;
    flight(g, p[1].0, z, p[2].0, zm, w, DECK_DEPTH, palette);
    flight(g, p[5].0, zm, p[6].0, gz, w, 0.35, palette);
    // The landing: one slab under both flights' ends, railed round its
    // three open sides, on two steel posts with a cross brace.
    let concrete = rgba(mul(palette.concrete(0), 0.85));
    let steel = rgba(palette.steel);
    let across = SWITCHBACK_FLIGHT * 2.0 + SWITCHBACK_GAP;
    let near = (p[2].0 + p[5].0) * 0.5;
    let mid = near + st.along * (SWITCHBACK_LANDING * 0.5);
    let yaw = st.along.y.atan2(st.along.x);
    g.part(Surface::Concrete).boxed(Vec3::new(mid.x, zm - 0.175, mid.y), Vec3::new(SWITCHBACK_LANDING, 0.35, across), yaw, concrete);
    let far = near + st.along * SWITCHBACK_LANDING;
    let half = st.out * (across * 0.5 - 0.05);
    let v = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
    let corners = [near - half, far - half, far + half, near + half];
    for k in 0..3 {
        let (c0, c1) = (corners[k], corners[k + 1]);
        g.part(Surface::Metal).beam(v(c0, zm + RAIL), v(c1, zm + RAIL), 0.07, steel);
        g.part(Surface::Glow).beam(v(c0, zm - 0.3), v(c1, zm - 0.3), 0.05, rgba(palette.footbridge_neon));
        let n = c0.distance(c1).ceil().max(1.0) as usize;
        for j in 0..n {
            let q = c0.lerp(c1, j as f32 / n as f32);
            g.part(Surface::Metal).beam(v(q, zm), v(q, zm + RAIL), 0.05, steel);
        }
    }
    let (b0, b1) = (far - half * 0.9 - st.along * 0.25, far + half * 0.9 - st.along * 0.25);
    for q in [b0, b1] {
        g.part(Surface::Metal).boxed(v(q, (gz + zm - 0.35) * 0.5), Vec3::new(0.3, zm - 0.35 - gz, 0.3), yaw, steel);
    }
    g.part(Surface::Metal).beam(v(b0, gz + 0.4), v(b1, zm - 0.6), 0.08, steel);
    // A post under the deck's end where the first flight leaves it.
    let col = p[0].0 + st.along * 0.6;
    g.part(Surface::Concrete).boxed(v(col, (z - DECK_DEPTH + gz) * 0.5), Vec3::new(0.6, z - DECK_DEPTH - gz, 0.6), yaw, rgba(mul(palette.concrete(0), 0.72)));
}

/// [r5-stairs] A spiral (`StairKind::Spiral`): open steel treads round a
/// concrete column, one and a half turns from the deck's end down to the
/// pavement, with a handrail and a neon strip spiralling round the outside
/// and a cage of steel posts tied by a ring at the top.
fn spiral(g: &mut Geometry, st: &Stair, z: f32, gz: f32, palette: &Palette) {
    use nd_sim::walk::{SPIRAL_INNER, SPIRAL_OUTER, SPIRAL_WALK};
    let rise = z - gz;
    if rise < 0.5 {
        return;
    }
    let steel = rgba(palette.steel);
    let tread_c = rgba(mul(palette.steel, 0.75));
    let concrete = rgba(mul(palette.concrete(0), 0.7));
    let c = st.centre;
    let v = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
    let height = |f: f32| z - rise * f;
    // The column, through the stair to above the top rail, on a plinth.
    g.part(Surface::Concrete).cylinder(c, SPIRAL_INNER, SPIRAL_INNER, gz, z + RAIL + 0.4, 8, concrete);
    g.part(Surface::Concrete).cylinder(c, SPIRAL_OUTER + 0.2, SPIRAL_OUTER + 0.15, gz - 0.2, gz + 0.12, 10, concrete);
    // The treads: thin steel wedges, open between (no risers).
    let steps = (rise / SPIRAL_STEP_RISE).ceil().max(2.0) as usize;
    for k in 0..steps - 1 {
        let (f0, f1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
        let h = height(f1);
        let (i0, o0, i1, o1) = (st.spiral_point(f0, SPIRAL_INNER), st.spiral_point(f0, SPIRAL_OUTER), st.spiral_point(f1, SPIRAL_INNER), st.spiral_point(f1, SPIRAL_OUTER));
        let t = 0.08;
        let m = g.part(Surface::Metal);
        m.quad_facing([v(i0, h), v(o0, h), v(o1, h), v(i1, h)], Vec3::Y, tread_c);
        m.quad_facing([v(i0, h - t), v(i1, h - t), v(o1, h - t), v(o0, h - t)], -Vec3::Y, tread_c);
        let mo = (o0 + o1) * 0.5 - c;
        m.quad_facing([v(o0, h - t), v(o1, h - t), v(o1, h), v(o0, h)], Vec3::new(mo.x, 0.0, mo.y), tread_c);
        // The leading edge faces back up the stair.
        let back = st.spiral_point(f0 - 0.02, SPIRAL_WALK) - st.spiral_point(f0, SPIRAL_WALK);
        m.quad_facing([v(i0, h - t), v(o0, h - t), v(o0, h), v(i0, h)], Vec3::new(back.x, 0.0, back.y), tread_c);
    }
    // The handrail, neon strip and balusters round the outside.
    let pieces = 18;
    let ro = SPIRAL_OUTER - 0.06;
    for k in 0..pieces {
        let (f0, f1) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
        let (p0, p1) = (st.spiral_point(f0, ro), st.spiral_point(f1, ro));
        let (h0, h1) = (height(f0), height(f1));
        g.part(Surface::Metal).beam(v(p0, h0 + RAIL), v(p1, h1 + RAIL), 0.07, steel);
        g.part(Surface::Glow).beam(v(p0, h0 - 0.12), v(p1, h1 - 0.12), 0.05, rgba(palette.footbridge_neon));
        g.part(Surface::Metal).beam(v(p0, h0 - 0.05), v(p0, h0 + RAIL), 0.04, steel);
    }
    // The cage: posts round the tower from the plinth to a ring over the
    // deck's rail, left open on the deck's side and over the foot.
    let cage = SPIRAL_OUTER + 0.1;
    let ring: Vec<Vec2> = (0..8).map(|k| c + Vec2::from_angle(k as f32 * std::f32::consts::TAU / 8.0) * cage).collect();
    let top_ring = z + RAIL + 0.3;
    let to_foot = (st.foot().unwrap_or(c) - c).normalize_or_zero();
    for (k, &q) in ring.iter().enumerate() {
        let dir = (q - c).normalize_or_zero();
        let r = ring[(k + 1) % ring.len()];
        let open = dir.dot(-st.along) > 0.8 || dir.dot(to_foot) > 0.8;
        if !open {
            g.part(Surface::Metal).beam(v(q, gz + 0.1), v(q, top_ring), 0.09, steel);
            // A diagonal tie to the next post.
            g.part(Surface::Metal).beam(v(q, gz + 2.2), v(r, gz + 4.4), 0.04, steel);
        }
        g.part(Surface::Metal).beam(v(q, top_ring), v(r, top_ring), 0.08, steel);
    }
}

/// [r5-stairs] Rise of a spiral's treads.
const SPIRAL_STEP_RISE: f32 = 0.2;

/// [walk-crossings] The footbridges' decks joined end to end into runs:
/// each a line of points (the tops of the bridges, in order) and whether
/// it closes on itself (a ring round a junction).
pub fn deck_runs(world: &World) -> Vec<(Vec<Vec2>, bool)> {
    let key = |p: Vec2| (p.x.to_bits(), p.y.to_bits());
    let mut pieces: Vec<[Vec2; 2]> = world.walk.footbridges.iter().map(|f| f.top).collect();
    pieces.extend(world.walk.footbridge_links.iter().copied());
    pieces.retain(|p| p[0].distance(p[1]) > 1e-3);
    let mut by_point: std::collections::HashMap<(u32, u32), Vec<usize>> = std::collections::HashMap::new();
    for (i, p) in pieces.iter().enumerate() {
        for q in p {
            by_point.entry(key(*q)).or_default().push(i);
        }
    }
    let mut used = vec![false; pieces.len()];
    let mut runs = Vec::new();
    // Walk on from `at` along unused pieces until the run ends or closes.
    let extend = |start: usize, from: Vec2, used: &mut Vec<bool>| -> (Vec<Vec2>, bool) {
        let mut pts = vec![from];
        let mut i = start;
        let mut at = from;
        loop {
            used[i] = true;
            let p = pieces[i];
            let next = if key(p[0]) == key(at) { p[1] } else { p[0] };
            pts.push(next);
            at = next;
            if key(at) == key(from) {
                pts.pop();
                return (pts, true);
            }
            let Some(&j) = by_point.get(&key(at)).and_then(|v| v.iter().find(|&&j| !used[j])) else { return (pts, false) };
            i = j;
        }
    };
    // Open runs first, from their free ends (points with one piece).
    for (i, p) in pieces.iter().enumerate() {
        for q in p {
            if !used[i] && by_point.get(&key(*q)).is_some_and(|v| v.len() == 1) {
                let (pts, closed) = extend(i, *q, &mut used);
                runs.push((pts, closed));
            }
        }
    }
    for i in 0..pieces.len() {
        if !used[i] {
            let (pts, closed) = extend(i, pieces[i][0], &mut used);
            runs.push((pts, closed));
        }
    }
    runs
}

/// [walk-crossings] One walkway along `pts` at height `z`: the deck, its
/// paving, glass balustrades with a steel top rail and posts, and a neon
/// strip under each edge, all following the line round its corners.
fn walkway(g: &mut Geometry, pts: &[Vec2], closed: bool, z: f32, w: f32, palette: &Palette) {
    if pts.len() < 2 {
        return;
    }
    let concrete = mul(palette.concrete(0), 0.9);
    // A closed ring is drawn once round and back to its start, with the
    // first corner repeated so the seam is mitred like the rest.
    // A free end (a stair head) runs on half the deck's width, so the
    // deck covers the head of the stairs.
    let line: Vec<Vec2> = if closed {
        let mut v = pts.to_vec();
        v.push(pts[0]);
        v.push(pts[1]);
        v
    } else {
        let mut v = pts.to_vec();
        let n = v.len();
        let (d0, d1) = ((v[0] - v[1]).normalize_or_zero(), (v[n - 1] - v[n - 2]).normalize_or_zero());
        v[0] += d0 * w * 0.5;
        v[n - 1] += d1 * w * 0.5;
        v
    };
    let pts = &line[..if closed { pts.len() } else { line.len() }];
    let span = |v: &[Vec2]| if closed { v[1..].to_vec() } else { v.to_vec() };
    let off_line = |o: f32| span(&crate::mesh::offset_path(&line, o));
    let deck_l = off_line(-w * 0.5);
    let deck_r = off_line(w * 0.5);
    // The deck: its top, sides and underside between the two edges.
    let up = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    for i in 0..deck_l.len().saturating_sub(1) {
        let (l0, l1, r0, r1) = (deck_l[i], deck_l[i + 1], deck_r[i], deck_r[i + 1]);
        let c = rgba(concrete);
        g.part(Surface::Concrete).quad_facing([up(l0, z), up(l1, z), up(r1, z), up(r0, z)], Vec3::Y, c);
        g.part(Surface::Concrete).quad_facing([up(l0, z - DECK_DEPTH), up(r0, z - DECK_DEPTH), up(r1, z - DECK_DEPTH), up(l1, z - DECK_DEPTH)], -Vec3::Y, c);
        let d = (l1 - l0).normalize_or_zero();
        let out_l = Vec3::new(d.y, 0.0, -d.x);
        g.part(Surface::Concrete).quad_facing([up(l0, z - DECK_DEPTH), up(l1, z - DECK_DEPTH), up(l1, z), up(l0, z)], out_l, c);
        g.part(Surface::Concrete).quad_facing([up(r0, z - DECK_DEPTH), up(r1, z - DECK_DEPTH), up(r1, z), up(r0, z)], -out_l, c);
    }
    if !closed {
        let n = deck_l.len();
        let d0 = (pts[1] - pts[0]).normalize_or_zero();
        let d1 = (pts[n - 1] - pts[n - 2]).normalize_or_zero();
        let c = rgba(concrete);
        g.part(Surface::Concrete).quad_facing([up(deck_l[0], z - DECK_DEPTH), up(deck_r[0], z - DECK_DEPTH), up(deck_r[0], z), up(deck_l[0], z)], Vec3::new(-d0.x, 0.0, -d0.y), c);
        g.part(Surface::Concrete).quad_facing([up(deck_l[n - 1], z - DECK_DEPTH), up(deck_r[n - 1], z - DECK_DEPTH), up(deck_r[n - 1], z), up(deck_l[n - 1], z)], Vec3::new(d1.x, 0.0, d1.y), c);
    }
    // The paving on top.
    let pave_l = off_line(-(w - 0.3) * 0.5);
    let pave_r = off_line((w - 0.3) * 0.5);
    for i in 0..pave_l.len().saturating_sub(1) {
        g.part(Surface::Paving).quad_facing([up(pave_l[i], z + 0.02), up(pave_l[i + 1], z + 0.02), up(pave_r[i + 1], z + 0.02), up(pave_r[i], z + 0.02)], Vec3::Y, rgba(mul(concrete, 1.15)));
    }
    // Each edge: glass, the top rail, posts, the neon strip below.
    for s in [-1.0f32, 1.0] {
        let edge = off_line(s * (w * 0.5 - 0.06));
        for i in 0..edge.len().saturating_sub(1) {
            let (a, b) = (edge[i], edge[i + 1]);
            let mid_y = z + RAIL * 0.5 + 0.05;
            g.part(Surface::Glass).slab(up(a, mid_y), up(b, mid_y), 0.04, RAIL * 0.5, rgba(palette.footbridge_glass));
            g.part(Surface::Metal).beam(up(a, z + RAIL), up(b, z + RAIL), 0.08, rgba(palette.steel));
            g.part(Surface::Glow).beam(up(a, z - DECK_DEPTH + 0.05), up(b, z - DECK_DEPTH + 0.05), 0.07, rgba(palette.footbridge_neon));
            let len = a.distance(b);
            let posts = (len / 2.5).ceil().max(1.0) as usize;
            for k in 0..posts {
                let p = a.lerp(b, k as f32 / posts as f32);
                g.part(Surface::Metal).beam(up(p, z), up(p, z + RAIL), 0.06, rgba(palette.steel));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::junctions::CrossingUpgrade;
    use nd_sim::{LandUse, WorldConfig};

    #[test]
    fn a_footbridge_is_drawn_up_at_its_walkway() {
        let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-200.0, 8.0), Vec2::new(200.0, 8.0), Vec2::new(200.0, 60.0), Vec2::new(-200.0, 60.0)], LandUse::Residential);
        let cr = w.walk.crossings.iter().position(|c| c.kind == nd_sim::walk::CrossingKind::Zebra).expect("a zebra") as u32;
        w.economy.treasury = 1.0e6;
        w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
        let g = footbridges(&w, &Palette::default());
        let top = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(f32::MIN, f32::max);
        assert!(top > nd_sim::junctions::FOOTBRIDGE_LEVEL, "the balustrade stands over the walkway: {top}");
        assert!(g.iter().any(|(s, m)| s == Surface::Glow && !m.positions.is_empty()), "neon strips");
    }

    /// [footbridge] The zebra (and its beacons) under a footbridge is not
    /// painted: the bridge replaced it.
    #[test]
    fn the_zebra_under_a_footbridge_is_not_painted() {
        use crate::street::{generate_street, StreetInput, StreetLayer};
        let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2, 2);
        w.add_zone(vec![Vec2::new(-200.0, 8.0), Vec2::new(200.0, 8.0), Vec2::new(200.0, 60.0), Vec2::new(-200.0, 60.0)], LandUse::Residential);
        let cr = w.walk.crossings.iter().position(|c| c.kind == nd_sim::walk::CrossingKind::Zebra).expect("a zebra") as u32;
        let mid = w.walk.crossings[cr as usize].mid;
        w.economy.treasury = 1.0e6;
        w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
        let closed = w.closed_crosswalks();
        assert_eq!(closed, vec![mid]);
        let pal = Palette::default();
        let paint_near = |closed: &[Vec2]| {
            let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: 1, heights: &[], inner: &[], layer: StreetLayer::Surface, closed, joined_ramps: &[] });
            // The street runs along x: the zebra's stripes lie across it.
            let paint = g.get(Surface::Paint).positions.iter().filter(|p| (p[0] - mid.x).abs() < 1.4 && (p[2] - mid.y).abs() < 9.0).count();
            let beacons = g.get(Surface::Glow).positions.iter().filter(|p| Vec2::new(p[0], p[2]).distance(mid) < 12.0).count();
            (paint, beacons)
        };
        let (paint, beacons) = paint_near(&[]);
        let (left, beacons_left) = paint_near(&closed);
        assert!(beacons > 0 && beacons_left == 0, "the beacons go: {beacons} -> {beacons_left}");
        assert!(paint >= left + 40, "the stripes go: {paint} -> {left}");
    }

    /// Review #4: on a footbridge across a way at 45 degrees the steps run
    /// across the stair and the stair-head columns stand square to it (they
    /// were turned by twice the angle).
    #[test]
    fn steps_lie_across_their_stair_at_any_angle() {
        for deg in [45.0f32, 30.0, 0.0] {
            let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
            let across = Vec2::new(deg.to_radians().cos(), deg.to_radians().sin());
            let along = Vec2::new(-across.y, across.x);
            let (t0, t1) = (Vec2::new(10.0, 10.0), Vec2::new(10.0, 10.0) + across * 16.0);
            let width = 3.0;
            w.walk.footbridges.push(nd_sim::walk::Footbridge { crossing: 0, top: [t0, t1], foot: [t0 + along * 9.0, t1 + along * 9.0], level: 5.4, width, nodes: [0; 4], joined: [false; 2], path_join: None, stairs: Default::default() });
            let g = footbridges(&w, &Palette::default());
            let steps = crate::terrain::upright_faces(g.get(Surface::Concrete), STEP_RISE);
            crate::terrain::faces_along_are(&steps, along, width - 0.2).map_err(|e| format!("steps at {deg}: {e}")).unwrap();
        }
    }
}
