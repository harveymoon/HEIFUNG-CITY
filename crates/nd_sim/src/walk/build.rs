//! Building the walk graph: pavements round every block, crossings at
//! junctions and midblock, lot gates, and the player's crossing upgrades.

use super::*;

/// Can people walk through this lot to the next one?
pub fn walkable_through(lot: &Lot, land_value: &LandValue) -> bool {
    let tier = Tier::for_land_value(land_value.at(lot.center));
    match lot.land_use {
        LandUse::Industrial => false,
        LandUse::Residential => tier != Tier::High,
        _ => true,
    }
}

/// [waterfront] May two neighbouring lots share a gate? A public beach is
/// reached from a street or path it fronts, or through a park next to
/// it, never across a private lot.
pub fn beach_gate_ok(a: &Lot, b: &Lot) -> bool {
    !(a.shore.beach || b.shore.beach) || (a.land_use == LandUse::Park && b.land_use == LandUse::Park)
}

pub(super) fn pos_key(p: Vec2, q: Vec2) -> [u32; 4] {
    [p.x.to_bits(), p.y.to_bits(), q.x.to_bits(), q.y.to_bits()]
}

/// A stop along one side of a street: a lot front or a mid-block zebra.
pub(super) enum Stop {
    Lot(LotId),
    Zebra(usize),
}

pub(super) type End = (u32, SegmentId, i8);

impl WalkGraph {
    pub(super) fn add_node(&mut self, pos: Vec2, lot: Option<LotId>) -> u32 {
        self.nodes.push(WalkNode { pos, lot, near: Vec::new(), level: 0.0 });
        self.adj.push(Vec::new());
        (self.nodes.len() - 1) as u32
    }

    pub(super) fn link(&mut self, a: u32, b: u32, interior: bool) {
        self.link_kind(a, b, interior, false);
    }

    pub(super) fn link_kind(&mut self, a: u32, b: u32, interior: bool, park: bool) {
        if a == b {
            return;
        }
        let len = self.nodes[a as usize].pos.distance(self.nodes[b as usize].pos).max(0.05);
        self.adj[a as usize].push(WalkEdge { to: b, len, interior, park, cross: NO_CROSSING, raised: false, gate: false });
        self.adj[b as usize].push(WalkEdge { to: a, len, interior, park, cross: NO_CROSSING, raised: false, gate: false });
    }

    /// Join two pavement nodes across a street.
    pub(super) fn link_cross(&mut self, a: u32, b: u32, segment: SegmentId, node: Option<NodeId>, kind: CrossingKind, roads: &RoadNetwork) {
        if a == b {
            return;
        }
        let (pa, pb) = (self.nodes[a as usize].pos, self.nodes[b as usize].pos);
        // The middle on the street's centre line.
        let (s0, s1) = roads.segment_points(segment);
        let mid = geom::project_on_segment((pa + pb) * 0.5, s0, s1).1;
        let k = self.crossings.len() as u32;
        self.crossings.push(Crossing { a, b, segment, node, kind, mid, button: false, bridged: false, closed: false });
        let len = pa.distance(pb).max(0.05);
        self.adj[a as usize].push(WalkEdge { to: b, len, interior: false, park: false, cross: k, raised: false, gate: false });
        self.adj[b as usize].push(WalkEdge { to: a, len, interior: false, park: false, cross: k, raised: false, gate: false });
    }

