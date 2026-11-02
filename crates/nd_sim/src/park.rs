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
    /// [r8-parks] A big park's own buildings and grounds: a community
    /// centre (a low hall), a cafe with tables out, a toilet block, a
    /// sports pitch and allotments (a community garden in plots).
    CommunityCentre,
    Cafe,
    Toilets,
    SportsPitch,
    Allotments,
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
            ParkFeature::CommunityCentre => "community centre",
            ParkFeature::Cafe => "cafe",
            ParkFeature::Toilets => "toilets",
            ParkFeature::SportsPitch => "sports pitch",
            ParkFeature::Allotments => "allotments",
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
            ParkFeature::CommunityCentre => 8.0,
            ParkFeature::Cafe => 5.0,
            ParkFeature::Toilets => 2.6,
            ParkFeature::SportsPitch => 13.0,
            ParkFeature::Allotments => 7.0,
        }
    }

    /// [r8-parks] The least ground it can do with, as a radius: a park
    /// shapes a feature to the room it has (a half court, a small pitch).
    pub fn min_radius(self) -> f32 {
        match self {
            ParkFeature::Court => 6.0,
            ParkFeature::Skatepark => 5.0,
            ParkFeature::Playground => 4.5,
            ParkFeature::Pond => 3.5,
            ParkFeature::Lake => 7.0,
            ParkFeature::Fountain => 3.0,
            ParkFeature::Bandstand => 4.0,
            ParkFeature::Carousel => 4.5,
            ParkFeature::FerrisWheel => 7.5,
            ParkFeature::Kiosk => 2.5,
            ParkFeature::FlowerBed => 2.0,
            ParkFeature::Statue => 1.8,
            ParkFeature::DogRun => 5.0,
            ParkFeature::Lawn => 5.0,
            ParkFeature::CommunityCentre => 5.5,
            ParkFeature::Cafe => 3.5,
            ParkFeature::Toilets => 2.2,
            ParkFeature::SportsPitch => 8.5,
            ParkFeature::Allotments => 4.5,
        }
    }

    /// People sit facing these.
    pub fn draws_benches(self) -> bool {
        matches!(self, ParkFeature::Pond | ParkFeature::Lake | ParkFeature::Fountain | ParkFeature::Playground | ParkFeature::Bandstand | ParkFeature::Statue | ParkFeature::SportsPitch)
    }

    /// [r8-parks] A building (or a ride) that stands tall in the park.
    pub fn tall(self) -> bool {
        matches!(self, ParkFeature::FerrisWheel | ParkFeature::Carousel | ParkFeature::Bandstand | ParkFeature::Statue | ParkFeature::Kiosk | ParkFeature::CommunityCentre | ParkFeature::Cafe | ParkFeature::Toilets)
    }
}

/// [r8-parks] A big park's car park: a paved yard off the street with its
/// bays, where visitors who drive leave their cars (drawn there, and
/// selectable, as at the kerb).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParkCarPark {
    /// The yard, world corners (along the street, then inward).
    pub outline: [Vec2; 4],
    /// The bays: where a car stands and the way it faces. Only the lot
    /// that holds the yard (`own`) counts them; a yard reaching over a
    /// neighbouring lot of the park is listed there too, without bays, so
    /// nothing is drawn over it.
    pub spaces: Vec<(Vec2, f32)>,
    /// Where cars turn in off the street.
    pub entry: Vec2,
    pub own: bool,
}

impl ParkCarPark {
    /// Distance from `p` to the yard (0 inside it).
    pub fn distance(&self, p: Vec2) -> f32 {
        if point_in_polygon(p, &self.outline) {
            0.0
        } else {
            crate::geom::distance_to_outline(p, &self.outline)
        }
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
    /// [r8-parks] Which park this lot is part of: the park's main lot.
    /// Every lot of a park carries the same one, and the city names,
    /// counts and builds a park by it. None in saves from before (they
    /// are planned again on load).
    #[serde(default)]
    pub cluster: Option<LotId>,
    /// [r8-parks] Features of the park's other lots that reach over this
    /// one (planned on the park's whole ground, across the seams): trees,
    /// benches and paths keep off them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nearby: Vec<(Vec2, f32)>,
    /// [r8-parks] The park's car park, on the lot that holds it (and on
    /// any other lot of the park it reaches over, `ParkCarPark::own`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_park: Option<ParkCarPark>,
    /// [r8-parks] Every stretch of the outline that faces another lot of
    /// the park, touching or across a footpath (`PATH_GAP`): the inside of
    /// the park, where no fence or hedge goes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inner_edges: Vec<(Vec2, Vec2)>,
}

fn anchor_default() -> bool {
    true
}

impl ParkLayout {
    pub fn has(&self, kind: ParkFeature) -> bool {
        self.features.iter().any(|f| f.kind == kind)
    }

    /// A one-lot layout with only its ways in, to be filled in.
    pub fn bare(tier: Tier, entrances: Vec<Vec2>, area: f32) -> Self {
        ParkLayout {
            tier,
            paths: Vec::new(),
            benches: Vec::new(),
            features: Vec::new(),
            entrances,
            shared_edges: Vec::new(),
            cluster_lots: 1,
            cluster_area: area,
            anchor: true,
            seams: Vec::new(),
            preserve: false,
            cluster: None,
            nearby: Vec::new(),
            car_park: None,
            inner_edges: Vec::new(),
        }
    }

