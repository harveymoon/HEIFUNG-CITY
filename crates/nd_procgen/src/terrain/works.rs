//! Works on and over the ground and the water: viaducts and their
//! pillars, underpasses, tunnel portals, abutments, bridges, piers, dams,
//! tidal caissons and the hot spots' vents.

use super::*;

/// A deck's section: slab depth, and the rail and parapet wall along each
/// edge (heights over the deck's datum).
#[derive(Clone, Copy)]
struct Deck {
    thick: f32,
    rail_y: f32,
    rail: f32,
    /// A parapet wall under the rail (a motorway's barrier stands in for it).
    walls: bool,
    wall_y: f32,
    wall_w: f32,
    wall_h: f32,
}

/// Bridges and the decks over underpasses.
const BRIDGE_DECK: Deck = Deck { thick: 1.4, rail_y: 1.0, rail: 0.15, walls: true, wall_y: 0.45, wall_w: 0.35, wall_h: 0.5 };
/// Viaducts (and the plan preview's raised roads).
const VIADUCT_DECK: Deck = Deck { thick: 1.2, rail_y: 1.05, rail: 0.12, walls: true, wall_y: 0.5, wall_w: 0.3, wall_h: 0.55 };

/// A deck slab from `a3` to `b3`, `hw` either side, with a rail (and a
/// parapet wall) at `side` and `-side` from the centreline.
#[allow(clippy::too_many_arguments)]
fn deck(g: &mut Geometry, a3: glam::Vec3, b3: glam::Vec3, hw: f32, side: Vec2, d: &Deck, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    g.part(Surface::Concrete).slab(a3, b3, hw * 2.0, d.thick, rgba(concrete));
    for s in [1.0f32, -1.0] {
        let off = Vec3::new(side.x * s, 0.0, side.y * s);
        g.part(Surface::Metal).beam(a3 + off + Vec3::Y * d.rail_y, b3 + off + Vec3::Y * d.rail_y, d.rail, rgba(steel));
        if d.walls {
            g.part(Surface::Concrete).slab(a3 + off + Vec3::Y * d.wall_y, b3 + off + Vec3::Y * d.wall_y, d.wall_w, d.wall_h, rgba(mul(concrete, 0.9)));
        }
    }
}

/// Where a way crosses the highway well above or below it
/// (`Ground::underpasses`): the upper way's deck over the cut with parapets
/// and rails, and abutment walls either side of the lower way holding the
/// embankment back, stepped down to the ground beyond them. The walls run
/// from just outside the lower way's pavement to past the cut's edge, so the
/// drawn terrain's slope into the cut is inside them.
fn underpasses(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    let all = &world.ground.underpasses;
    for (i, u) in all.iter().enumerate() {
        // Both carriageways of the highway cut the same street: its walls
        // are drawn once, by the first.
        let drawn = |p: Vec2| all[..i].iter().any(|o| o.under_dir.dot(u.under_dir).abs() > 0.999 && o.in_cut(p));
        let s = u.sin_angle();
        let opening = u.under_half_width + 0.6;
        let back = u.cut_half_width + 8.5;
        // The deck bears on the walls a few metres either side.
        let reach = (opening + 4.0) / s;
        let (a, b) = (u.at - u.over_dir * reach, u.at + u.over_dir * reach);
        let a3 = Vec3::new(a.x, u.over_z_at(-reach) - 0.05, a.y);
        let m3 = Vec3::new(u.at.x, u.over_z - 0.05, u.at.y);
        let b3 = Vec3::new(b.x, u.over_z_at(reach) - 0.05, b.y);
        let hw = u.over_half_width + 0.4;
        let side = Vec2::new(-u.over_dir.y, u.over_dir.x) * (hw - 0.2);
        // A highway or ramp carried over has its own concrete barrier along
        // the deck's edge (`street`): a parapet there stood in the same
        // place, the two boxes flickering through each other. It keeps the
        // rail on top.
        let motorway_over = world.roads.segments_in_box(u.at - Vec2::splat(2.0), u.at + Vec2::splat(2.0)).any(|o| {
            let s = world.roads.segment(o);
            let (oa, ob) = world.roads.segment_points(o);
            s.alive && (s.is_motorway() || s.ring) && (ob - oa).normalize_or_zero().dot(u.over_dir).abs() > 0.98 && nd_sim::roads::project_on_segment(u.at, oa, ob).1.distance(u.at) < 1.0
        });
        // Two halves meeting over the crossing: it may be the crest of a
        // way lifted over the other.
        // On the barrier's top (0.8 m) or the parapet's.
        let spec = if motorway_over { Deck { rail_y: 0.88, walls: false, ..BRIDGE_DECK } } else { BRIDGE_DECK };
        for (p0, p1) in [(a3, m3), (m3, b3)] {
            deck(g, p0, p1, hw, side, &spec, concrete, steel);
        }
        // Abutment and wing walls along the lower way, their tops following
        // the embankment beyond them down to its toe.
        let across = Vec2::new(-u.under_dir.y, u.under_dir.x);
        let step = 3.0;
        let steps = (u.cut_half_length / step).ceil() as i32;
        let wall = rgba(mul(concrete, 0.78));
        let coping = rgba(mul(concrete, 0.7));
        for sgn in [1.0f32, -1.0] {
            let at = |along: f32, lat: f32, y: f32| {
                let p = u.at + u.under_dir * along + across * sgn * lat;
                Vec3::new(p.x, y, p.y)
            };
            // Wall top at each station: the highest ground the wall holds
            // back there, never above the upper way's carriageway.
            let top = |along: f32| {
                let base = u.at + u.under_dir * along;
                let ground = [0.5f32, 0.75, 1.0]
                    .iter()
                    .map(|f| opening + (back - opening) * f)
                    .chain([back + 4.0])
                    .map(|o| world.ground_height(base + across * sgn * o))
                    .fold(f32::MIN, f32::max);
                let mid = base + across * sgn * (opening + back) * 0.5;
                ground.min(u.over_z_at((mid - u.at).dot(u.over_dir)) - 0.15).max(u.under_z_at(along))
            };
            for k in -steps..steps {
                let (a0, a1) = (k as f32 * step, (k + 1) as f32 * step);
                if drawn(u.at + u.under_dir * (a0 + a1) * 0.5) {
                    continue;
                }
                let (f0, f1) = (u.under_z_at(a0), u.under_z_at(a1));
                let (t0, t1) = (top(a0), top(a1));
                if (t0 - f0).max(t1 - f1) < 0.4 {
                    continue;
                }
                let (b0, b1) = (f0 - 0.6, f1 - 0.6);
                let out = Vec3::new(across.x * sgn, 0.0, across.y * sgn);
                let m = g.part(Surface::Concrete);
                // Face to the lower way, back into the fill, coping on top.
                m.quad_facing([at(a0, opening, b0), at(a1, opening, b1), at(a1, opening, t1), at(a0, opening, t0)], -out, wall);
                m.quad_facing([at(a0, back, b0), at(a1, back, b1), at(a1, back, t1), at(a0, back, t0)], out, wall);
                m.quad_facing([at(a0, opening, t0), at(a1, opening, t1), at(a1, back, t1), at(a0, back, t0)], Vec3::Y, coping);
            }
        }
    }
}

