//! [stable-parcels] Recutting parcels where the roads changed, and nowhere
//! else.
//!
//! A full cut ([`cut_parcels`]) is a pure function of the whole network,
//! and boolean ops over the whole map are not stable to the last bit: a
//! road laid on one edge of town used to move a parcel line on the other,
//! and the lots, buildings and plants on it with it. Now the parcels are
//! kept between rebuilds ([`ParcelCut`], saved with the city) and a rebuild
//! recuts only the land near road pieces that changed ([`recut_parcels`]):
//! every other parcel is carried over exactly, bit for bit. Ground under a
//! player's building (`locked`) is never recut: the parcels around it stop
//! at its outline.

use super::*;
use serde::{Deserialize, Serialize};

/// How far from a changed road piece parcels are recut, beyond the reach
/// parcels hang off a road by: corridor, reach and a margin.
pub const RECUT_REACH: f32 = 14.0;

/// Samples along a changed road piece, metres apart.
const SAMPLE_STEP: f32 = 8.0;

/// The land round the changes a recut looks at, metres past them: every
/// chunk the changes reach must lie inside it to be recut whole.
const CHUNK_WINDOW: f32 = 320.0;

/// Chunks (`chunk_block`) the changes reach are recut whole, as a fresh
/// cut would cut them, when they are no bigger than this across (their
/// bounding box's diagonal, m); a bigger one (a long curving block, which
/// chunks only at its straight stretches) is recut only near the changes.
const MAX_RECUT_CHUNK: f32 = 300.0;

/// A chunk the changes reach is not recut whole if that would reshape a
/// built lot the works do not come over (farther than this past a changed
/// piece's corridor): it is recut near the changes only, and there a built
/// parcel goes only if the works come this close. Empty land (a new
/// district being laid out) is always recut whole. [fix/roads-tools] Was:
/// any building 110 m from every change kept a chunk from a whole recut,
/// so the works reshaped built lots up to 110 m off the road that the plan
/// never outlined in red.
const PROTECT: f32 = 2.0;

/// A place where the land may cut differently since the last cut: a
/// sample along a road piece that came or went (`along`, reaching as far
/// as the piece's rows), or a node whose junction changed (only its
/// corners), with the corridor half-width there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change {
    pub at: Vec2,
    pub hw: f32,
    pub along: bool,
}

/// One road piece as the cutter sees it: its ends, corridor half-width and
/// everything that decides whether and how it cuts or fronts land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutPiece {
    pub a: Vec2,
    pub b: Vec2,
    pub hw: f32,
    pub flags: u32,
}

impl CutPiece {
    fn key(&self) -> [u32; 6] {
        let (a, b) = if (self.a.x, self.a.y) <= (self.b.x, self.b.y) { (self.a, self.b) } else { (self.b, self.a) };
        [a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits(), self.hw.to_bits(), self.flags]
    }

    /// Does `other` lie along this piece (the same corridor, collinear and
    /// within its ends)? A street split by a new junction is two pieces
    /// along the old one: only the new node moved anything.
    fn covers(&self, other: &CutPiece) -> bool {
        if self.hw.to_bits() != other.hw.to_bits() || self.flags != other.flags {
            return false;
        }
        let d = self.b - self.a;
        let len2 = d.length_squared();
        if len2 < 1e-6 {
            return false;
        }
        [other.a, other.b].iter().all(|&p| {
            let t = (p - self.a).dot(d) / len2;
            (-1e-4..=1.0 + 1e-4).contains(&t) && (self.a + d * t).distance(p) < 0.02
        })
    }
}

/// What the parcels were last cut from: every road piece, the degree of
/// every node (a junction's corridor reaches past it), the clip, the shore
/// reach and the water.
#[derive(Clone, Debug, Default)]
pub struct CutBasis {
    pub pieces: Vec<CutPiece>,
    pub degrees: Vec<(Vec2, f32, u8)>,
    pub clip: Option<(Vec2, Vec2)>,
    pub shore_reach: Option<f32>,
    pub water: u64,
    /// False until a cut has been made (a full cut follows).
    pub valid: bool,
}