    /// [r8-parks] The parts of this lot's outline on the outside of the
    /// park (not facing another lot of it): where fences, hedges and
    /// railings go.
    pub fn outer_stretches(&self, poly: &[Vec2]) -> Vec<(Vec2, Vec2)> {
        open_stretches(poly, &self.inner_edges)
    }
}

/// The parts of the outline `poly` not covered by any of `cover` (world
/// segments lying along it, within 0.6 m of it), as world segments.
pub fn open_stretches(poly: &[Vec2], cover: &[(Vec2, Vec2)]) -> Vec<(Vec2, Vec2)> {
    let n = poly.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let len = a.distance(b);
        if len < 1e-3 {
            continue;
        }
        let dir = (b - a) / len;
        let mut cov: Vec<(f32, f32)> = cover
            .iter()
            .filter(|(p, q)| (*p - a).perp_dot(dir).abs() < 0.6 && (*q - a).perp_dot(dir).abs() < 0.6)
            .map(|(p, q)| {
                let (t0, t1) = ((*p - a).dot(dir), (*q - a).dot(dir));
                (t0.min(t1).max(0.0), t0.max(t1).min(len))
            })
            .filter(|(lo, hi)| hi > lo)
            .collect();
        cov.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut t = 0.0f32;
        for (lo, hi) in cov {
            if lo > t + 0.01 {
                out.push((a + dir * t, a + dir * lo));
            }
            t = t.max(hi);
        }
        if t < len - 0.01 {
            out.push((a + dir * t, b));
        }
    }
    out
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

/// [r8-parks] Park lots this far apart, edge to edge, face each other
/// across a footpath (or touch): one park, the footpath one of its paths.
pub const PATH_GAP: f32 = 6.0;
/// [r8-parks] Street gates of one park at least this far apart: a park of
/// many narrow lots has a few ways in, not a gate (and a spoke) per lot.
const GATE_SPACING: f32 = 26.0;
/// [r8-parks] A park this big gets a car park by the street.
pub const CAR_PARK_AREA: f32 = 3000.0;
/// [r8-parks] The car park's yard: two rows of bays and the aisle.
const YARD_DEPTH: f32 = 17.0;
const YARD_MIN: f32 = 14.0;

/// [r8-parks] Where two park lots meet (or face each other across a
/// footpath): how long, the way through, and each side's stretches.
struct Joint {
    a: usize,
    b: usize,
    len: f32,
    /// The longest facing stretch on `a`'s side, its offset to `b` and
    /// `a`'s outward normal there.
    best: (Vec2, Vec2, f32, Vec2),
    a_side: Vec<(Vec2, Vec2)>,
    b_side: Vec<(Vec2, Vec2)>,
    a_edges: Vec<usize>,
    b_edges: Vec<usize>,
}

/// Stretches of `a`'s outline that face `b`'s outline from at most `gap`
/// outside it: (edge of a, edge of b, start, end, offset, a's outward
/// normal there).
fn facing(a: &[Vec2], b: &[Vec2], gap: f32) -> Vec<(usize, usize, Vec2, Vec2, f32, Vec2)> {
    let mut out = Vec::new();
    let (na, nb) = (a.len(), b.len());
    for i in 0..na {
        let (p, q) = (a[i], a[(i + 1) % na]);
        let len = p.distance(q);
        if len < 0.5 {
            continue;
        }
        let dir = (q - p) / len;
        let nrm = dir.perp();
        let mid = (p + q) * 0.5;
        let outward = if point_in_polygon(mid + nrm * 0.15, a) { -nrm } else { nrm };
        for j in 0..nb {
            let (r, s) = (b[j], b[(j + 1) % nb]);
            let lb = r.distance(s);
            if lb < 0.5 || ((s - r) / lb).dot(dir).abs() < 0.97 {
                continue;
            }
            let (or, os) = ((r - p).dot(outward), (s - p).dot(outward));
            if or < -SEAM_TOL || os < -SEAM_TOL || or > gap || os > gap {
                continue;
            }
            let (tr, ts) = ((r - p).dot(dir), (s - p).dot(dir));
            let (lo, hi) = (tr.min(ts).max(0.0), tr.max(ts).min(len));
            if hi - lo >= 1.0 {
                out.push((i, j, p + dir * lo, p + dir * hi, ((or + os) * 0.5).max(0.0), outward));
            }
        }
    }
    out
}

fn joint(polys: &[&[Vec2]], a: usize, b: usize) -> Option<Joint> {
    let fa = facing(polys[a], polys[b], PATH_GAP);
    let len: f32 = fa.iter().map(|f| f.2.distance(f.3)).sum();
    if len < SHARED_MIN {
        return None;
    }
    let fb = facing(polys[b], polys[a], PATH_GAP);
    let best = fa.iter().max_by(|x, y| x.2.distance(x.3).total_cmp(&y.2.distance(y.3))).map(|f| (f.2, f.3, f.4, f.5))?;
    // Edges touching (or all but): nothing is built along them.
    let close = |f: &&(usize, usize, Vec2, Vec2, f32, Vec2)| f.4 <= crate::geom::COLLINEAR_TOL;
    let mut a_edges: Vec<usize> = fa.iter().filter(close).map(|f| f.0).collect();
    let mut b_edges: Vec<usize> = fb.iter().filter(close).map(|f| f.0).collect();
    a_edges.dedup();
    b_edges.dedup();
    Some(Joint { a, b, len, best, a_side: fa.iter().map(|f| (f.2, f.3)).collect(), b_side: fb.iter().map(|f| (f.2, f.3)).collect(), a_edges, b_edges })
}

/// [parks] (D5) `plan_parks_with`, with the points of the parks left as
/// nature preserves (`World::parks`): a park any of whose lots holds one
/// is planned wild.
///
/// [r8-parks] A park is every park lot that touches another, or faces it
/// across a footpath: it is laid out as one ground. Its ways in are a few
/// street gates (not one a lot) and one gap on a spanning tree of its
/// seams, so its walks read as a park's, not a row of plots'; its
/// features are placed over the whole ground, across the seams; a big
/// park gets a car park by the street; and every lot carries the park's
/// key (`ParkLayout::cluster`), by which the city names, counts and
/// builds it.
pub fn plan_parks_wild(lots: &mut SlotMap<LotId, Lot>, land_value: &LandValue, links: &[Vec2], preserves: &[Vec2]) {
    let ids: Vec<LotId> = lots.iter().filter(|(_, l)| l.land_use == LandUse::Park && l.polygon.len() >= 3 && !l.shore.beach).map(|(id, _)| id).collect();
    for (id, l) in lots.iter_mut() {
        if !ids.contains(&id) {
            l.park = None;
        }
    }
    let n = ids.len();
    let polys: Vec<&[Vec2]> = ids.iter().map(|id| lots[*id].polygon.as_slice()).collect();
    let boxes: Vec<(Vec2, Vec2)> = polys.iter().map(|p| bounds(p)).collect();
    let mut joints: Vec<Joint> = Vec::new();
    let mut seams: Vec<Vec<(Vec2, Vec2)>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (boxes[i], boxes[j]);
            if a.0.x > b.1.x + PATH_GAP || b.0.x > a.1.x + PATH_GAP || a.0.y > b.1.y + PATH_GAP || b.0.y > a.1.y + PATH_GAP {
                continue;
            }
            if let Some(jt) = joint(&polys, i, j) {
                for s in shared_stretches(polys[i], polys[j]) {
                    seams[i].push(s);
                    seams[j].push(s);
                }
                joints.push(jt);
            }
        }
    }
    // [park-paths] Each joined path end is an extra gap of the park lot
    // nearest it.
    let mut linked: Vec<Vec<Vec2>> = vec![Vec::new(); n];
    for &p in links {
        let near = (0..n)
            .map(|i| (i, if point_in_polygon(p, polys[i]) { 0.0 } else { distance_to_boundary(p, polys[i]) }))
            .filter(|(_, d)| *d <= crate::walk::GROUNDS_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        if let Some((i, _)) = near {
            linked[i].push(p);
        }
    }
    let mut cluster: Vec<usize> = (0..n).collect();
    fn root(c: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while c[r] != r {
            r = c[r];
        }
        let mut k = i;
        while c[k] != r {
            let nx = c[k];
            c[k] = r;
            k = nx;
        }
        r
    }
    for jt in &joints {
        let (ra, rb) = (root(&mut cluster, jt.a), root(&mut cluster, jt.b));
        if ra != rb {
            cluster[ra.max(rb)] = ra.min(rb);
        }
    }
    let roots: Vec<usize> = (0..n).map(|i| root(&mut cluster, i)).collect();
    let mut done = vec![false; n];
    let mut planned: Vec<(LotId, ParkLayout)> = Vec::with_capacity(n);
    for s in 0..n {
        if done[roots[s]] {
            continue;
        }
        done[roots[s]] = true;
        let members: Vec<usize> = (0..n).filter(|&i| roots[i] == roots[s]).collect();
        let cj: Vec<&Joint> = joints.iter().filter(|j| roots[j.a] == roots[s]).collect();
        let preserve = members.iter().any(|&i| preserves.iter().any(|p| point_in_polygon(*p, polys[i])));
        let mut layouts = plan_cluster(&members, &ids, &cj, &seams, &linked, lots, land_value, preserve);
        // [parks] A nature preserve: no lawns' features, a trail, a bench
        // in three.
        if preserve {
            for l in layouts.iter_mut() {
                l.preserve = true;
                l.features.clear();
                l.nearby.clear();
                let mut k = 0;
                l.benches.retain(|_| {
                    k += 1;
                    k % 3 == 1
                });
            }
        }
        for (m, mut layout) in members.iter().zip(layouts) {
            let mut shared: Vec<usize> = Vec::new();
            let mut inner: Vec<(Vec2, Vec2)> = Vec::new();
            for jt in &cj {
                if jt.a == *m {
                    shared.extend(&jt.a_edges);
                    inner.extend(&jt.a_side);
                } else if jt.b == *m {
                    shared.extend(&jt.b_edges);
                    inner.extend(&jt.b_side);
                }
            }
            layout.shared_edges = shared;
            layout.inner_edges = inner;
            layout.seams = seams[*m].clone();
            planned.push((ids[*m], layout));
        }
    }
    drop(polys);
    for (id, layout) in planned {
        lots[id].park = Some(layout);
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
            layout.inner_edges = seams[k].clone();
            layout.cluster = Some(group[anchor]);
            lots[*id].park = Some(layout);
        }
    }
}

