//! Street names. The road network is rebuilt from the plan whenever the
//! player implements a project, so names cannot live on segments: they
//! would be lost (or shuffled) every time. Instead the network is cut into
//! **streets**, chains of segments that carry straight on through their
//! junctions, and each street keeps its name across rebuilds by matching
//! the new chains against the outlines of the streets named before. Only a
//! chain that matches nothing is given a new name, drawn deterministically
//! from the city seed and a running serial, so a replay of the same plan
//! names the same streets the same way.
//!
//! Saved with the world (`World::streets`, serde default: a save from before
//! names gets them on load). The per-segment lookup is derived, not saved.

use crate::roads::{RoadKind, RoadNetwork, SegmentId};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// What sort of way a street is, for its suffix and how the map draws it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreetClass {
    /// A two-lane town street.
    #[default]
    Street,
    /// Four or more lanes, or two each way: a main road.
    Avenue,
    /// A highway carriageway or slip road.
    Highway,
    /// A roundabout's ring.
    Ring,
}

/// One named street: its name and the polyline it was named for.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Street {
    pub name: String,
    #[serde(default)]
    pub class: StreetClass,
    /// The chain's centreline, node to node, in order. New chains are
    /// matched against this to keep the name.
    pub points: Vec<Vec2>,
    /// The segments of the chain in order (derived on every naming pass).
    #[serde(skip)]
    pub segments: Vec<SegmentId>,
}

