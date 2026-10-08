//! [park-paths] Player paths joined to the walkways of parks and other
//! public grounds (Cities: Skylines style). Every park, and the grounds of
//! a stadium, a monument's plaza, a school's campus and a railway station,
//! shows gates on its edge (`walk::edge_gates`) where the Path tool snaps.
//! A path whose end is put on one is joined there: the park lays a walkway
//! out to meet it (`park::plan_parks_with`) and the walk graph gets an
//! edge from the path's end to the grounds (`WalkGraph::add_grounds`).
//! The path stays the player's gesture; only the join is remembered here.
//!
//! When the grounds go (rezoned, bulldozed, the lot re-cut away) the join
//! has nothing to hold: the path is cut back to its last point before the
//! vanished end, or removed when nothing would be left, and a
//! `SimEvent::PathTrimmed` tells the player. No walk edge is left dangling.

use super::World;
use crate::events::SimEvent;
use crate::plan::{Gesture, GestureId};
use crate::roads::RoadKind;
use crate::walk::{edge_gates, Grounds, GroundsJunction, GroundsKind};
use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// A path end this close to a snap point when its project is implemented
/// is joined there.
pub const LINK_SNAP: f32 = 1.0;

/// One end of a player path joined to public grounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathLink {
    pub gesture: GestureId,
    /// Where the end is (the gesture's first or last control point).
    pub pos: Vec2,
    pub kind: GroundsKind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PathLinks {
    pub links: Vec<PathLink>,
    /// Joins whose grounds went at the last walk rebuild: cut back at the
    /// next settle.
    #[serde(skip)]
    pub vanished: Vec<PathLink>,
    #[serde(skip)]
    settling: bool,
}

/// The first and last control point of a built path gesture.
fn path_ends(g: &Gesture) -> Option<[Vec2; 2]> {
    match g {
        Gesture::Road { points, kind: RoadKind::Path, .. } if points.len() >= 2 => Some([points[0], points[points.len() - 1]]),
        _ => None,
    }
}

impl World {
    /// Every piece of public ground a path may be joined to.
    pub fn grounds(&self) -> Vec<Grounds> {
        let mut out = Vec::new();
        for (lid, lot) in self.lots.iter() {
            if lot.polygon.len() < 3 {
                continue;
            }
            let kind = if lot.land_use == LandUse::Park && lot.park.is_some() {
                Some(GroundsKind::Park)
            } else {
                lot.building.and_then(|b| self.buildings.get(b)).filter(|b| b.lot == lid).and_then(|b| {
                    use crate::services::ServiceKind;
                    match (b.service.map(|s| s.kind), b.rail.as_ref().map(|r| r.kind)) {
                        (Some(ServiceKind::Stadium), _) => Some(GroundsKind::Stadium),
                        (Some(ServiceKind::Monument), _) => Some(GroundsKind::Plaza),
                        (Some(ServiceKind::School), _) => Some(GroundsKind::Campus),
                        (_, Some(crate::rail::RailFacility::Station | crate::rail::RailFacility::Central)) => Some(GroundsKind::Station),
                        _ => None,
                    }
                })
            };
            let Some(kind) = kind else { continue };
            let skip: &[usize] = lot.park.as_ref().map_or(&[], |p| p.shared_edges.as_slice());
            let gates = edge_gates(&lot.polygon, skip).into_iter().filter(|q| !self.landscape.is_water(*q)).collect();
            out.push(Grounds { kind, lot: Some(lid), outline: lot.polygon.clone(), gates });
        }
        out
    }

    /// The Path tool's snap points onto grounds, joined or free.
    pub fn grounds_junctions(&self) -> &[GroundsJunction] {
        &self.walk.junctions
    }

    /// The snap point nearest `p` within `r`.
    pub fn grounds_junction_near(&self, p: Vec2, r: f32) -> Option<&GroundsJunction> {
        self.walk.junction_near(p, r)
    }