/// A one-lot park with no neighbours, for previews and tests.
pub fn plan_single(lot: &Lot, tier: Tier) -> ParkLayout {
    let mut lots: SlotMap<LotId, Lot> = SlotMap::with_key();
    let id = lots.insert(lot.clone());
    let mut lv = LandValue::new(1.0, 1.0);
    lv.fill(0.5);
    let mut l = plan_cluster_tier(&[0], &[id], &[], &[Vec::new()], &[Vec::new()], &lots, &lv, false, Some(tier)).remove(0);
    l.cluster = None;
    // A preview or a landmark's grounds: no visitors' car park.
    l.car_park = None;
    l
}

#[allow(clippy::too_many_arguments)]
fn plan_cluster(members: &[usize], ids: &[LotId], joints: &[&Joint], seams: &[Vec<(Vec2, Vec2)>], linked: &[Vec<Vec2>], lots: &SlotMap<LotId, Lot>, land_value: &LandValue, preserve: bool) -> Vec<ParkLayout> {
    plan_cluster_tier(members, ids, joints, seams, linked, lots, land_value, preserve, None)
}

#[allow(clippy::too_many_arguments)]
fn plan_cluster_tier(members: &[usize], ids: &[LotId], joints: &[&Joint], seams: &[Vec<(Vec2, Vec2)>], linked: &[Vec<Vec2>], lots: &SlotMap<LotId, Lot>, land_value: &LandValue, preserve: bool, tier: Option<Tier>) -> Vec<ParkLayout> {
    let lot = |m: usize| &lots[ids[members[m]]];
    let k = members.len();
    let areas: Vec<f32> = (0..k).map(|m| polygon_area(&lot(m).polygon).abs()).collect();
    let total: f32 = areas.iter().sum();
    let lv = (0..k).map(|m| land_value.at(lot(m).center) * areas[m]).sum::<f32>() / total.max(1.0);
    let tier = tier.unwrap_or_else(|| Tier::for_land_value(lv));
    // The anchor: the largest lot on a street (any lot if none is).
    let anchor = (0..k)
        .max_by(|&a, &b| {
            let on_street = |m: usize| !lot(m).landlocked;
            on_street(a).cmp(&on_street(b)).then(areas[a].total_cmp(&areas[b])).then(b.cmp(&a))
        })
        .unwrap_or(0);
    let key = ids[members[anchor]];
    let local = |i: usize| members.iter().position(|&x| x == i);
    let polys: Vec<&[Vec2]> = (0..k).map(|m| lot(m).polygon.as_slice()).collect();

    // The car park first: the gates and walks keep off it.
    let car = if total >= CAR_PARK_AREA { plan_car_park(&(0..k).map(lot).collect::<Vec<_>>(), &polys, &areas) } else { None };
    let yard = car.as_ref().map(|(_, c)| c);
    let off_yard = |p: Vec2, m: f32| yard.is_none_or(|c| c.distance(p) > m);

    // The ways between lots: a spanning tree of the seams (longest first),
    // and any long seam besides.
    let mut order: Vec<&&Joint> = joints.iter().collect();
    order.sort_by(|x, y| y.len.total_cmp(&x.len).then((x.a, x.b).cmp(&(y.a, y.b))));
    let mut uf: Vec<usize> = (0..k).collect();
    let mut gaps: Vec<Vec<Vec2>> = vec![Vec::new(); k];
    for jt in order {
        let (Some(a), Some(b)) = (local(jt.a), local(jt.b)) else { continue };
        let (mut ra, mut rb) = (a, b);
        while uf[ra] != ra {
            ra = uf[ra];
        }
        while uf[rb] != rb {
            rb = uf[rb];
        }
        if ra == rb && jt.len < 30.0 {
            continue;
        }
        uf[ra.max(rb)] = ra.min(rb);
        // The gap: on the longest facing stretch, off the car park, half
        // way across a footpath.
        let (p, q, off, out) = jt.best;
        let g = [0.5f32, 0.3, 0.7, 0.15, 0.85].iter().map(|t| p.lerp(q, *t) + out * (off * 0.5)).find(|g| off_yard(*g, 2.5)).unwrap_or(p.lerp(q, 0.5) + out * (off * 0.5));
        gaps[a].push(g);
        gaps[b].push(g);
    }

    // Street gates, a few for the park: the main lot's first, then the
    // others by size, each well along from the last.
    let mut gate_of: Vec<Option<Vec2>> = vec![None; k];
    let mut by_size: Vec<usize> = (0..k).filter(|&m| !lot(m).landlocked).collect();
    by_size.sort_by(|&a, &b| (b == anchor).cmp(&(a == anchor)).then(areas[b].total_cmp(&areas[a])).then(a.cmp(&b)));
    let mut chosen: Vec<Vec2> = Vec::new();
    for m in by_size {
        let l = lot(m);
        let (f0, f1) = (l.front_edge[0], l.front_edge[1]);
        // The middle of the frontage, or along it clear of the car park.
        let Some(at) = [0.5f32, 0.35, 0.65, 0.2, 0.8].iter().map(|t| f0.lerp(f1, *t)).find(|p| off_yard(*p, 2.5)) else { continue };
        let lonely = gaps[m].is_empty() && linked[members[m]].is_empty();
        if !lonely && chosen.iter().any(|c| c.distance(at) < GATE_SPACING) {
            continue;
        }
        let g = at - l.front_dir * 0.8;
        let g = if point_in_polygon(g, &l.polygon) { g } else { pull_inside(&l.polygon, at, l.center) };
        chosen.push(at);
        gate_of[m] = Some(g);
    }

    let mut layouts: Vec<ParkLayout> = (0..k)
        .map(|m| {
            let mut entrances: Vec<Vec2> = gate_of[m].into_iter().collect();
            let l = lot(m);
            for g in gaps[m].iter().chain(&linked[members[m]]) {
                let p = entrance_for(l, *g, linked[members[m]].contains(g));
                if entrances.iter().all(|e| e.distance(p) > 4.0) {
                    entrances.push(p);
                }
            }
            let mut layout = plan_lot_paths(l, entrances, &linked[members[m]], yard);
            layout.tier = tier;
            layout
        })
        .collect();

    // Features over the park's whole ground, across the seams.
    if !preserve {
        let open: Vec<(Vec2, Vec2)> = (0..k).flat_map(|m| open_stretches(polys[m], &seams[members[m]])).collect();
        let paths: Vec<(Vec2, Vec2)> = layouts.iter().flat_map(|l| l.paths.iter().flat_map(|p| p.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>())).collect();
        let entrances: Vec<Vec2> = layouts.iter().flat_map(|l| l.entrances.iter().copied()).collect();
        let ground = Ground::new(&polys, total, &open, &paths, &entrances, yard);
        let biggest = ground.pts.iter().map(|g| g.2).fold(0.0, f32::max);
        let mut taken: Vec<(Vec2, f32)> = Vec::new();
        let all_paths: Vec<Vec<Vec2>> = layouts.iter().flat_map(|l| l.paths.iter().cloned()).collect();
        for kind in feature_budget(tier, total, biggest) {
            let Some((p, m, r)) = ground.spot(kind, tier, &taken) else { continue };
            taken.push((p, r));
            let heading = heading_to_path(p, &all_paths);
            layouts[m].features.push(ParkFeatureSlot { kind, pos: p, radius: r, heading });
            for (o, l) in layouts.iter_mut().enumerate() {
                if o != m && (point_in_polygon(p, polys[o]) || distance_to_boundary(p, polys[o]) < r + 0.5) {
                    l.nearby.push((p, r));
                }
            }
        }
    }
    for (m, l) in layouts.iter_mut().enumerate() {
        if let Some((owner, c)) = &car {
            if *owner == m {
                l.car_park = Some(c.clone());
            } else if yard_reaches(&c.outline, polys[m]) {
                l.car_park = Some(ParkCarPark { spaces: Vec::new(), own: false, ..c.clone() });
            }
        }
        finish_lot(lot(m), l);
        l.cluster_lots = k as u32;
        l.cluster_area = total;
        l.anchor = m == anchor;
        l.cluster = Some(key);
    }
    layouts
}

