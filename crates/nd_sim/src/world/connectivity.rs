//! [pt-walk] Network connectivity: which parts of the city cannot be walked
//! to (or driven to) from the rest, and the trips that failed for it.
//!
//! The walk graph labels its own components (`WalkGraph::compute_components`)
//! and the road network keeps its drive components
//! (`World::rebuild_road_components`). The *main* network is the component
//! that most lots with a building front; every other component holding a
//! building is a stranded area: its people cannot walk (or drive) to the
//! rest of the city, as when an industrial estate is reached only by a
//! tunnel (tunnels have no pavements). The areas are recomputed when the
//! networks change and every game hour (for the head counts); they are
//! derived, never saved.
//!
//! Trips that fail for want of any route (`travel_with` running out of
//! modes) are logged per destination; the third failure to one place in a
//! day (then the thirtieth, and every 300th) raises a "no route" alert, as
//! does a new stranded area with people in it. The app toasts alerts as
//! they come (`alerts` by `seq`) and the remote API lists everything
//! (`connectivity`).

use std::collections::{HashMap, VecDeque};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::*;

/// Alerts kept.
pub const ALERTS_KEPT: usize = 40;
/// Failed trips kept, newest last.
pub const FAILURES_KEPT: usize = 60;
/// A failed trip explains a loiterer for this long (ticks).
pub const STUCK_TICKS: u64 = 360;

/// Which network a stranded area is cut off from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Net {
    /// The walk graph: pavements, paths, crossings, footbridges.
    Walk,
    /// The streets cars drive (tunnels and highways included).
    Road,
}

impl Net {
    /// "pedestrian grid", "road network".
    pub fn label(self) -> &'static str {
        match self {
            Net::Walk => "pedestrian grid",
            Net::Road => "road network",
        }
    }

    /// The inspector's warning line.
    pub fn warning(self) -> String {
        format!("! not connected to the rest of the {}", self.label())
    }
}

/// A part of the city cut off from the main network.
#[derive(Clone, Debug)]
pub struct StrandedArea {
    pub net: Net,
    /// Its component (walk-graph or drive label).
    pub comp: u32,
    pub center: Vec2,
    pub lo: Vec2,
    pub hi: Vec2,
    /// Lots with a building in it.
    pub lots: Vec<LotId>,
    pub buildings: Vec<BuildingId>,
    /// People who live there, work there, and are there right now.
    pub residents: u32,
    pub workers: u32,
    pub here_now: u32,
}

impl StrandedArea {
    /// "3 buildings (Fulcrum Fab 6, ...) · 42 work, 0 live, 17 there now".
    pub fn describe(&self, w: &World) -> String {
        let names: Vec<String> = self.buildings.iter().take(3).filter_map(|b| w.buildings.get(*b).map(|b| b.name.clone())).collect();
        let more = if self.buildings.len() > names.len() { ", ..." } else { "" };
        format!(
            "{} building{} ({}{more}) not connected to the rest of the {}: {} live there, {} work there, {} there now",
            self.buildings.len(),
            if self.buildings.len() == 1 { "" } else { "s" },
            names.join(", "),
            self.net.label(),
            self.residents,
            self.workers,
            self.here_now
        )
    }
}

/// One trip that found no way at all.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FailedTrip {
    pub tick: u64,
    pub citizen: CitizenId,
    pub from: Vec2,
    pub target: BuildingId,
    /// Why, in words ("Fulcrum Fab 6 is not connected to ...").
    pub why: String,
}

/// What an alert is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertKind {
    /// Repeated failed trips to one place.
    NoRoute,
    /// A newly stranded area with people in it.
    Stranded(Net),
}

