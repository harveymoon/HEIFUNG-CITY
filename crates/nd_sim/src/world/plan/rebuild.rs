//! The rebuild from the master plan: roads laid, lots recut, buildings
//! carried over (or bought out), and everything keyed by lot or road
//! following.

use super::*;

impl World {
    /// [interchange] Rebuild roads, lots and lanes from the master plan as
    /// implementing does, with nothing changed in the plan: the network is
    /// laid again with the current road-building rules (the `rebuild_roads`
    /// dev command, and probes comparing old saves).
    pub fn rebuild_from_plan(&mut self) {
        self.rebuild_from_master();
    }

    pub(in crate::world) fn rebuild_from_master(&mut self) {
        self.rebuild_from_master_with(false);
    }

    /// [stable-parcels] `rebuild_from_master`; a `trial` (the project
    /// check's copy of the world) stops once the lots and buildings are
    /// settled: what it asks (the structures lost, the columns' room) is
    /// known by then, and the ground, lanes, walks and coverage after it
    /// were most of its cost.
    pub(in crate::world) fn rebuild_from_master_with(&mut self, trial: bool) {
        crate::prof_scope!("rebuild");
        crate::prof!("rb.plan_changed", self.plan_changed());

        // Roads.
        let (mut roads, raised) = self.lay_roads(&self.master);
        // A zoning edit, or a facility fused onto what is there, gives back
        // exactly the roads that are built. Then the network, its ids and
        // everything hanging off them (lanes, vehicles, trains, bus stops,
        // signals, takeover blocks) stay as they are: only lots are re-cut.
        let roads_changed = !same_layout(&self.roads, &roads);
        if roads_changed {
            // Player arrows note where they stand before ids are renumbered.
            self.traffic.anchor_lane_use(&self.roads);
            roads.version = self.roads.version + 1;
            self.roads.set(roads);
            self.mark_bridges();
            self.rebuild_road_components();
            self.streets.update(&self.roads, self.config.seed);
        } else {
            // Still a new version: the app regrades and redraws the ground
            // and lots on it (lot pads change with the zoning).
            self.roads.version += 1;
        }

        // [tools] Columns under the raised roads, placed against the old
        // lots (they still hold the buildings): grown buildings in the way
        // are bought out below.
        let pillar_taken: std::collections::HashSet<BuildingId> = if roads_changed { self.place_pillars(&raised, true) } else { Default::default() };

        // Parcels along every road, the master's zoning onto them, a lot
        // for every zoned parcel; then old lots with buildings are matched
        // to the new ones. A save from before parcels were cut ahead of
        // zoning has lots of other shapes: its first rebuild moves every
        // building onto the lot that now holds it, without eminent domain.
        // [stable-parcels] The lots stay in place while they are rebuilt:
        // those the works do not touch keep their ids, outlines and
        // buildings; `old_lots` is how everything was.
        let old_lots: SlotMap<LotId, Lot> = (*self.lots).clone();
        let lenient = !self.parcel_lots;
        self.parcel_lots = true;
        // (Docks, stations and yards stand on lots of their footprints, made
        // again below.)
        let lockable = |b: BuildingId| self.is_city_building(b) && self.buildings.get(b).is_some_and(|x| x.port.is_none() && x.rail.is_none());
        let locked: Vec<LotId> = old_lots.iter().filter(|(_, l)| l.building.is_some_and(lockable)).map(|(id, _)| id).collect();
        let parks: Vec<Vec<Vec2>> = old_lots
            .values()
            .filter(|l| l.building.and_then(|b| self.buildings.get(b)).is_some_and(|b| b.land_use == LandUse::Park))
            .map(|l| l.polygon.clone())
            .collect();
        let pinned = crate::prof!("rb.lots", self.build_lots_from_parcels(&locked, &parks, !lenient));
        // Lots on ground too steep to level are left as rough land.
        crate::prof!("rb.grade", self.grade_lots());
        // [waterfront] frontage on the water, and the beaches
        crate::prof!("rb.shore", self.mark_shore_lots());
        // Facilities: their footprints are lots of their own, and zoned lots
        // under them give way.
        // A lot whose centre is under a footprint, or whose outline crosses
        // one, would share ground with the station or yard.
        let footprints = self.structure_footprints();
        self.lots.retain(|_, l| !footprints.iter().any(|fp| point_in_polygon(l.center, fp) || polygons_overlap(&shrink_about_centroid(fp, 0.995), &l.polygon)));
        // [tools] A column takes its lot from the zoning, and so does a deck
        // too low over a lot for a storey under it (parks and the city's
        // own structures stay: a column never stands on one, and a project
        // that would put a deck too low over one is refused).
        if !self.pillars.is_empty() || self.roads.alive_segments().any(|(sid, _)| self.roads.spans_lots(sid)) {
            let drop: Vec<LotId> = self
                .lots
                .iter()
                .filter(|(id, l)| l.land_use != LandUse::Park && !pinned.contains(id))
                .filter(|(_, l)| self.pillars_on(&l.polygon) || self.deck_cap_of(&l.polygon) == Some(0))
                .map(|(id, _)| id)
                .collect();
            // [p1-sim] Its parcels are no lot's (they kept naming the lost
            // one as built); parcels marked `under_deck` are never zoned, so
            // this is only a joined lot whose union reaches a column.
            for p in self.parcels.parcels.iter_mut().filter(|p| p.lot.is_some_and(|l| drop.contains(&l))) {
                p.lot = None;
                p.land_use = None;
            }
            for id in drop {
                self.lots.remove(id);
            }
        }
        let fids: Vec<FacilityId> = self.facilities.keys().collect();
        for fid in fids {
            // A facility with no street anywhere to front gets no lot this
            // time round; its building goes with the old lot below.
            self.facilities[fid].lot = self.facility_lot(fid).map(|lot| self.lots.insert(lot));
        }
        let pids: Vec<crate::ports::PortId> = self.ports.ports.keys().collect();
        for pid in pids {
            self.ports.ports[pid].lot = self.port_lot(pid).map(|lot| self.lots.insert(lot));
        }
        // Match old lots to new ones through a coarse grid of the new lots'
        // bounding boxes, so a rebuild costs lots × neighbours, not lots².
        let grid = LotGrid::new(&self.lots);
        // (building, where it stood, pulled down for the works?)
        let mut demolish: Vec<(BuildingId, Vec2, bool)> = Vec::new();
        // Old lot key to the new lot its building moved onto.
        let mut carried: HashMap<LotId, LotId> = HashMap::new();
        // [land] A building over merged plots names each of them; only its
        // primary lot moves the building, the others follow as extras.
        let primary_of: HashMap<BuildingId, LotId> = self.buildings.iter().map(|(id, b)| (id, b.lot)).collect();
        let mut extras: HashMap<BuildingId, Vec<LotId>> = HashMap::new();
        for (old_id, old) in old_lots.iter() {
            let Some(bid) = old.building else { continue };
            // [stable-parcels] Still on its lot, which the works left as it
            // was: it stays, unless a column or a deck now takes it.
            if self.lots.get(old_id).is_some_and(|l| l.building == Some(bid)) {
                let floors = self.buildings.get(bid).map_or(0, |b| b.floors);
                let taken = !self.is_city_building(bid) && (pillar_taken.contains(&bid) || self.deck_cap(old_id).is_some_and(|cap| floors > cap));
                if !taken {
                    carried.insert(old_id, old_id);
                    if primary_of.get(&bid).is_some_and(|p| *p != old_id) {
                        extras.entry(bid).or_default().push(old_id);
                    }
                    continue;
                }
                self.lots[old_id].building = None;
                if primary_of.get(&bid).is_some_and(|p| *p != old_id) {
                    continue;
                }
                demolish.push((bid, old.center, true));
                continue;
            }
            if primary_of.get(&bid).is_some_and(|p| *p != old_id) {
                let found = grid
                    .near(old.center, 6.0)
                    .into_iter()
                    .filter(|id| self.lots[*id].building.is_none() && self.lots[*id].contains(old.center))
                    .find(|id| !outlines_differ(&old.polygon, &self.lots[*id].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL));
                if let Some(new_lot) = found {
                    carried.insert(old_id, new_lot);
                    self.lots[new_lot].building = Some(bid);
                    extras.entry(bid).or_default().push(new_lot);
                }
                continue;
            }
            // [free-placement] City buildings (city parking too) stay on the
            // city lot under them: their own recut ground, kept while it
            // fronts any way at all, whatever its shape (a road far off
            // reshaping the block must not take a plant).
            let civic = self.is_city_building(bid);
            let fits = |id: LotId, l: &Lot| l.usable() || (civic && pinned.contains(&id) && crate::zoning::fronts_a_way(&l.polygon, &self.roads));
            let found = grid
                .near(old.center, 6.0)
                .into_iter()
                .map(|id| (id, &self.lots[id]))
                .filter(|(id, l)| l.building.is_none() && fits(*id, l))
                .map(|(id, l)| (id, if l.contains(old.center) { 0.0 } else { l.center.distance(old.center) }))
                .filter(|(_, d)| *d < 6.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            let floors = self.buildings.get(bid).map_or(0, |b| b.floors);
            match found {
                // [tools] Bought out for a column, or too tall for the deck
                // now spanning over it.
                _ if !civic && pillar_taken.contains(&bid) => demolish.push((bid, old.center, true)),
                Some(new_lot) if !civic && self.deck_cap(new_lot).is_some_and(|cap| floors > cap) => demolish.push((bid, old.center, true)),
                // Eminent domain: a parcel reshaped by road or path works
                // cannot keep its building, even if a lot is still there.
                Some(new_lot)
                    if !lenient
                        && !civic
                        && outlines_differ(&old.polygon, &self.lots[new_lot].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL) =>
                {
                    demolish.push((bid, old.center, true));
                }
                Some(new_lot) => {
                    carried.insert(old_id, new_lot);
                    self.lots[new_lot].building = Some(bid);
                    if let Some(b) = self.buildings.get_mut(bid) {
                        b.lot = new_lot;
                    }
                }
                None => demolish.push((bid, old.center, false)),
            }
        }
        // [land] Extras follow their building; bought plots still waiting
        // are let go (the works changed them).
        for (bid, b) in self.buildings.iter_mut() {
            b.plot.extra_lots = extras.remove(&bid).unwrap_or_default();
            b.plot.pending_lots.clear();
        }
        self.estate.reserved.clear();
        // [relocate] Where each building stood, for a business that moves.
        let old_site: HashMap<BuildingId, (f32, LandUse)> = old_lots.iter().filter_map(|(_, l)| l.building.map(|b| (b, (l.area(), l.land_use)))).collect();
        for (bid, at, expropriated) in demolish {
            if let Some(&(area, lu)) = old_site.get(&bid) {
                self.pack_business_at(bid, "the road works", at, area, lu);
            }
            if expropriated {
                // [review] The old key is stale after the recut: the null key.
                self.buy_out(bid, LotId::default(), Some(at), "Our block was pulled down for the road works. Homeless again.");
                self.tag_demolition(bid, crate::buildings::DemolitionReason::RoadWorks);
            } else {
                self.demolish_with(bid, Some(at), None);
                self.tag_demolition(bid, crate::buildings::DemolitionReason::Bulldozed);
            }
        }

        // Everything else that names a lot by key follows it to its new key.
        crate::prof!("rb.remap", self.remap_lot_keys(&old_lots, &grid, &carried));
        // [free-placement] City buildings' lots are the city's (an older
        // save's first rebuild switches them); city ground whose building did
        // not survive the works goes back to the plan.
        self.settle_city_ground(&pinned, false);
        if trial {
            return;
        }

        // Road heights and the ground they are graded into; new bridges are paid for.
        crate::prof!("rb.ground", self.rebuild_ground());
        self.charge_bridges();

        if roads_changed {
            // [r2-traffic] Parked cars keep their slots under the new ids
            // before anyone stranded by the rebuild looks for a free one.
            self.remap_kerb_slots();
            crate::prof!("rb.traffic", self.carry_traffic());
            // Trains ran on track that may be gone or re-laid, and their stops
            // and node ids are stale either way: everyone gets off where the
            // train stands and the next headway brings fresh ones.
            self.clear_trains();
            self.resnap_bus_stops();
        }
        // [carlife] Parked cars on streets that changed find a kerb again.
        crate::prof!("rb.kerbs", self.refresh_kerbs());
        crate::prof!("rb.links", { self.refresh_rail_links(); self.refresh_port_links() });
        crate::prof!("rb.walk", self.rebuild_walk());
        // [fix/buildings-ui] Camps the works paved over move on now (G16).
        crate::prof!("rb.camps", self.move_camps_on(false));
        // Coverage is per lot key: the old map names other lots now.
        crate::prof!("rb.coverage", self.refresh_coverage());

        self.events.push(SimEvent::RoadsChanged);
        self.events.push(SimEvent::LotsChanged);
        self.events.push(SimEvent::PlanChanged);
        // [park-paths] Paths whose park went with these works are cut back.
        crate::prof!("rb.paths", self.settle_path_links());
    }

    /// [fix/roads-tools] The road network `plan` lays (and its raised ways:
    /// key, built polyline, levels, deck half width), as the rebuild lays
    /// it; nothing in the world changes.
    #[allow(clippy::type_complexity)]
    /// [r4-crossings] On load: when the plan lays roads other than the
    /// save's (the laying rules changed since: `roads::MIN_PIECE`,
    /// `roads::PATH_MEETS`), rebuild from it now, traffic carried across,
    /// rather than at the player's next edit. Whether it did.
    pub(in crate::world) fn relay_roads_after_load(&mut self) -> bool {
        if self.master.gestures.is_empty() {
            return false;
        }
        let (roads, _) = self.lay_roads(&self.master);
        if same_layout(&self.roads, &roads) {
            return false;
        }
        self.rebuild_from_master();
        // The player's crossing upgrades (footbridges, buttons) are kept by
        // the crossing's middle: one whose crossing moved with the street's
        // nodes (a stub's crosswalk is set back from the junction now) goes
        // to the crossing that took its place.
        if self.reanchor_crossing_rules() {
            self.build_walk_graph();
        }
        true
    }

    /// [r4-crossings] Crossing rules that match no crossing now
    /// (`WalkGraph::apply_upgrades` matches within 2 m) move to the nearest
    /// crossing over a street running the same way within
    /// `REANCHOR_REACH` that no other rule holds. Whether any moved.
    pub(in crate::world) fn reanchor_crossing_rules(&mut self) -> bool {
        const REANCHOR_REACH: f32 = 12.0;
        let dir_of = |c: &crate::walk::Crossing| {
            let (s0, s1) = self.roads.segment_points(c.segment);
            (s1 - s0).normalize_or_zero()
        };
        let live: Vec<(Vec2, Vec2)> = self
            .walk
            .crossings
            .iter()
            .filter(|c| c.kind != crate::walk::CrossingKind::Jay && self.roads.try_segment(c.segment).is_some())
            .map(|c| (c.mid, dir_of(c)))
            .collect();
        let matches = |at: Vec2, dir: Vec2, c: &(Vec2, Vec2)| c.0.distance(at) < 2.0 && c.1.dot(dir).abs() > 0.9;
        let mut held: Vec<bool> = live.iter().map(|c| self.crosswalks.rules.iter().any(|r| matches(r.at, r.dir, c))).collect();
        let mut moved = false;
        for r in &mut self.crosswalks.rules {
            if live.iter().any(|c| matches(r.at, r.dir, c)) {
                continue;
            }
            let best = live
                .iter()
                .enumerate()
                .filter(|(k, c)| !held[*k] && c.1.dot(r.dir).abs() > 0.9 && c.0.distance(r.at) <= REANCHOR_REACH)
                .min_by(|a, b| a.1 .0.distance(r.at).total_cmp(&b.1 .0.distance(r.at)).then(a.0.cmp(&b.0)));
            if let Some((k, c)) = best {
                r.at = c.0;
                r.dir = c.1;
                held[k] = true;
                moved = true;
            }
        }
        moved
    }

    pub(in crate::world) fn lay_roads(&self, plan: &Plan) -> (RoadNetwork, Vec<(u64, Vec<Vec2>, Vec<f32>, f32)>) {
        let mut roads = RoadNetwork::default();
        // Roundabouts cut the roads entering them and join them to their
        // rings; the rings go down first so the arms snap onto them.
        let (built, rings) = plan.built_roads_levels();
        roads.rings = rings;
        // [tools] The raised ways, for their columns: key, built polyline,
        // levels, deck half width.
        let mut raised: Vec<(u64, Vec<Vec2>, Vec<f32>, f32)> = Vec::new();
        for (gid, drawn, levels, fwd, bwd, kind) in built {
            // [w1-roads] The road's use as set (None: read from its lanes).
            let car_only = match plan.gestures.get(&gid) {
                Some(Gesture::Road { car_only, .. }) => *car_only,
                _ => None,
            };
            let way = crate::pillars::is_raised(&levels).then(|| crate::pillars::way_key(&drawn, &levels));
            // Track is eased into smooth curves and tangential switches.
            let points = if kind == RoadKind::Rail { rail::smooth_track(&drawn, &roads) } else { drawn.clone() };
            // An end at the map edge runs on out of sight: a way to elsewhere.
            let points = crate::landscape::reach_elsewhere(&points);
            // Ramps meet the highway tangentially: a lead point before a
            // merge (after a diverge) brings the last piece alongside.
            // (A roundabout's ring is a one-way street too, but no ramp.)
            let ring = roads.is_ring_way(&points, fwd, bwd, kind);
            let points = if crate::roads::is_ramp_use(kind, fwd, bwd, car_only) && !ring { crate::roads::ease_ramp_ends(&points, fwd > 0, &roads) } else { points };
            // Breaks at every waterline, so a bridge is exactly the span.
            let points = self.split_at_water(&points);
            // A height sample every so often, so a long straight way follows
            // the land instead of running dead straight between its ends.
            // Only ways that front nothing (highways, track): a street's
            // vertices decide where its parcels are cut.
            let sampled = kind == RoadKind::Rail || (crate::roads::is_highway_use(kind, fwd, bwd, car_only) && !ring);
            let points = if sampled { crate::roads::sample_heights(&points, crate::roads::HEIGHT_SAMPLE) } else { points };
            // Levels follow the points wherever they moved or multiplied.
            let levels = if points == drawn { levels } else { crate::plan::carry_levels(&drawn, &levels, &points) };
            let (fwd, bwd) = match kind {
                RoadKind::Street if fwd == 0 && bwd == 0 => (1, 1),
                RoadKind::Street => (fwd, bwd),
                RoadKind::Path | RoadKind::Rail => (0, 0),
            };
            // [tools] Its ends as the network lays them (an end dropped on a
            // road takes that road's level: a raised road ramps down to it),
            // for its columns, before it is laid.
            let landed = way.map(|_| roads.as_built(&points, &levels, kind));
            roads.add_polyline_use(&points, &levels, fwd, bwd, kind, car_only);
            if let (Some(key), Some((pts, lv))) = (way, landed) {
                raised.push((key, pts, lv, super::deck_half_width_use(kind, fwd, bwd, car_only)));
            }
        }
        (roads, raised)
    }

    /// Lanes for the new roads, with every vehicle carried across them
    /// (`traffic/remap.rs`): a vehicle on a lane that did not change drives
    /// on as it was, one on a reshaped lane goes on from the new lane under
    /// it. Only one whose street was removed, or that has no way on from
    /// where it stands, leaves the road: through traffic rejoins what is
    /// left of the highway, anything else parks or is stranded there.
    pub(in crate::world) fn carry_traffic(&mut self) {
        let report = self.traffic.rebuild_carrying(&self.roads);
        self.apply_tolls();
        // [events] Wear follows the pieces that are still there.
        self.apply_road_wear();
        for (id, car) in report.lost {
            // (A service unit only looks like through traffic: its station
            // recovers it.)
            if Self::is_through_car(&car) && self.services.unit_of_car(id).is_none() && self.resume_through_car(&car) {
                continue;
            }
            self.strand_car(car);
        }
    }

    /// Bus stops follow the streets they sat on, or go. A stop that goes is
    /// taken out the way the player removing it would be
    /// (`stop_removed`), so buses and riders bound for later stops keep
    /// their places on the line.
    pub(in crate::world) fn resnap_bus_stops(&mut self) {
        let ids: Vec<RouteId> = self.transit.routes.keys().collect();
        for rid in ids {
            let snapped: Vec<Option<BusStop>> = self.transit.routes[rid].stops.iter().map(|s| BusStop::snap(&self.roads, s.pos)).collect();
            for (k, s) in snapped.iter().enumerate() {
                if let Some(s) = s {
                    self.transit.routes[rid].stops[k] = *s;
                }
            }
            for k in (0..snapped.len()).rev() {
                if snapped[k].is_none() {
                    self.transit.routes[rid].stops.remove(k);
                    self.stop_removed(rid, k);
                }
            }
        }
        self.transit.refresh_legs(&self.traffic.net);
    }

    /// Every rebuild cuts lots afresh, and a fresh SlotMap hands out the
    /// same keys again in a new order: an old key held anywhere now names
    /// some other lot (and no `.get()` guard notices). Each old key is
    /// mapped to the new lot its building moved onto, else the new lot
    /// under its old centre, and the keys kept elsewhere follow: rubble by
    /// where it lies, camps and footfall still owed to shops by their lot,
    /// campus plots pending an annex, gardens, murals (by their building)
    /// and a street race's meet and finish. Keys with no lot any more are
    /// dropped (a race spot or a group's record of a work gets the null key).
    pub(super) fn remap_lot_keys(&mut self, old_lots: &SlotMap<LotId, Lot>, grid: &LotGrid, carried: &HashMap<LotId, LotId>) {
        let lots = &self.lots;
        let lot_at = |p: Vec2| -> Option<LotId> {
            let near = grid.near(p, 6.0);
            near.iter().copied().find(|&id| lots.get(id).is_some_and(|l| l.contains(p))).or_else(|| {
                near.iter()
                    .filter_map(|&id| lots.get(id).map(|l| (id, l.center.distance(p))))
                    .filter(|(_, d)| *d < 6.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                    .map(|(id, _)| id)
            })
        };
        let map: HashMap<LotId, LotId> = old_lots.iter().filter_map(|(id, l)| carried.get(&id).copied().or_else(|| lot_at(l.center)).map(|n| (id, n))).collect();
        let rubble = std::mem::take(&mut self.services.rubble);
        let rubble: Vec<(LotId, Vec2, u64)> = rubble.into_iter().filter_map(|(lot, at, until)| lot_at(at).or_else(|| map.get(&lot).copied()).map(|n| (n, at, until))).collect();
        self.services.rubble = rubble;
        for camp in &mut self.society.homeless.camps {
            camp.lot = camp.lot.and_then(|l| map.get(&l).copied());
        }
        for c in self.citizens.values_mut() {
            if !c.pending_footfall.is_empty() {
                c.pending_footfall = c.pending_footfall.iter().filter_map(|l| map.get(l).copied()).collect();
            }
        }
        // [review] Campus plots being cleared for an annex (their
        // reservation is renewed: the rebuild cleared them all).
        for k in &mut self.corps.campuses {
            k.pending = k.pending.iter().filter_map(|(l, a)| map.get(l).map(|n| (*n, *a))).collect();
            for (l, _) in &k.pending {
                if self.lots.get(*l).is_some_and(|x| x.building.is_some()) {
                    self.estate.reserved.insert(*l, k.building);
                }
            }
        }
        // Gardens by where they grow, murals by their wall's building.
        let gardens = std::mem::take(&mut self.social.gardens);
        self.social.gardens = gardens.into_iter().filter_map(|mut g| (map.get(&g.lot).copied().or_else(|| lot_at(g.pos))).map(|n| { g.lot = n; g })).collect();
        let buildings = &self.buildings;
        let murals = std::mem::take(&mut self.social.murals);
        self.social.murals = murals.into_iter().filter_map(|mut m| (m.building.and_then(|b| buildings.get(b)).map(|b| b.lot).or_else(|| map.get(&m.lot).copied())).map(|n| { m.lot = n; m })).collect();
        for g in self.social.groups.iter_mut() {
            for w in &mut g.works {
                if let crate::social::Work::Mural { lot, .. } | crate::social::Work::Garden { lot, .. } = w {
                    *lot = map.get(lot).copied().unwrap_or_default();
                }
            }
        }
        // A street race's meet and finish.
        if let Some(r) = self.street_events.race.as_mut() {
            for s in [&mut r.meet, &mut r.finish] {
                s.lot = map.get(&s.lot).copied().or_else(|| lot_at(s.centre)).unwrap_or_default();
            }
        }
    }

    /// Union-find over the alive street and path segments: two nodes share
    /// a component when a walker or car can get from one to the other.
    pub(in crate::world) fn rebuild_road_components(&mut self) {
        let n = self.roads.nodes.len();
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn find(parent: &mut [u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                let p = parent[x as usize];
                parent[x as usize] = parent[p as usize];
                x = p;
            }
            x
        }
        // [sweep-sim] Streets first: their union is the drive network,
        // then the paths join the walkers' network on top of it.
        for street in [true, false] {
            for (_, seg) in self.roads.alive_segments() {
                if seg.is_rail() || seg.is_street() != street {
                    continue;
                }
                let (a, b) = (find(&mut parent, seg.a.0), find(&mut parent, seg.b.0));
                if a != b {
                    parent[a as usize] = b;
                }
            }
            if street {
                self.drive_component = (0..n as u32).map(|i| find(&mut parent, i)).collect();
            }
        }
        self.road_component = (0..n as u32).map(|i| find(&mut parent, i)).collect();
        self.connectivity.mark_dirty();
    }

    /// Component id of a road node, if the node exists.
    pub(in crate::world) fn road_component(&self, n: NodeId) -> Option<u32> {
        self.road_component.get(n.0 as usize).copied()
    }

    /// [sweep-sim] Component id of a road node over streets alone.
    pub(in crate::world) fn drive_component(&self, n: NodeId) -> Option<u32> {
        self.drive_component.get(n.0 as usize).copied()
    }

    /// Take every train off the line; its passengers stand where it was
    /// and decide again. [freight] Goods aboard go back out of town, first
    /// in the queue for the next freight train (or by road, should their
    /// yard no longer work: `drain_stranded_rail_imports`).
    pub(in crate::world) fn clear_trains(&mut self) {
        let trains: Vec<Train> = self.rail.trains.drain().map(|(_, t)| t).collect();
        let since = self.clock.tick.saturating_sub(crate::rail::FREIGHT_HEADWAY);
        for t in trains {
            self.rail.orders.extend(t.cargo.iter().map(|c| crate::rail::RailOrder { cargo: *c, since }));
            let pos = t.head();
            for p in t.passengers {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.transit = None;
                }
                self.strand_driver(p, pos);
            }
        }
    }
}