/// [r8-parks] Does the yard reach over a lot (any of it more than a
/// hair inside)?
fn yard_reaches(outline: &[Vec2; 4], poly: &[Vec2]) -> bool {
    let (a, u, v) = (outline[0], outline[1] - outline[0], outline[3] - outline[0]);
    (0..=8).any(|i| (0..=4).any(|j| {
        let p = a + u * (0.02 + 0.96 * i as f32 / 8.0) + v * (0.02 + 0.96 * j as f32 / 4.0);
        point_in_polygon(p, poly) && distance_to_boundary(p, poly) > 0.3
    })) || poly.iter().any(|p| point_in_polygon(*p, outline))
}

/// Where a way in at `g` (a gap on a seam, or a joined path's end) comes
/// into lot `l`: a step inside it.
fn entrance_for(l: &Lot, g: Vec2, link: bool) -> Vec2 {
    let poly = &l.polygon;
    let mut inward = (l.center - g).normalize_or_zero();
    // [sweep-visual] A joined path's end outside the edge comes in square
    // to the edge, straight on from the path.
    if link && !point_in_polygon(g, poly) {
        let n = poly.len();
        let q = (0..n).map(|i| crate::geom::project_on_segment(g, poly[i], poly[(i + 1) % n]).1).min_by(|a, b| a.distance(g).total_cmp(&b.distance(g)));
        if let Some(q) = q.filter(|q| q.distance(g) > 0.05) {
            inward = (q - g).normalize_or_zero();
            let p = q + inward * 0.8;
            if point_in_polygon(p, poly) {
                return p;
            }
        }
    }
    // [r8-parks] Right at the edge, square in from the nearest point of
    // it: the walks of two lots of a park meet across the seam (or the
    // footpath) with no lawn between their ends.
    let n = poly.len();
    if let Some((i, q)) = (0..n).map(|i| (i, crate::geom::project_on_segment(g, poly[i], poly[(i + 1) % n]).1)).min_by(|a, b| a.1.distance(g).total_cmp(&b.1.distance(g))) {
        let d = (poly[(i + 1) % n] - poly[i]).normalize_or_zero().perp();
        for inward in [d, -d] {
            let p = q + inward * 0.15;
            if point_in_polygon(p, poly) && point_in_polygon(q + inward * 1.0, poly) {
                return p;
            }
        }
    }
    let p = g + inward * 0.8;
    if point_in_polygon(p, poly) { p } else { pull_inside(poly, g, l.center) }
}