/// Roads the player raised off the ground: a deck the width of the road
/// and its pavements under the street mesh, parapets and rails, and the
/// concrete columns under it. A way the player drew raised stands on the
/// columns placed for it (`World::pillars`: shifted along it to clear the
/// roads and buildings underneath); a way hoisted by the grading alone (a
/// highway lifted over a street) gets one every 24 m or so wherever the
/// deck stands clear of the ground.
pub fn viaducts(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    let roads = &world.roads;
    // [walk-crossings] Raised footpaths are drawn a run at a time, so a
    // walkway turning a corner is one deck with unbroken parapets (G14)
    // rather than two slabs whose rails cross.
    path_viaducts(world, g, concrete, steel);
    for (sid, seg) in roads.alive_segments() {
        if !seg.viaduct || seg.kind == nd_sim::roads::RoadKind::Path {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let za = world.road_z.get(seg.a.0 as usize).copied().unwrap_or(0.0);
        let zb = world.road_z.get(seg.b.0 as usize).copied().unwrap_or(0.0);
        // Highways and ramps have no pavements: the deck is the lanes and shoulders.
        let motorway = seg.is_motorway() || seg.ring;
        let hw = seg.half_width() + if seg.is_street() && !motorway { nd_sim::zoning::SIDEWALK } else { 0.6 };
        let planned = roads.node(seg.a).level.max(roads.node(seg.b).level) >= nd_sim::roads::VIADUCT_LEVEL && !world.pillars.is_empty();
        viaduct_span(g, a, b, za, zb, hw, &|p| world.ground_height(p), concrete, steel, !planned);
    }
    for p in &world.pillars {
        // The deck over it: the raised way it carries, nearest first.
        let deck = roads
            .segments_in_box(p.at - Vec2::splat(p.half_width + 2.0), p.at + Vec2::splat(p.half_width + 2.0))
            .filter(|&s| roads.segment(s).viaduct)
            .map(|s| {
                let (a, b) = roads.segment_points(s);
                let (t, q) = nd_sim::roads::project_on_segment(p.at, a, b);
                (s, t, q.distance(p.at))
            })
            .filter(|&(_, _, d)| d < p.half_width)
            .min_by(|x, y| x.2.total_cmp(&y.2));
        let Some((sid, t, _)) = deck else { continue };
        let top = world.road_height(sid, t) - 1.2;
        let foot = world.ground_height(p.at);
        pillar_column(g, p.at, p.dir, top, foot, p.half_width, concrete);
    }
}

/// [walk-crossings] A raised path piece shorter than this (a stub left by
/// a join) does not break the run of deck it sits on.
const PATH_STUB: f32 = 3.0;

/// [walk-crossings] The decks of the raised footpaths, joined into runs
/// through every node where the walkway just carries on (two raised path
/// pieces meet, short stubs aside): each run one mitred deck with its
/// parapets and rails following it round the corners. Where three or more
/// meet each run ends square, as before.
fn path_viaducts(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    use nd_sim::roads::{NodeId, RoadKind, SegmentId};
    let roads = &world.roads;
    let raised_path = |s: SegmentId| roads.try_segment(s).is_some_and(|sg| sg.viaduct && sg.kind == RoadKind::Path);
    let long = |s: SegmentId| {
        let (a, b) = roads.segment_points(s);
        a.distance(b) >= PATH_STUB
    };
    // The raised path pieces at a node that count for the run (stubs not).
    let through = |n: NodeId| -> Vec<SegmentId> { roads.node(n).segments.iter().copied().filter(|&s| raised_path(s) && long(s)).collect() };
    let z_of = |n: NodeId| world.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
    let mut done: std::collections::HashSet<SegmentId> = std::collections::HashSet::new();
    let pieces: Vec<SegmentId> = roads.alive_segments().filter(|(s, _)| raised_path(*s)).map(|(s, _)| s).collect();
    for &first in &pieces {
        if done.contains(&first) {
            continue;
        }
        let seg = roads.segment(first);
        let hw = seg.half_width() + 0.6;
        if !long(first) {
            // A stub: its own short deck, no parapets.
            done.insert(first);
            let (a, b) = roads.segment_points(first);
            let (za, zb) = (z_of(seg.a), z_of(seg.b));
            g.part(Surface::Concrete).slab(glam::Vec3::new(a.x, za - 0.02, a.y), glam::Vec3::new(b.x, zb - 0.02, b.y), hw * 2.0, VIADUCT_DECK.thick, rgba(concrete));
            continue;
        }
        // Grow the run both ways from `first` through nodes where exactly
        // two long raised path pieces meet.
        let mut nodes: std::collections::VecDeque<NodeId> = std::collections::VecDeque::from([seg.a, seg.b]);
        done.insert(first);
        for forward in [true, false] {
            let mut prev_seg = first;
            loop {
                let n = if forward { *nodes.back().unwrap() } else { *nodes.front().unwrap() };
                let here = through(n);
                if here.len() != 2 {
                    break;
                }
                let Some(&next) = here.iter().find(|&&s| s != prev_seg) else { break };
                if done.contains(&next) {
                    break;
                }
                done.insert(next);
                let ns = roads.segment(next);
                let far = if ns.a == n { ns.b } else { ns.a };
                if forward {
                    nodes.push_back(far);
                } else {
                    nodes.push_front(far);
                }
                prev_seg = next;
            }
        }
        let closed = nodes.len() > 3 && nodes.front() == nodes.back();
        if closed {
            nodes.pop_back();
        }
        let mut pts: Vec<Vec2> = nodes.iter().map(|&n| roads.node(n).pos).collect();
        let mut zs: Vec<f32> = nodes.iter().map(|&n| z_of(n)).collect();
        // [r1-roads] (K14) A footbridge joined to the path where it came
        // down to the street: the deck stays up and runs on into the
        // bridge's end (the bridge's stairs on the far side take walkers
        // down), not down its ramp to the kerb.
        let mut joined_end = [false; 2];
        if !closed && pts.len() >= 2 {
            let last = pts.len() - 1;
            for (end, (i, j)) in [(0usize, 1usize), (last, last - 1)].into_iter().enumerate() {
                let (n, r) = (nodes[i], nodes[j]);
                if let Some((f, k)) = world.walk.footbridges.iter().find_map(|f| f.path_join.filter(|pj| pj.1 == r && pj.2 == n).map(|pj| (f, pj.0))) {
                    pts[i] = f.top[k];
                    zs[i] = world.surface_height(f.top[0]).max(world.surface_height(f.top[1])) + f.level;
                    joined_end[end] = true;
                }
            }
        }
        // [walk-crossings] A raised path that comes down onto a street ends
        // on its pavement, not out in the middle of the road (G12): the deck
        // stops at the back of the kerb, where the walkers step off.
        if !closed {
            let ends = [(0usize, 1usize), (pts.len() - 1, pts.len() - 2)];
            for (e, (i, j)) in ends.into_iter().enumerate() {
                if joined_end[e] {
                    continue;
                }
                let n = if i == 0 { *nodes.front().unwrap() } else { *nodes.back().unwrap() };
                let street = roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street() && !roads.segment(s).fronts_nothing()).map(|&s| roads.segment(s).half_width()).fold(0.0f32, f32::max);
                if street <= 0.0 {
                    continue;
                }
                let len = pts[i].distance(pts[j]);
                let back = street + nd_sim::zoning::SIDEWALK * 0.5;
                if len > back + 1.0 {
                    pts[i] = pts[i] + (pts[j] - pts[i]) / len * back;
                }
            }
        }
        deck_run(g, &pts, &zs, closed, hw, &VIADUCT_DECK, concrete, steel);
        let planned = nodes.iter().any(|&n| roads.node(n).level >= nd_sim::roads::VIADUCT_LEVEL) && !world.pillars.is_empty();
        if !planned {
            // Columns as `viaduct_span` stands them, piece by piece.
            for w in pts.windows(2).zip(zs.windows(2)) {
                let ((a, b), (za, zb)) = ((w.0[0], w.0[1]), (w.1[0], w.1[1]));
                columns_under(g, a, b, za, zb, hw, &|p| world.ground_height(p), concrete);
            }
        }
    }
}

