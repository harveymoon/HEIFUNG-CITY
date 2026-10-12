//! Parks. A park is every park parcel that touches another, so a zone cut
//! into lots by a road or a footpath is still one park: its paths run from
//! lot to lot, its features are spread over the lots by area, and every lot
//! gets benches. The plan is in world coordinates and is deterministic from
//! the lot shapes, so a rebuild gives the same park.
//!
//! What a park holds follows the land around it: on cheap land it is a bare
//! lot with a court and a couple of benches; in the middle it is a lawn with
//! a playground, a pond and a kiosk; on dear land it has a lake, a fountain,
//! a bandstand and rides.

use crate::buildings::Tier;
use crate::geom::{bounds, point_in_polygon, polygon_area, polygon_centroid, project_on_segment as project, shared_stretch};
use crate::land_value::LandValue;
use crate::zoning::{LandUse, Lot, LotId};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

pub use crate::geom::distance_to_outline as distance_to_boundary;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParkFeature {
    Court,
    Skatepark,
    Playground,
    Pond,
    Lake,
    Fountain,
    Bandstand,
    Carousel,
    FerrisWheel,
    Kiosk,
    FlowerBed,
    Statue,
    DogRun,
    Lawn,
}

impl ParkFeature {
    pub fn label(self) -> &'static str {
        match self {
            ParkFeature::Court => "basketball court",
            ParkFeature::Skatepark => "skate park",
            ParkFeature::Playground => "playground",
            ParkFeature::Pond => "pond",
            ParkFeature::Lake => "lake",
            ParkFeature::Fountain => "fountain",
            ParkFeature::Bandstand => "bandstand",
            ParkFeature::Carousel => "carousel",
            ParkFeature::FerrisWheel => "ferris wheel",
            ParkFeature::Kiosk => "kiosk",
            ParkFeature::FlowerBed => "flower bed",
            ParkFeature::Statue => "statue",
            ParkFeature::DogRun => "dog run",
            ParkFeature::Lawn => "lawn",
        }
    }

    /// Ground the feature needs, as a radius.
    pub fn radius(self) -> f32 {
        match self {
            ParkFeature::Court => 8.0,
            ParkFeature::Skatepark => 6.5,
            ParkFeature::Playground => 6.0,
            ParkFeature::Pond => 5.0,
            ParkFeature::Lake => 12.0,
            ParkFeature::Fountain => 4.0,
            ParkFeature::Bandstand => 5.0,
            ParkFeature::Carousel => 5.5,
            ParkFeature::FerrisWheel => 8.0,
            ParkFeature::Kiosk => 3.0,
            ParkFeature::FlowerBed => 2.8,
            ParkFeature::Statue => 2.2,
            ParkFeature::DogRun => 7.0,
            ParkFeature::Lawn => 7.0,
        }
    }

    /// People sit facing these.
    pub fn draws_benches(self) -> bool {
        matches!(self, ParkFeature::Pond | ParkFeature::Lake | ParkFeature::Fountain | ParkFeature::Playground | ParkFeature::Bandstand | ParkFeature::Statue)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParkFeatureSlot {
    pub kind: ParkFeature,
    /// World position of the centre.
    pub pos: Vec2,
    pub radius: f32,
    /// Which way it faces (towards the nearest path).
    pub heading: f32,
}

/// One parcel's share of a park.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParkLayout {
    pub tier: Tier,
    /// Walkable paths in world coordinates; the first point of each is an
    /// entrance (a gate on the street, or a gap to the next park lot).
    pub paths: Vec<Vec<Vec2>>,
    /// Benches: world position and the direction a sitter faces.
    pub benches: Vec<(Vec2, f32)>,
    pub features: Vec<ParkFeatureSlot>,
    pub entrances: Vec<Vec2>,
    /// Indices of polygon edges that border another lot of the same park:
    /// nothing is built along them.
    pub shared_edges: Vec<usize>,
    /// The connected park this lot belongs to.
    pub cluster_lots: u32,
    pub cluster_area: f32,
    /// The park's main lot (the largest on a street): the one that gets
    /// the forecourt, the gate pillars and the name sign, so a park of
    /// many lots reads as one park with one entrance rather than a row of
    /// small ones. Saves from before have every lot as its own anchor.
    #[serde(default = "anchor_default")]
    pub anchor: bool,
    /// Every stretch of this lot's outline that the park's other lots also
    /// border, world coordinates: the park carries on over these, so trees
    /// may lean across them (an edge in `shared_edges` may be shared only
    /// in part; these are the exact parts). Empty for older saves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seams: Vec<(Vec2, Vec2)>,
    /// [parks] (D5) Left wild, a nature preserve: meadow and woodland, its
    /// paths a trail, no features, few benches.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preserve: bool,
}