/// A connectivity alert, for the toast and the alerts panel.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectivityAlert {
    pub seq: u32,
    pub tick: u64,
    pub kind: AlertKind,
    /// Where to look.
    pub pos: Vec2,
    /// The place (`NoRoute`) or the area's first building.
    pub building: Option<BuildingId>,
    pub count: u32,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Connectivity {
    /// Recompute the areas at the next chance (the networks changed).
    #[serde(skip)]
    dirty: bool,
    /// The walk-graph version and hour the areas were computed for.
    #[serde(skip)]
    key: Option<(u64, u64)>,
    /// Main components (None in an empty city).
    #[serde(skip)]
    pub walk_main: Option<u32>,
    #[serde(skip)]
    pub drive_main: Option<u32>,
    #[serde(skip)]
    pub areas: Vec<StrandedArea>,
    /// Lots cut off, by network.
    #[serde(skip)]
    pub walk_cut: std::collections::BTreeSet<LotId>,
    #[serde(skip)]
    pub road_cut: std::collections::BTreeSet<LotId>,
    /// [pt-walk] Who is standing about because their last trip found no
    /// way, to which building and since when (for "doing").
    #[serde(skip)]
    pub stuck: HashMap<CitizenId, (BuildingId, u64)>,
    /// Failed trips today by destination, and the day they are for.
    pub day: u64,
    pub failed_today: Vec<(BuildingId, u32)>,
    pub failed: VecDeque<FailedTrip>,
    pub alerts: VecDeque<ConnectivityAlert>,
    pub next_seq: u32,
    /// The areas already alerted (net, first building), so a standing
    /// problem is told once.
    pub alerted: Vec<(Net, BuildingId)>,
    /// [pt-walk] Metres on foot from each walk node to the nearest park
    /// and night out, as the last land value pass worked them out.
    #[serde(skip)]
    pub access: Option<AccessFields>,
}

/// Distance fields over the walk graph (`World::amenity_fields`).
#[derive(Clone, Debug, Default)]
pub struct AccessFields {
    pub walk_version: u64,
    pub park: Vec<f32>,
    pub night: Vec<f32>,
}

/// [park-access] One lot of [`World::park_access`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParkAccessRow {
    pub lot: LotId,
    pub minutes: Option<f32>,
    pub residents: u32,
    /// The lot is a park itself.
    pub park: bool,
}

/// [park-access] Residents by their walk to a park: within 5, 10 and 15
/// minutes, and with none in reach.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParkAccessSummary {
    pub residents: u32,
    pub within_5: u32,
    pub within_10: u32,
    pub within_15: u32,
    pub none: u32,
    /// Homes (lots with residents) with no park in reach.
    pub homes_without: u32,
}

impl ParkAccessSummary {
    pub fn of(rows: &[ParkAccessRow]) -> Self {
        let mut s = Self::default();
        for r in rows.iter().filter(|r| r.residents > 0) {
            s.residents += r.residents;
            match r.minutes {
                Some(m) => {
                    s.within_5 += if m <= 5.0 { r.residents } else { 0 };
                    s.within_10 += if m <= 10.0 { r.residents } else { 0 };
                    s.within_15 += if m <= 15.0 { r.residents } else { 0 };
                }
                None => {
                    s.none += r.residents;
                    s.homes_without += 1;
                }
            }
        }
        s
    }

    /// `n` as a share of all residents, 0..1.
    pub fn share(&self, n: u32) -> f32 {
        if self.residents == 0 { 0.0 } else { n as f32 / self.residents as f32 }
    }
}

impl Connectivity {
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn push_alert(&mut self, tick: u64, kind: AlertKind, pos: Vec2, building: Option<BuildingId>, count: u32, text: String) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.alerts.push_back(ConnectivityAlert { seq, tick, kind, pos, building, count, text });
        while self.alerts.len() > ALERTS_KEPT {
            self.alerts.pop_front();
        }
    }

    /// The area (if any) lot `l` is stranded in, per network.
    pub fn area_of(&self, l: LotId, net: Net) -> Option<&StrandedArea> {
        self.areas.iter().find(|a| a.net == net && a.lots.contains(&l))
    }
}

/// How a road segment (or path) sits in the networks: `false` where it is
/// cut off from the main one, `None` where the question does not apply
/// (a path has no road network, a tunnel no pavement).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SegmentLinks {
    pub walk: Option<bool>,
    pub road: Option<bool>,
}