/// [walk-crossings] A deck along `pts` (heights `zs` at each point), `hw`
/// either side, mitred at every corner, with parapet walls and rails along
/// both edges; square ends unless `closed`.
#[allow(clippy::too_many_arguments)]
fn deck_run(g: &mut Geometry, pts: &[Vec2], zs: &[f32], closed: bool, hw: f32, d: &Deck, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    if pts.len() < 2 {
        return;
    }
    let (line, hs): (Vec<Vec2>, Vec<f32>) = if closed {
        let mut l = pts.to_vec();
        l.push(pts[0]);
        l.push(pts[1]);
        let mut h = zs.to_vec();
        h.push(zs[0]);
        h.push(zs[1]);
        (l, h)
    } else {
        (pts.to_vec(), zs.to_vec())
    };
    let skip = closed as usize;
    let off = |o: f32| -> Vec<Vec2> { crate::mesh::offset_path(&line, o).into_iter().skip(skip).collect() };
    let hs: Vec<f32> = hs.into_iter().skip(skip).map(|z| z - 0.02).collect();
    let (l, r) = (off(-hw), off(hw));
    let v = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    let c = rgba(concrete);
    for i in 0..l.len() - 1 {
        let (z0, z1) = (hs[i], hs[i + 1]);
        let (b0, b1) = (z0 - d.thick, z1 - d.thick);
        let dir = (l[i + 1] - l[i]).normalize_or_zero();
        let out_l = Vec3::new(dir.y, 0.0, -dir.x);
        let m = g.part(Surface::Concrete);
        m.quad_facing([v(l[i], z0), v(l[i + 1], z1), v(r[i + 1], z1), v(r[i], z0)], Vec3::Y, c);
        m.quad_facing([v(l[i], b0), v(r[i], b0), v(r[i + 1], b1), v(l[i + 1], b1)], -Vec3::Y, c);
        m.quad_facing([v(l[i], b0), v(l[i + 1], b1), v(l[i + 1], z1), v(l[i], z0)], out_l, c);
        m.quad_facing([v(r[i], b0), v(r[i + 1], b1), v(r[i + 1], z1), v(r[i], z0)], -out_l, c);
    }
    if !closed {
        let n = l.len();
        for (i, j, out) in [(0usize, 1usize, -1.0f32), (n - 1, n - 2, -1.0)] {
            let dir = (line[i] - line[j]).normalize_or_zero() * -out;
            g.part(Surface::Concrete).quad_facing([v(l[i], hs[i] - d.thick), v(r[i], hs[i] - d.thick), v(r[i], hs[i]), v(l[i], hs[i])], Vec3::new(dir.x, 0.0, dir.y), c);
        }
    }
    for s in [-1.0f32, 1.0] {
        let edge = off(s * (hw - 0.2));
        for i in 0..edge.len() - 1 {
            let (a3, b3) = (v(edge[i], hs[i]), v(edge[i + 1], hs[i + 1]));
            g.part(Surface::Metal).beam(a3 + Vec3::Y * d.rail_y, b3 + Vec3::Y * d.rail_y, d.rail, rgba(steel));
            if d.walls {
                g.part(Surface::Concrete).slab(a3 + Vec3::Y * d.wall_y, b3 + Vec3::Y * d.wall_y, d.wall_w, d.wall_h, rgba(mul(concrete, 0.9)));
            }
        }
    }
}

/// Columns under a deck from `a` to `b` where it stands clear of the
/// ground, about every 24 m (as `viaduct_span` does).
#[allow(clippy::too_many_arguments)]
fn columns_under(g: &mut Geometry, a: Vec2, b: Vec2, za: f32, zb: f32, hw: f32, ground: &dyn Fn(Vec2) -> f32, concrete: [f32; 3]) {
    use glam::Vec3;
    let len = a.distance(b);
    if len < 0.3 {
        return;
    }
    let dir = (b - a) / len;
    let spans = (len / 24.0).ceil().max(1.0) as i32;
    let yaw = dir.y.atan2(dir.x);
    for k in 0..=spans {
        let t = k as f32 / spans as f32;
        let p = a.lerp(b, t);
        let top = za + (zb - za) * t - 1.2;
        let foot = ground(p);
        if top - foot < 1.0 {
            continue;
        }
        let w = (hw * 0.35).clamp(1.2, 3.0);
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, (top + foot) * 0.5, p.y), Vec3::new(1.6, top - foot, w), yaw, rgba(mul(concrete, 0.8)));
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, top - 0.35, p.y), Vec3::new(1.9, 0.7, hw * 1.7), yaw, rgba(mul(concrete, 0.85)));
    }
}

/// One column on its footing from the ground at `foot` up to the deck's
/// underside at `top`, with a crosshead under the deck (`hw` its half
/// width) lying across the way (`dir` along it).
pub fn pillar_column(g: &mut Geometry, at: Vec2, dir: Vec2, top: f32, foot: f32, hw: f32, concrete: [f32; 3]) {
    use glam::Vec3;
    if top - foot < 1.0 {
        return;
    }
    let yaw = dir.y.atan2(dir.x);
    let w = (hw * 0.35).clamp(1.2, 3.0);
    g.part(Surface::Concrete).boxed(Vec3::new(at.x, (top + foot) * 0.5, at.y), Vec3::new(1.6, top - foot, w), yaw, rgba(mul(concrete, 0.8)));
    // The footing, just showing above the ground.
    g.part(Surface::Concrete).boxed(Vec3::new(at.x, foot + 0.15, at.y), Vec3::new(2.6, 0.3, w + 1.0), yaw, rgba(mul(concrete, 0.7)));
    // A crosshead under the deck.
    g.part(Surface::Concrete).boxed(Vec3::new(at.x, top - 0.35, at.y), Vec3::new(1.9, 0.7, hw * 1.7), yaw, rgba(mul(concrete, 0.85)));
}

/// What shows of the tunnels: a portal frame where each tunnel comes out
/// of the ground, retaining walls along the open cutting of its ramp, and
/// a ventilation shaft head on the surface every 150 m or so along it.
/// [r4-hillside] Least height of a bench's wall (`Hold::Bench`): below it
/// the verge meets the land without one.
pub const BENCH_WALL_MIN: f32 = 0.5;