fn anchor_default() -> bool {
    true
}

impl ParkLayout {
    pub fn has(&self, kind: ParkFeature) -> bool {
        self.features.iter().any(|f| f.kind == kind)
    }
}

/// How far apart two lot edges may lie and still be one seam of a park.
const SEAM_TOL: f32 = 0.3;

/// Minimum shared boundary for two park lots to count as one park.
const SHARED_MIN: f32 = 4.0;

/// Plan every park in the city. Lots that are not usable park land lose
/// their layout.
pub fn plan_parks(lots: &mut SlotMap<LotId, Lot>, land_value: &LandValue) {
    plan_parks_with(lots, land_value, &[]);
}

/// [park-paths] `plan_parks`, with the ends of player paths joined to a
/// park (`links`): each becomes an entrance of the park lot it reaches
/// (inside it or within `walk::GROUNDS_REACH` of its edge), so the park's
/// walkways run out to meet the path.
pub fn plan_parks_with(lots: &mut SlotMap<LotId, Lot>, land_value: &LandValue, links: &[Vec2]) {
    plan_parks_wild(lots, land_value, links, &[]);
}

/// [parks] (D5) `plan_parks_with`, with the points of the parks left as
/// nature preserves (`World::parks`): a park any of whose lots holds one
/// is planned wild.
pub fn plan_parks_wild(lots: &mut SlotMap<LotId, Lot>, land_value: &LandValue, links: &[Vec2], preserves: &[Vec2]) {
    let ids: Vec<LotId> = lots.iter().filter(|(_, l)| l.land_use == LandUse::Park && l.polygon.len() >= 3 && !l.shore.beach).map(|(id, _)| id).collect();
    for (id, l) in lots.iter_mut() {
        if !ids.contains(&id) {
            l.park = None;
        }
    }
    let n = ids.len();
    // Adjacency: lots sharing a few metres of edge, with the gap's midpoint.
    let mut adj: Vec<Vec<(usize, Vec2)>> = vec![Vec::new(); n];
    let mut shared: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut seams: Vec<Vec<(Vec2, Vec2)>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in (i + 1)..n {
            if let Some((mid, ei, ej)) = shared_edge(&lots[ids[i]].polygon, &lots[ids[j]].polygon) {
                for s in shared_stretches(&lots[ids[i]].polygon, &lots[ids[j]].polygon) {
                    seams[i].push(s);
                    seams[j].push(s);
                }
                adj[i].push((j, mid));
                adj[j].push((i, mid));
                shared[i].push(ei);
                shared[j].push(ej);
            }
        }
    }
    // [park-paths] Each joined path end is an extra gap of the park lot
    // nearest it.
    let mut linked: Vec<Vec<Vec2>> = vec![Vec::new(); n];
    for &p in links {
        let near = (0..n)
            .map(|i| {
                let poly = &lots[ids[i]].polygon;
                (i, if point_in_polygon(p, poly) { 0.0 } else { distance_to_boundary(p, poly) })
            })
            .filter(|(_, d)| *d <= crate::walk::GROUNDS_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        if let Some((i, _)) = near {
            linked[i].push(p);
        }
    }
    let mut cluster = vec![usize::MAX; n];
    let mut next = 0;
    for s in 0..n {
        if cluster[s] != usize::MAX {
            continue;
        }
        let mut stack = vec![s];
        cluster[s] = next;
        while let Some(i) = stack.pop() {
            for &(j, _) in &adj[i] {
                if cluster[j] == usize::MAX {
                    cluster[j] = next;
                    stack.push(j);
                }
            }
        }
        next += 1;
    }
    for c in 0..next {
        let members: Vec<usize> = (0..n).filter(|&i| cluster[i] == c).collect();
        let mut layouts = plan_cluster(&members, &ids, &adj, &linked, lots, land_value);
        // [parks] A nature preserve: no lawns' features, a trail, a bench
        // in three.
        if members.iter().any(|&i| preserves.iter().any(|p| point_in_polygon(*p, &lots[ids[i]].polygon))) {
            for l in layouts.iter_mut() {
                l.preserve = true;
                l.features.clear();
                let mut k = 0;
                l.benches.retain(|_| {
                    k += 1;
                    k % 3 == 1
                });
            }
        }
        for (i, mut layout) in members.iter().zip(layouts) {
            layout.shared_edges = shared[*i].clone();
            layout.seams = seams[*i].clone();
            lots[ids[*i]].park = Some(layout);
        }
    }
    // [waterfront] Public beaches are sand: a boardwalk down to the water
    // and towel spots (`crate::shore::plan_beach`). [beaches] (H2) Beach
    // lots side by side are one beach: one tier, the sand running over the
    // seams between them, one anchor (the lifeguard, the sign).
    for group in crate::shore::beach_groups(lots) {
        let areas: Vec<f32> = group.iter().map(|id| polygon_area(&lots[*id].polygon).abs()).collect();
        let total: f32 = areas.iter().sum();
        let lv = group.iter().zip(&areas).map(|(id, a)| land_value.at(lots[*id].center) * a).sum::<f32>() / total.max(1.0);
        let tier = Tier::for_land_value(lv);
        let anchor = (0..group.len()).max_by(|&a, &b| areas[a].total_cmp(&areas[b]).then(b.cmp(&a))).unwrap_or(0);
        let seams: Vec<Vec<(Vec2, Vec2)>> = group
            .iter()
            .map(|id| group.iter().filter(|o| *o != id).flat_map(|o| shared_stretches(&lots[*id].polygon, &lots[*o].polygon)).collect())
            .collect();
        for (k, id) in group.iter().enumerate() {
            let mut layout = crate::shore::plan_beach_with(&lots[*id], tier, &seams[k]);
            layout.cluster_lots = group.len() as u32;
            layout.cluster_area = total;
            layout.anchor = k == anchor;
            layout.seams = seams[k].clone();
            lots[*id].park = Some(layout);
        }
    }
}