impl CutBasis {
    pub fn of(roads: &RoadNetwork, water: &WaterMask, clip: Option<(Vec2, Vec2)>, shore_reach: Option<f32>) -> Self {
        let mut pieces = Vec::new();
        for (sid, s) in roads.alive_segments() {
            // A tunnel, or a deck high enough to span the lots, takes no
            // ground and fronts nothing: it cuts no parcel (its portals and
            // ramps do, and the degrees of their nodes count below).
            if s.tunnel || roads.spans_lots(sid) {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            // A ramp's piers are set after the cut, when the ground is
            // graded (and a way that fronts nothing is never fronted from a
            // bridge anyway): its deck flags do not count.
            let deck = !(s.ramp || s.fronts_nothing());
            let bits = [s.bridge && deck, s.viaduct && deck, s.tunnel, s.ring, s.ramp, s.is_rail(), s.is_street(), s.fronts_nothing(), roads.spans_lots(sid)];
            let flags = bits.iter().enumerate().fold((s.kind as u32) << 16, |acc, (i, &b)| acc | (b as u32) << i);
            pieces.push(CutPiece { a, b, hw: s.corridor_half_width(), flags });
        }
        // Nodes on ground-taking ways only (a node deep in a tunnel cuts
        // nothing), with every way meeting there counted, as `road_chains`
        // counts them.
        let ground = |s: crate::roads::SegmentId| { let g = roads.segment(s); g.alive && !g.tunnel && !roads.spans_lots(s) };
        let degrees = roads
            .nodes
            .iter()
            .filter(|n| n.segments.iter().any(|&s| ground(s)))
            .map(|n| (n.pos, n.level, n.segments.iter().filter(|&&s| roads.segment(s).alive).count().min(255) as u8))
            .collect();
        CutBasis { pieces, degrees, clip, shore_reach, water: water_hash(water), valid: true }
    }

    /// Where the land may cut differently between `self` (old) and `new`:
    /// points along every road piece one has and the other does not, and
    /// nodes whose degree changed. None when everything may (the water,
    /// the shore reach or the basis itself changed): recut it all.
    pub fn changes_to(&self, new: &CutBasis) -> Option<Vec<Change>> {
        if !self.valid || !new.valid || self.water != new.water || self.shore_reach.map(f32::to_bits) != new.shore_reach.map(f32::to_bits) {
            return None;
        }
        // Pieces as a multiset, so a piece laid twice is not lost in a set.
        let mut have: HashMap<[u32; 6], i32> = HashMap::new();
        for p in &self.pieces {
            *have.entry(p.key()).or_default() += 1;
        }
        let mut came: Vec<CutPiece> = Vec::new();
        for p in &new.pieces {
            match have.get_mut(&p.key()) {
                Some(n) if *n > 0 => *n -= 1,
                _ => came.push(*p),
            }
        }
        let mut gone: Vec<CutPiece> = Vec::new();
        for p in &self.pieces {
            if let Some(n) = have.get_mut(&p.key()) {
                if *n > 0 {
                    *n -= 1;
                    gone.push(*p);
                }
            }
        }
        let mut out: Vec<Change> = Vec::new();
        // A piece that lies along a piece on the other side (split or
        // joined at a node) changes nothing but at its own ends; one not
        // covered by such pieces changed along its whole length.
        let sample = |p: &CutPiece, others: &[CutPiece], out: &mut Vec<Change>| {
            if let Some(big) = others.iter().find(|o| o.covers(p)) {
                for e in [p.a, p.b] {
                    if e.distance(big.a) > 0.02 && e.distance(big.b) > 0.02 {
                        out.push(Change { at: e, hw: p.hw, along: false });
                    }
                }
                return;
            }
            let along: f32 = others.iter().filter(|o| p.covers(o)).map(|o| o.a.distance(o.b)).sum();
            if along >= p.a.distance(p.b) - 0.05 && along > 0.0 {
                return;
            }
            let n = (p.a.distance(p.b) / SAMPLE_STEP).ceil().max(1.0) as usize;
            out.extend((0..=n).map(|k| Change { at: p.a.lerp(p.b, k as f32 / n as f32), hw: p.hw, along: true }));
        };
        for p in &gone {
            sample(p, &came, &mut out);
        }
        for p in &came {
            sample(p, &gone, &mut out);
        }
        // Degrees as a multiset: nodes may share a position (a crossing
        // at two levels).
        let count = |d: &[(Vec2, f32, u8)]| {
            let mut m: HashMap<(u32, u32, u32, u8), i32> = HashMap::new();
            for (p, l, k) in d {
                *m.entry((p.x.to_bits(), p.y.to_bits(), l.to_bits(), *k)).or_default() += 1;
            }
            m
        };
        let (old_deg, new_deg) = (count(&self.degrees), count(&new.degrees));
        // (Sorted: a map's order is not the same from run to run, and the
        // order of the change points must not change the cut.)
        let mut nodes: Vec<(u32, u32)> = new_deg.iter().filter(|(k, n)| old_deg.get(*k) != Some(*n)).chain(old_deg.iter().filter(|(k, n)| new_deg.get(*k) != Some(*n))).map(|(k, _)| (k.0, k.1)).collect();
        nodes.sort_unstable();
        nodes.dedup();
        out.extend(nodes.into_iter().map(|(x, y)| Change { at: Vec2::new(f32::from_bits(x), f32::from_bits(y)), hw: 0.0, along: false }));
        if std::env::var_os("ND_RECUT_DEBUG").is_some() {
            eprintln!("recut: {} road pieces gone, {} came, {} change points", gone.len(), came.len(), out.len());
        }
        Some(out)
    }
}

fn water_hash(water: &WaterMask) -> u64 {
    let mut words: Vec<u64> = vec![water.polys.len() as u64];
    for ((lo, hi), p) in &water.polys {
        words.extend([lo.x, lo.y, hi.x, hi.y].map(|v| v.to_bits() as u64));
        for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
            words.push(ring.0.len() as u64);
            words.extend(ring.0.iter().flat_map(|c| [c.x.to_bits(), c.y.to_bits()]));
        }
    }
    crate::hash::fnv_words(words)
}