pub fn tunnel_works(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    use nd_sim::landscape::Hold;
    let roads = &world.roads;
    let z_of = |n: nd_sim::NodeId| world.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
    let wall = rgba(mul(concrete, 0.75));
    let coping = rgba(mul(concrete, 0.6));
    for (sid, seg) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        let (za, zb) = (z_of(seg.a), z_of(seg.b));
        let len = a.distance(b);
        if len < 0.5 {
            continue;
        }
        let dir = (b - a) / len;
        let side = Vec2::new(-dir.y, dir.x);
        let yaw = dir.y.atan2(dir.x);
        if seg.tunnel {
            // Portals: where this tunnel piece meets a way that is not one.
            for (n, z) in [(seg.a, za), (seg.b, zb)] {
                let Some(o) = roads.node(n).segments.iter().copied().find(|&o| o != sid && roads.segment(o).alive && !roads.segment(o).tunnel) else { continue };
                let p = roads.node(n).pos;
                // Square to the ramp coming down to it (the bore may turn
                // at the portal).
                let os = roads.segment(o);
                let far = roads.node(if os.a == n { os.b } else { os.a }).pos;
                let out = (far - p).normalize_or_zero();
                // The bore has no pavements: the arch spans its walls.
                let behind = p - out * (PORTAL_HOLD * 0.5);
                let hill = world.landscape.height(behind).max(if world.landscape.is_flat() { 0.0 } else { BANK_HEIGHT });
                portal_arch(g, p, out, z, seg.half_width() + crate::street::TUNNEL_WALL, hill, wall, coping, rgba(mul(steel, 2.2)));
            }
            // Vent shafts on the surface along the bore.
            let vents = (len / 150.0).floor() as i32;
            for k in 1..=vents {
                let p = a.lerp(b, k as f32 / (vents + 1) as f32);
                // Not on a way crossing over the bore: the shaft head stood
                // in the lanes of the street above it.
                let on_way = roads.segments_in_box(p - Vec2::splat(30.0), p + Vec2::splat(30.0)).any(|o| {
                    let os = roads.segment(o);
                    if o == sid || os.tunnel || !os.alive {
                        return false;
                    }
                    let (oa, ob) = roads.segment_points(o);
                    let reach = os.half_width() + if os.is_street() && !os.fronts_nothing() { nd_sim::zoning::SIDEWALK } else { 0.5 } + 2.2;
                    nd_sim::roads::project_on_segment(p, oa, ob).1.distance(p) < reach
                });
                if on_way {
                    continue;
                }
                let gz = world.ground_height(p);
                g.part(Surface::Concrete).boxed(Vec3::new(p.x, gz + 1.2, p.y), Vec3::new(3.0, 2.4, 3.0), yaw, wall);
                g.part(Surface::Metal).boxed(Vec3::new(p.x, gz + 2.5, p.y), Vec3::new(2.6, 0.25, 2.6), yaw, rgba(mul(steel, 0.6)));
            }
            continue;
        }
        // [pt-visual] Held earthworks (`earthwork_hold`): a deep cutting or a
        // high fill is held by a retaining wall filling the band the ground
        // is held in (`earthwork_band`), its face at the shoulder or the
        // back of the pavement. A way eased into a gentle slope is graded,
        // not walled: walls stood free on the slopes there (F11).
        let Some(hold) = nd_sim::landscape::earthwork_hold(&world.landscape, roads, &world.road_z, &world.road_inner, sid) else { continue };
        let (face, back) = nd_sim::landscape::held_band(seg, hold);
        let bench = hold == Hold::Bench;
        let steps = (len / 4.0).ceil().max(1.0) as usize;
        for k in 0..steps {
            let (t0, t1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
            let (p0, p1) = (a.lerp(b, t0), a.lerp(b, t1));
            // Through the way's inner heights (a street on a hillside).
            let (r0, r1) = (world.road_height(sid, t0), world.road_height(sid, t1));
            for sgn in [-1.0f32, 1.0] {
                let out2 = side * sgn;
                // The ground just behind the band, as graded: the wall's
                // top is flush with it (a neighbouring way's slope may have
                // cut the hill down), so no block stands proud of the grass.
                let probe = out2 * (back + 1.0);
                // [r4-hillside] A bench's wall meets the land itself: the
                // hillside is left as it was right behind it.
                let natural = |p: Vec2| world.landscape.height(p).max(BANK_HEIGHT);
                // Never taller than `BENCH_WALL_MAX`: past it the hill is
                // cut back (or the fill banked) as the built ground shows.
                let cap = nd_sim::landscape::BENCH_WALL_MAX;
                // A lot beside a bench is levelled to the way: the ground
                // there is its pad (no wall up to the hill it replaced).
                let on_lot = |p: Vec2| world.parcels.at(p).and_then(|i| world.parcels.parcels.get(i)).is_some_and(|q| q.lot.is_some());
                let side_ground = |p: Vec2, r: f32| if on_lot(p) { world.ground_height(p) } else { natural(p).clamp(r - cap, r + cap) };
                let (g0, g1) = if bench {
                    (side_ground(p0 + out2 * (back + 2.0), r0), side_ground(p1 + out2 * (back + 2.0), r1))
                } else {
                    (world.ground_height(p0 + probe), world.ground_height(p1 + probe))
                };
                let (q0, q1) = (p0 + out2 * face, p1 + out2 * face);
                let (e0, e1) = (p0 + out2 * back, p1 + out2 * back);
                // [sweep-visual] Not across another way: at a junction on a
                // slope the wall along one arm ran on over the next arm's
                // lanes and pavements, a concrete band across the road.
                if [q0, q1, (q0 + q1) * 0.5, e0, e1].iter().any(|q| on_other_way(roads, sid, *q)) {
                    continue;
                }
                let out = Vec3::new(out2.x, 0.0, out2.y);
                let m = g.part(Surface::Concrete);
                // [r4-hillside] A bench on a hillside: on each side the wall
                // the land there needs, holding the hill back above the way
                // or holding the edge up over the drop (lower walls than a
                // motorway's: a town street's verge is a metre or two).
                let hold = match hold {
                    Hold::Bench if g0.min(g1) - r0.max(r1) >= BENCH_WALL_MIN || (g0 - r0 >= BENCH_WALL_MIN && g1 - r1 >= BENCH_WALL_MIN) => Hold::Cut,
                    Hold::Bench if r0.min(r1) - g0.max(g1) >= BENCH_WALL_MIN || (r0 - g0 >= BENCH_WALL_MIN && r1 - g1 >= BENCH_WALL_MIN) => Hold::Fill,
                    Hold::Bench => continue,
                    h => h,
                };
                let least = if bench { BENCH_WALL_MIN } else { 1.2 };
                match hold {
                    Hold::Cut if bench => {
                        // [r4-hillside] A thin wall up to the hillside with
                        // a narrow coping; the ground mesh is cut out to its
                        // face (`holes::bench_holes`).
                        let (t0, t1) = (g0.min(natural(q0)).clamp(r0, r0 + cap) + 0.35, g1.min(natural(q1)).clamp(r1, r1 + cap) + 0.35);
                        let (c0, c1) = (q0 + out2 * 0.45, q1 + out2 * 0.45);
                        m.quad_facing([Vec3::new(q0.x, r0 - 0.5, q0.y), Vec3::new(q1.x, r1 - 0.5, q1.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(q0.x, t0, q0.y)], -out, wall);
                        m.quad_facing([Vec3::new(q0.x, t0, q0.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(c1.x, t1, c1.y), Vec3::new(c0.x, t0, c0.y)], Vec3::Y, coping);
                    }
                    Hold::Cut => {
                        if g0 - r0 < least && g1 - r1 < least {
                            continue;
                        }
                        let (t0, t1) = (g0.max(r0) + 0.6, g1.max(r1) + 0.6);
                        m.quad_facing([Vec3::new(q0.x, r0 - 0.5, q0.y), Vec3::new(q1.x, r1 - 0.5, q1.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(q0.x, t0, q0.y)], -out, wall);
                        m.quad_facing([Vec3::new(q0.x, t0, q0.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(e1.x, t1, e1.y), Vec3::new(e0.x, t0, e0.y)], Vec3::Y, coping);
                    }
                    Hold::Fill => {
                        if r0 - g0 < least && r1 - g1 < least {
                            continue;
                        }
                        // A parapet on the road side, the wall face outward
                        // down to the land.
                        // (A path's parapet is a waist-high rail wall.)
                        let parapet = if seg.kind == nd_sim::roads::RoadKind::Path { 1.0 } else { 0.9 };
                        let (t0, t1) = (r0 + parapet, r1 + parapet);
                        m.quad_facing([Vec3::new(q0.x, r0 - 0.1, q0.y), Vec3::new(q1.x, r1 - 0.1, q1.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(q0.x, t0, q0.y)], -out, wall);
                        m.quad_facing([Vec3::new(q0.x, t0, q0.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(e1.x, t1, e1.y), Vec3::new(e0.x, t0, e0.y)], Vec3::Y, coping);
                        m.quad_facing([Vec3::new(e0.x, g0.min(r0) - 0.5, e0.y), Vec3::new(e1.x, g1.min(r1) - 0.5, e1.y), Vec3::new(e1.x, t1, e1.y), Vec3::new(e0.x, t0, e0.y)], out, wall);
                    }
                    Hold::Bench => {}
                }
            }
        }
    }
}

/// [r4-hillside] The verge of a way in a bench on a hillside, from the edge
/// of its paving (a street's pavement) to its walls' faces, at the way's
/// height: the ground mesh is cut out there (`holes::bench_holes`).
pub fn bench_verges(world: &World, g: &mut Geometry, concrete: [f32; 3]) {
    use glam::Vec3;
    use nd_sim::landscape::Hold;
    let roads = &world.roads;
    let col = rgba(mul(concrete, 0.7));
    for (sid, seg) in roads.alive_segments() {
        if nd_sim::landscape::earthwork_hold(&world.landscape, roads, &world.road_z, &world.road_inner, sid) != Some(Hold::Bench) {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let len = a.distance(b);
        if len < 0.5 {
            continue;
        }
        let dir = (b - a) / len;
        let side = Vec2::new(-dir.y, dir.x);
        let (face, _) = nd_sim::landscape::held_band(seg, Hold::Bench);
        // From the paving's edge (a street's pavement is drawn with it).
        let from = if seg.is_street() { seg.corridor_half_width() } else { seg.half_width() } - 0.05;
        let steps = (len / 2.0).ceil().max(1.0) as usize;
        let m = g.part(Surface::Concrete);
        for k in 0..steps {
            let (t0, t1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
            // A little past the ends, under the next piece's verge.
            let (p0, p1) = (a.lerp(b, t0) - dir * if k == 0 { 0.3 } else { 0.0 }, a.lerp(b, t1) + dir * if k + 1 == steps { 0.3 } else { 0.0 });
            let (r0, r1) = (world.road_height(sid, t0) + 0.03, world.road_height(sid, t1) + 0.03);
            for sgn in [-1.0f32, 1.0] {
                let o = side * sgn;
                let v = |p: Vec2, h: f32| Vec3::new(p.x, h, p.y);
                m.quad_facing([v(p0 + o * from, r0), v(p1 + o * from, r1), v(p1 + o * face, r1), v(p0 + o * face, r0)], Vec3::Y, col);
            }
        }
    }
}

/// [sweep-visual] Does `p` stand on a way other than `sid` (its lanes, its
/// pavements, a little margin): no cutting wall goes there.
fn on_other_way(roads: &nd_sim::roads::RoadNetwork, sid: nd_sim::roads::SegmentId, p: Vec2) -> bool {
    roads.segments_in_box(p - Vec2::splat(25.0), p + Vec2::splat(25.0)).any(|o| {
        let os = roads.segment(o);
        if o == sid || !os.alive || os.tunnel {
            return false;
        }
        let (oa, ob) = roads.segment_points(o);
        let reach = os.half_width() + if os.is_street() && !os.fronts_nothing() { nd_sim::zoning::SIDEWALK } else { 0.6 } + 0.5;
        nd_sim::roads::project_on_segment(p, oa, ob).1.distance(p) < reach
    })
}

/// A tunnel mouth: a concrete headwall square across the way at `p`, facing
/// `out` (towards the open ramp), with an elliptical arch over the opening,
/// the arch's soffit run back through the wall's thickness, a coping along
/// the top and a lit band under the crown. The opening clears the
/// carriageway and its shoulders (`hw` either side); the bore's walls stand
/// behind the dark mouth.
/// Two posts and a lintel box stood here before and read as a blob.
#[allow(clippy::too_many_arguments)]
fn portal_arch(g: &mut Geometry, p: Vec2, out: Vec2, z: f32, hw: f32, hill: f32, wall: [f32; 4], coping: [f32; 4], lamp: [f32; 4]) {
    use glam::Vec3;
    if out == Vec2::ZERO {
        return;
    }
    let side = Vec2::new(-out.y, out.x);
    // Opening: half width, spring line and rise; the wall around it.
    let r = hw + 0.2;
    // Low enough that a tunnel drawn at the Tunnel preset's 8 m keeps its
    // headwall and lid at about the hill's surface.
    let spring = 4.4;
    let rise = (r * 0.3).clamp(1.0, 1.6);
    let wall_half = r + 1.6;
    let wall_top = spring + rise + 0.9;
    let foot = -0.4;
    let thick = 0.9;
    // A point of the headwall at `x` across (right of `out` positive) and
    // `y` up, on the face `t` along `out` (front +, back -).
    let at = |x: f32, y: f32, t: f32| {
        let q = p + side * x + out * t;
        Vec3::new(q.x, z + y, q.y)
    };
    // The opening's outline from the right foot round the arch to the left
    // foot, and for each point the matching point of the wall's outline
    // (straight out sideways below the spring line, up through the arch's
    // centre to the edge or the top above it).
    let n = 14;
    let mut opening: Vec<(f32, f32)> = vec![(r, foot), (r, spring)];
    for k in 1..n {
        let a = std::f32::consts::PI * k as f32 / n as f32;
        opening.push((r * a.cos(), spring + rise * a.sin()));
    }
    opening.push((-r, spring));
    opening.push((-r, foot));
    let outline: Vec<(f32, f32)> = opening
        .iter()
        .map(|&(x, y)| {
            if y <= spring + 1e-3 {
                return (x.signum() * wall_half, y);
            }
            // Along the ray from the arch's centre on the spring line.
            let (dx, dy) = (x, y - spring);
            let to_side = if dx.abs() > 1e-4 { wall_half / dx.abs() } else { f32::MAX };
            let to_top = (wall_top - spring) / dy;
            let k = to_side.min(to_top);
            (dx * k, spring + dy * k)
        })
        .collect();
    let o3 = Vec3::new(out.x, 0.0, out.y);
    let m = g.part(Surface::Concrete);
    for w in 0..opening.len() - 1 {
        let (a, b) = (opening[w], opening[w + 1]);
        let (c, d) = (outline[w], outline[w + 1]);
        for (t, face) in [(thick * 0.5, o3), (-thick * 0.5, -o3)] {
            m.quad_facing([at(a.0, a.1, t), at(b.0, b.1, t), at(d.0, d.1, t), at(c.0, c.1, t)], face, wall);
        }
        // The soffit (and the jambs), facing into the opening.
        let mid = Vec2::new((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
        let inward = Vec2::new(-mid.x, if mid.y > spring { spring - mid.y } else { 0.0 }).normalize_or_zero();
        let inward3 = Vec3::new(side.x * inward.x, inward.y, side.y * inward.x);
        m.quad_facing([at(a.0, a.1, thick * 0.5), at(b.0, b.1, thick * 0.5), at(b.0, b.1, -thick * 0.5), at(a.0, a.1, -thick * 0.5)], inward3, wall);
    }
    // The wall's ends and top.
    for sgn in [-1.0f32, 1.0] {
        let x = sgn * wall_half;
        let s3 = Vec3::new(side.x * sgn, 0.0, side.y * sgn);
        m.quad_facing([at(x, foot, thick * 0.5), at(x, wall_top, thick * 0.5), at(x, wall_top, -thick * 0.5), at(x, foot, -thick * 0.5)], s3, wall);
    }
    // A coping a little proud of the wall, along the top.
    let c = p;
    let yaw = out.y.atan2(out.x);
    m.boxed(Vec3::new(c.x, z + wall_top + 0.2, c.y), Vec3::new(thick + 0.4, 0.4, wall_half * 2.0 + 0.4), yaw, coping);
    // A lit band under the crown, just inside the mouth.
    let lit = p - out * (thick * 0.5 + 0.3);
    g.part(Surface::Glow).boxed(Vec3::new(lit.x, z + spring + rise - 0.35, lit.y), Vec3::new(0.08, 0.2, r * 1.2), yaw, lamp);
    // The dark of the bore, across the opening just behind the wall: the
    // built ground is a height field and cannot overhang, so through the
    // arch one saw the hillside the tunnel runs into. The ground is held
    // at the road's level for a few metres in (`Ground::build`), out of
    // sight behind this, and a lid over that stretch, at the wall's top,
    // closes it from above.
    let back = -thick * 0.5 - 0.05;
    let dark = crate::buildings::parts::rgba(crate::buildings::parts::DARK);
    g.part(Surface::Matte).quad_facing([at(-r - 0.1, foot, back), at(r + 0.1, foot, back), at(r + 0.1, spring + rise + 0.1, back), at(-r - 0.1, spring + rise + 0.1, back)], o3, dark);
    // The lid lies at the hill's surface (`hill`, the natural ground over
    // the bore), never lower than the arch nor higher than the wall, and
    // reaches a cell past the held ground, over the slope up to the hill.
    let reach = PORTAL_HOLD + nd_sim::landscape::GROUND_CELL;
    let lid = p - out * (thick * 0.5 + reach * 0.5);
    let top = crate::mesh::clamp_safe(hill - z, spring + rise + 0.4, wall_top);
    g.part(Surface::Concrete).boxed(Vec3::new(lid.x, z + top - 0.3, lid.y), Vec3::new(reach, 0.6, wall_half * 2.0 + 2.0), yaw, coping);
}

/// How far into the hill behind a tunnel portal the built ground is held at
/// the road's level (`nd_sim::landscape::PORTAL_HOLD`), lidded by the portal.
const PORTAL_HOLD: f32 = nd_sim::landscape::PORTAL_HOLD;

/// [interchange] A bridge's abutment on the bank at `p`, the deck running
/// out over the water along `out` at height `z` (`hw` its half width): a
/// bank seat under the deck's end and wing walls running back along the
/// approach, down into the ground at `ground`.
pub fn abutment(g: &mut Geometry, p: Vec2, out: Vec2, z: f32, hw: f32, ground: f32, concrete: [f32; 3]) {
    use glam::Vec3;
    let top = z - 1.4;
    let foot = ground - 1.0;
    if top - foot < 0.6 {
        return;
    }
    let yaw = out.y.atan2(out.x);
    // The seat: from just behind the deck's end to under it.
    let seat = p + out * 0.6;
    g.part(Surface::Concrete).boxed(Vec3::new(seat.x, (top + foot) * 0.5, seat.y), Vec3::new(2.4, top - foot, hw * 2.0 + 0.8), yaw, rgba(mul(concrete, 0.82)));
    // The wing walls, stepping down as they run back into the embankment.
    let side = Vec2::new(-out.y, out.x);
    for sgn in [-1.0f32, 1.0] {
        for k in 0..3 {
            let back = 1.5 + k as f32 * 2.5;
            let c = p - out * back + side * sgn * (hw + 0.25);
            let h = (z - 0.1 - foot) * (1.0 - k as f32 * 0.3);
            if h < 0.3 {
                continue;
            }
            g.part(Surface::Concrete).boxed(Vec3::new(c.x, foot + h * 0.5, c.y), Vec3::new(2.5, h, 0.5), yaw, rgba(mul(concrete, 0.86)));
        }
    }
}

/// One span of viaduct deck from `a` at height `za` to `b` at `zb`,
/// `hw` either side of the centreline, standing on `ground`, with its
/// columns unless `columns` is false (the columns were placed for it:
/// `World::pillars`). Shared with the plan preview.
#[allow(clippy::too_many_arguments)]
pub fn viaduct_span(g: &mut Geometry, a: Vec2, b: Vec2, za: f32, zb: f32, hw: f32, ground: &dyn Fn(Vec2) -> f32, concrete: [f32; 3], steel: [f32; 3], columns: bool) {
    use glam::Vec3;
    let len = a.distance(b);
    if len < 0.3 {
        return;
    }
    let dir = (b - a) / len;
    let a3 = Vec3::new(a.x, za - 0.02, a.y);
    let b3 = Vec3::new(b.x, zb - 0.02, b.y);
    deck(g, a3, b3, hw, Vec2::new(-dir.y, dir.x) * (hw - 0.2), &VIADUCT_DECK, concrete, steel);
    // Columns where the deck stands clear of the ground, one at each end
    // too unless the end is near the ground (a ramp foot): only for a deck
    // whose columns were not placed for it (`World::pillars`).
    if !columns {
        return;
    }
    let spans = (len / 24.0).ceil().max(1.0) as i32;
    let yaw = dir.y.atan2(dir.x);
    for k in 0..=spans {
        let t = k as f32 / spans as f32;
        let p = a.lerp(b, t);
        let top = za + (zb - za) * t - 1.2;
        let foot = ground(p);
        if top - foot < 1.0 {
            continue;
        }
        let w = (hw * 0.35).clamp(1.2, 3.0);
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, (top + foot) * 0.5, p.y), Vec3::new(1.6, top - foot, w), yaw, rgba(mul(concrete, 0.8)));
        // A crosshead under the deck.
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, top - 0.35, p.y), Vec3::new(1.9, 0.7, hw * 1.7), yaw, rgba(mul(concrete, 0.85)));
    }
}

/// Everything built in or over the water, and the hot ground: bridge decks
/// with railings and piers, dock piers with piles, bollards and the crane
/// boom, weirs where hydro plants take their water, tidal turbine
/// caissons, and steaming vents at the hot spots.
pub fn waterworks(world: &World, palette: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let land = &world.landscape;
    let concrete = mul(palette.concrete(0), 0.85);
    let steel = mul(palette.steel, 0.8);
    if land.is_flat() {
        // The flat map has no water: only what the player built up.
        if world.relief {
            viaducts(world, &mut g, concrete, steel);
            underpasses(world, &mut g, concrete, steel);
            tunnel_works(world, &mut g, concrete, steel);
        }
        return g;
    }
    viaducts(world, &mut g, concrete, steel);
    tunnel_works(world, &mut g, concrete, steel);
    bench_verges(world, &mut g, concrete);
    bridges(world, &mut g, concrete, steel);
    underpasses(world, &mut g, concrete, steel);
    port_docks(world, palette, &mut g, concrete);
    water_plants(world, palette, &mut g, concrete, steel);
    hot_vents(world, palette, &mut g);
    g
}

/// Bridge decks over the water with their rails, abutments on the banks,
/// the deck under a ramp's gore and piers down to the bed.
fn bridges(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    let land = &world.landscape;
    let roads = &world.roads;
    for (sid, seg) in roads.alive_segments() {
        if !seg.bridge || seg.viaduct {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let za = world.road_z.get(seg.a.0 as usize).copied().unwrap_or(0.0);
        let zb = world.road_z.get(seg.b.0 as usize).copied().unwrap_or(0.0);
        // The deck carries the pavements (a highway's or ramp's, its
        // shoulders and barriers), with the parapet at its outer edge, as a
        // viaduct's: it was half a pavement narrower, so the parapet's rail
        // ran down the middle of the pavement.
        let motorway = seg.is_motorway() || seg.ring;
        let hw = seg.half_width() + if seg.is_street() && !motorway { nd_sim::zoning::SIDEWALK } else { 0.6 };
        let a3 = Vec3::new(a.x, za - 0.05, a.y);
        let b3 = Vec3::new(b.x, zb - 0.05, b.y);
        let dir = (b - a).normalize_or_zero();
        deck(g, a3, b3, hw, Vec2::new(-dir.y, dir.x) * (hw - 0.2), &BRIDGE_DECK, concrete, steel);
        // [interchange] An abutment at each end on the bank (no other bridge
        // piece goes on from it): a bank seat under the deck's end and wing
        // walls back along the approach into the embankment. The deck used
        // to simply stop, a concrete block standing on the bank.
        for (n, p, out, z) in [(seg.a, a, dir, za), (seg.b, b, -dir, zb)] {
            let goes_on = roads.node(n).segments.iter().any(|&o| o != sid && roads.segment(o).alive && roads.segment(o).bridge && !roads.segment(o).viaduct);
            if !goes_on {
                abutment(g, p, out, z, hw, land.height(p).max(WATER_LEVEL - 1.0), concrete);
            }
        }
        // [interchange] A ramp leaving (or joining) a highway on the bridge:
        // deck under the gore between the two, which used to be a thin
        // asphalt sheet over the water.
        let seg_len = a.distance(b);
        if seg.ramp {
            for (n, p, out) in [(seg.a, a, dir), (seg.b, b, -dir)] {
                let hw_seg = roads.node(n).segments.iter().copied().find(|&o| o != sid && roads.segment(o).alive && roads.segment(o).is_highway() && roads.segment(o).bridge);
                let Some(h) = hw_seg else { continue };
                let (h0, h1) = roads.segment_points(h);
                let hd = if h0.distance(p) < h1.distance(p) { (h1 - h0).normalize_or_zero() } else { (h0 - h1).normalize_or_zero() };
                let run = seg_len.min(40.0);
                let z0 = world.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
                let z1 = if n == seg.a { za + (zb - za) * (run / seg_len.max(1e-3)) } else { zb + (za - zb) * (run / seg_len.max(1e-3)) };
                let mut s = 0.0f32;
                while s < run {
                    let r = p + out * (s + 2.0);
                    let q = p + hd * (r - p).dot(hd);
                    let gap = r.distance(q);
                    if gap > 0.5 {
                        let mid = (r + q) * 0.5;
                        let zz = z0 + (z1 - z0) * (s + 2.0) / run.max(1e-3) - 0.05;
                        let yaw = hd.y.atan2(hd.x);
                        g.part(Surface::Concrete).boxed(Vec3::new(mid.x, zz - 0.7, mid.y), Vec3::new(4.2, 1.4, gap), yaw, rgba(concrete));
                    }
                    s += 4.0;
                }
            }
        }
        // Piers down to the bed every 28 m or so.
        let len = a.distance(b);
        let spans = (len / 28.0).ceil().max(1.0) as i32;
        let yaw = dir.y.atan2(dir.x);
        for k in 1..spans {
            let t = k as f32 / spans as f32;
            let p = a.lerp(b, t);
            let top = za + (zb - za) * t - 1.4;
            let bed = land.height(p).min(WATER_LEVEL - 1.0);
            g.part(Surface::Concrete).boxed(Vec3::new(p.x, (top + bed) * 0.5, p.y), Vec3::new(2.2, top - bed, hw * 1.6), yaw, rgba(mul(concrete, 0.8)));
        }
    }
}

/// Dock piers on their piles: the shipping port's crane boom, the ferry
/// pontoon, and the marinas' and fishing ports' pontoons, quays and boats.
fn port_docks(world: &World, palette: &Palette, g: &mut Geometry, concrete: [f32; 3]) {
    use glam::Vec3;
    let land = &world.landscape;
    // Dock piers.
    for port in world.ports.ports.values() {
        let (_, width) = port.kind.pier();
        let [root, tip] = port.pier;
        // [economy] A marina's pontoons float just off the water.
        let floating = port.kind.is_marina();
        let deck = if floating { WATER_LEVEL + PONTOON_DECK } else { BANK_HEIGHT + 0.6 };
        let r3 = Vec3::new(root.x, deck, root.y);
        let t3 = Vec3::new(tip.x, deck, tip.y);
        if floating {
            g.part(Surface::Wood).slab(r3, t3, width, 0.6, rgba(mul(palette.wood, 0.85)));
        } else {
            g.part(Surface::Concrete).slab(r3, t3, width, 0.8, rgba(mul(concrete, 0.95)));
        }
        let along = (tip - root).normalize_or_zero();
        let across = Vec2::new(-along.y, along.x);
        let len = root.distance(tip);
        let piles = (len / 6.0) as i32;
        for k in 0..=piles {
            let p = root + along * (k as f32 * 6.0);
            for s in [-0.45f32, 0.45] {
                let q = p + across * width * s;
                g.part(Surface::Wood).cylinder(q, 0.35, 0.35, WATER_LEVEL - 3.0, deck - 0.6, 6, rgba(mul(palette.wood, 0.6)));
            }
            if k % 2 == 0 {
                for s in [-0.5f32, 0.5] {
                    let q = p + across * (width * s - s.signum() * 0.5);
                    g.part(Surface::Metal).cylinder(q, 0.25, 0.3, deck, deck + 0.6, 6, rgba(mul(palette.steel, 0.4)));
                }
            }
        }
        match port.kind {
            PortKind::ShippingPort => {
                // The crane's boom reaching out over the berth.
                let base = port.center + port.seaward * (port.kind.size().1 * 0.5 - 9.0) + port.along * (port.kind.size().0 * 0.12);
                let out = base + port.seaward * (len + 30.0);
                g.part(Surface::Metal).beam(Vec3::new(base.x, 31.0, base.y), Vec3::new(out.x, 31.0, out.y), 1.4, rgba(palette.metal_paint(0)));
                g.part(Surface::Glow).boxed(Vec3::new(out.x, 31.8, out.y), Vec3::splat(0.6), 0.0, rgba(palette.aviation_light));
            }
            PortKind::FerryTerminal | PortKind::FerryTerminalLarge => {
                // A floating pontoon at the tip with a lamp.
                let p = tip + along * 4.0;
                g.part(Surface::Metal).boxed(Vec3::new(p.x, WATER_LEVEL + 0.5, p.y), Vec3::new(8.0, 1.0, 5.0), along.y.atan2(along.x), rgba(mul(palette.metal_paint(2), 0.8)));
                g.part(Surface::Metal).cylinder(p, 0.08, 0.08, WATER_LEVEL + 1.0, WATER_LEVEL + 5.0, 6, rgba([0.2, 0.2, 0.22]));
                g.part(Surface::Glow).boxed(Vec3::new(p.x, WATER_LEVEL + 5.1, p.y), Vec3::new(0.4, 0.25, 0.4), 0.0, rgba(palette.lamp));
            }
            // [economy] Pontoons and quays with their boats moored.
            PortKind::MarinaSmall | PortKind::MarinaLarge | PortKind::FishingPortSmall | PortKind::FishingPortLarge => {
                let layout = dock_layout(port, land);
                let top = if floating { WATER_LEVEL + PONTOON_DECK } else { deck };
                for (a, b, w) in &layout.decks {
                    let (a3, b3) = (Vec3::new(a.x, top, a.y), Vec3::new(b.x, top, b.y));
                    if floating {
                        g.part(Surface::Wood).slab(a3, b3, *w, 0.6, rgba(mul(palette.wood, 0.9)));
                    } else {
                        g.part(Surface::Concrete).slab(a3, b3, *w, 0.8, rgba(mul(concrete, 0.95)));
                        // Piles under the quay.
                        let n = (a.distance(*b) / 6.0) as i32;
                        for k in 0..=n {
                            let q = a.lerp(*b, k as f32 / n.max(1) as f32);
                            g.part(Surface::Wood).cylinder(q, 0.35, 0.35, WATER_LEVEL - 3.0, deck - 0.6, 6, rgba(mul(palette.wood, 0.6)));
                        }
                    }
                }
                // A lamp at the pier head.
                let lamp = tip - along * 1.0 + across * (width * 0.5 - 0.4);
                g.part(Surface::Matte).cylinder(lamp, 0.07, 0.07, top, top + 4.0, 6, rgba([0.2, 0.2, 0.22]));
                g.part(Surface::Glow).boxed(Vec3::new(lamp.x, top + 4.1, lamp.y), Vec3::new(0.35, 0.22, 0.35), 0.0, rgba(palette.lamp));
                // [pt-visual] The boats are `moored_boats`', drawn apart.
            }
        }
    }
}

/// Plants that work the water: a hydro plant's arch dam, a tidal plant's
/// turbine caissons and causeway.
fn water_plants(world: &World, palette: &Palette, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    let land = &world.landscape;
    // Plants that work the water.
    for (id, b) in world.buildings.iter() {
        let Some(kind) = b.plant else { continue };
        let Some(lot) = world.lots.get(b.lot) else { continue };
        match kind {
            PowerPlantKind::Hydro => {
                // [dams] (H1) The wall the player drew, bank to bank.
                if let Some(wall) = world.dams.wall_of(id) {
                    let span = land.rivers.get(wall.river).map_or(wall.length() * 0.6, |r| r.width);
                    crate::dam::hydro_dam_wall(g, wall, span, lot.center, world.plant_scale(b), &|p| world.ground_height(p), concrete, steel);
                } else if let Some((c, flow, width)) = world.auto_dam(id) {
                    // [plant-groups] A plant from before the wall tool: a
                    // curved arch dam across the river beside it. (None
                    // while a new plant waits for its wall.)
                    crate::dam::hydro_dam(g, c, flow, width, lot.center, world.plant_scale(b), &|p| world.ground_height(p), concrete, steel);
                }
            }
            PowerPlantKind::Tidal => {
                if let Some(shore) = lot.polygon.iter().filter_map(|p| land.nearest_shore(*p, 50.0)).find(|s| s.water.kind == WaterKind::Sea) {
                    // [freight] A turbine caisson per turbine bay and two
                    // more out at the head (five at the nameplate): a bigger
                    // plant runs further out to sea, the caissons the same size.
                    let caissons = PowerPlantKind::turbine_bays(world.plant_scale(b)) + 2;
                    for k in 0..caissons {
                        let p = shore.point + shore.seaward * (12.0 + k as f32 * 11.0);
                        g.part(Surface::Concrete).boxed(Vec3::new(p.x, WATER_LEVEL, p.y), Vec3::new(7.0, 4.0, 7.0), 0.0, rgba(concrete));
                        g.part(Surface::Metal).boxed(Vec3::new(p.x, WATER_LEVEL + 2.8, p.y), Vec3::new(2.0, 1.6, 2.0), 0.0, rgba(mul(palette.metal_paint(3), 0.9)));
                    }
                    let a = shore.point - shore.seaward * 2.0;
                    let b = shore.point + shore.seaward * (16.0 + (caissons - 1) as f32 * 11.0);
                    g.part(Surface::Concrete).slab(Vec3::new(a.x, BANK_HEIGHT + 0.8, a.y), Vec3::new(b.x, BANK_HEIGHT + 0.8, b.y), 3.0, 0.6, rgba(mul(concrete, 0.9)));
                }
            }
            _ => {}
        }
    }
}

/// Vents at the hot spots: crusted rocks round a pale sinter ring.
fn hot_vents(world: &World, palette: &Palette, g: &mut Geometry) {
    let land = &world.landscape;
    // Vents at the hot spots: crusted rocks round a pale sinter ring, on
    // open ground only (the city may have been laid over a hot spot). The
    // steam itself is animated by the renderer.
    for (k, s) in land.hot_spots.iter().enumerate() {
        let mut rng = rng_for(land.params.seed, 900 + k as u64);
        for _ in 0..7 {
            let p = s.pos + Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(0.0..s.radius * 0.8);
            let size = 0.8 + rng.random::<f32>();
            if !climate::vent_clear(world, p) {
                continue;
            }
            let y = world.ground_height(p);
            rock_world(g, &mut rng, p, y - 0.1, size, mul(palette.stone, 0.7));
            g.part(Surface::Stone).ground_polygon(&ngon(p.x, p.y, size * 2.2, 9, 0.0), y + 0.06, rgba([0.62, 0.58, 0.46]));
        }
    }
}

#[cfg(test)]
mod sweep_visual_tests {
    use super::*;

    /// [sweep-visual] A cutting wall along one arm of a junction is not
    /// stood on the next arm: `on_other_way` sees that arm's lanes and
    /// pavements, and nothing past them.
    #[test]
    fn cutting_walls_keep_off_the_other_arms() {
        let mut roads = nd_sim::roads::RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        let along_x = roads.alive_segments().find(|(sid, _)| {
            let (a, b) = roads.segment_points(*sid);
            (a.y - b.y).abs() < 0.1 && a.x.max(b.x) > 50.0
        });
        let (sid, s) = along_x.expect("an east piece");
        let off = s.half_width() + nd_sim::zoning::SIDEWALK + 3.4;
        // Its wall line close to the junction crosses the north-south street.
        assert!(on_other_way(&roads, sid, Vec2::new(1.0, off)));
        // Further along it is clear.
        assert!(!on_other_way(&roads, sid, Vec2::new(40.0, off)));
    }
}

/// Test support for the orientation checks: the vertical faces of `m`
/// standing `height` tall (one kind of box in a mix), as (horizontal
/// normal, horizontal width). A box turned the wrong way round has its
/// long faces where its short ones should be.
#[cfg(test)]
pub(crate) fn upright_faces(m: &crate::mesh::MeshData, height: f32) -> Vec<(Vec2, f32)> {
    let mut out = Vec::new();
    for t in m.indices.chunks_exact(3) {
        let v = [t[0], t[1], t[2]].map(|i| glam::Vec3::from(m.positions[i as usize]));
        let n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
        if n.y.abs() > 0.05 || n == glam::Vec3::ZERO {
            continue;
        }
        let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        if ((hi - lo) - height).abs() > 0.01 {
            continue;
        }
        let along = Vec2::new(-n.z, n.x);
        let (a, b) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), p| {
            let s = Vec2::new(p.x, p.z).dot(along);
            (a.min(s), b.max(s))
        });
        out.push((Vec2::new(n.x, n.z), b - a));
    }
    out
}

/// Every face of `faces` whose normal runs along `dir` is `want` wide
/// (within 5 cm): the box lies the right way round on the way.
#[cfg(test)]
pub(crate) fn faces_along_are(faces: &[(Vec2, f32)], dir: Vec2, want: f32) -> Result<(), String> {
    let hits: Vec<f32> = faces.iter().filter(|(n, _)| n.dot(dir).abs() > 0.95).map(|(_, w)| *w).collect();
    if hits.is_empty() {
        return Err("no faces along the way".into());
    }
    match hits.iter().find(|w| (**w - want).abs() > 0.05) {
        Some(w) => Err(format!("a face square to the way is {w:.2} m wide, not {want:.2}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod orientation_tests {
    use super::*;

    /// Review #4: every box on a way at 45 degrees lies along or across it
    /// as drawn on an axis-aligned one. `boxed` turns local +x to (cos yaw,
    /// sin yaw); the terrain works passed -yaw and turned each by twice
    /// the way's angle, so a crosshead ran along the deck.
    #[test]
    fn works_on_a_45_degree_way_face_the_way() {
        let c = [0.6, 0.6, 0.6];
        for deg in [45.0f32, 30.0, 0.0, 135.0] {
            let dir = Vec2::new(deg.to_radians().cos(), deg.to_radians().sin());
            let hw = 6.0;
            // A pillar: its crosshead lies across the deck.
            let mut g = Geometry::default();
            pillar_column(&mut g, Vec2::new(40.0, -30.0), dir, 10.0, 0.0, hw, c);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 0.7), dir, hw * 1.7).map_err(|e| format!("pillar at {deg}: {e}")).unwrap();
            // A viaduct span's own columns.
            let mut g = Geometry::default();
            let a = Vec2::new(-20.0, 15.0);
            viaduct_span(&mut g, a, a + dir * 60.0, 12.0, 12.0, hw, &|_| 0.0, c, c, true);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 0.7), dir, hw * 1.7).map_err(|e| format!("viaduct at {deg}: {e}")).unwrap();
            // A bridge abutment's seat spans the deck's width.
            let mut g = Geometry::default();
            abutment(&mut g, Vec2::new(5.0, 5.0), dir, 8.0, hw, 0.0, c);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 7.6), dir, hw * 2.0 + 0.8).map_err(|e| format!("abutment at {deg}: {e}")).unwrap();
            // A tunnel portal's coping runs along the headwall's top.
            let mut g = Geometry::default();
            let col = [0.5, 0.5, 0.5, 1.0];
            portal_arch(&mut g, Vec2::new(-7.0, 3.0), dir, 0.0, 5.0, 12.0, col, col, col);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 0.4), dir, (5.0 + 0.2 + 1.6) * 2.0 + 0.4).map_err(|e| format!("portal at {deg}: {e}")).unwrap();
        }
    }

    /// Review §1.5: clearing ground (a road through it) drops the scatter
    /// there and leaves every other piece where it was; at the cap at most
    /// as many others come in as were cleared.
    #[test]
    fn clearing_a_cell_changes_only_that_cell() {
        // Wide enough for the cap to bite.
        let cfg = TerrainConfig { half_size: 3200.0, ..TerrainConfig::default() };
        let growth = 1.0;
        let all = scatter_cells(&cfg, 9, growth, &|_| true);
        assert!(all.len() >= (3500.0 * growth) as usize, "the cap is in play: {}", all.len());
        let hole = |p: Vec2| p.distance(Vec2::new(300.0, -200.0)) > 120.0;
        let some = scatter_cells(&cfg, 9, growth, &hole);
        let spot = |(i, j): (i32, i32)| scatter_spot(&cfg, &mut scatter_rng(9, i, j), i, j);
        let cleared = all.iter().filter(|c| !hole(spot(**c))).count();
        assert!(cleared > 0);
        for c in &all {
            assert!(!hole(spot(*c)) || some.contains(c), "cell {c:?} outside the cleared ground went");
        }
        let new = some.iter().filter(|c| !all.contains(c)).count();
        assert!(new <= cleared, "{new} cells came in for {cleared} cleared");
    }
}