/// A one-lot park with no neighbours, for previews and tests.
pub fn plan_single(lot: &Lot, tier: Tier) -> ParkLayout {
    let mut l = plan_lot_paths(lot, &[], &[], true);
    l.tier = tier;
    let area = polygon_area(&lot.polygon).abs();
    let wanted = feature_budget(tier, area, inradius(&lot.polygon));
    let mut taken: Vec<(Vec2, f32)> = Vec::new();
    for kind in wanted {
        let r = feature_radius(kind, tier, inradius(&lot.polygon));
        if let Some(p) = find_spot(lot, r, &taken, &l.paths) {
            taken.push((p, r));
            l.features.push(ParkFeatureSlot { kind, pos: p, radius: r, heading: heading_to_path(p, &l.paths) });
        }
    }
    finish_lot(lot, &mut l);
    l.cluster_lots = 1;
    l.cluster_area = area;
    l
}

fn plan_cluster(members: &[usize], ids: &[LotId], adj: &[Vec<(usize, Vec2)>], linked: &[Vec<Vec2>], lots: &SlotMap<LotId, Lot>, land_value: &LandValue) -> Vec<ParkLayout> {
    let areas: Vec<f32> = members.iter().map(|&i| polygon_area(&lots[ids[i]].polygon).abs()).collect();
    let total: f32 = areas.iter().sum();
    let lv = members.iter().zip(&areas).map(|(&i, a)| land_value.at(lots[ids[i]].center) * a).sum::<f32>() / total.max(1.0);
    let tier = Tier::for_land_value(lv);
    let max_inradius = members.iter().map(|&i| inradius(&lots[ids[i]].polygon)).fold(0.0, f32::max);

    // Paths first: entrances are street gates and the gaps to neighbouring
    // park lots, so the spokes run lot to lot.
    let mut layouts: Vec<ParkLayout> = members
        .iter()
        .map(|&i| {
            let gaps: Vec<Vec2> = adj[i].iter().map(|(_, m)| *m).chain(linked[i].iter().copied()).collect();
            let mut l = plan_lot_paths(&lots[ids[i]], &gaps, &linked[i], !lots[ids[i]].landlocked);
            l.tier = tier;
            l
        })
        .collect();

    // Features, biggest first, each to the lot with the least feature area
    // per square metre that has room for it.
    let wanted = feature_budget(tier, total, max_inradius);
    let mut used: Vec<f32> = vec![0.0; members.len()];
    let mut taken: Vec<Vec<(Vec2, f32)>> = vec![Vec::new(); members.len()];
    for kind in wanted {
        let mut order: Vec<usize> = (0..members.len()).collect();
        order.sort_by(|&a, &b| (used[a] / areas[a]).total_cmp(&(used[b] / areas[b])));
        for m in order {
            let lot = &lots[ids[members[m]]];
            let r = feature_radius(kind, tier, inradius(&lot.polygon));
            if inradius(&lot.polygon) < r + 2.0 {
                continue;
            }
            if let Some(p) = find_spot(lot, r, &taken[m], &layouts[m].paths) {
                taken[m].push((p, r));
                used[m] += std::f32::consts::PI * r * r;
                let heading = heading_to_path(p, &layouts[m].paths);
                layouts[m].features.push(ParkFeatureSlot { kind, pos: p, radius: r, heading });
                break;
            }
        }
    }
    // The anchor: the largest lot on a street (any lot if none is).
    let anchor = (0..members.len()).max_by(|&a, &b| {
        let on_street = |m: usize| !lots[ids[members[m]]].landlocked;
        on_street(a).cmp(&on_street(b)).then(areas[a].total_cmp(&areas[b])).then(b.cmp(&a))
    });
    for (m, l) in layouts.iter_mut().enumerate() {
        finish_lot(&lots[ids[members[m]]], l);
        l.cluster_lots = members.len() as u32;
        l.cluster_area = total;
        l.anchor = Some(m) == anchor;
    }
    layouts
}