    /// A junction of three or more streets: a crosswalk across every arm
    /// (none over a highway), and the pavements of neighbouring arms meet
    /// round the corner between them, with a corner node where the arms
    /// are far apart so nobody cuts through the middle.
    pub(super) fn junction(&mut self, roads: &RoadNetwork, nid: NodeId, centre: Vec2, e: &[End]) {
        struct Arm {
            seg: SegmentId,
            ang: f32,
            ccw: u32,
            cw: u32,
            street: bool,
        }
        let mut segs: Vec<SegmentId> = Vec::new();
        for x in e {
            if !segs.contains(&x.1) {
                segs.push(x.1);
            }
        }
        let mut arms: Vec<Arm> = Vec::new();
        for s in segs {
            let sg = roads.segment(s);
            let (a, b) = roads.segment_points(s);
            let away = (if sg.a == nid { b - a } else { a - b }).normalize_or_zero();
            let ends: Vec<u32> = e.iter().filter(|x| x.1 == s).map(|x| x.0).collect();
            let side = |n: &u32| away.perp_dot(self.nodes[*n as usize].pos - centre);
            let ccw = *ends.iter().max_by(|x, y| side(x).total_cmp(&side(y))).unwrap();
            let cw = *ends.iter().min_by(|x, y| side(x).total_cmp(&side(y))).unwrap();
            arms.push(Arm { seg: s, ang: away.y.atan2(away.x), ccw, cw, street: sg.is_street() });
        }
        arms.sort_by(|x, y| x.ang.total_cmp(&y.ang).then(x.seg.cmp(&y.seg)));
        let ring = roads.ring_of_node(nid).is_some();
        for arm in &arms {
            if arm.street && arm.ccw != arm.cw && !roads.segment(arm.seg).fronts_nothing() {
                let kind = if ring && roads.ring_of_segment(arm.seg).is_some() { CrossingKind::Informal } else { CrossingKind::Junction };
                self.link_cross(arm.cw, arm.ccw, arm.seg, Some(nid), kind, roads);
            }
        }
        // [ped-highway] Slip roads joining here have no pavements and no
        // painted crosswalk: a corner whose way round passes the mouth of
        // one crosses it there, unmarked (the walker finds a gap), rather
        // than as a plain pavement edge laid over its carriageway.
        let slips: Vec<(f32, SegmentId)> = roads
            .node(nid)
            .segments
            .iter()
            .copied()
            .filter(|&s| {
                let sg = roads.segment(s);
                sg.alive && sg.is_street() && sg.fronts_nothing() && !sg.tunnel
            })
            .map(|s| {
                let sg = roads.segment(s);
                let (a, b) = roads.segment_points(s);
                let away = (if sg.a == nid { b - a } else { a - b }).normalize_or_zero();
                (away.y.atan2(away.x), s)
            })
            .collect();
        let reach = widest(roads, nid) + SIDEWALK * 0.55 + 0.4;
        let n = arms.len();
        for i in 0..n {
            let j = (i + 1) % n;
            let gap = (arms[j].ang - arms[i].ang).rem_euclid(std::f32::consts::TAU);
            let gap = if gap <= 1e-4 { std::f32::consts::TAU } else { gap };
            let slip = slips.iter().filter(|(ang, _)| {
                let off = (ang - arms[i].ang).rem_euclid(std::f32::consts::TAU);
                off > 1e-4 && off < gap - 1e-4
            });
            if let Some(&(_, s)) = slip.min_by(|x, y| (x.0 - arms[i].ang).rem_euclid(std::f32::consts::TAU).total_cmp(&(y.0 - arms[i].ang).rem_euclid(std::f32::consts::TAU)).then(x.1.cmp(&y.1))) {
                self.link_cross(arms[i].ccw, arms[j].cw, s, Some(nid), CrossingKind::Informal, roads);
                continue;
            }
            if gap > 2.6 {
                // A wide corner: go round it, not across the junction.
                let bis = Vec2::from_angle(arms[i].ang + gap * 0.5);
                let c = self.add_node(centre + bis * reach, None);
                self.link(arms[i].ccw, c, false);
                self.link(c, arms[j].cw, false);
            } else {
                self.link(arms[i].ccw, arms[j].cw, false);
            }
        }
    }