    /// Before the parks are planned: joins whose path was moved or removed
    /// are dropped, and path ends now built on a snap point (as shown when
    /// the path was drawn) are joined.
    pub(super) fn note_path_links(&mut self) {
        let master = &self.master;
        self.path_links.links.retain(|l| master.gestures.get(&l.gesture).and_then(path_ends).is_some_and(|e| e.iter().any(|p| p.distance(l.pos) < 0.05)));
        let mut new: Vec<PathLink> = Vec::new();
        for (id, g) in &self.master.gestures {
            let Some(ends) = path_ends(g) else { continue };
            for p in ends {
                if self.path_links.links.iter().chain(&new).any(|l| l.gesture == *id && l.pos.distance(p) < 0.05) {
                    continue;
                }
                if let Some(j) = self.walk.junctions.iter().filter(|j| !j.attached && j.pos.distance(p) <= LINK_SNAP).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p))) {
                    new.push(PathLink { gesture: *id, pos: p, kind: j.kind });
                }
            }
        }
        self.path_links.links.extend(new);
    }

    /// The walk graph from the roads and lots, the crossing upgrades on it,
    /// and the joined paths; joins with no grounds left are set aside for
    /// `settle_path_links`.
    pub(super) fn build_walk_graph(&mut self) {
        self.walk = crate::walk::WalkGraph::build(&self.roads, &self.lots, &self.land_value, self.walk.version + 1);
        self.walk.apply_upgrades(&self.crosswalks.rules, &self.roads);
        let grounds = self.grounds();
        let links: Vec<Vec2> = self.path_links.links.iter().map(|l| l.pos).collect();
        let joined = self.walk.add_grounds(&self.roads, &grounds, &links);
        let gone: Vec<PathLink> = self.path_links.links.iter().zip(&joined).filter(|(_, ok)| !**ok).map(|(l, _)| l.clone()).collect();
        for l in gone {
            if !self.path_links.vanished.contains(&l) {
                self.path_links.vanished.push(l);
            }
        }
    }

    /// Cut back every path whose grounds went: the vanished end's control
    /// point is dropped (the path now ends at the one before), or the path
    /// is removed when fewer than two points would be left. Then one
    /// rebuild, so no walk edge points at grounds that are not there.
    pub fn settle_path_links(&mut self) {
        if self.path_links.vanished.is_empty() || self.path_links.settling {
            return;
        }
        let gone = std::mem::take(&mut self.path_links.vanished);
        self.path_links.links.retain(|l| !gone.contains(l));
        let mut ids: Vec<GestureId> = gone.iter().map(|l| l.gesture).collect();
        ids.sort();
        ids.dedup();
        let mut changed = false;
        for id in ids {
            let Some(Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind, elevation }) = self.master.gestures.get(&id).cloned() else { continue };
            if kind != RoadKind::Path || points.len() < 2 {
                continue;
            }
            let ends: Vec<&PathLink> = gone.iter().filter(|l| l.gesture == id).collect();
            let n = points.len();
            let drop_first = ends.iter().any(|l| l.pos.distance(points[0]) < 0.05);
            let drop_last = ends.iter().any(|l| l.pos.distance(points[n - 1]) < 0.05);
            if !drop_first && !drop_last {
                continue;
            }
            let lo = usize::from(drop_first);
            let hi = n - usize::from(drop_last);
            let what = gone.iter().find(|l| l.gesture == id).map_or(GroundsKind::Park, |l| l.kind);
            changed = true;
            if hi <= lo + 1 {
                self.master.gestures.remove(&id);
                self.path_links.links.retain(|l| l.gesture != id);
                self.events.push(SimEvent::PathTrimmed { gesture: id, removed: true, grounds: what });
            } else {
                let pts = points[lo..hi].to_vec();
                let elevation = if elevation.is_empty() { elevation } else { elevation.iter().copied().skip(lo).take(hi - lo).collect() };
                self.master.gestures.insert(id, Gesture::Road { points: pts, lanes_fwd, lanes_bwd, curved, kind, elevation });
                self.events.push(SimEvent::PathTrimmed { gesture: id, removed: false, grounds: what });
            }
        }
        if changed {
            self.path_links.settling = true;
            self.rebuild_from_master();
            self.path_links.settling = false;
        }
    }
}