/// Which features a park of this size on this land deserves, largest first.
fn feature_budget(tier: Tier, area: f32, max_inradius: f32) -> Vec<ParkFeature> {
    use ParkFeature::*;
    let mut v = Vec::new();
    match tier {
        Tier::Low => {
            if area >= 900.0 {
                v.push(Court);
            }
            if area >= 3200.0 {
                v.push(Skatepark);
            }
            if area >= 6000.0 {
                v.push(Court);
            }
        }
        Tier::Mid => {
            if area >= 4000.0 {
                v.push(DogRun);
            }
            if area >= 3500.0 {
                v.push(Court);
            }
            let playgrounds = ((area / 2500.0).floor() as usize).clamp(usize::from(area >= 450.0), 4);
            for _ in 0..playgrounds {
                v.push(Playground);
            }
            if area >= 1500.0 {
                v.push(Pond);
            }
            if area >= 1200.0 {
                v.push(Kiosk);
            }
            let beds = ((area / 1500.0).floor() as usize).min(4);
            for _ in 0..beds {
                v.push(FlowerBed);
            }
        }
        Tier::High => {
            if area >= 2500.0 && max_inradius >= 11.0 {
                v.push(Lake);
            } else if area >= 900.0 {
                v.push(Pond);
            }
            if area >= 7000.0 && max_inradius >= 12.0 {
                v.push(FerrisWheel);
            }
            if area >= 3500.0 {
                v.push(Carousel);
            }
            if area >= 2000.0 {
                v.push(Bandstand);
            }
            let playgrounds = ((area / 3000.0).floor() as usize).min(3);
            for _ in 0..playgrounds {
                v.push(Playground);
            }
            if area >= 500.0 {
                v.push(Fountain);
            }
            if area >= 2000.0 {
                v.push(Kiosk);
            }
            if area >= 800.0 {
                v.push(Statue);
            }
            let beds = ((area / 1000.0).floor() as usize).clamp(1, 6);
            for _ in 0..beds {
                v.push(FlowerBed);
            }
        }
    }
    v
}

fn feature_radius(kind: ParkFeature, tier: Tier, lot_inradius: f32) -> f32 {
    let base = kind.radius();
    let r = match kind {
        ParkFeature::Lake => (lot_inradius * 0.5).clamp(7.0, 16.0),
        ParkFeature::Pond => (lot_inradius * 0.35).clamp(3.5, 6.0),
        _ => base,
    };
    if tier == Tier::Low { r * 0.9 } else { r }
}