    /// Pair each end of the first segment in `among` with the nearest end
    /// of the other; false when there is no other.
    pub(super) fn pair_ends(&mut self, among: &[End]) -> bool {
        let Some(first) = among.first() else { return false };
        let first_seg = first.1;
        let (mine, theirs): (Vec<End>, Vec<End>) = among.iter().partition(|x| x.1 == first_seg);
        for (na, _, _) in mine {
            let p = self.nodes[na as usize].pos;
            if let Some((nb, _, _)) = theirs.iter().copied().min_by(|x, y| self.nodes[x.0 as usize].pos.distance(p).total_cmp(&self.nodes[y.0 as usize].pos.distance(p))) {
                self.link(na, nb, false);
            }
        }
        !theirs.is_empty()
    }

    pub(super) fn hub(&mut self, at: Vec2, e: &[End]) {
        let hub = self.add_node(at, None);
        for (n, _, _) in e {
            self.link(*n, hub, false);
        }
    }

    /// [ped-highway] `hub`, except at a road node on a highway or slip road
    /// carriageway (a footpath dropped onto a ramp): no node out on the
    /// carriageway; the ends there are joined to each other directly, and
    /// a lone end is left a dead end short of it.
    pub(super) fn hub_off_carriageway(&mut self, roads: &RoadNetwork, nid: NodeId, at: Vec2, e: &[End]) {
        let carriageway = roads.node(nid).segments.iter().any(|&s| {
            let sg = roads.segment(s);
            sg.alive && sg.is_motorway() && !sg.tunnel
        });
        if !carriageway {
            self.hub(at, e);
            return;
        }
        if let Some((first, rest)) = e.split_first() {
            for x in rest {
                self.link(first.0, x.0, false);
            }
        }
    }