impl World {
    /// Every tick: recompute the stranded areas when the networks changed,
    /// and each game hour for the head counts.
    pub(super) fn step_connectivity(&mut self) {
        let hour = self.clock.tick / 60;
        let fresh = match self.connectivity.key {
            Some((v, h)) => v == self.walk.version && (h == hour || self.clock.tick % 60 != 0),
            None => false,
        };
        if fresh && !self.connectivity.dirty {
            return;
        }
        self.refresh_connectivity();
    }

    /// Recompute the main networks and the stranded areas now.
    pub fn refresh_connectivity(&mut self) {
        crate::prof_scope!("refresh_connectivity");
        self.connectivity.dirty = false;
        self.connectivity.key = Some((self.walk.version, self.clock.tick / 60));
        // [walk-networks] Head counts on each network, hourly too.
        self.refresh_walk_networks();
        // Lots worth counting: those with a building.
        // A public beach with no way on says so itself (`shore`): not a
        // stranded area.
        let built: Vec<(LotId, BuildingId)> = self.lots.iter().filter(|(_, lot)| !lot.shore.beach).filter_map(|(l, lot)| lot.building.filter(|b| self.buildings.contains_key(*b)).map(|b| (l, b))).collect();
        // ── walk: each lot's components (its own nodes, or the pavement
        // in front when it has none inside).
        let mut walk_comps: HashMap<LotId, Vec<u32>> = HashMap::new();
        if self.walk.comp.len() == self.walk.nodes.len() {
            for (i, n) in self.walk.nodes.iter().enumerate() {
                if self.walk.adj[i].is_empty() {
                    continue;
                }
                let c = self.walk.comp[i];
                for l in n.lot.iter().chain(n.near.iter()) {
                    let v = walk_comps.entry(*l).or_default();
                    if !v.contains(&c) {
                        v.push(c);
                    }
                }
            }
        }
        let mut count: HashMap<u32, u32> = HashMap::new();
        for (l, _) in &built {
            for c in walk_comps.get(l).into_iter().flatten() {
                *count.entry(*c).or_default() += 1;
            }
        }
        let walk_main = count.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(c, _)| *c);
        // ── roads: the drive component of each lot's street.
        let drive_of = |lot: &Lot| lot.car_segment.and_then(|s| self.roads.try_segment(s)).and_then(|s| self.drive_component(s.a));
        let mut dcount: HashMap<u32, u32> = HashMap::new();
        for (l, _) in &built {
            if let Some(c) = self.lots.get(*l).and_then(drive_of) {
                *dcount.entry(c).or_default() += 1;
            }
        }
        let drive_main = dcount.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(c, _)| *c);
        // ── the areas.
        let mut groups: std::collections::BTreeMap<(u8, u32), Vec<(LotId, BuildingId)>> = Default::default();
        let mut walk_cut = std::collections::BTreeSet::new();
        let mut road_cut = std::collections::BTreeSet::new();
        for &(l, b) in &built {
            if let (Some(main), Some(cs)) = (walk_main, walk_comps.get(&l)) {
                if !cs.contains(&main) {
                    walk_cut.insert(l);
                    groups.entry((0, *cs.iter().min().unwrap())).or_default().push((l, b));
                }
            }
            if let (Some(main), Some(c)) = (drive_main, self.lots.get(l).and_then(drive_of)) {
                if c != main {
                    road_cut.insert(l);
                    groups.entry((1, c)).or_default().push((l, b));
                }
            }
        }
        let mut areas = Vec::new();
        for ((net, comp), members) in groups {
            let net = if net == 0 { Net::Walk } else { Net::Road };
            let mut lo = Vec2::splat(f32::INFINITY);
            let mut hi = Vec2::splat(f32::NEG_INFINITY);
            let mut sum = Vec2::ZERO;
            let (mut residents, mut workers, here_now) = (0u32, 0u32, 0u32);
            for &(l, b) in &members {
                let c = self.lots[l].center;
                lo = lo.min(c);
                hi = hi.max(c);
                sum += c;
                let bld = &self.buildings[b];
                residents += bld.residents.len() as u32;
                workers += bld.workers.len() as u32;
            }
            let bs: Vec<BuildingId> = members.iter().map(|m| m.1).collect();
            areas.push(StrandedArea { net, comp, center: sum / members.len() as f32, lo, hi, lots: members.iter().map(|m| m.0).collect(), buildings: bs, residents, workers, here_now });
        }
        // Who is in there now: inside its buildings, or (cut off on foot)
        // standing about in it.
        let mut area_of: HashMap<BuildingId, Vec<usize>> = HashMap::new();
        for (i, a) in areas.iter().enumerate() {
            for b in &a.buildings {
                area_of.entry(*b).or_default().push(i);
            }
        }
        if !area_of.is_empty() {
            for c in self.citizens.values() {
                for &i in c.inside.and_then(|b| area_of.get(&b)).into_iter().flatten() {
                    areas[i].here_now += 1;
                }
            }
        }
        if areas.iter().any(|a| a.net == Net::Walk) {
            let comps: Vec<(usize, u32)> = areas.iter().enumerate().filter(|(_, a)| a.net == Net::Walk).map(|(i, a)| (i, a.comp)).collect();
            for c in self.citizens.values() {
                if c.inside.is_some() || c.activity.is_traveling() {
                    continue;
                }
                let Some(k) = self.walk.nearest_within(c.pos, 12.0) else { continue };
                let Some(comp) = self.walk.component(k) else { continue };
                if let Some((i, _)) = comps.iter().find(|(_, x)| *x == comp) {
                    areas[*i].here_now += 1;
                }
            }
        }
        // A new area with people in it is worth an alert, once.
        let tick = self.clock.tick;
        for a in &areas {
            let Some(&first) = a.buildings.iter().min() else { continue };
            let people = a.residents + a.workers + a.here_now;
            if people == 0 || self.connectivity.alerted.iter().any(|(n, b)| *n == a.net && a.buildings.contains(b)) {
                continue;
            }
            self.connectivity.alerted.push((a.net, first));
            let text = format!("Stranded: {}.", a.describe(self));
            self.connectivity.push_alert(tick, AlertKind::Stranded(a.net), a.center, Some(first), people, text);
        }
        // Forget areas that have been joined up since.
        let cut = |n: Net, b: BuildingId| if n == Net::Walk { self.buildings.get(b).is_some_and(|x| walk_cut.contains(&x.lot)) } else { self.buildings.get(b).is_some_and(|x| road_cut.contains(&x.lot)) };
        let keep: Vec<(Net, BuildingId)> = self.connectivity.alerted.iter().copied().filter(|&(n, b)| cut(n, b)).collect();
        self.connectivity.alerted = keep;
        let k = &mut self.connectivity;
        k.walk_main = walk_main;
        k.drive_main = drive_main;
        k.areas = areas;
        k.walk_cut = walk_cut;
        k.road_cut = road_cut;
    }

    /// The areas cut off from the main networks (as last computed).
    pub fn stranded_areas(&self) -> &[StrandedArea] {
        &self.connectivity.areas
    }

    /// Is lot `l` cut off from the main walk (or road) network?
    pub fn lot_cut_off(&self, l: LotId, net: Net) -> bool {
        match net {
            Net::Walk => self.connectivity.walk_cut.contains(&l),
            Net::Road => self.connectivity.road_cut.contains(&l),
        }
    }

    /// The inspector's warning lines for a building's lot ("! not connected
    /// to the rest of the pedestrian grid").
    pub fn lot_link_warnings(&self, l: LotId) -> Vec<String> {
        [Net::Walk, Net::Road].into_iter().filter(|&n| self.lot_cut_off(l, n)).map(Net::warning).collect()
    }

    /// The walk-graph node a segment's walkers use near its middle: the
    /// path's own centre line, or a pavement.
    fn walk_node_of_segment(&self, sid: SegmentId) -> Option<u32> {
        let s = self.roads.try_segment(sid)?;
        if s.is_rail() || s.fronts_nothing() {
            return None;
        }
        let (a, b) = self.roads.segment_points(sid);
        let mid = (a + b) * 0.5;
        let reach = s.half_width() + crate::zoning::SIDEWALK + 3.0;
        self.walk.nearest_matching(mid, reach, |n| self.walk.nodes[n as usize].lot.is_none())
    }

    /// How segment `sid` sits in the networks (see [`SegmentLinks`]).
    pub fn segment_links(&self, sid: SegmentId) -> SegmentLinks {
        let Some(s) = self.roads.try_segment(sid) else { return SegmentLinks::default() };
        let walk = self.connectivity.walk_main.and_then(|main| self.walk_node_of_segment(sid).and_then(|n| self.walk.component(n)).map(|c| c == main));
        let road = if s.is_street() { self.connectivity.drive_main.and_then(|main| self.drive_component(s.a).map(|c| c == main)) } else { None };
        SegmentLinks { walk, road }
    }

    /// The inspector's warning lines for a road or path.
    pub fn segment_link_warnings(&self, sid: SegmentId) -> Vec<String> {
        let l = self.segment_links(sid);
        let mut out = Vec::new();
        if l.walk == Some(false) {
            out.push(Net::Walk.warning());
        }
        if l.road == Some(false) {
            out.push(Net::Road.warning());
        }
        out
    }

    /// [pt-walk] Overlay hook (for the map layers): every alive street or
    /// path with whether it is on the main walk and road networks.
    pub fn connectivity_segments(&self) -> Vec<(SegmentId, SegmentLinks)> {
        self.roads.alive_segments().filter(|(_, s)| !s.is_rail()).map(|(id, _)| (id, self.segment_links(id))).collect()
    }

    /// Why a trip from `from` to `target` cannot be made at all, in words.
    pub(super) fn no_route_reason(&self, from: Vec2, target: BuildingId) -> String {
        let Some(b) = self.buildings.get(target) else { return "the grid doesn't connect".into() };
        let name = b.name.clone();
        let there = self.lot_cut_off(b.lot, Net::Walk);
        let here = self.walk.nearest_within(from, 20.0).and_then(|k| self.walk.component(k)).zip(self.connectivity.walk_main).is_some_and(|(c, m)| c != m);
        match (here, there) {
            (_, true) => format!("{name} isn't connected to the rest of the pedestrian grid"),
            (true, false) => "I'm cut off from the rest of the pedestrian grid".into(),
            _ => "the grid doesn't connect".into(),
        }
    }

    /// Log a trip that found no way, raising an alert on repeats.
    pub(super) fn note_failed_trip(&mut self, id: CitizenId, from: Vec2, target: BuildingId, why: &str) {
        // [insight] Into the citizen's history too.
        self.note_no_route(id, from, target, why);
        let tick = self.clock.tick;
        let day = self.clock.day();
        let k = &mut self.connectivity;
        if k.day != day {
            k.day = day;
            k.failed_today.clear();
        }
        k.stuck.insert(id, (target, tick));
        k.failed.push_back(FailedTrip { tick, citizen: id, from, target, why: why.to_string() });
        while k.failed.len() > FAILURES_KEPT {
            k.failed.pop_front();
        }
        let n = match k.failed_today.iter_mut().find(|(b, _)| *b == target) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                k.failed_today.push((target, 1));
                1
            }
        };
        // The 3rd and 30th failure today, then every 300th.
        if n == 3 || n == 30 || n % 300 == 0 {
            let (name, pos) = self.buildings.get(target).map_or(("somewhere".to_string(), from), |b| (b.name.clone(), self.lots.get(b.lot).map_or(from, |l| l.center)));
            let text = format!("No route found: {n} trips to {name} failed today ({why}).");
            self.connectivity.push_alert(tick, AlertKind::NoRoute, pos, Some(target), n, text);
        }
    }

    /// [park-access] The Park access layer's data: every lot with a
    /// building, its walk to the nearest open park in minutes (`None`: no
    /// park in walking reach) and how many people live there.
    pub fn park_access(&self) -> Vec<ParkAccessRow> {
        self.lots
            .iter()
            .filter_map(|(id, l)| {
                let b = self.buildings.get(l.building?)?;
                let a = self.lot_walk_access(id)?;
                Some(ParkAccessRow { lot: id, minutes: a.park_min, residents: b.residents.len() as u32, park: b.land_use == LandUse::Park })
            })
            .collect()
    }

    /// [pt-walk] Walking access from lot `l` itself (F2, F30): the best of
    /// the walk nodes in it and along its front, rather than the land
    /// value cell its centre falls in (whose nearest node may be on a lot
    /// next door, or one that joins nothing). An open park lot is a park
    /// at no minutes. Falls back to the cell before the first pass.
    pub fn lot_walk_access(&self, l: LotId) -> Option<crate::land_value::Access> {
        let lot = self.lots.get(l)?;
        let cell = self.land_value.access_at(lot.center);
        let Some(f) = self.connectivity.access.as_ref().filter(|f| f.walk_version == self.walk.version && f.park.len() == self.walk.nodes.len()) else { return cell };
        let (lo, hi) = lot.polygon.iter().fold((Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let pad = Vec2::splat(16.0);
        let mut best = (f32::INFINITY, f32::INFINITY);
        let mut any = false;
        for k in self.walk.nodes_in_box(lo - pad, hi + pad) {
            let n = &self.walk.nodes[k as usize];
            if self.walk.adj[k as usize].is_empty() || !(n.lot == Some(l) || n.near.contains(&l)) {
                continue;
            }
            any = true;
            best.0 = best.0.min(f.park[k as usize]);
            best.1 = best.1.min(f.night[k as usize]);
        }
        if !any {
            return cell;
        }
        let open = |lu: LandUse| lot.building.and_then(|b| self.buildings.get(b)).is_some_and(|b| b.operating() && b.land_use == lu) || self.buildings.values().any(|b| b.land_use == lu && b.operating() && b.plot.extra_lots.contains(&l));
        if open(LandUse::Park) {
            best.0 = 0.0;
        }
        let per_min = WALK_MPS * 60.0;
        let mins = |d: f32| d.is_finite().then_some(d / per_min);
        let (park_min, nightlife_min) = (mins(best.0), mins(best.1));
        Some(crate::land_value::Access { park_min, nightlife_min, value: crate::land_value::access_value(park_min, nightlife_min) })
    }

    /// [pt-walk] The read-out of a lot's walking access, saying so when a
    /// park within reach is still being built (it counts once open).
    pub fn lot_access_text(&self, l: LotId) -> Option<String> {
        let a = self.lot_walk_access(l)?;
        let mut text = a.describe();
        if a.park_min.is_none_or(|m| m > 10.0) {
            let at = self.lots.get(l)?.center;
            // A park zoned but not laid out yet, or still a site.
            let site = self.lots.values().filter(|pl| pl.land_use == LandUse::Park && pl.center.distance(at) < 400.0).any(|pl| pl.building.and_then(|b| self.buildings.get(b)).is_none_or(|b| !b.operating()) && !pl.shore.beach);
            if site {
                text.push_str(" (a park zoned nearby is not open yet: it counts once it is laid out)");
            }
            if self.lot_cut_off(l, Net::Walk) {
                text.push_str(" (cut off from the rest of the pedestrian grid)");
            }
        }
        Some(text)
    }

    /// A trip got going: the citizen is no longer stuck.
    pub(super) fn clear_stuck(&mut self, id: CitizenId) {
        if !self.connectivity.stuck.is_empty() {
            self.connectivity.stuck.remove(&id);
        }
    }

    /// [pt-walk] Standing about because the last trip found no way: where
    /// they wanted to go.
    pub fn stuck_for(&self, id: CitizenId) -> Option<BuildingId> {
        let c = self.citizens.get(id)?;
        let &(b, since) = self.connectivity.stuck.get(&id)?;
        (matches!(c.activity, Activity::Idle) && c.inside.is_none() && self.clock.tick.saturating_sub(since) <= STUCK_TICKS).then_some(b)
    }
}