/// Spokes from a hub deep in the lot to every entrance, plus a loop on big
/// lots with only one way in.
/// `links` (some of `gaps`) are the ends of player paths joined to the
/// park: their spokes start out at the path's end itself, so the walkway
/// is paved right up to the path rather than stopping inside the edge.
fn plan_lot_paths(lot: &Lot, gaps: &[Vec2], links: &[Vec2], has_gate: bool) -> ParkLayout {
    let poly = &lot.polygon;
    let mut entrances: Vec<Vec2> = Vec::new();
    // Street gate: the front edge midpoint, pulled a step inside. Backland
    // with no road is reached only through the gaps to its neighbours.
    if has_gate {
        let gate = lot.entrance() - lot.front_dir * 0.8;
        entrances.push(if point_in_polygon(gate, poly) { gate } else { pull_inside(poly, lot.entrance(), lot.center) });
    }
    for g in gaps {
        let mut inward = (lot.center - *g).normalize_or_zero();
        // [sweep-visual] A joined path's end outside the edge comes in
        // square to the edge, straight on from the path.
        if links.contains(g) && !point_in_polygon(*g, poly) {
            let n = poly.len();
            let q = (0..n).map(|i| crate::geom::project_on_segment(*g, poly[i], poly[(i + 1) % n]).1).min_by(|a, b| a.distance(*g).total_cmp(&b.distance(*g)));
            if let Some(q) = q.filter(|q| q.distance(*g) > 0.05) {
                inward = (q - *g).normalize_or_zero();
                let p = q + inward * 0.8;
                if point_in_polygon(p, poly) && entrances.iter().all(|e| e.distance(p) > 4.0) {
                    entrances.push(p);
                    continue;
                }
            }
        }
        let p = *g + inward * 0.8;
        let p = if point_in_polygon(p, poly) { p } else { pull_inside(poly, *g, lot.center) };
        if entrances.iter().all(|e| e.distance(p) > 4.0) {
            entrances.push(p);
        }
    }
    let hub = hub_point(poly, &entrances);
    if entrances.is_empty() {
        // Nothing reaches this lot: a loop of its own, at least.
        entrances.push(pull_inside(poly, hub + (polygon_centroid(poly) - hub).normalize_or_zero() * 2.0 + Vec2::X * 0.5, hub));
    }
    let mut paths: Vec<Vec<Vec2>> = entrances.iter().map(|e| vec![*e, hub]).collect();
    // [sweep-visual] A joined path ends a step outside the edge (at its
    // gate, `walk::GATE_OUT`): the spoke nearest it runs out to meet it, so
    // no strip of grass is left between the two.
    for l in links {
        if point_in_polygon(*l, poly) {
            continue;
        }
        let near = paths.iter_mut().filter(|p| p[0].distance(*l) < 6.0).min_by(|a, b| a[0].distance(*l).total_cmp(&b[0].distance(*l)));
        if let Some(path) = near {
            if path[0].distance(*l) > 0.3 {
                path.insert(0, *l);
            }
        }
    }
    let r = inradius(poly);
    if entrances.len() == 1 && r >= 9.0 {
        // A loop out to the far side and back along the other edge.
        let far = poly.iter().copied().max_by(|a, b| a.distance(entrances[0]).total_cmp(&b.distance(entrances[0]))).unwrap_or(hub);
        let out = hub + (far - hub) * 0.55;
        let out = pull_inside(poly, out, hub);
        let side = (out - hub).perp().normalize_or_zero() * (r * 0.5);
        let a = pull_inside(poly, hub + side * 0.8 + (out - hub) * 0.5, hub);
        let b = pull_inside(poly, hub - side * 0.8 + (out - hub) * 0.5, hub);
        paths.push(vec![hub, a, out, b, hub]);
    }
    ParkLayout { tier: Tier::Mid, paths, benches: Vec::new(), features: Vec::new(), entrances, shared_edges: Vec::new(), cluster_lots: 1, cluster_area: polygon_area(poly).abs(), anchor: true, seams: Vec::new(), preserve: false }
}