    pub fn build(roads: &RoadNetwork, lots: &SlotMap<LotId, Lot>, land_value: &LandValue, version: u64) -> Self {
        let mut g = WalkGraph { version, ..Default::default() };

        // ── streets and paths ────────────────────────────────────────────
        // Endpoints per road node: (node index, segment, side).
        let mut ends: Vec<Vec<End>> = vec![Vec::new(); roads.nodes.len()];
        let mut street_nodes: Vec<u32> = Vec::new();
        // Pavement node opposite every lot front, keyed by lot.
        let mut front_nodes: HashMap<LotId, u32> = HashMap::new();
        // Usable lots by the segment they front, in lot order, so each
        // segment looks only at its own lots.
        let mut fronting: HashMap<SegmentId, Vec<(LotId, &Lot)>> = HashMap::new();
        for (id, l) in lots.iter() {
            if !l.landlocked {
                fronting.entry(l.front_segment).or_default().push((id, l));
            }
        }
        let mut zebras: HashMap<SegmentId, Vec<f32>> = HashMap::new();
        for (s, t) in midblock_crossings(roads) {
            zebras.entry(s).or_default().push(t);
        }
        // How far back from a node a street's pavements end: at the
        // crosswalk of a junction, at the node elsewhere.
        let setback = |n: NodeId, s: SegmentId| if is_junction(roads, n) { crosswalk_setback(roads, n, s) } else { 0.0 };
        // A footpath meeting streets ends on their pavement.
        let path_setback = |n: NodeId| if street_arms(roads, n) > 0 { widest(roads, n) + SIDEWALK * 0.55 } else { 0.0 };
        for (sid, seg) in roads.alive_segments() {
            // No one walks in a road tunnel, nor along a highway or its
            // slip roads: they have hard shoulders, not pavements.
            if seg.fronts_nothing() {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let len = a.distance(b).max(1e-3);
            let d = (b - a) / len;
            let right = Vec2::new(-d.y, d.x);
            let fronts = fronting.get(&sid).map(Vec::as_slice).unwrap_or(&[]);
            match seg.kind {
                RoadKind::Street => {
                    let off = seg.half_width() + SIDEWALK * 0.55;
                    let (mut sa, mut sb) = (setback(seg.a, sid), setback(seg.b, sid));
                    if sa + sb > len * 0.8 {
                        let k = len * 0.8 / (sa + sb);
                        sa *= k;
                        sb *= k;
                    }
                    let (ta, tb) = (sa / len + 0.01, 1.0 - sb / len - 0.01);
                    let mids: Vec<f32> = zebras.get(&sid).cloned().unwrap_or_default();
                    let mut zebra_nodes: Vec<[u32; 2]> = vec![[0; 2]; mids.len()];
                    let mut lot_nodes: [Vec<(f32, u32)>; 2] = [Vec::new(), Vec::new()];
                    for (si, side) in [-1i8, 1].into_iter().enumerate() {
                        let o = right * off * side as f32;
                        // Lots fronting this side, and the zebras, ordered along the segment.
                        let mut stops: Vec<(f32, Stop)> = fronts
                            .iter()
                            .filter(|(_, l)| ((l.entrance() - a).dot(right) >= 0.0) == (side > 0))
                            .map(|(id, l)| (l.front_t.clamp(0.02, 0.98).clamp(ta.min(0.5), tb.max(0.5)), Stop::Lot(*id)))
                            .collect();
                        stops.extend(mids.iter().enumerate().map(|(k, &t)| (t, Stop::Zebra(k))));
                        stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                        let na = g.add_node(a + d * sa + o, None);
                        let mut prev = na;
                        for (t, stop) in stops {
                            let n = g.add_node(a + d * (t * len) + o, None);
                            match stop {
                                Stop::Lot(lid) => {
                                    g.nodes[n as usize].near.push(lid);
                                    front_nodes.insert(lid, n);
                                    lot_nodes[si].push((t, n));
                                }
                                Stop::Zebra(k) => zebra_nodes[k][si] = n,
                            }
                            g.link(prev, n, false);
                            street_nodes.push(n);
                            prev = n;
                        }
                        let nb = g.add_node(b - d * sb + o, None);
                        g.link(prev, nb, false);
                        ends[seg.a.0 as usize].push((na, sid, side));
                        ends[seg.b.0 as usize].push((nb, sid, side));
                        street_nodes.push(na);
                        street_nodes.push(nb);
                    }
                    for z in zebra_nodes {
                        g.link_cross(z[0], z[1], sid, None, CrossingKind::Zebra, roads);
                    }
                    // Jaywalkers' short cuts: straight over to the lot
                    // front opposite.
                    if !seg.fronts_nothing() {
                        for &(t, n) in &lot_nodes[0] {
                            let best = lot_nodes[1].iter().min_by(|x, y| (x.0 - t).abs().total_cmp(&(y.0 - t).abs()));
                            if let Some(&(_, n2)) = best.filter(|x| (x.0 - t).abs() * len <= JAY_REACH) {
                                g.link_cross(n, n2, sid, None, CrossingKind::Jay, roads);
                            }
                        }
                    }
                }
                RoadKind::Rail => {}
                RoadKind::Path => {
                    // Homes may line a path: a node opposite each of them.
                    let (sa, sb) = (path_setback(seg.a).min(len * 0.4), path_setback(seg.b).min(len * 0.4));
                    let mut stops: Vec<(f32, LotId)> = fronts.iter().map(|(id, l)| (l.front_t.clamp(0.02, 0.98), *id)).collect();
                    stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                    let na = g.add_node(a + d * sa, None);
                    let mut prev = na;
                    for (t, lid) in stops {
                        let n = g.add_node(a + d * (t * len), None);
                        g.nodes[n as usize].near.push(lid);
                        g.link(prev, n, false);
                        front_nodes.insert(lid, n);
                        street_nodes.push(n);
                        prev = n;
                    }
                    let nb = g.add_node(b - d * sb, None);
                    g.link(prev, nb, false);
                    ends[seg.a.0 as usize].push((na, sid, 0));
                    ends[seg.b.0 as usize].push((nb, sid, 0));
                    street_nodes.push(na);
                    street_nodes.push(nb);
                }
            }
        }
        // Joins at road nodes: plain bends link matching sides directly;
        // junctions get crosswalks and corners; a footpath meeting a street
        // gets a zebra; a dead end is crossed at its end.
        for (ni, node) in roads.nodes.iter().enumerate() {
            let e = &ends[ni];
            if e.is_empty() {
                continue;
            }
            let nid = NodeId(ni as u32);
            let mut streets: Vec<SegmentId> = Vec::new();
            let mut paths = 0;
            let mut arms: Vec<SegmentId> = Vec::new();
            for x in e {
                if !arms.contains(&x.1) {
                    arms.push(x.1);
                    if roads.segment(x.1).is_street() {
                        streets.push(x.1);
                    } else {
                        paths += 1;
                    }
                }
            }
            // A junction of three or more streets, slip roads counted (the
            // crosswalks are painted where the kerbs meet them all).
            if streets.len() >= 3 || (streets.len() == 2 && is_junction(roads, nid)) {
                g.junction(roads, nid, node.pos, e);
                continue;
            }
            if streets.is_empty() || (arms.len() == 2 && streets.len() == 2) || (arms.len() == 1 && paths == 1) {
                // Footpaths only, or a plain bend of two streets.
                if arms.len() == 2 && e.len() >= 2 {
                    if !g.pair_ends(e) {
                        g.hub_off_carriageway(roads, nid, node.pos, e);
                    }
                } else {
                    g.hub_off_carriageway(roads, nid, node.pos, e);
                }
                continue;
            }
            // One or two streets with footpaths, or a dead end.
            let on_street: Vec<End> = e.iter().copied().filter(|x| roads.segment(x.1).is_street()).collect();
            if streets.len() == 2 {
                g.pair_ends(&on_street);
            }
            let s0 = streets[0];
            let sides: Vec<u32> = on_street.iter().filter(|x| x.1 == s0).map(|x| x.0).collect();
            if sides.len() == 2 && !roads.segment(s0).fronts_nothing() {
                let kind = if streets.len() == 2 && paths > 0 { CrossingKind::Zebra } else { CrossingKind::Informal };
                g.link_cross(sides[0], sides[1], s0, Some(nid), kind, roads);
            }
            for x in e.iter().filter(|x| !roads.segment(x.1).is_street()) {
                let p = g.nodes[x.0 as usize].pos;
                if let Some(best) = on_street.iter().min_by(|u, v| g.nodes[u.0 as usize].pos.distance(p).total_cmp(&g.nodes[v.0 as usize].pos.distance(p))).map(|y| y.0) {
                    g.link(x.0, best, false);
                }
            }
        }

        // ── lots ─────────────────────────────────────────────────────────
        // Each usable lot gets a loop inset from its edges with a node at
        // every vertex and every edge midpoint; the front midpoint links to
        // the nearest street node.
        let street_grid = Grid::new(NODE_CELL, street_nodes.iter().map(|&n| (g.nodes[n as usize].pos, g.nodes[n as usize].pos)));
        struct LoopInfo {
            lot: LotId,
            /// Node at each edge midpoint, by edge index.
            mids: Vec<u32>,
            through: bool,
        }
        let mut loops: Vec<LoopInfo> = Vec::new();
        for (lid, lot) in lots.iter() {
            if lot.polygon.len() < 3 {
                continue;
            }
            let ring = geom::inset_ring(&lot.polygon, LOOP_INSET);
            if ring.len() != lot.polygon.len() {
                continue;
            }
            let n = ring.len();
            let verts: Vec<u32> = ring.iter().map(|p| g.add_node(*p, Some(lid))).collect();
            let mut mids = Vec::with_capacity(n);
            for i in 0..n {
                let m = g.add_node(ring[i].lerp(ring[(i + 1) % n], 0.5), Some(lid));
                g.link(verts[i], m, true);
                g.link(m, verts[(i + 1) % n], true);
                mids.push(m);
            }
            // Front edge midpoint to the pavement.
            let front_i = (0..n)
                .min_by(|&a, &b| {
                    let ma = (lot.polygon[a] + lot.polygon[(a + 1) % n]) * 0.5;
                    let mb = (lot.polygon[b] + lot.polygon[(b + 1) % n]) * 0.5;
                    ma.distance(lot.entrance()).total_cmp(&mb.distance(lot.entrance()))
                })
                .unwrap_or(0);
            let front = mids[front_i];
            let fp = g.nodes[front as usize].pos;
            match front_nodes.get(&lid) {
                Some(&n) => g.link(front, n, true),
                None => {
                    // Items are positions in `street_nodes`, so a tie goes
                    // to the earlier entry as a scan would resolve it.
                    let within = |i: u32| {
                        let d = g.nodes[street_nodes[i as usize] as usize].pos.distance(fp);
                        if d < FRONT_REACH { d } else { f32::INFINITY }
                    };
                    if let Some((i, _)) = street_grid.nearest(fp, within) {
                        let best = street_nodes[i as usize];
                        g.link(front, best, true);
                        g.nodes[best as usize].near.push(lid);
                    }
                }
            }
            // The Stacks get an alley cross through the middle, and so does
            // unusable ground: it is open land people cross freely.
            if lot.landlocked || (lot.land_use == LandUse::Residential && Tier::for_land_value(land_value.at(lot.center)) == Tier::Low) {
                let c = g.add_node(lot.center, Some(lid));
                let mut picks: Vec<(f32, u32)> = mids.iter().map(|&m| (g.nodes[m as usize].pos.distance(lot.center), m)).collect();
                picks.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (_, m) in picks.into_iter().take(4) {
                    g.link(c, m, true);
                }
            }
            loops.push(LoopInfo { lot: lid, mids, through: walkable_through(lot, land_value) });
        }

        // ── gates between neighbouring lots ──────────────────────────────
        // Edges that run along each other for a few metres get a gate at the
        // shared stretch, linking the two loops' midpoint nodes.
        let mut grid: HashMap<(i32, i32), Vec<(usize, usize)>> = HashMap::new();
        let cell = 12.0;
        for (li, info) in loops.iter().enumerate() {
            let poly = &lots[info.lot].polygon;
            let n = poly.len();
            for i in 0..n {
                let m = (poly[i] + poly[(i + 1) % n]) * 0.5;
                grid.entry(((m.x / cell).floor() as i32, (m.y / cell).floor() as i32)).or_default().push((li, i));
            }
        }
        let mut gated: Vec<(u32, u32)> = Vec::new();
        for (li, info) in loops.iter().enumerate() {
            if !info.through {
                continue;
            }
            let poly = &lots[info.lot].polygon;
            let n = poly.len();
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let m = (a + b) * 0.5;
                let cx = (m.x / cell).floor() as i32;
                let cy = (m.y / cell).floor() as i32;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        let Some(list) = grid.get(&(cx + dx, cy + dy)) else { continue };
                        for &(lj, j) in list {
                            if lj <= li || !loops[lj].through {
                                continue;
                            }
                            // [waterfront] A public beach opens only onto a park.
                            if !beach_gate_ok(&lots[info.lot], &lots[loops[lj].lot]) {
                                continue;
                            }
                            let other = &lots[loops[lj].lot].polygon;
                            let (c, d) = (other[j], other[(j + 1) % other.len()]);
                            if let Some((lo, hi)) = geom::shared_stretch(a, b, c, d) {
                                if hi - lo >= GATE_MIN_SHARED {
                                    let na = info.mids[i];
                                    let nb = loops[lj].mids[j];
                                    if !gated.contains(&(na, nb)) {
                                        gated.push((na, nb));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for (a, b) in gated {
            g.link(a, b, true);
        }

        // ── parks ────────────────────────────────────────────────────────
        // Park paths are public: every path vertex is a node, each path's
        // first point (an entrance) joins the lot loop, and benches hang
        // off the nearest path node so people can walk to them and sit.
        let loop_mids: HashMap<LotId, Vec<u32>> = loops.iter().map(|l| (l.lot, l.mids.clone())).collect();
        for (lid, lot) in lots.iter() {
            let Some(park) = &lot.park else { continue };
            let mut path_nodes: Vec<u32> = Vec::new();
            for path in &park.paths {
                let mut prev: Option<u32> = None;
                for p in path {
                    // Reuse a node already at this spot (the hub is shared).
                    let n = match path_nodes.iter().copied().find(|&n| g.nodes[n as usize].pos.distance(*p) < 0.3) {
                        Some(n) => n,
                        None => {
                            let n = g.add_node(*p, Some(lid));
                            path_nodes.push(n);
                            n
                        }
                    };
                    if let Some(pv) = prev {
                        g.link_kind(pv, n, true, true);
                    }
                    prev = Some(n);
                }
                if let (Some(&first), Some(mids)) = (path.first(), loop_mids.get(&lid)) {
                    let entrance = path_nodes.iter().copied().find(|&n| g.nodes[n as usize].pos.distance(first) < 0.3);
                    let nearest = mids.iter().copied().min_by(|&a, &b| g.nodes[a as usize].pos.distance(first).total_cmp(&g.nodes[b as usize].pos.distance(first)));
                    if let (Some(e), Some(m)) = (entrance, nearest) {
                        g.link_kind(e, m, true, true);
                    }
                }
            }
            for (pos, facing) in &park.benches {
                let n = g.add_node(*pos, Some(lid));
                if let Some(&near) = path_nodes.iter().min_by(|&&a, &&b| g.nodes[a as usize].pos.distance(*pos).total_cmp(&g.nodes[b as usize].pos.distance(*pos))) {
                    g.link_kind(n, near, true, true);
                }
                g.benches.push((n, lid, *facing));
            }
        }
        for (k, b) in g.benches.iter().enumerate() {
            g.bench_lots.entry(b.1).or_default().push(k as u32);
        }
        g.reindex();
        g
    }

    /// The node grid and the crossing lookup, after nodes were added.
    pub(super) fn reindex(&mut self) {
        self.index = Grid::new(NODE_CELL, self.nodes.iter().map(|n| (n.pos, n.pos)));
        self.cross_at.clear();
        for (k, c) in self.crossings.iter().enumerate() {
            let (pa, pb) = (self.nodes[c.a as usize].pos, self.nodes[c.b as usize].pos);
            self.cross_at.insert(pos_key(pa, pb), k as u32);
            self.cross_at.insert(pos_key(pb, pa), k as u32);
        }
        self.raised_at = self.nodes.iter().filter(|n| n.level != 0.0).map(|n| ((n.pos.x.to_bits(), n.pos.y.to_bits()), n.level)).collect();
    }

    /// [tools] Put the Intersection editor's crosswalk upgrades on the
    /// crossings they were made for (matched by position and the street's
    /// direction): a push button, or a footbridge with its raised walkway.
    pub fn apply_upgrades(&mut self, rules: &[crate::junctions::CrossingRule], roads: &RoadNetwork) {
        use crate::junctions::CrossingUpgrade;
        if rules.is_empty() {
            return;
        }
        for k in 0..self.crossings.len() {
            let c = &self.crossings[k];
            if c.kind == CrossingKind::Jay || roads.try_segment(c.segment).is_none() {
                continue;
            }
            let (s0, s1) = roads.segment_points(c.segment);
            let dir = (s1 - s0).normalize_or_zero();
            let Some(rule) = rules.iter().find(|r| r.at.distance(c.mid) < 2.0 && r.dir.dot(dir).abs() > 0.9) else { continue };
            match rule.upgrade {
                CrossingUpgrade::None => {}
                CrossingUpgrade::Button => self.crossings[k].button = true,
                CrossingUpgrade::Footbridge => {
                    let along = Self::stair_dir(c, dir, roads);
                    self.add_footbridge(k as u32, along);
                    // [footbridge] The bridge replaces the crosswalk under
                    // it unless the player kept it: everyone crossing here
                    // climbs over, and nobody waits in the road.
                    if !rule.keep_crosswalk {
                        self.close_crossing(k as u32);
                    }
                }
            }
        }
        self.reindex();
    }
}