impl Street {
    pub fn length(&self) -> f32 {
        self.points.windows(2).map(|w| w[0].distance(w[1])).sum()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StreetNames {
    pub streets: Vec<Street>,
    /// Names handed out so far: the seed of the next one.
    pub serial: u64,
    /// `streets` index of every segment (by `SegmentId`), `None` for paths,
    /// track and dead segments.
    #[serde(skip)]
    pub of_segment: Vec<Option<u32>>,
    /// The roads version the lookup was built for.
    #[serde(skip)]
    pub roads_version: u64,
}

/// Through a junction a street carries on into the way that continues it
/// most nearly straight, if the turn is gentler than this (radians). A bend
/// between two pieces of one drawn road (a node of degree two) may be
/// sharper before the name changes.
const STRAIGHT_ON: f32 = 0.62;
const BEND: f32 = 1.05;
/// A new chain keeps an old street's name when this share of it lies within
/// [`MATCH_DIST`] of the old outline.
const MATCH_SHARE: f32 = 0.5;
const MATCH_DIST: f32 = 12.0;
/// Two highway carriageways this close run as one road and share a name.
const CARRIAGEWAY_DIST: f32 = 45.0;

const STEMS: &[&str] = &[
    "Neon", "Kowloon", "Lotus", "Harbor", "Ember", "Cinder", "Jade", "Onyx", "Mercury", "Aurora", "Cobalt", "Saffron", "Marrow", "Umbra", "Vellane", "Kessler",
    "Okafor", "Volkov", "Tanaka", "Reyes", "Lindqvist", "Nakamura", "Diallo", "Castellano", "Marsh", "Osei", "Haddad", "Nguyen", "Fontaine", "Achebe", "Moreau", "Varga",
    "Delacroix", "Baptiste", "Ferreira", "Chrome", "Signal", "Static", "Velvet", "Copper", "Lantern", "Monsoon", "Silk", "Paper", "Glass", "Iron", "Mill", "Canal",
    "Market", "Foundry", "Station", "Ferry", "Orchard", "Quarry", "Tannery", "Cable", "Relay", "Pylon", "Circuit", "Arcade", "Bazaar", "Pagoda", "Temple", "Garden",
    "Union", "Liberty", "Juniper", "Willow", "Cedar", "Maple", "Birch", "Elm", "Hawthorn", "Rowan", "Larch", "Alder", "Sable", "Vesper", "Halcyon", "Zenith",
];
const STREET_SUFFIX: &[&str] = &["Street", "Street", "Street", "Road", "Lane", "Way", "Row", "Walk", "Drive", "Place", "Alley", "Terrace"];
const AVENUE_SUFFIX: &[&str] = &["Avenue", "Avenue", "Boulevard", "Parade", "Broadway", "Esplanade"];
const HIGHWAY_FORMS: &[&str] = &["Expressway", "Freeway", "Skyway", "Parkway", "Highway"];

use crate::hash::mix;

/// The `serial`th name drawn for a street of `class` in a city of `seed`.
pub fn generate_name(seed: u64, serial: u64, class: StreetClass) -> String {
    let h = mix(seed, serial);
    let stem = STEMS[(h % STEMS.len() as u64) as usize];
    let pick = |list: &[&'static str]| list[((h >> 23) % list.len() as u64) as usize];
    match class {
        StreetClass::Street => format!("{stem} {}", pick(STREET_SUFFIX)),
        StreetClass::Avenue => format!("{stem} {}", pick(AVENUE_SUFFIX)),
        StreetClass::Ring => format!("{stem} Circus"),
        StreetClass::Highway => {
            if (h >> 40) % 3 == 0 {
                format!("Route {}", 1 + (h >> 44) % 99)
            } else {
                format!("{stem} {}", pick(HIGHWAY_FORMS))
            }
        }
    }
}

fn class_of(roads: &RoadNetwork, sid: SegmentId) -> StreetClass {
    let s = roads.segment(sid);
    if s.ring {
        StreetClass::Ring
    } else if s.is_highway() || s.ramp {
        StreetClass::Highway
    } else if s.lanes() >= 4 || (s.lanes_fwd >= 2 && s.lanes_bwd >= 2) {
        StreetClass::Avenue
    } else {
        StreetClass::Street
    }
}

/// Classes that may continue one another through a junction: a ring only
/// into ring, a highway only into highway; streets and avenues mix (a
/// street that widens keeps its name).
fn compatible(a: StreetClass, b: StreetClass) -> bool {
    match (a, b) {
        (StreetClass::Ring, StreetClass::Ring) | (StreetClass::Highway, StreetClass::Highway) => true,
        (StreetClass::Street | StreetClass::Avenue, StreetClass::Street | StreetClass::Avenue) => true,
        _ => false,
    }
}

fn named(roads: &RoadNetwork, sid: SegmentId) -> bool {
    let s = roads.segment(sid);
    s.alive && s.kind == RoadKind::Street
}

/// Cut the network into chains: at every node, pair the ways that carry
/// straight on through it (gentlest turn first, each way once), then follow
/// the pairs. Returns each chain's segments in order with its node polyline.
pub fn chains(roads: &RoadNetwork) -> Vec<(Vec<SegmentId>, Vec<Vec2>)> {
    let n_seg = roads.segments.len();
    // next[s][end]: the segment continuing s through its end (0 = a, 1 = b).
    let mut next: Vec<[Option<SegmentId>; 2]> = vec![[None, None]; n_seg];
    for (ni, node) in roads.nodes.iter().enumerate() {
        let here: Vec<SegmentId> = node.segments.iter().copied().filter(|s| named(roads, *s)).collect();
        if here.len() < 2 {
            continue;
        }
        // Direction out of the node along each way.
        let out = |sid: SegmentId| {
            let s = roads.segment(sid);
            let other = if s.a.0 as usize == ni { s.b } else { s.a };
            (roads.node(other).pos - node.pos).normalize_or_zero()
        };
        let limit = if here.len() == 2 { BEND } else { STRAIGHT_ON };
        let mut pairs: Vec<(f32, SegmentId, SegmentId)> = Vec::new();
        for i in 0..here.len() {
            for j in (i + 1)..here.len() {
                let (si, sj) = (here[i], here[j]);
                if !compatible(class_of(roads, si), class_of(roads, sj)) {
                    continue;
                }
                // Straight on: the two ways leave the node in opposite directions.
                let turn = (-out(si)).angle_to(out(sj)).abs();
                if turn <= limit {
                    pairs.push((turn, si, sj));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut used: Vec<SegmentId> = Vec::new();
        for (_, si, sj) in pairs {
            if used.contains(&si) || used.contains(&sj) {
                continue;
            }
            used.push(si);
            used.push(sj);
            let end = |sid: SegmentId| usize::from(roads.segment(sid).a.0 as usize != ni);
            next[si.0 as usize][end(si)] = Some(sj);
            next[sj.0 as usize][end(sj)] = Some(si);
        }
    }
    let mut seen = vec![false; n_seg];
    let mut out = Vec::new();
    for start in 0..n_seg {
        let sid = SegmentId(start as u32);
        if seen[start] || !named(roads, sid) {
            continue;
        }
        // Walk back to one end of the chain (or round a loop to the start).
        let (mut cur, mut from_end) = (sid, 0usize);
        loop {
            let Some(prev) = next[cur.0 as usize][from_end] else { break };
            if prev == sid {
                break;
            }
            // Leave `prev` through the end away from `cur`.
            let joint = if from_end == 0 { roads.segment(cur).a } else { roads.segment(cur).b };
            from_end = usize::from(roads.segment(prev).a == joint);
            cur = prev;
        }
        // Now walk forward from `cur`, entering through `from_end`.
        let first = cur;
        let mut segs = Vec::new();
        let mut pts = Vec::new();
        let mut enter = from_end;
        loop {
            seen[cur.0 as usize] = true;
            segs.push(cur);
            let s = roads.segment(cur);
            let (a, b) = if enter == 0 { (s.a, s.b) } else { (s.b, s.a) };
            if pts.is_empty() {
                pts.push(roads.node(a).pos);
            }
            pts.push(roads.node(b).pos);
            let Some(nx) = next[cur.0 as usize][1 - enter] else { break };
            if nx == first || seen[nx.0 as usize] {
                break;
            }
            enter = usize::from(roads.segment(nx).a != b);
            cur = nx;
        }
        out.push((segs, pts));
    }
    out
}

/// Points every few metres along a polyline (its ends included).
fn samples(pts: &[Vec2], step: f32) -> Vec<Vec2> {
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let len = w[0].distance(w[1]);
        let n = (len / step).ceil().max(1.0) as usize;
        for k in 0..n {
            out.push(w[0].lerp(w[1], k as f32 / n as f32));
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}

fn distance_to_polyline(p: Vec2, pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| crate::geom::project_on_segment(p, w[0], w[1]).1.distance(p)).fold(f32::MAX, f32::min)
}

fn bounds(pts: &[Vec2]) -> (Vec2, Vec2) {
    pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
}

/// Share of `samples` within `dist` of `pts`.
fn overlap(samples: &[Vec2], pts: &[Vec2], dist: f32) -> f32 {
    if samples.is_empty() || pts.len() < 2 {
        return 0.0;
    }
    samples.iter().filter(|p| distance_to_polyline(**p, pts) <= dist).count() as f32 / samples.len() as f32
}

impl StreetNames {
    /// The street a segment belongs to.
    pub fn of(&self, sid: SegmentId) -> Option<&Street> {
        self.of_segment.get(sid.0 as usize).copied().flatten().and_then(|i| self.streets.get(i as usize))
    }

    /// Name the network's streets: chains matching a street named before
    /// keep its name, the rest get new ones. Deterministic.
    pub fn update(&mut self, roads: &RoadNetwork, seed: u64) {
        let chains = chains(roads);
        let old = std::mem::take(&mut self.streets);
        let old_bounds: Vec<(Vec2, Vec2)> = old.iter().map(|s| bounds(&s.points)).collect();
        let mut streets: Vec<Street> = Vec::with_capacity(chains.len());
        // Best old match per chain, strongest claims first so a street cut
        // in two keeps its name on the larger piece (both may keep it).
        let mut claims: Vec<(usize, Option<usize>, f32)> = Vec::new();
        for (ci, (segs, pts)) in chains.iter().enumerate() {
            let class = class_of(roads, segs[0]);
            let sm = samples(pts, 8.0);
            let (lo, hi) = bounds(pts);
            let mut best: Option<(usize, f32)> = None;
            for (oi, o) in old.iter().enumerate() {
                let (olo, ohi) = old_bounds[oi];
                if olo.x > hi.x + MATCH_DIST || olo.y > hi.y + MATCH_DIST || ohi.x < lo.x - MATCH_DIST || ohi.y < lo.y - MATCH_DIST {
                    continue;
                }
                if !compatible(o.class, class) {
                    continue;
                }
                let share = overlap(&sm, &o.points, MATCH_DIST);
                if share >= MATCH_SHARE && best.is_none_or(|(_, b)| share > b + 1e-4) {
                    best = Some((oi, share));
                }
            }
            claims.push((ci, best.map(|b| b.0), best.map_or(0.0, |b| b.1)));
        }
        let mut names: Vec<Option<String>> = vec![None; chains.len()];
        for (ci, oi, _) in &claims {
            if let Some(oi) = oi {
                names[*ci] = Some(old[*oi].name.clone());
            }
        }
        // New names for the rest, longest chains first (so the main road
        // drawn with side streets takes the first serial).
        let mut fresh: Vec<usize> = (0..chains.len()).filter(|c| names[*c].is_none()).collect();
        let len = |c: usize| chains[c].1.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>();
        fresh.sort_by(|a, b| len(*b).total_cmp(&len(*a)).then(a.cmp(b)));
        for ci in fresh {
            let class = class_of(roads, chains[ci].0[0]);
            // A highway's other carriageway, already named, lends its name.
            if class == StreetClass::Highway {
                let sm = samples(&chains[ci].1, 15.0);
                let twin = (0..chains.len()).filter(|o| *o != ci && names[*o].is_some() && class_of(roads, chains[*o].0[0]) == StreetClass::Highway).find(|o| overlap(&sm, &chains[*o].1, CARRIAGEWAY_DIST) >= MATCH_SHARE);
                if let Some(o) = twin {
                    names[ci] = names[o].clone();
                    continue;
                }
            }
            let mut name = String::new();
            for _ in 0..24 {
                name = generate_name(seed, self.serial, class);
                self.serial += 1;
                if !names.iter().flatten().any(|n| *n == name) && !old.iter().any(|o| o.name == name) {
                    break;
                }
            }
            names[ci] = Some(name);
        }
        let mut of_segment = vec![None; roads.segments.len()];
        for (ci, (segs, pts)) in chains.into_iter().enumerate() {
            for s in &segs {
                of_segment[s.0 as usize] = Some(streets.len() as u32);
            }
            let class = class_of(roads, segs[0]);
            streets.push(Street { name: names[ci].take().unwrap_or_default(), class, points: pts, segments: segs });
        }
        self.streets = streets;
        self.of_segment = of_segment;
        self.roads_version = roads.version;
    }

    /// Rebuild only the per-segment lookup (after a load): the saved streets
    /// are matched against the network as it stands, which names every
    /// chain exactly as it was when saved.
    pub fn relink(&mut self, roads: &RoadNetwork, seed: u64) {
        self.update(roads, seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> RoadNetwork {
        let mut r = RoadNetwork::default();
        // A long east-west avenue with two side streets crossing it.
        r.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
        r.add_polyline(&[Vec2::new(-100.0, -200.0), Vec2::new(-100.0, 200.0)], 1, 1);
        r.add_polyline(&[Vec2::new(120.0, -200.0), Vec2::new(120.0, 200.0)], 1, 1);
        r
    }

    #[test]
    fn chains_run_straight_through_junctions() {
        let r = grid();
        let c = chains(&r);
        assert_eq!(c.len(), 3, "one avenue and two cross streets: {:?}", c.iter().map(|x| x.1.clone()).collect::<Vec<_>>());
        let lens: Vec<f32> = c.iter().map(|x| x.1.windows(2).map(|w| w[0].distance(w[1])).sum()).collect();
        assert!(lens.iter().any(|l| (*l - 600.0).abs() < 1.0), "{lens:?}");
    }

    #[test]
    fn names_are_deterministic_and_survive_a_rebuild() {
        let r = grid();
        let mut a = StreetNames::default();
        a.update(&r, 7);
        let mut b = StreetNames::default();
        b.update(&r, 7);
        let na: Vec<String> = a.streets.iter().map(|s| s.name.clone()).collect();
        let nb: Vec<String> = b.streets.iter().map(|s| s.name.clone()).collect();
        assert_eq!(na, nb);
        assert!(a.streets.iter().all(|s| !s.name.is_empty()));
        let avenue = a.streets.iter().find(|s| s.class == StreetClass::Avenue).unwrap().name.clone();

        // A new street added: the old ones keep their names, it gets a new one.
        let mut r2 = grid();
        r2.add_polyline(&[Vec2::new(250.0, -200.0), Vec2::new(250.0, 0.0)], 1, 1);
        a.update(&r2, 7);
        assert_eq!(a.streets.len(), 4);
        let mid = a.of(r2.nearest_street(Vec2::new(0.0, 0.0)).unwrap().0).unwrap();
        assert_eq!(mid.name, avenue, "the avenue keeps its name after being split by a new junction");
        let new_one = a.of(r2.nearest_street(Vec2::new(250.0, -100.0)).unwrap().0).unwrap();
        assert!(!na.contains(&new_one.name), "{} is new", new_one.name);

        // Saved and loaded: the lookup is rebuilt and the names hold.
        let text = ron::to_string(&a).unwrap();
        let mut back: StreetNames = ron::from_str(&text).unwrap();
        assert!(back.of_segment.is_empty());
        back.relink(&r2, 7);
        for (sid, _) in r2.alive_segments() {
            assert_eq!(back.of(sid).map(|s| &s.name), a.of(sid).map(|s| &s.name));
        }
    }

    #[test]
    fn a_corner_turns_the_name_but_a_bend_does_not() {
        let mut r = RoadNetwork::default();
        // A gentle bend: one street.
        r.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(200.0, 40.0)], 1, 1);
        // A T-junction: the stem is its own street.
        r.add_polyline(&[Vec2::new(100.0, 0.0), Vec2::new(100.0, -150.0)], 1, 1);
        let mut n = StreetNames::default();
        n.update(&r, 1);
        assert_eq!(n.streets.len(), 2);
    }
}