/// Spurs to the features, bends around them, then benches along every path
/// and around the water.
fn finish_lot(lot: &Lot, l: &mut ParkLayout) {
    let poly = &lot.polygon;
    let hub = l.paths.first().and_then(|p| p.last().copied()).unwrap_or(lot.center);
    // Spur from the hub to the edge of each feature that is not a lawn.
    let features = l.features.clone();
    for f in &features {
        if matches!(f.kind, ParkFeature::Lawn | ParkFeature::FlowerBed) {
            continue;
        }
        let d = (f.pos - hub).normalize_or_zero();
        let end = f.pos - d * (f.radius + 0.8);
        if end.distance(hub) > 3.0 {
            l.paths.push(vec![hub, end]);
        }
    }
    // Bend every straight leg around features it would cut through.
    for path in l.paths.iter_mut() {
        let mut out: Vec<Vec2> = Vec::with_capacity(path.len() * 2);
        for w in path.windows(2) {
            let (a, b) = (w[0], w[1]);
            out.push(a);
            for f in &features {
                let (t, q) = project(f.pos, a, b);
                if t <= 0.05 || t >= 0.95 {
                    continue;
                }
                let d = f.pos.distance(q);
                if d < f.radius + 1.2 && a.distance(f.pos) > f.radius + 1.0 && b.distance(f.pos) > f.radius + 1.0 {
                    let away = if d > 0.1 { (q - f.pos) / d } else { (b - a).perp().normalize_or_zero() };
                    let bend = f.pos + away * (f.radius + 1.8);
                    out.push(pull_inside(poly, bend, hub));
                }
            }
        }
        if let Some(last) = path.last() {
            out.push(*last);
        }
        *path = out;
    }
    // Benches along the paths, alternating sides, off the features.
    let spacing = match l.tier {
        Tier::Low => 24.0,
        Tier::Mid => 13.0,
        Tier::High => 9.0,
    };
    let mut benches: Vec<(Vec2, f32)> = Vec::new();
    let mut side = 1.0f32;
    let mut carry = spacing * 0.5;
    for path in &l.paths {
        for w in path.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.distance(b);
            if len < 1e-3 {
                continue;
            }
            let d = (b - a) / len;
            let right = Vec2::new(-d.y, d.x);
            let mut s = carry;
            while s < len {
                let p = a + d * s + right * side * 1.7;
                let facing = (-right * side).to_angle();
                if point_in_polygon(p, poly) && distance_to_boundary(p, poly) > 0.9 && features.iter().all(|f| f.pos.distance(p) > f.radius + 0.8) && benches.iter().all(|(q, _)| q.distance(p) > 3.0) {
                    benches.push((p, facing));
                }
                side = -side;
                s += spacing;
            }
            carry = s - len;
        }
    }
    // Around the water and the stage.
    for f in features.iter().filter(|f| f.kind.draws_benches()) {
        let n = if l.tier == Tier::High { 5 } else { 3 };
        for k in 0..n {
            let a = f.heading + std::f32::consts::PI + (k as f32 - (n - 1) as f32 * 0.5) * 0.9;
            let p = f.pos + Vec2::from_angle(a) * (f.radius + 1.6);
            if point_in_polygon(p, poly) && distance_to_boundary(p, poly) > 0.9 && benches.iter().all(|(q, _)| q.distance(p) > 2.5) {
                benches.push((p, (f.pos - p).to_angle()));
            }
        }
    }
    // Every lot gets at least one bench, by the hub, looking at the gate.
    if benches.is_empty() {
        let gate = l.entrances[0];
        let p = pull_inside(poly, hub + (gate - hub).perp().normalize_or_zero() * 2.0, hub);
        benches.push((p, (gate - p).to_angle()));
    }
    l.benches = benches;
}

/// A free circle of radius `r` inside the lot, away from the paths, the
/// other features and the edge; the spot furthest from everything wins.
fn find_spot(lot: &Lot, r: f32, taken: &[(Vec2, f32)], paths: &[Vec<Vec2>]) -> Option<Vec2> {
    let poly = &lot.polygon;
    let (lo, hi) = bounds(poly);
    let step = ((hi - lo).max_element() / 14.0).clamp(1.5, 4.0);
    let mut best: Option<(f32, Vec2)> = None;
    let mut v = lo.y + step * 0.5;
    while v < hi.y {
        let mut u = lo.x + step * 0.5;
        while u < hi.x {
            let p = Vec2::new(u, v);
            u += step;
            if !point_in_polygon(p, poly) {
                continue;
            }
            let edge = distance_to_boundary(p, poly) - r - 1.0;
            if edge < 0.0 {
                continue;
            }
            let mut score = edge.min(6.0);
            for (q, qr) in taken {
                let gap = p.distance(*q) - r - qr - 1.5;
                if gap < 0.0 {
                    score = -1.0;
                    break;
                }
                score = score.min(gap + 2.0);
            }
            if score < 0.0 {
                continue;
            }
            for path in paths {
                for w in path.windows(2) {
                    let (_, q) = project(p, w[0], w[1]);
                    let gap = p.distance(q) - r - 1.5;
                    if gap < 0.0 {
                        score = -1.0;
                    } else {
                        score = score.min(gap + 3.0);
                    }
                }
            }
            if score < 0.0 {
                continue;
            }
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, p));
            }
        }
        v += step;
    }
    best.map(|(_, p)| p)
}