/// [r8-parks] The park's ground as a grid of points, each with how big a
/// feature may stand there: clear of the park's outside edge (not its
/// seams: features run over them), its walks, its ways in and its car park.
struct Ground {
    /// Point, the lot (index in the park) it is on, room.
    pts: Vec<(Vec2, usize, f32)>,
}

impl Ground {
    fn new(polys: &[&[Vec2]], total: f32, open: &[(Vec2, Vec2)], paths: &[(Vec2, Vec2)], entrances: &[Vec2], yard: Option<&ParkCarPark>) -> Self {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let boxes: Vec<(Vec2, Vec2)> = polys.iter().map(|p| bounds(p)).collect();
        for (a, b) in &boxes {
            lo = lo.min(*a);
            hi = hi.max(*b);
        }
        // About four thousand points at most, a metre apart at best.
        let step = (total / 4000.0).sqrt().clamp(1.0, 4.0);
        let mut pts = Vec::new();
        let mut v = lo.y + step * 0.5;
        while v < hi.y {
            let mut u = lo.x + step * 0.5;
            while u < hi.x {
                let p = Vec2::new(u, v);
                u += step;
                let Some(m) = (0..polys.len()).find(|&m| p.cmpge(boxes[m].0).all() && p.cmple(boxes[m].1).all() && point_in_polygon(p, polys[m])) else { continue };
                let mut room = f32::MAX;
                for (a, b) in open {
                    room = room.min(p.distance(project(p, *a, *b).1) - 1.0);
                }
                for (a, b) in paths {
                    room = room.min(p.distance(project(p, *a, *b).1) - 2.4);
                }
                for e in entrances {
                    room = room.min(p.distance(*e) - 3.0);
                }
                if let Some(c) = yard {
                    room = room.min(c.distance(p) - 2.0);
                }
                if room >= 1.8 {
                    pts.push((p, m, room));
                }
            }
            v += step;
        }
        Ground { pts }
    }

    /// The best spot for a feature: as big as it may be (down to its
    /// least), then as far from the others as can be.
    fn spot(&self, kind: ParkFeature, tier: Tier, taken: &[(Vec2, f32)]) -> Option<(Vec2, usize, f32)> {
        let scale = if tier == Tier::Low { 0.9 } else { 1.0 };
        let (rmax, rmin) = (kind.radius() * scale, kind.min_radius() * scale);
        let rmax = if kind == ParkFeature::Lake { 16.0 } else { rmax };
        let mut best: Option<(f32, Vec2, usize, f32)> = None;
        for &(p, m, room) in &self.pts {
            if room < rmin {
                continue;
            }
            let mut r = rmax.min(room);
            for (q, qr) in taken {
                r = r.min(p.distance(*q) - qr - 1.5);
            }
            if r < rmin {
                continue;
            }
            let spread = taken.iter().map(|(q, qr)| p.distance(*q) - qr - r).fold(12.0f32, f32::min);
            let score = r * 4.0 + (room - r).min(4.0) + spread * 0.6;
            if best.is_none_or(|b| score > b.0 + 1e-4) {
                best = Some((score, p, m, r));
            }
        }
        best.map(|(_, p, m, r)| (p, m, r))
    }
}

/// [r8-parks] A car park for a park this big: a yard of two rows of bays
/// along a street frontage of its lots (it may run on over the seams into
/// the next lot of the park), on the lot that holds its middle.
fn plan_car_park(lots: &[&Lot], polys: &[&[Vec2]], areas: &[f32]) -> Option<(usize, ParkCarPark)> {
    let k = lots.len();
    let inside = |p: Vec2| polys.iter().any(|poly| point_in_polygon(p, poly) || distance_to_boundary(p, poly) < 0.35);
    let mut order: Vec<usize> = (0..k).filter(|&m| lots[m].car_access() && !lots[m].landlocked).collect();
    order.sort_by(|&a, &b| areas[b].total_cmp(&areas[a]).then(a.cmp(&b)));
    for w in [36.0f32, 30.0, 24.0, 19.0, YARD_MIN] {
        for &m in &order {
            let l = lots[m];
            let (f0, f1) = (l.front_edge[0], l.front_edge[1]);
            let len = f0.distance(f1);
            if len < 4.0 {
                continue;
            }
            let along = (f1 - f0) / len;
            let mut inward = -l.front_dir.normalize_or_zero();
            if !point_in_polygon(f0.lerp(f1, 0.5) + inward * 2.0, &l.polygon) {
                inward = -inward;
            }
            // Slide it along the frontage, the yard's middle on this lot;
            // nearest the frontage's middle first.
            let mut starts: Vec<f32> = Vec::new();
            let mut t = -w + 1.0;
            while t <= len - 1.0 {
                starts.push(t);
                t += 2.5;
            }
            starts.sort_by(|a, b| (a + w * 0.5 - len * 0.5).abs().total_cmp(&(b + w * 0.5 - len * 0.5).abs()));
            for t in starts {
                let a = f0 + along * t + inward * 0.8;
                let outline = [a, a + along * w, a + along * w + inward * YARD_DEPTH, a + inward * YARD_DEPTH];
                if !point_in_polygon(polygon_centroid(&outline), &l.polygon) {
                    continue;
                }
                let mut ok = true;
                let (nu, nv) = ((w / 1.5).ceil() as i32, (YARD_DEPTH / 1.5).ceil() as i32);
                'grid: for i in 0..=nu {
                    for j in 0..=nv {
                        let p = a + along * (0.3 + (w - 0.6) * i as f32 / nu as f32) + inward * (0.3 + (YARD_DEPTH - 0.6) * j as f32 / nv as f32);
                        if !inside(p) {
                            ok = false;
                            break 'grid;
                        }
                    }
                }
                // Room behind it for the park.
                let behind = a + along * (w * 0.5) + inward * (YARD_DEPTH + 4.0);
                if !ok || !inside(behind) {
                    continue;
                }
                // Two rows of bays across an aisle; the drive in at the end
                // of the street-side row.
                let n = ((w - 2.0) / crate::parking_layout::BAY_W).floor() as i32;
                let drive = w - 4.0;
                let mut spaces = Vec::new();
                for row in 0..2 {
                    let v = if row == 0 { 3.0 } else { 14.0 };
                    let face = if row == 0 { -inward } else { inward };
                    for b in 0..n {
                        let u = 1.0 + crate::parking_layout::BAY_W * (b as f32 + 0.5);
                        if row == 0 && (u - drive).abs() < 3.4 {
                            continue;
                        }
                        spaces.push((a + along * u + inward * v, face.to_angle()));
                    }
                }
                let entry = a + along * drive - inward * 0.8;
                return Some((m, ParkCarPark { outline, spaces, entry, own: true }));
            }
        }
    }
    None
}