/// The parcels as last cut (before stations and yards take their ground),
/// with what they were cut from. Saved as outlines: everything else about
/// a parcel is derived from its outline and the roads.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(from = "SavedCut", into = "SavedCut")]
pub struct ParcelCut {
    pub raw: Vec<Parcel>,
    pub basis: CutBasis,
    /// Outlines read from a save, made into parcels after the load.
    pub saved: Vec<Vec<Vec2>>,
    /// The last cut made new parcels (not only carried the old ones).
    pub recut_some: bool,
}

impl ParcelCut {
    /// Forget the basis: the next cut is a full one.
    pub fn invalidate(&mut self) {
        self.basis.valid = false;
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedCut {
    outlines: Vec<Vec<Vec2>>,
}

impl From<SavedCut> for ParcelCut {
    fn from(s: SavedCut) -> Self {
        ParcelCut { raw: Vec::new(), basis: CutBasis::default(), saved: s.outlines, recut_some: false }
    }
}

impl From<ParcelCut> for SavedCut {
    fn from(c: ParcelCut) -> Self {
        if c.raw.is_empty() {
            return SavedCut { outlines: c.saved };
        }
        SavedCut { outlines: c.raw.into_iter().map(|p| p.polygon).collect() }
    }
}

/// Parcels from saved outlines, as they were cut ([`parcel_of`] of each,
/// with its frontage on the water).
pub fn parcels_from_outlines(outlines: &[Vec<Vec2>], roads: &RoadNetwork, water: &WaterMask) -> Vec<Parcel> {
    outlines
        .iter()
        .filter_map(|o| {
            let mut p = parcel_of(o.clone(), roads)?;
            if !water.is_empty() {
                p.water_frontage = water.shoreline(&p.polygon).0;
            }
            Some(p)
        })
        .collect()
}

/// The result of a recut: the parcels, and for each whether it was carried
/// over from the last cut unchanged (its rough-ground verdict too).
#[derive(Clone, Debug, Default)]
pub struct Recut {
    pub parcels: Vec<Parcel>,
    pub kept: Vec<bool>,
    pub basis: CutBasis,
}

/// Recut parcels for `roads`, given the last cut (`prev`, cut for
/// `prev_basis`): parcels within [`PARCEL_REACH`] + [`RECUT_REACH`] of a
/// road piece that changed are cut again, the rest are carried over as
/// they are. `locked` outlines (the ground under the player's buildings)
/// are never recut: parcels near them stop at their outline, and a locked
/// outline its old parcels no longer cover becomes a parcel of its own.
/// With no usable last cut, everything is cut afresh (still around
/// `locked`).
///
/// Memoised like [`generate_parcels_with`]: the project check's trial
/// rebuild and the implement after it recut once.
pub fn recut_parcels(prev: &[Parcel], prev_basis: &CutBasis, roads: &RoadNetwork, water: &WaterMask, clip: Option<(Vec2, Vec2)>, shore_reach: Option<f32>, locked: &[Vec<Vec2>], built: &[Vec2]) -> Recut {
    use std::sync::{Arc, Mutex};
    const KEEP: usize = 3;
    static RECENT: Mutex<Vec<(Vec<u64>, Arc<Recut>)>> = Mutex::new(Vec::new());
    let mut key = parcel_inputs(roads, water, clip, shore_reach);
    let f = |x: f32| x.to_bits() as u64;
    key.push(prev_basis.valid as u64);
    key.push(prev.len() as u64);
    for p in prev {
        key.push(p.polygon.len() as u64);
        key.extend(p.polygon.iter().flat_map(|v| [f(v.x), f(v.y)]));
        key.push(p.rough as u64);
    }
    key.push(prev_basis.pieces.len() as u64);
    for c in &prev_basis.pieces {
        key.extend([f(c.a.x), f(c.a.y), f(c.b.x), f(c.b.y), f(c.hw), c.flags as u64]);
    }
    key.push(prev_basis.degrees.len() as u64);
    key.extend(prev_basis.degrees.iter().flat_map(|(p, l, d)| [f(p.x), f(p.y), f(*l), *d as u64]));
    match prev_basis.clip {
        Some((lo, hi)) => key.extend([1, f(lo.x), f(lo.y), f(hi.x), f(hi.y)]),
        None => key.push(0),
    }
    key.push(built.len() as u64);
    key.extend(built.iter().flat_map(|v| [f(v.x), f(v.y)]));
    key.push(locked.len() as u64);
    for l in locked {
        key.push(l.len() as u64);
        key.extend(l.iter().flat_map(|v| [f(v.x), f(v.y)]));
    }
    if let Ok(recent) = RECENT.lock() {
        if let Some((_, hit)) = recent.iter().find(|(k, _)| *k == key) {
            return (**hit).clone();
        }
    }
    let out = recut_now(prev, prev_basis, roads, water, clip, shore_reach, locked, built);
    if let Ok(mut recent) = RECENT.lock() {
        recent.retain(|(k, _)| *k != key);
        if recent.len() >= KEEP {
            recent.remove(0);
        }
        recent.push((key, Arc::new(out.clone())));
    }
    out
}

fn geo_union(polys: &[Vec<Vec2>]) -> MultiPolygon<f64> {
    let g: Vec<Polygon<f64>> = polys.iter().filter(|p| p.len() >= 3).map(|p| to_geo(p)).collect();
    if g.is_empty() {
        MultiPolygon(vec![])
    } else {
        geo::unary_union(&g)
    }
}

#[allow(clippy::too_many_arguments)]
fn recut_now(prev: &[Parcel], prev_basis: &CutBasis, roads: &RoadNetwork, water: &WaterMask, clip: Option<(Vec2, Vec2)>, shore_reach: Option<f32>, locked: &[Vec<Vec2>], built: &[Vec2]) -> Recut {
    use geo::Intersects;
    let basis = CutBasis::of(roads, water, clip, shore_reach);
    let changes = if prev.is_empty() { None } else { prev_basis.changes_to(&basis) };
    let reach_r = shore_reach.filter(|r| *r > PARCEL_REACH && !water.is_empty()).unwrap_or(PARCEL_REACH);
    let locked: Vec<&Vec<Vec2>> = locked.iter().filter(|l| l.len() >= 3).collect();
    let locked_union = (!locked.is_empty()).then(|| geo_union(&locked.iter().map(|l| (*l).clone()).collect::<Vec<_>>()));
    let Some(mut changes) = changes else {
        // Everything afresh, around the locked ground.
        let mut parcels = match cut_region(roads, water, clip, shore_reach, None) {
            Some(mut region) => {
                if let Some(l) = &locked_union {
                    region = region.difference(l);
                }
                slice_region(region, roads, water, shore_reach, CutRules::CURRENT)
            }
            None => Vec::new(),
        };
        for l in &locked {
            if let Some(mut p) = parcel_of((*l).clone(), roads) {
                if !water.is_empty() {
                    p.water_frontage = water.shoreline(&p.polygon).0;
                }
                parcels.push(p);
            }
        }
        let kept = vec![false; parcels.len()];
        return Recut { parcels, kept, basis };
    };
    // Parcels cut off at a clip that has since grown are recut too.
    let clip_grew = prev_basis.clip.map(|(lo, hi)| [lo.x, lo.y, hi.x, hi.y].map(f32::to_bits)) != clip.map(|(lo, hi)| [lo.x, lo.y, hi.x, hi.y].map(f32::to_bits));
    if let (true, Some((clo, chi))) = (clip_grew, prev_basis.clip) {
        for p in prev {
            let (lo, hi) = bounds_of(&p.polygon);
            if lo.x <= clo.x + 1.0 || lo.y <= clo.y + 1.0 || hi.x >= chi.x - 1.0 || hi.y >= chi.y - 1.0 {
                changes.push(Change { at: p.center, hw: 0.0, along: true });
            }
        }
    }
    if changes.is_empty() {
        // Nothing to recut; locked ground its parcels do not cover still
        // becomes a parcel (below).
        let mut parcels = prev.to_vec();
        let mut kept = vec![true; parcels.len()];
        add_locked_rest(&locked, &parcels.clone(), roads, water, &mut parcels, &mut kept);
        return Recut { parcels, kept, basis };
    }
    // Where the land itself may differ: within a changed piece's reach of
    // it (a new or removed street's rows), or within a corridor's width of
    // a node whose junction changed (the corners there).
    let radius = |c: &Change| if c.along { c.hw + reach_r + 2.0 } else { c.hw.max(12.0) + 4.0 };
    let squares: Vec<Vec<Vec2>> = changes
        .iter()
        .map(|c| {
            let r = radius(c);
            vec![c.at + Vec2::new(-r, -r), c.at + Vec2::new(r, -r), c.at + Vec2::new(r, r), c.at + Vec2::new(-r, r)]
        })
        .collect();
    let touched = crate::prof!("rc.touched", geo_union(&squares));
    // [fix/roads-tools] A building the works themselves come over: within
    // `PROTECT` of a changed piece's corridor (what the plan outlines in
    // red), or at a junction that changed.
    let under_works = |b: Vec2| changes.iter().any(|c| c.at.distance(b) <= c.hw + PROTECT + if c.along { SAMPLE_STEP * 0.5 } else { 4.0 });
    let built_in = |poly: &[Vec2]| built.iter().copied().find(|b| point_in_polygon(*b, poly));
    let (mut clo, mut chi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for s in &squares {
        clo = clo.min(s[0]);
        chi = chi.max(s[2]);
    }
    // The region round the changes, far enough out that every chunk the
    // changes touch is whole (a bigger chunk is recut only near them).
    let window = (clo - Vec2::splat(CHUNK_WINDOW), chi + Vec2::splat(CHUNK_WINDOW));
    let region = crate::prof!("rc.region", cut_region(roads, water, clip, shore_reach, Some(window)).unwrap_or(MultiPolygon(vec![])));
    let boxes: Vec<(Vec2, Vec2)> = prev.iter().map(|p| bounds_of(&p.polygon)).collect();
    let mut drop = vec![false; prev.len()];
    // Every chunk of every block the changes reach, and which of them are
    // cut again whole (`fresh`) or only round the parcels near the changes.
    let mut chunks: Vec<(Vec<Vec2>, (Vec2, Vec2), bool)> = Vec::new();
    let mut fresh: Vec<bool> = Vec::new();
    let mut piecemeal: Vec<Polygon<f64>> = Vec::new();
    let inner = (window.0 + Vec2::splat(5.0), window.1 - Vec2::splat(5.0));
    for block in region.0.iter() {
        if area_f(block) < MIN_AREA || !boxes_overlap(bounds_of(&from_geo(block)), (clo, chi), 0.0) || !block.intersects(&touched) {
            continue;
        }
        for chunk in chunk_block(block.clone(), roads) {
            let cpts = from_geo(&chunk);
            let (lo, hi) = bounds_of(&cpts);
            let hit = chunk.intersects(&touched);
            // Recut whole only where that moves nothing built far from the
            // changes: a building in the chunk farther than `PROTECT` from
            // every change keeps its parcel (the chunk is recut near the
            // changes only).
            // [fix/roads-tools] Any building in the chunk beyond the
            // reach of the changes protects it (was: beyond 110 m), so the
            // works reshape no built lot but those the road comes near:
            // implementing took buildings 50 to 180 m off the road that
            // the plan never outlined (todo 2026-10-11).
            let far_built = || built.iter().any(|b| b.x >= lo.x && b.y >= lo.y && b.x <= hi.x && b.y <= hi.y && point_in_polygon(*b, &cpts) && !under_works(*b));
            let whole = lo.x >= inner.0.x && lo.y >= inner.0.y && hi.x <= inner.1.x && hi.y <= inner.1.y && lo.distance(hi) <= MAX_RECUT_CHUNK && !(hit && far_built());
            if hit && !whole {
                piecemeal.push(chunk);
                continue;
            }
            fresh.push(hit);
            chunks.push((cpts, (lo, hi), whole));
        }
    }
    // A chunk recut whole takes the parcels it holds and any reaching into
    // it. (A parcel taken that reached in from outside leaves the rest of
    // its ground to be cut again round the parcels that stay, below: the
    // chunks of a block cut near the changes need not match the chunks the
    // last cut made, so taking whole neighbouring chunks too would run on
    // along a block.)
    for (k, (cpts, cb, _)) in chunks.iter().enumerate() {
        if !fresh[k] {
            continue;
        }
        let shrunk = shrink(cpts, 0.995);
        for (i, p) in prev.iter().enumerate() {
            if !drop[i] && boxes_overlap(boxes[i], *cb, 0.0) && (point_in_polygon(p.center, cpts) || polygons_overlap(&shrink(&p.polygon, 0.98), &shrunk)) {
                drop[i] = true;
            }
        }
    }
    // Parcels near the changes whose ground is no longer land (a road over
    // them, or the street they hung off gone) go too.
    let land_near: Vec<&Polygon<f64>> = region.0.iter().filter(|b| boxes_overlap(bounds_of(&from_geo(b)), (clo, chi), 1.0)).collect();
    for (i, p) in prev.iter().enumerate() {
        if drop[i] || !boxes_overlap(boxes[i], (clo, chi), 0.0) || !touched.intersects(&to_geo(&p.polygon)) {
            continue;
        }
        let at = geo::Point::new(p.center.x as f64, p.center.y as f64);
        if !land_near.iter().any(|b| b.intersects(&at)) {
            drop[i] = true;
        }
    }
    // In a chunk too big to recut whole (a long curving block): the
    // parcels within reach of a change go, and their ground, with any new
    // land near the changes, is cut again round the parcels that stay.
    let mut piece_land: Option<MultiPolygon<f64>> = None;
    // A parcel taken for overlapping a fresh chunk whose centre lies in no
    // chunk being recut whole leaves ground outside it: that is cut again
    // round the parcels that stay.
    let in_fresh = |c: Vec2| chunks.iter().zip(&fresh).any(|((q, qb, _), f)| *f && c.x >= qb.0.x && c.y >= qb.0.y && c.x <= qb.1.x && c.y <= qb.1.y && point_in_polygon(c, q));
    let orphans: Vec<Vec<Vec2>> = prev.iter().zip(&drop).filter(|(p, d)| **d && !in_fresh(p.center)).map(|(p, _)| p.polygon.clone()).collect();
    if !orphans.is_empty() {
        piece_land = Some(region.intersection(&geo_union(&orphans)));
    }
    if !piecemeal.is_empty() {
        let big = MultiPolygon(piecemeal);
        let reach = reach_r + RECUT_REACH;
        let mut near: Vec<Vec<Vec2>> = squares.clone();
        for (i, p) in prev.iter().enumerate() {
            if drop[i] || !boxes_overlap(boxes[i], (clo, chi), reach) || !big.intersects(&geo::Point::new(p.center.x as f64, p.center.y as f64)) {
                continue;
            }
            // [fix/roads-tools] A built parcel goes only where the works
            // come over its ground; the new land is cut round the rest.
            let takes = match built_in(&p.polygon) {
                Some(_) => changes.iter().any(|c| point_in_polygon(c.at, &p.polygon) || distance_to_outline(c.at, &p.polygon) < c.hw - 0.5),
                None => changes.iter().any(|c| point_in_polygon(c.at, &p.polygon) || distance_to_outline(c.at, &p.polygon) < reach),
            };
            if takes {
                drop[i] = true;
                near.push(p.polygon.clone());
            }
        }
        let more = big.intersection(&geo_union(&near));
        piece_land = Some(match piece_land {
            Some(l) => l.union(&more),
            None => more,
        });
    }
    let mut parcels: Vec<Parcel> = Vec::new();
    let mut kept: Vec<bool> = Vec::new();
    for (i, p) in prev.iter().enumerate() {
        if !drop[i] {
            parcels.push(p.clone());
            kept.push(true);
        }
    }
    // Fresh chunks: each sliced as the full cut slices it, round any
    // locked ground in it.
    crate::prof_scope!("rc.slice");
    for (k, (cpts, _, _)) in chunks.iter().enumerate() {
        if !fresh[k] {
            continue;
        }
        let chunk = to_geo(cpts);
        let pieces: Vec<Polygon<f64>> = match &locked_union {
            Some(l) if l.intersects(&chunk) => chunk.difference(l).0,
            _ => vec![chunk],
        };
        for piece in pieces {
            for p in slice_chunk(piece, roads, water, shore_reach, CutRules::CURRENT) {
                parcels.push(p);
                kept.push(false);
            }
        }
    }
    if let Some(mut land) = piece_land {
        let blockers: Vec<Vec<Vec2>> = parcels
            .iter()
            .filter(|p| boxes_overlap(bounds_of(&p.polygon), window, 1.0))
            .map(|p| p.polygon.clone())
            .chain(locked.iter().filter(|l| boxes_overlap(bounds_of(l), window, 1.0)).map(|l| (*l).clone()))
            .collect();
        if !blockers.is_empty() {
            land = land.difference(&geo_union(&blockers));
        }
        for p in slice_region(land, roads, water, shore_reach, CutRules::CURRENT) {
            parcels.push(p);
            kept.push(false);
        }
    }
    if std::env::var_os("ND_RECUT_DEBUG").is_some() {
        eprintln!("recut: {} changes, {} chunks ({} fresh), {} dropped, {} new", changes.len(), chunks.len(), fresh.iter().filter(|f| **f).count(), drop.iter().filter(|d| **d).count(), kept.iter().filter(|k| !**k).count());
    }
    let carried: Vec<Parcel> = parcels.clone();
    add_locked_rest(&locked, &carried, roads, water, &mut parcels, &mut kept);
    Recut { parcels, kept, basis }
}

fn shrink(poly: &[Vec2], f: f32) -> Vec<Vec2> {
    let c = polygon_centroid(poly);
    poly.iter().map(|&p| c + (p - c) * f).collect()
}

/// Locked ground the parcels do not cover (all of it, when its parcels
/// were recut away) becomes parcels of its own outline.
fn add_locked_rest(locked: &[&Vec<Vec2>], have: &[Parcel], roads: &RoadNetwork, water: &WaterMask, parcels: &mut Vec<Parcel>, kept: &mut Vec<bool>) {
    for l in locked {
        let lb = bounds_of(l);
        let inside: Vec<Vec<Vec2>> = have.iter().filter(|p| boxes_overlap(bounds_of(&p.polygon), lb, 0.5) && point_in_polygon(p.center, l)).map(|p| p.polygon.clone()).collect();
        let rest: Vec<Vec<Vec2>> = if inside.is_empty() {
            vec![(*l).clone()]
        } else {
            let covered: f32 = inside.iter().map(|p| polygon_area(p).abs()).sum();
            if covered >= polygon_area(l).abs() * 0.98 {
                continue;
            }
            to_geo(l).difference(&geo_union(&inside)).0.iter().filter(|p| area_f(p) >= MIN_AREA).map(from_geo).collect()
        };
        for poly in rest {
            if let Some(mut p) = parcel_of(poly, roads) {
                if !water.is_empty() {
                    p.water_frontage = water.shoreline(&p.polygon).0;
                }
                parcels.push(p);
                kept.push(false);
            }
        }
    }
}


/// [parcel-size] An older save's parcels (cut before slivers were mended
/// and sizes capped) tidied where nothing stands on them: the `free` ones
/// (no lot) are mended and capped among themselves (`mend_parcels`,
/// `cap_parcel`); every parcel under a lot stays exactly as it is, so no
/// lot or building moves.
pub fn tidy_free_parcels(parcels: Vec<Parcel>, free: &[bool], roads: &RoadNetwork, water: &WaterMask) -> Vec<Parcel> {
    let loose: Vec<Vec<Vec2>> = parcels.iter().zip(free).filter(|(_, f)| **f).map(|(p, _)| p.polygon.clone()).collect();
    if loose.is_empty() {
        return parcels;
    }
    let mut out: Vec<Parcel> = parcels.into_iter().zip(free).filter(|(_, f)| !**f).map(|(p, _)| p).collect();
    let mut capped = Vec::new();
    for piece in mend_parcels(loose, roads, water) {
        cap_parcel(piece, water, 0, &mut capped);
    }
    for poly in capped {
        if let Some(mut p) = parcel_of(poly, roads) {
            if !water.is_empty() {
                p.water_frontage = water.shoreline(&p.polygon).0;
            }
            out.push(p);
        }
    }
    out
}