/// The point of the lot furthest from its edges that is not an entrance.
fn hub_point(poly: &[Vec2], entrances: &[Vec2]) -> Vec2 {
    let (lo, hi) = bounds(poly);
    let step = ((hi - lo).max_element() / 18.0).clamp(1.0, 3.0);
    let mut best = (f32::MIN, polygon_centroid(poly));
    let mut v = lo.y + step * 0.5;
    while v < hi.y {
        let mut u = lo.x + step * 0.5;
        while u < hi.x {
            let p = Vec2::new(u, v);
            u += step;
            if !point_in_polygon(p, poly) {
                continue;
            }
            let d = distance_to_boundary(p, poly);
            // Prefer the middle, but not right at a gate.
            let e = entrances.iter().map(|q| q.distance(p)).fold(f32::MAX, f32::min).min(10.0);
            let score = d + e * 0.3;
            if score > best.0 {
                best = (score, p);
            }
        }
        v += step;
    }
    best.1
}

fn heading_to_path(p: Vec2, paths: &[Vec<Vec2>]) -> f32 {
    let mut best: Option<(f32, Vec2)> = None;
    for path in paths {
        for w in path.windows(2) {
            let (_, q) = project(p, w[0], w[1]);
            let d = p.distance(q);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        }
    }
    best.map_or(0.0, |(_, q)| (q - p).to_angle())
}

// ── geometry ─────────────────────────────────────────────────────────────────

/// Distance from the centroid to the nearest edge.
pub fn inradius(poly: &[Vec2]) -> f32 {
    let c = polygon_centroid(poly);
    if point_in_polygon(c, poly) {
        distance_to_boundary(c, poly)
    } else {
        // A bent lot: the best of a few samples.
        let (lo, hi) = bounds(poly);
        let mut best = 0.0f32;
        for i in 0..8 {
            for j in 0..8 {
                let p = lo + (hi - lo) * Vec2::new((i as f32 + 0.5) / 8.0, (j as f32 + 0.5) / 8.0);
                if point_in_polygon(p, poly) {
                    best = best.max(distance_to_boundary(p, poly));
                }
            }
        }
        best
    }
}