/// Which features a park of this size on this land deserves, largest first.
/// `room`: the most room any spot of it has (a lake needs a wide one).
fn feature_budget(tier: Tier, area: f32, room: f32) -> Vec<ParkFeature> {
    use ParkFeature::*;
    let mut v = Vec::new();
    match tier {
        Tier::Low => {
            // Cheap land: dirt, courts and a pitch, a hall the
            // neighbourhood runs, plots people grow food in.
            if area >= 4500.0 {
                v.push(SportsPitch);
            }
            if area >= 5000.0 {
                v.push(CommunityCentre);
            }
            if area >= 900.0 {
                v.push(Court);
            }
            if area >= 3500.0 {
                v.push(Allotments);
            }
            if area >= 3200.0 {
                v.push(Skatepark);
            }
            if area >= 6000.0 {
                v.push(Court);
            }
            if area >= 2500.0 {
                v.push(Toilets);
            }
        }
        Tier::Mid => {
            if area >= 5000.0 {
                v.push(SportsPitch);
            }
            if area >= 4500.0 {
                v.push(CommunityCentre);
            }
            if area >= 4000.0 {
                v.push(DogRun);
            }
            if area >= 3500.0 {
                v.push(Court);
            }
            if area >= 6000.0 {
                v.push(Allotments);
            }
            let playgrounds = ((area / 2500.0).floor() as usize).clamp(usize::from(area >= 450.0), 4);
            for _ in 0..playgrounds {
                v.push(Playground);
            }
            if area >= 1500.0 {
                v.push(Pond);
            }
            if area >= 3000.0 {
                v.push(Cafe);
            } else if area >= 1200.0 {
                v.push(Kiosk);
            }
            if area >= 2500.0 {
                v.push(Toilets);
            }
            let beds = ((area / 1500.0).floor() as usize).min(4);
            for _ in 0..beds {
                v.push(FlowerBed);
            }
        }
        Tier::High => {
            if area >= 2500.0 && room >= 9.0 {
                v.push(Lake);
            } else if area >= 900.0 {
                v.push(Pond);
            }
            if area >= 7000.0 && room >= 8.5 {
                v.push(FerrisWheel);
            }
            if area >= 8000.0 {
                v.push(SportsPitch);
            }
            if area >= 5000.0 {
                v.push(CommunityCentre);
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
            if area >= 2500.0 {
                v.push(Cafe);
            }
            if area >= 500.0 {
                v.push(Fountain);
            }
            if (2000.0..2500.0).contains(&area) || area >= 9000.0 {
                v.push(Kiosk);
            }
            if area >= 2000.0 {
                v.push(Toilets);
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
    // Biggest first (a stable sort keeps the order above among equals).
    v.sort_by(|a, b| b.radius().total_cmp(&a.radius()));
    v
}

/// Spokes from a hub to every entrance (the first, a street gate if it
/// has one), plus a loop on big lots with only one way in. The hub is
/// where the ways in meet best (the middle of two: a walk straight
/// through), else the point deepest in the lot; never on the car park.
/// `links` (some of the entrances) are the ends of player paths joined to
/// the park: their spokes start out at the path's end itself, so the
/// walkway is paved right up to the path rather than stopping inside the
/// edge.
fn plan_lot_paths(lot: &Lot, mut entrances: Vec<Vec2>, links: &[Vec2], yard: Option<&ParkCarPark>) -> ParkLayout {
    let poly = &lot.polygon;
    let clear = |p: Vec2| point_in_polygon(p, poly) && distance_to_boundary(p, poly) >= 1.5 && yard.is_none_or(|c| c.distance(p) > 2.5);
    let mut hub = None;
    if entrances.len() >= 2 {
        // Weiszfeld's median of the ways in.
        let mut h = entrances.iter().copied().sum::<Vec2>() / entrances.len() as f32;
        for _ in 0..12 {
            let (mut num, mut den) = (Vec2::ZERO, 0.0f32);
            for e in &entrances {
                let d = e.distance(h).max(0.5);
                num += *e / d;
                den += 1.0 / d;
            }
            h = num / den;
        }
        if clear(h) && entrances.iter().all(|e| e.distance(h) > 2.0) {
            hub = Some(h);
        }
    }
    let hub = hub.unwrap_or_else(|| hub_point(poly, &entrances, yard));
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
        if [a, out, b].iter().all(|p| yard.is_none_or(|c| c.distance(*p) > 2.0)) {
            paths.push(vec![hub, a, out, b, hub]);
        }
    }
    ParkLayout { paths, ..ParkLayout::bare(Tier::Mid, entrances, polygon_area(poly).abs()) }
}

/// Spurs to the features, bends around them (and the park's features from
/// the next lots reaching over this one), then benches along every path
/// and around the water.
fn finish_lot(lot: &Lot, l: &mut ParkLayout) {
    let poly = &lot.polygon;
    let hub = l.paths.first().and_then(|p| p.last().copied()).unwrap_or(lot.center);
    let features = l.features.clone();
    let mut keep_off: Vec<(Vec2, f32)> = features.iter().map(|f| (f.pos, f.radius)).chain(l.nearby.iter().copied()).collect();
    keep_off.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)));
    let yard = l.car_park.clone();
    // Spur from the hub to the edge of each feature that is not a lawn.
    for f in &features {
        if matches!(f.kind, ParkFeature::Lawn | ParkFeature::FlowerBed) {
            continue;
        }
        let d = (f.pos - hub).normalize_or_zero();
        let end = f.pos - d * (f.radius + 0.8);
        if end.distance(hub) > 3.0 && point_in_polygon(end, poly) {
            l.paths.push(vec![hub, end]);
        }
    }
    // Bend every straight leg around features it would cut through.
    for path in l.paths.iter_mut() {
        let mut out: Vec<Vec2> = Vec::with_capacity(path.len() * 2);
        for w in path.windows(2) {
            let (a, b) = (w[0], w[1]);
            out.push(a);
            for (fp, fr) in &keep_off {
                let (t, q) = project(*fp, a, b);
                if t <= 0.05 || t >= 0.95 {
                    continue;
                }
                let d = fp.distance(q);
                if d < fr + 1.2 && a.distance(*fp) > fr + 1.0 && b.distance(*fp) > fr + 1.0 {
                    let away = if d > 0.1 { (q - *fp) / d } else { (b - a).perp().normalize_or_zero() };
                    let bend = *fp + away * (fr + 1.8);
                    out.push(pull_inside(poly, bend, hub));
                }
            }
        }
        if let Some(last) = path.last() {
            out.push(*last);
        }
        *path = out;
    }
    let free = |p: Vec2, m: f32| keep_off.iter().all(|(q, r)| q.distance(p) > r + m) && yard.as_ref().is_none_or(|c| c.distance(p) > 1.0);
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
                if point_in_polygon(p, poly) && distance_to_boundary(p, poly) > 0.9 && free(p, 0.8) && benches.iter().all(|(q, _)| q.distance(p) > 3.0) {
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
            if point_in_polygon(p, poly) && distance_to_boundary(p, poly) > 0.9 && free(p, 1.0) && benches.iter().all(|(q, _)| q.distance(p) > 2.5) {
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

/// The point of the lot furthest from its edges that is not an entrance
/// (nor on the park's car park).
fn hub_point(poly: &[Vec2], entrances: &[Vec2], yard: Option<&ParkCarPark>) -> Vec2 {
    let (lo, hi) = bounds(poly);
    let step = ((hi - lo).max_element() / 18.0).clamp(1.0, 3.0);
    let mut best = (f32::MIN, polygon_centroid(poly));
    let mut v = lo.y + step * 0.5;
    while v < hi.y {
        let mut u = lo.x + step * 0.5;
        while u < hi.x {
            let p = Vec2::new(u, v);
            u += step;
            if !point_in_polygon(p, poly) || yard.is_some_and(|c| c.distance(p) < 2.5) {
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
            // [r8-parks] A feature stands on the park's ground (it may run
            // over a seam into the next lot of it, never off the park).
            for f in &l.features {
                for k in 0..12 {
                    let q = f.pos + Vec2::from_angle(k as f32 / 12.0 * std::f32::consts::TAU) * f.radius;
                    assert!(lots.values().filter(|x| x.park.is_some()).any(|x| point_in_polygon(q, &x.polygon) || distance_to_boundary(q, &x.polygon) < 0.2), "{:?} r {} at {:?} fits on the park: {q:?} off it", f.kind, f.radius, f.pos);
                }
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
        // [r8-parks] Cheap land: courts, a pitch, a hall, toilets, plots;
        // no water, flowers or rides.
        assert!(pk.iter().all(|k| matches!(k, ParkFeature::Court | ParkFeature::Skatepark | ParkFeature::SportsPitch | ParkFeature::CommunityCentre | ParkFeature::Toilets | ParkFeature::Allotments)), "poor parks are bare: {pk:?}");
        assert!(rk.contains(&ParkFeature::Fountain) && (rk.contains(&ParkFeature::Lake) || rk.contains(&ParkFeature::Pond)), "rich parks have water and a fountain: {rk:?}");
        let poor_benches: usize = poor.values().filter_map(|l| l.park.as_ref()).map(|p| p.benches.len()).sum();
        let rich_benches: usize = rich.values().filter_map(|l| l.park.as_ref()).map(|p| p.benches.len()).sum();
        assert!(rich_benches > poor_benches && poor_benches > 0);
    }

    /// [r8-parks] A block of narrow lots, square to the street, with a
    /// strip of backland behind them: Harvey's Lotus Greenway.
    fn strip_block(n: usize, w: f32, depth: f32) -> (SlotMap<LotId, Lot>, Vec<LotId>, LotId) {
        let mut lots: SlotMap<LotId, Lot> = SlotMap::with_key();
        let mk = |poly: Vec<Vec2>, landlocked: bool| {
            let (lo, hi) = bounds(&poly);
            let corners = [Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y), Vec2::new(hi.x, hi.y), Vec2::new(lo.x, hi.y)];
            Lot {
                zone: Default::default(),
                land_use: LandUse::Park,
                center: polygon_centroid(&poly),
                polygon: poly,
                corners,
                front_edge: [corners[0], corners[1]],
                front_dir: Vec2::new(0.0, -1.0),
                front_segment: crate::roads::SegmentId(0),
                front_t: 0.5,
                access_node: crate::roads::NodeId(0),
                building: None,
                landlocked,
                footfall_today: 0,
                footfall_hour: 0,
                car_segment: (!landlocked).then_some(crate::roads::SegmentId(0)),
                car_point: (corners[0] + corners[1]) * 0.5 - Vec2::Y * 2.0,
                park: None,
                shore: Default::default(),
            }
        };
        let strips: Vec<LotId> = (0..n)
            .map(|i| {
                let x = i as f32 * w;
                lots.insert(mk(vec![Vec2::new(x, 0.0), Vec2::new(x + w, 0.0), Vec2::new(x + w, depth), Vec2::new(x, depth)], false))
            })
            .collect();
        let back = lots.insert(mk(vec![Vec2::new(0.0, depth), Vec2::new(n as f32 * w, depth), Vec2::new(n as f32 * w, depth + 7.0), Vec2::new(0.0, depth + 7.0)], true));
        (lots, strips, back)
    }

    fn on_park(lots: &SlotMap<LotId, Lot>, q: Vec2) -> bool {
        lots.values().any(|l| point_in_polygon(q, &l.polygon) || distance_to_boundary(q, &l.polygon) < 0.2)
    }

    #[test]
    fn a_strip_of_narrow_lots_is_one_park_with_features_across_its_seams() {
        for value in [0.5f32, 0.85] {
            let (mut lots, strips, back) = strip_block(14, 13.0, 40.0);
            let mut lv = LandValue::new(400.0, 32.0);
            lv.fill(value);
            plan_parks(&mut lots, &lv);
            let keys: Vec<Option<LotId>> = lots.values().map(|l| l.park.as_ref().and_then(|p| p.cluster)).collect();
            assert!(keys[0].is_some() && keys.iter().all(|k| *k == keys[0]), "one park, one key: {keys:?}");
            assert!(lots[back].park.as_ref().is_some_and(|p| p.cluster_lots == 15), "the backland is part of it");
            let all: Vec<&ParkFeatureSlot> = lots.values().flat_map(|l| l.park.as_ref().unwrap().features.iter()).collect();
            assert!(all.len() >= 8, "a 7,800 m2 park has features ({value}): {:?}", all.iter().map(|f| f.kind).collect::<Vec<_>>());
            // Placed on the whole ground: some across a seam (no 13 m lot
            // holds a feature of 6.5 m or more), every one on the park.
            assert!(all.iter().any(|f| f.radius >= 6.0), "big features fit on the strip");
            let across = lots.values().any(|l| l.park.as_ref().unwrap().features.iter().any(|f| distance_to_boundary(f.pos, &l.polygon) < f.radius - 0.1));
            assert!(across, "a feature runs over a seam");
            for f in &all {
                for k in 0..16 {
                    let q = f.pos + Vec2::from_angle(k as f32 / 16.0 * std::f32::consts::TAU) * f.radius;
                    assert!(on_park(&lots, q), "{:?} stays on the park", f.kind);
                }
                for g in &all {
                    if !std::ptr::eq(*f, *g) {
                        assert!(f.pos.distance(g.pos) >= f.radius + g.radius + 1.0, "{:?} and {:?} apart", f.kind, g.kind);
                    }
                }
            }
            // The next lots know of a feature reaching over them.
            assert!(lots.values().any(|l| !l.park.as_ref().unwrap().nearby.is_empty()));
            // A few gates for the park, not one a lot.
            let gates = strips.iter().filter(|id| lots[**id].park.as_ref().unwrap().entrances.iter().any(|e| e.y < 1.5)).count();
            assert!((2..=8).contains(&gates), "a few street gates: {gates}");
            // Its car park: once, by the street, on the park, clear of the features.
            let own: Vec<&ParkCarPark> = lots.values().filter_map(|l| l.park.as_ref().unwrap().car_park.as_ref()).filter(|c| c.own).collect();
            assert_eq!(own.len(), 1, "one car park");
            let c = own[0];
            assert!(c.spaces.len() >= 12, "{} bays", c.spaces.len());
            assert!(c.outline.iter().all(|p| on_park(&lots, *p)) && c.outline.iter().any(|p| p.y < 2.0), "by the street, on the park");
            for f in &all {
                assert!(c.distance(f.pos) > f.radius, "{:?} off the car park", f.kind);
            }
            for (p, _) in &c.spaces {
                assert!(c.distance(*p) == 0.0);
            }
        }
    }

    #[test]
    fn a_park_has_no_fences_inside_it() {
        // The seams of a strip of lots, and a footpath through a park.
        let (mut lots, strips, _) = strip_block(6, 13.0, 40.0);
        let mut lv = LandValue::new(400.0, 32.0);
        lv.fill(0.5);
        plan_parks(&mut lots, &lv);
        for (i, id) in strips.iter().enumerate() {
            let l = &lots[*id];
            let outer = l.park.as_ref().unwrap().outer_stretches(&l.polygon);
            let x0 = i as f32 * 13.0;
            let on = |p: Vec2| outer.iter().any(|(a, b)| project(p, *a, *b).1.distance(p) < 0.3);
            assert!(on(Vec2::new(x0 + 6.5, 0.0)), "the street side is the outside");
            assert!(!on(Vec2::new(x0 + 6.5, 40.0)), "no fence along the backland");
            if i > 0 {
                assert!(!on(Vec2::new(x0, 20.0)), "no fence down a seam");
            }
            if i + 1 < strips.len() {
                assert!(!on(Vec2::new(x0 + 13.0, 20.0)), "no fence down a seam");
            } else {
                assert!(on(Vec2::new(x0 + 13.0, 20.0)), "the end of the park is fenced");
            }
        }
        let (mut lots, lv) = park_lots(0.5);
        plan_parks(&mut lots, &lv);
        let parks: Vec<&Lot> = lots.values().filter(|l| l.usable() && l.park.as_ref().is_some_and(|p| p.cluster_lots >= 2)).collect();
        assert!(parks.len() >= 2);
        for l in &parks {
            let p = l.park.as_ref().unwrap();
            // The edge facing the path through the park (x = 0) is inside it.
            let facing: f32 = p.inner_edges.iter().filter(|(a, b)| a.x.abs() < 6.0 && b.x.abs() < 6.0).map(|(a, b)| a.distance(*b)).sum();
            assert!(facing > 10.0, "the lot faces the rest of the park across the path: {:?}", p.inner_edges);
            let outer = p.outer_stretches(&l.polygon);
            assert!(outer.iter().all(|(a, b)| !(a.x.abs() < 6.0 && b.x.abs() < 6.0 && a.distance(*b) > 2.0)), "no fence along the footpath: {outer:?}");
        }
    }

    #[test]
    fn a_small_park_gets_no_car_park_and_a_single_lot_keeps_its_gate() {
        let (mut lots, strips, _) = strip_block(2, 13.0, 30.0);
        let mut lv = LandValue::new(400.0, 32.0);
        lv.fill(0.5);
        plan_parks(&mut lots, &lv);
        assert!(lots.values().all(|l| l.park.as_ref().unwrap().car_park.is_none()), "a small park: no car park");
        let l = &lots[strips[0]];
        let single = plan_single(l, Tier::Mid);
        assert!(single.entrances[0].y < 1.5, "its gate on the street");
        assert!(single.car_park.is_none() && single.cluster.is_none());
    }
}