/// Move `p` towards `anchor` until it is inside the polygon with a little
/// margin. If even the anchor is outside, the anchor is returned.
pub fn pull_inside(poly: &[Vec2], p: Vec2, anchor: Vec2) -> Vec2 {
    let ok = |q: Vec2| point_in_polygon(q, poly) && distance_to_boundary(q, poly) >= 0.8;
    if ok(p) {
        return p;
    }
    if !point_in_polygon(anchor, poly) {
        return anchor;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        if ok(anchor.lerp(p, mid)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    anchor.lerp(p, lo)
}

/// Midpoint of the stretch two polygons share, if it is long enough, with
/// the edge index on each side.
/// Every stretch (a metre or more) of `a`'s outline that `b`'s outline runs
/// along, as world segments.
pub(crate) fn shared_stretches(a: &[Vec2], b: &[Vec2]) -> Vec<(Vec2, Vec2)> {
    let mut out = Vec::new();
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        let dir = (q - p).normalize_or_zero();
        for j in 0..b.len() {
            let (r, s) = (b[j], b[(j + 1) % b.len()]);
            // Only edges that truly meet (touching lots may otherwise stand
            // up to a metre apart, and a canopy leaning over would hang
            // over the gap).
            let off = |x: Vec2| (x - p).perp_dot(dir).abs();
            if off(r) > SEAM_TOL || off(s) > SEAM_TOL {
                continue;
            }
            if let Some((lo, hi)) = shared_stretch(p, q, r, s) {
                if hi - lo >= 1.0 {
                    out.push((p + dir * lo, p + dir * hi));
                }
            }
        }
    }
    out
}

fn shared_edge(a: &[Vec2], b: &[Vec2]) -> Option<(Vec2, usize, usize)> {
    let mut best: Option<(f32, (Vec2, usize, usize))> = None;
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        let dir = (q - p).normalize_or_zero();
        for j in 0..b.len() {
            let (r, s) = (b[j], b[(j + 1) % b.len()]);
            let Some((lo, hi)) = shared_stretch(p, q, r, s) else { continue };
            let shared = hi - lo;
            if shared >= SHARED_MIN && best.is_none_or(|(bs, _)| shared > bs) {
                best = Some((shared, (p + dir * ((lo + hi) * 0.5), i, j)));
            }
        }
    }
    best.map(|(_, m)| m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roads::{RoadKind, RoadNetwork};
    use crate::zoning::{generate_lots, Zone, ZoneId};

    fn park_lots(value: f32) -> (SlotMap<LotId, Lot>, LandValue) {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, -40.0), Vec2::new(300.0, -40.0)], 1, 1);
        // A footpath through the middle splits the park into two lots.
        roads.add_polyline_kind(&[Vec2::new(0.0, -40.0), Vec2::new(0.0, 160.0)], 0, 0, RoadKind::Path);
        let zone = Zone { polygon: vec![Vec2::new(-80.0, -20.0), Vec2::new(80.0, -20.0), Vec2::new(80.0, 80.0), Vec2::new(-80.0, 80.0)], land_use: LandUse::Park };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let mut lv = LandValue::new(400.0, 32.0);
        lv.fill(value);
        (lots, lv)
    }

    #[test]
    fn parks_line_paths_and_plan_as_one_connected_park() {
        let (mut lots, lv) = park_lots(0.5);
        let parks: Vec<LotId> = lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
        assert!(parks.len() >= 2, "the path splits the park into usable lots: {}", parks.len());
        assert!(lots.iter().any(|(_, l)| l.usable() && !l.car_access()), "a park lot fronts the footpath");
        plan_parks(&mut lots, &lv);
        let layouts: Vec<&ParkLayout> = parks.iter().map(|&id| lots[id].park.as_ref().expect("planned")).collect();
        let biggest = layouts.iter().map(|l| l.cluster_lots).max().unwrap();
        assert!(biggest >= 2, "lots across the path form one park");
        for l in &layouts {
            assert!(!l.benches.is_empty(), "every lot has a bench");
            assert!(!l.paths.is_empty());
            assert_eq!(l.tier, Tier::Mid);
            let lot = lots.values().find(|x| x.park.as_ref().map(|p| p.entrances[0]) == Some(l.entrances[0])).unwrap();
            for path in &l.paths {
                for p in path {
                    assert!(point_in_polygon(*p, &lot.polygon), "path point {p:?} is on the lot");
                }
            }
            for (p, _) in &l.benches {
                assert!(point_in_polygon(*p, &lot.polygon), "bench {p:?} is on the lot");
            }
            for f in &l.features {
                assert!(distance_to_boundary(f.pos, &lot.polygon) >= f.radius, "{:?} fits", f.kind);
            }
        }
        // Features are spread: with a playground and a pond in the budget,
        // no single lot of the park hoards everything.
        let with_features = layouts.iter().filter(|l| !l.features.is_empty()).count();
        assert!(with_features >= 2, "features are spread over the lots");
        let multi: Vec<&&ParkLayout> = layouts.iter().filter(|l| l.cluster_lots >= 2).collect();
        assert!(multi.iter().any(|l| l.entrances.len() >= 2), "a lot in the park has a gap to its neighbour as well as a gate");
    }

    #[test]
    fn park_character_follows_land_value() {
        let (mut poor, lv) = park_lots(0.2);
        plan_parks(&mut poor, &lv);
        let (mut rich, lv) = park_lots(0.9);
        plan_parks(&mut rich, &lv);
        let kinds = |lots: &SlotMap<LotId, Lot>| -> Vec<ParkFeature> { lots.values().filter_map(|l| l.park.as_ref()).flat_map(|p| p.features.iter().map(|f| f.kind)).collect() };
        let pk = kinds(&poor);
        let rk = kinds(&rich);
        assert!(pk.iter().all(|k| matches!(k, ParkFeature::Court | ParkFeature::Skatepark)), "poor parks are bare: {pk:?}");
        assert!(rk.contains(&ParkFeature::Fountain) && (rk.contains(&ParkFeature::Lake) || rk.contains(&ParkFeature::Pond)), "rich parks have water and a fountain: {rk:?}");
        let poor_benches: usize = poor.values().filter_map(|l| l.park.as_ref()).map(|p| p.benches.len()).sum();
        let rich_benches: usize = rich.values().filter_map(|l| l.park.as_ref()).map(|p| p.benches.len()).sum();
        assert!(rich_benches > poor_benches && poor_benches > 0);
    }
}
