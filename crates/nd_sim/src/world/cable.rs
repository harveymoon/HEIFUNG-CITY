//! [cable] Cable car stations and lines in the city, the cabins' riders,
//! upkeep and the pull on land and visitors (`crate::cable` has the types,
//! the pylon placement and the cabins' motion).
//!
//! A station stands where the player clicks: on a lot it buys out (a
//! ground station, the lot's building a plaza with `Building::cable`), on
//! the roof of a building at least `cable::ROOF_MIN_HEIGHT` tall (the
//! building stays; riders take its lifts from its door), or on open
//! ground such as a hillside or a park (walked to from the nearest way
//! people walk). Riders choose a line like the metro: the walk to a
//! station, the wait for a cabin, the ride, the walk on (`mode_options`);
//! the gondola beats the streets where they go round a river or climb a
//! hill (`cable_reprice`).

use super::*;
use crate::cable::{self as cable, Aboard, Below, CableKind, CableLine, CableLineId, CablePlan, CablePreview, CableStation, CableStationId, LoopPart, Terrain};
use crate::world::trips::{self, TripQuery};

/// Minutes from a station's entrance up to its platform (stairs, a lift).
pub const CABLE_STAIRS: f32 = 1.5;

/// Where a station is wanted, as the player asks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SiteWant {
    /// A roof if the building is tall enough, else the lot or open ground.
    #[default]
    Auto,
    Ground,
    Roof,
}

impl SiteWant {
    pub fn parse(s: &str) -> Option<SiteWant> {
        match s.to_ascii_lowercase().as_str() {
            "auto" | "" => Some(SiteWant::Auto),
            "ground" | "lot" => Some(SiteWant::Ground),
            "roof" | "rooftop" => Some(SiteWant::Roof),
            _ => None,
        }
    }
}

/// Where a new station would go and what building it would take.
#[derive(Clone, Debug, PartialEq)]
pub struct StationSite {
    pub pos: Vec2,
    pub floor: f32,
    pub ground: f32,
    pub roof: bool,
    /// The lot it buys out (a ground station on a lot).
    pub lot: Option<LotId>,
    /// The building on whose roof it sits.
    pub host: Option<BuildingId>,
    pub entrance: Vec2,
    /// For the player: "on the roof of ...", "buys out ...", "on open ground".
    pub what: String,
}

/// One stop of a line as the player gives it: a station there already, or
/// a point to build one at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CableStop {
    Station(CableStationId),
    At(Vec2, SiteWant),
}

/// The city as the pylon placement sees it, round one line.
struct CityLand<'a> {
    w: &'a World,
    /// Lots near the line with their bounds and what stands there.
    lots: Vec<(Vec2, Vec2, LotId, Option<BuildingId>, f32, bool)>,
    /// Stations the line ends at (their lots are not obstacles).
    own: Vec<LotId>,
}

impl<'a> CityLand<'a> {
    fn new(w: &'a World, points: &[Vec2], own: Vec<LotId>) -> CityLand<'a> {
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for p in points {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let (lo, hi) = (lo - Vec2::splat(80.0), hi + Vec2::splat(80.0));
        // Near the line itself, not just its box: a long diagonal line's
        // box holds half the city.
        let near_line = |a: Vec2, b: Vec2| {
            points.windows(2).any(|w| {
                let c = (a + b) * 0.5;
                let r = a.distance(b) * 0.5 + 40.0;
                crate::roads::project_on_segment(c, w[0], w[1]).1.distance(c) <= r
            })
        };
        let lots = w
            .lots
            .iter()
            .filter_map(|(id, l)| {
                let (a, b) = crate::geom::bounds(&l.polygon);
                if b.x < lo.x || b.y < lo.y || a.x > hi.x || a.y > hi.y || !near_line(a, b) {
                    return None;
                }
                let bld = l.building.filter(|b| w.buildings.get(*b).is_some_and(|x| !x.cable && (x.land_use != LandUse::Park || x.service.is_some())));
                let h = bld.map_or(0.0, |b| w.cable_obstacle_height(b));
                let park = l.building.and_then(|b| w.buildings.get(b)).is_some_and(|x| x.land_use == LandUse::Park);
                Some((a, b, id, bld, h, park))
            })
            .collect();
        CityLand { w, lots, own }
    }

    fn lot_at(&self, p: Vec2) -> Option<&(Vec2, Vec2, LotId, Option<BuildingId>, f32, bool)> {
        self.lots.iter().find(|(a, b, id, ..)| p.x >= a.x && p.y >= a.y && p.x <= b.x && p.y <= b.y && self.w.lots.get(*id).is_some_and(|l| point_in_polygon(p, &l.polygon)))
    }

    /// The way under `p`, if `p` is on one: its deck height and whether it is rail.
    fn way_at(&self, p: Vec2) -> Option<(f32, bool)> {
        let roads = &self.w.roads;
        let mut best: Option<(f32, bool)> = None;
        for sid in roads.segments_near(p, 30.0) {
            let seg = roads.segment(sid);
            if seg.tunnel {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let (t, q) = crate::roads::project_on_segment(p, a, b);
            if q.distance(p) <= seg.half_width() + 1.0 {
                let z = if self.w.relief { self.w.road_height(sid, t) } else { 0.0 };
                if best.is_none_or(|x| z > x.0) {
                    best = Some((z, seg.is_rail()));
                }
            }
        }
        best
    }
}

impl Terrain for CityLand<'_> {
    fn ground(&self, p: Vec2) -> f32 {
        if self.w.relief {
            self.w.ground_height(p)
        } else {
            0.0
        }
    }

    fn top(&self, p: Vec2) -> (f32, Below) {
        let g = self.ground(p);
        let mut best = (g, Below::Ground);
        if !self.w.landscape.is_flat() && self.w.landscape.is_water(p) {
            best = (g.max(crate::landscape::WATER_LEVEL), Below::Water);
        }
        if let Some((z, rail)) = self.way_at(p) {
            if z >= best.0 - 0.5 {
                best = (z.max(best.0), if rail { Below::Rail } else { Below::Road });
            }
        }
        if let Some(&(_, _, id, Some(b), h, _)) = self.lot_at(p) {
            if !self.own.contains(&id) && g + h > best.0 {
                best = (g + h, Below::Building(b));
            }
        }
        best
    }

    fn pylon_refusal(&self, p: Vec2) -> Option<&'static str> {
        let half = self.w.config.half_extent + 400.0;
        if p.x.abs() > half || p.y.abs() > half {
            return Some("the map ends");
        }
        if !self.w.landscape.is_flat() && self.w.landscape.is_water(p) {
            return Some("water");
        }
        if let Some((_, rail)) = self.way_at(p) {
            return Some(if rail { "the railway" } else { "roads" });
        }
        if let Some(&(_, _, id, b, _, _)) = self.lot_at(p) {
            if b.is_some() && !self.own.contains(&id) {
                return Some("buildings");
            }
        }
        if self.w.cable.stations.values().any(|s| s.pos.distance(p) < 26.0) {
            return Some("a station");
        }
        None
    }

    fn name(&self, b: Below) -> String {
        match b {
            Below::Ground => "the ground".into(),
            Below::Water => "the water".into(),
            Below::Road => "the road".into(),
            Below::Rail => "the railway".into(),
            Below::Building(id) => self.w.buildings.get(id).map_or_else(|| "a building".to_string(), |b| b.name.clone()),
        }
    }
}

impl World {
    /// How tall a building stands for the rope to clear: by its floors (a
    /// landmark in a park, which `building_height` counts as 0, too).
    fn cable_obstacle_height(&self, b: BuildingId) -> f32 {
        let h = self.building_height(b);
        if h > 0.0 {
            return h.max(4.0);
        }
        self.buildings.get(b).map_or(0.0, |x| x.floors as f32 * 4.0).max(4.0)
    }

    fn cable_ground(&self, p: Vec2) -> f32 {
        if self.relief {
            self.ground_height(p)
        } else {
            0.0
        }
    }

    /// The cable station a click at `p` means: the nearest within `radius`.
    pub fn cable_station_at(&self, p: Vec2, radius: f32) -> Option<CableStationId> {
        self.cable.station_near(p, radius)
    }

    /// The lot under a point.
    fn lot_under(&self, p: Vec2) -> Option<LotId> {
        self.lots.iter().find(|(_, l)| l.center.distance_squared(p) < 200.0 * 200.0 && point_in_polygon(p, &l.polygon)).map(|(id, _)| id)
    }

    /// Where a station clicked at `p` would stand, or why it cannot.
    pub fn cable_station_site(&self, p: Vec2, want: SiteWant) -> Result<StationSite, String> {
        // The buildable square and the land laid out round it.
        let half = self.config.half_extent + 400.0;
        if p.x.abs() > half || p.y.abs() > half {
            return Err("that is off the map".into());
        }
        if !self.landscape.is_flat() && self.landscape.is_water(p) {
            return Err("a station cannot stand in the water: put it on the bank".into());
        }
        if let Some((_, s)) = self.cable.stations.iter().map(|(id, s)| (id, s)).find(|(_, s)| s.pos.distance(p) < cable::MIN_STATION_GAP * 0.5) {
            return Err(format!("too close to {} (stations stand {:.0} m apart; click it to use it)", s.name, cable::MIN_STATION_GAP * 0.5));
        }
        let lot = self.lot_under(p);
        let standing = lot.and_then(|l| self.lots[l].building).filter(|b| self.buildings.contains_key(*b));
        match (lot, standing) {
            (Some(lid), Some(bid)) => {
                let b = &self.buildings[bid];
                let l = &self.lots[lid];
                let h = self.building_height(bid);
                let (lo, hi) = crate::geom::bounds(&l.polygon);
                let wide = (hi.x - lo.x).min(hi.y - lo.y) >= 10.0;
                let roofable = b.operating() && !b.is_derelict() && h >= cable::ROOF_MIN_HEIGHT && wide && !b.cable && b.plant.is_none() && b.land_use != LandUse::Park;
                if want == SiteWant::Roof || (want == SiteWant::Auto && roofable) {
                    if !roofable {
                        return Err(if h < cable::ROOF_MIN_HEIGHT {
                            format!("{} is only {:.0} m tall: a rooftop station needs {:.0} m (a lower building is bought out for a ground station)", b.name, h, cable::ROOF_MIN_HEIGHT)
                        } else {
                            format!("{}'s roof cannot take a station", b.name)
                        });
                    }
                    let ground = self.cable_ground(l.center);
                    return Ok(StationSite { pos: l.center, floor: ground + h + cable::ROOF_FLOOR, ground, roof: true, lot: None, host: Some(bid), entrance: l.entrance(), what: format!("on the roof of {} ({:.0} m up; riders take its lifts)", b.name, h) });
                }
                if b.land_use == LandUse::Park {
                    // In the park: the park stays, the station stands in it.
                    return self.open_ground_site(p, format!("in {}", b.name));
                }
                if Self::is_civic_structure(b) {
                    return Err(format!("{} stands there; bulldoze it first", b.name));
                }
                if l.area() < 140.0 {
                    return Err("the lot is too small for a station (140 m² at least)".into());
                }
                let ground = self.cable_ground(l.center);
                Ok(StationSite { pos: l.center, floor: ground + cable::GROUND_FLOOR, ground, roof: false, lot: Some(lid), host: None, entrance: l.entrance(), what: format!("buys out {} for a ground station", b.name) })
            }
            (Some(lid), None) => {
                if want == SiteWant::Roof {
                    return Err("there is no building there to put a station on".into());
                }
                let l = &self.lots[lid];
                if l.area() < 140.0 {
                    return Err("the lot is too small for a station (140 m² at least)".into());
                }
                let ground = self.cable_ground(l.center);
                Ok(StationSite { pos: l.center, floor: ground + cable::GROUND_FLOOR, ground, roof: false, lot: Some(lid), host: None, entrance: l.entrance(), what: "a ground station on an empty lot".into() })
            }
            (None, _) => {
                if want == SiteWant::Roof {
                    return Err("there is no building there to put a station on".into());
                }
                self.open_ground_site(p, "on open ground".into())
            }
        }
    }

    /// A station on open ground or in a park: not on a road, with a way
    /// to walk in from within `LINK_REACH`.
    fn open_ground_site(&self, p: Vec2, what: String) -> Result<StationSite, String> {
        // Clear of the buildings round it (the hall is some 30 m long).
        let near = self.lots.values().find(|l| {
            l.center.distance_squared(p) < 160.0 * 160.0
                && l.building.and_then(|b| self.buildings.get(b)).is_some_and(|x| !x.cable && (x.land_use != LandUse::Park || x.service.is_some()))
                && (point_in_polygon(p, &l.polygon) || crate::geom::distance_to_outline(p, &l.polygon) < 14.0)
        });
        if let Some(l) = near {
            let name = l.building.and_then(|b| self.buildings.get(b)).map_or_else(String::new, |b| b.name.clone());
            return Err(format!("too close to {name}: a station on open ground needs 14 m clear round it"));
        }
        if let Some((sid, _, q)) = self.roads.nearest_where(p, |s| !s.tunnel) {
            if q.distance(p) < self.roads.segment(sid).half_width() + 4.0 {
                return Err("a station cannot stand on a road".into());
            }
        }
        let node = self.walk.nearest_within(p, cable::LINK_REACH).filter(|&k| !self.walk.adj[k as usize].is_empty());
        let Some(k) = node else {
            return Err(format!("no street or path within {:.0} m to walk in from: lay a footpath to it", cable::LINK_REACH));
        };
        let ground = self.cable_ground(p);
        Ok(StationSite { pos: p, floor: ground + cable::GROUND_FLOOR, ground, roof: false, lot: None, host: None, entrance: self.walk.nodes[k as usize].pos, what })
    }

    /// A name for a new station, by where it stands.
    fn cable_station_name(&self, at: Vec2) -> String {
        let n = cable::STATION_NAMES.len();
        let start = ((at.x.abs() * 5.0 + at.y.abs() * 11.0) as usize) % n;
        let taken = |name: &str| self.cable.stations.values().any(|s| s.name == name);
        (0..n).map(|k| cable::STATION_NAMES[(start + k) % n]).find(|name| !taken(name)).map_or_else(|| format!("Cable Station {}", self.cable.stations.len() + 1), str::to_string)
    }

    /// Build a station at a site (paid from the treasury).
    fn build_cable_station(&mut self, site: &StationSite, kind: CableKind) -> Result<CableStationId, String> {
        let spec = kind.spec();
        if self.economy.treasury < spec.station_cost {
            return Err(format!("the treasury has {:.0}; a {} station costs {:.0}", self.economy.treasury, kind.label(), spec.station_cost));
        }
        let name = self.cable_station_name(site.pos);
        let mut building = site.host;
        let mut entrance = site.entrance;
        if let Some(lot) = site.lot {
            self.take_over_lot(lot, "cable car station")?;
            let id = self.place_special(lot, name.clone(), spec.staff, 0, |b| b.cable = true);
            building = Some(id);
            entrance = self.lots[lot].entrance();
        }
        self.economy.debit(Flow::Construction, spec.station_cost);
        let tick = self.clock.tick;
        let id = self.cable.stations.insert(CableStation { name, pos: site.pos, floor: site.floor, ground: site.ground, roof: site.roof, building, entrance, boardings_today: 0, boardings_yesterday: 0, built_tick: tick });
        self.cable.version += 1;
        Ok(id)
    }

    /// The stops as places: each station's point, platform floor, name and
    /// lot, and the sites still to build.
    fn cable_stops(&self, stops: &[CableStop]) -> Result<Vec<(Vec2, f32, String, Option<LotId>, Option<StationSite>)>, String> {
        let mut out: Vec<(Vec2, f32, String, Option<LotId>, Option<StationSite>)> = Vec::new();
        for (k, s) in stops.iter().enumerate() {
            match *s {
                CableStop::Station(id) => {
                    let st = self.cable.stations.get(id).ok_or("no such cable station")?;
                    let lot = st.building.and_then(|b| self.buildings.get(b)).map(|b| b.lot);
                    out.push((st.pos, st.floor, st.name.clone(), lot, None));
                }
                CableStop::At(p, want) => {
                    // A click on (or by) a station is that station.
                    if let Some(id) = self.cable.station_near(p, 30.0) {
                        let st = &self.cable.stations[id];
                        let lot = st.building.and_then(|b| self.buildings.get(b)).map(|b| b.lot);
                        out.push((st.pos, st.floor, st.name.clone(), lot, None));
                        continue;
                    }
                    let site = self.cable_station_site(p, want).map_err(|e| format!("stop {}: {e}", k + 1))?;
                    if out.iter().any(|o| o.0.distance(site.pos) < 1.0) {
                        return Err(format!("stop {} is the same place as another", k + 1));
                    }
                    let name = format!("new station {}", k + 1);
                    // A roof station's own building is no obstacle to its rope.
                    let lot = site.lot.or(site.host.and_then(|h| self.buildings.get(h)).map(|b| b.lot));
                    out.push((site.pos, site.floor, name, lot, Some(site)));
                }
            }
        }
        Ok(out)
    }

    /// What a line of `kind` through `stops` would be: its pylons, length,
    /// ride, capacity and cost, or why it cannot be built (`refusal`).
    pub fn cable_line_preview(&self, kind: CableKind, stops: &[CableStop]) -> CablePreview {
        let mut pv = CablePreview { kind, stations: stops.len(), ..Default::default() };
        let places = match self.cable_stops(stops) {
            Ok(p) => p,
            Err(e) => {
                pv.refusal = Some(e);
                return pv;
            }
        };
        pv.new_stations = places.iter().filter(|p| p.4.is_some()).count();
        let points: Vec<Vec2> = places.iter().map(|p| p.0).collect();
        let floors: Vec<f32> = places.iter().map(|p| p.1).collect();
        let names: Vec<String> = places.iter().map(|p| p.2.clone()).collect();
        let own: Vec<LotId> = places.iter().filter_map(|p| p.3).collect();
        let land = CityLand::new(self, &points, own);
        let laid = match cable::lay_line(kind, &points, &floors, &land, &names) {
            Ok(l) => l,
            Err(e) => {
                pv.length_m = cable::path_of(&points).last().copied().unwrap_or(0.0);
                pv.refusal = Some(e);
                return pv;
            }
        };
        let spec = kind.spec();
        let line = self.cable_line_from(kind, vec![CableStationId::default(); places.len()], &laid, 0);
        let secs = self.config.traffic_seconds_per_tick;
        pv.length_m = line.length();
        pv.pylons = line.pylons().count();
        pv.tallest_pylon_m = line.pylons().map(|p| p.height()).fold(0.0, f32::max);
        pv.max_above_ground_m = laid.max_above_ground;
        pv.over_water = laid.over_water;
        pv.ride_min = line.ride_minutes(0, places.len() - 1, secs);
        pv.cabins = line.cabins;
        pv.interval_min = line.interval(secs);
        pv.capacity_per_hour = line.capacity_per_hour(secs);
        pv.build_cost = cable::works_cost(spec, &laid.supports, line.length()) + pv.new_stations as f32 * spec.station_cost;
        let mut stations_upkeep = 0.0;
        for p in &places {
            if p.4.is_some() || !self.cable.lines.values().any(|l| l.stations.iter().any(|s| self.cable.stations.get(*s).is_some_and(|x| x.pos.distance(p.0) < 1.0))) {
                stations_upkeep += spec.station_upkeep;
            }
        }
        pv.upkeep_per_day = stations_upkeep + line.length() / 1000.0 * spec.upkeep_per_km + line.cabins as f32 * spec.cabin_upkeep;
        pv.scenic = self.cable_scenic(&laid, &floors, &points);
        pv.supports = laid.supports;
        if pv.build_cost > self.economy.treasury {
            pv.refusal = Some(format!("the treasury has {:.0}; the line costs {:.0}", self.economy.treasury, pv.build_cost));
        } else if self.cable.lines.len() >= cable::MAX_LINES {
            pv.refusal = Some(format!("the city runs {} cable lines already (the most there may be)", cable::MAX_LINES));
        }
        pv
    }

    /// How scenic a line laid like this would be.
    fn cable_scenic(&self, laid: &cable::LineLaid, floors: &[f32], points: &[Vec2]) -> f32 {
        let lo = floors.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = floors.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        // A summit over the town, not a roof over the street.
        let ground_hi = points.iter().map(|p| self.cable_ground(*p)).fold(f32::NEG_INFINITY, f32::max);
        let ground_lo = points.iter().map(|p| self.cable_ground(*p)).fold(f32::INFINITY, f32::min);
        let summit = (ground_hi - ground_lo).max(0.0) * 0.8 + (hi - lo - (ground_hi - ground_lo)).max(0.0) * 0.3;
        cable::scenic_score(laid.max_above_ground, laid.over_water, summit)
    }

    fn cable_line_from(&self, kind: CableKind, stations: Vec<CableStationId>, laid: &cable::LineLaid, n: u32) -> CableLine {
        let mut l = CableLine {
            name: match kind {
                CableKind::Gondola => format!("Gondola {n}"),
                CableKind::Tramway => format!("Skytram {n}"),
            },
            kind,
            color: ((n.max(1) - 1) % cable::LINE_COLOURS as u32) as u8,
            stations,
            supports: laid.supports.clone(),
            stops: laid.stops.clone(),
            cabins: 2,
            open: cable::OPEN,
            close: cable::CLOSE,
            fare: kind.spec().fare,
            phase: 0.0,
            running: false,
            riders_today: 0,
            riders_yesterday: 0,
            left_behind_today: 0,
            left_behind_yesterday: 0,
            seats_today: 0,
            seats_yesterday: 0,
            seated_today: 0,
            seated_yesterday: 0,
            sightseers_today: 0,
            sightseers_yesterday: 0,
            scenic: 0.0,
            cost: 0.0,
            built_tick: self.clock.tick,
            timeline: Default::default(),
            aboard: Vec::new(),
        };
        l.cabins = cable::default_cabins(kind, l.loop_layout().length);
        l
    }

    /// Build a line of `kind` through `stops` (building the stations that
    /// are points) and pay for it.
    pub fn add_cable_line(&mut self, kind: CableKind, stops: &[CableStop]) -> Result<CableLineId, String> {
        let pv = self.cable_line_preview(kind, stops);
        if let Some(why) = pv.refusal {
            return Err(why);
        }
        let places = self.cable_stops(stops)?;
        let mut ids: Vec<CableStationId> = Vec::with_capacity(places.len());
        for (k, p) in places.iter().enumerate() {
            match (&p.4, stops[k]) {
                (Some(site), _) => ids.push(self.build_cable_station(site, kind)?),
                (None, CableStop::Station(id)) => ids.push(id),
                (None, CableStop::At(q, _)) => ids.push(self.cable.station_near(q, 30.0).ok_or("the station went")?),
            }
        }
        let spec = kind.spec();
        let works = pv.build_cost - pv.new_stations as f32 * spec.station_cost;
        self.economy.debit(Flow::Construction, works.max(0.0));
        self.cable.next_line += 1;
        let n = self.cable.next_line;
        let laid = cable::LineLaid { supports: pv.supports.clone(), stops: cable::path_of(&ids.iter().map(|s| self.cable.stations[*s].pos).collect::<Vec<_>>()), max_above_ground: pv.max_above_ground_m, over_water: pv.over_water };
        let mut line = self.cable_line_from(kind, ids, &laid, n);
        line.scenic = pv.scenic;
        line.cost = pv.build_cost;
        line.running = line.open_at(self.clock.minute_of_day() as u32);
        let id = self.cable.lines.insert(line);
        self.cable.version += 1;
        self.rebuild_cable_corridor();
        Ok(id)
    }

    /// Close a line: riders aboard are set down at the nearest station,
    /// people waiting go on another way, and stations no other line calls
    /// at come down with it.
    pub fn remove_cable_line(&mut self, id: CableLineId) -> bool {
        let Some(mut line) = self.cable.lines.remove(id) else { return false };
        for a in std::mem::take(&mut line.aboard) {
            let at = self.cable_nearest_entrance(&line, self.citizens.get(a.citizen).map_or(Vec2::ZERO, |c| c.pos));
            self.cable_rider_off(a.citizen, at);
        }
        self.strand_cable_waiting(|l, _, _| l == id);
        for s in line.stations {
            if !self.cable.lines.values().any(|l| l.stations.contains(&s)) {
                self.remove_cable_station_record(s);
            }
        }
        self.cable.version += 1;
        self.rebuild_cable_corridor();
        true
    }

    /// A station comes down: its lot's station building is pulled down (a
    /// roof's building stays).
    fn remove_cable_station_record(&mut self, s: CableStationId) {
        let Some(st) = self.cable.stations.remove(s) else { return };
        if !st.roof {
            if let Some(b) = st.building.filter(|b| self.buildings.get(*b).is_some_and(|x| x.cable)) {
                self.demolish(b);
            }
        }
        self.cable.version += 1;
    }

    /// Cabins on a gondola line (more: shorter waits, more seats, more upkeep).
    pub fn set_cable_cabins(&mut self, id: CableLineId, n: u32) -> Result<u32, String> {
        let l = self.cable.lines.get_mut(id).ok_or("no such line")?;
        if l.kind != CableKind::Gondola {
            return Err("an aerial tramway has its two cabins".into());
        }
        let (lo, hi) = cable::cabin_limits(l.kind, l.loop_layout().length);
        let n = n.clamp(lo, hi);
        if n != l.cabins {
            // Riders keep their cabin numbers only while those exist: the
            // rest are set down at the next station they pass (cabins
            // re-spaced on the rope, a quiet moment in a real one).
            l.cabins = n;
            for a in &mut l.aboard {
                a.cabin = a.cabin.min(n - 1);
            }
            self.cable.version += 1;
        }
        Ok(n)
    }

    /// Opening and closing times (minutes of the day).
    pub fn set_cable_hours(&mut self, id: CableLineId, open: u32, close: u32) -> Result<(u32, u32), String> {
        let l = self.cable.lines.get_mut(id).ok_or("no such line")?;
        if open % 1440 == close % 1440 {
            return Err("opening and closing at the same time".into());
        }
        l.open = open % 1440;
        l.close = close % 1440;
        Ok((l.open, l.close))
    }

    pub fn set_cable_fare(&mut self, id: CableLineId, fare: f32) -> Result<f32, String> {
        let l = self.cable.lines.get_mut(id).ok_or("no such line")?;
        l.fare = fare.clamp(0.0, cable::MAX_FARE);
        Ok(l.fare)
    }

    /// Where riders walk in and out of a station.
    fn cable_entrance(&self, s: CableStationId) -> Option<Vec2> {
        let st = self.cable.stations.get(s)?;
        Some(st.building.and_then(|b| self.building_entrance(b)).unwrap_or(st.entrance))
    }

    fn cable_nearest_entrance(&self, line: &CableLine, p: Vec2) -> Vec2 {
        line.stations.iter().filter_map(|s| self.cable_entrance(*s)).min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap_or(p)
    }

    /// The best cable ride for a trip setting off now: only lines open when
    /// the rider gets to the station and still running to get them there.
    pub(super) fn plan_cable_trip_now(&self, from: Vec2, to: Vec2) -> Option<CablePlan> {
        if self.cable.lines.is_empty() {
            return None;
        }
        let now = self.clock.minute_of_day() as u32;
        let (walk, secs) = (self.config.walk_speed.max(0.1), self.config.traffic_seconds_per_tick);
        let waiting = &self.cable_waiting;
        self.cable.plan_trip(
            from,
            to,
            secs,
            walk,
            |l, d| {
                let at = now + (d * 1.25 / walk).ceil() as u32 + CABLE_STAIRS as u32;
                l.open_at(at) && l.open_at(at + l.ride_minutes(0, l.stations.len() - 1, secs) as u32 + 5)
            },
            |s| waiting.get(&s).copied().unwrap_or(0),
        )
    }

    /// [cable] The ride as one of the ways to make a trip, and the other
    /// ways re-reckoned where the cable car is an option: the walk (and the
    /// ride or drive) round a river by the nearest bridge instead of as
    /// the crow flies, and the climb up a hill on foot or by bike.
    pub(super) fn cable_option(&self, id: CitizenId, q: &TripQuery, time_w: f32, money_w: f32) -> Option<trips::ModeOption> {
        let c = self.citizens.get(id)?;
        let p = self.plan_cable_trip_now(q.from, q.to)?;
        let line = self.cable.lines.get(p.line)?;
        let walk = self.config.walk_speed.max(0.1);
        let [sociable, _ambition, _hedonism, caution, curiosity] = crate::vehicle::traits_of(c.seed);
        let minutes = (q.from.distance(p.entrance) + p.exit.distance(q.to)) / walk + CABLE_STAIRS + p.minutes_waiting + p.minutes_riding;
        let crowd = (6.0 - 6.0 * sociable) * if line.kind == CableKind::Gondola { 0.3 } else { 0.8 };
        // The view pleases the curious; the drop worries the cautious.
        let comfort = crowd - (2.0 + 7.0 * curiosity) * line.scenic + 3.0 * caution * (1.0 - curiosity);
        let pain = c.commute.pain_of(crate::citizens::TripMode::Cable);
        let cost = minutes * time_w + line.fare * money_w + comfort + trips::PAIN_WEIGHT * pain;
        Some(trips::ModeOption { mode: crate::citizens::TripMode::Cable, minutes, money: line.fare, comfort, pain, cost, plan: trips::ModePlan::Cable { line: p.line, board: p.board, alight: p.alight, stop: p.entrance } })
    }

    /// Walking, cycling and driving re-reckoned for the lie of the land
    /// (only where a cable ride is on offer, so nothing else changes): the
    /// real way round when the straight line crosses water, and the climb.
    pub(super) fn cable_reprice(&self, out: &mut [trips::ModeOption], q: &TripQuery, time_w: f32) {
        use crate::citizens::TripMode as M;
        let dist = q.from.distance(q.to).max(1.0);
        let wet = !self.landscape.is_flat() && {
            let n = (dist / 20.0).ceil() as usize;
            (1..n).any(|k| self.landscape.is_water(q.from.lerp(q.to, k as f32 / n as f32)))
        };
        let detour = if wet {
            match self.walk.route(q.from, q.to) {
                Some((pts, _)) => (crate::geom::path_length(&pts) / (dist * 1.25)).clamp(1.0, 4.0),
                None => 4.0,
            }
        } else {
            1.0
        };
        let climb = (self.cable_ground(q.to) - self.cable_ground(q.from)).max(0.0);
        if detour <= 1.01 && climb < 3.0 {
            return;
        }
        let (walk, bike) = (self.config.walk_speed.max(0.1), self.config.bike_speed.max(0.1));
        for o in out.iter_mut() {
            let (extra, sweat) = match o.mode {
                // Naismith: a metre up is about eight on the flat.
                M::Walk => (o.minutes * (detour - 1.0) + climb * 8.0 / walk, climb * 0.12),
                M::Bike => (o.minutes * (detour - 1.0) + climb * 10.0 / bike, climb * 0.1),
                M::Car | M::Taxi => (o.minutes * (detour - 1.0) * 0.8, 0.0),
                _ => continue,
            };
            o.minutes += extra;
            o.comfort += sweat;
            o.cost += extra * time_w + sweat;
        }
    }

    /// People waiting for a cable ride `pred` rules out go on another way.
    pub(super) fn strand_cable_waiting(&mut self, pred: impl Fn(CableLineId, CableStationId, CableStationId) -> bool) {
        let tick = self.clock.tick;
        let stranded: Vec<(CitizenId, CableStationId)> = self
            .citizens
            .iter()
            .filter(|(_, c)| !matches!(c.activity, Activity::OnCable { .. }))
            .filter_map(|(id, c)| match c.transit.map(|t| t.leg) {
                Some(TransitLeg::Cable { line, board, alight }) if pred(line, board, alight) => Some((id, board)),
                _ => None,
            })
            .collect();
        for (id, board) in stranded {
            let out = self.cable_entrance(board);
            let c = &mut self.citizens[id];
            if matches!(c.activity, Activity::Waiting { .. }) {
                if let Some(p) = out {
                    c.pos = p;
                    c.prev_pos = p;
                }
            }
            let trip = c.transit.take();
            c.activity = Activity::Idle;
            c.thought = "The cable car's shut. Going another way.".into();
            c.next_decision_tick = tick + 1;
            if let Some(t) = trip.filter(|t| self.buildings.contains_key(t.target)) {
                self.travel_to_impl(id, Some(t.target), t.purpose, false, false);
            }
        }
    }

    /// Off a cabin at a station's entrance and on to wherever they were going.
    fn cable_rider_off(&mut self, p: CitizenId, at: Vec2) {
        let Some(trip) = self.citizens.get_mut(p).and_then(|c| {
            c.pos = at;
            c.prev_pos = at;
            c.activity = Activity::Idle;
            c.transit.take()
        }) else {
            if self.citizens.contains_key(p) {
                self.strand_driver(p, at);
            }
            return;
        };
        if self.buildings.contains_key(trip.target) {
            self.travel_to_impl(p, Some(trip.target), trip.purpose, true, false);
        } else {
            self.strand_driver(p, at);
        }
    }

    /// [cable] Someone waiting in a cable station who goes anywhere else
    /// comes down to its entrance first.
    pub(super) fn out_of_cable_station(&mut self, id: CitizenId) {
        let Some(c) = self.citizens.get(id) else { return };
        if !matches!(c.activity, Activity::Waiting { .. }) {
            return;
        }
        let Some(TransitLeg::Cable { board, .. }) = c.transit.map(|t| t.leg) else { return };
        if let Some(p) = self.cable_entrance(board) {
            let c = &mut self.citizens[id];
            c.pos = p;
            c.prev_pos = p;
        }
    }

    /// Where someone waiting for a cabin at station `s` stands: in the hall.
    pub fn cable_platform(&self, s: CableStationId) -> Option<Vec2> {
        self.cable.stations.get(s).map(|st| st.pos)
    }

    /// Stations whose building went (bulldozed, or the roof under them)
    /// come down, and the lines calling there close.
    fn prune_cable(&mut self) {
        let gone: Vec<CableStationId> = self
            .cable
            .stations
            .iter()
            .filter(|(_, s)| match (s.building.and_then(|b| self.buildings.get(b)), s.building) {
                (_, None) => false,
                (None, Some(_)) => true,
                // The roof under it is coming down (or being rebuilt).
                (Some(x), _) if s.roof => x.demolition.is_some(),
                // Its own lot's building: gone when bulldozed (a new
                // station's building clears what stood there first, with
                // a successor waiting: that is not the station going).
                (Some(x), _) => !x.cable || x.demolition.as_ref().is_some_and(|d| d.clear_lot),
            })
            .map(|(id, _)| id)
            .collect();
        if gone.is_empty() {
            return;
        }
        let lines: Vec<CableLineId> = self.cable.lines.iter().filter(|(_, l)| l.stations.iter().any(|s| gone.contains(s))).map(|(id, _)| id).collect();
        for s in &gone {
            if let Some(st) = self.cable.stations.get_mut(*s) {
                // The building is already going: do not pull it down twice.
                st.building = None;
                st.roof = false;
            }
        }
        for id in lines {
            self.remove_cable_line(id);
        }
        for s in gone {
            self.cable.stations.remove(s);
        }
        self.cable.version += 1;
        self.rebuild_cable_corridor();
    }

    /// Every tick: run the ropes, set riders down and take them on as
    /// cabins pass through stations.
    pub(super) fn step_cable(&mut self) {
        if self.cable.lines.is_empty() && self.cable.stations.is_empty() {
            return;
        }
        self.prune_cable();
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day() as u32;
        let secs = self.config.traffic_seconds_per_tick;
        // Who waits where, for the planners' queue estimate.
        if tick % 5 == 0 || self.cable_waiting.is_empty() {
            let mut waiting: HashMap<CableStationId, u32> = HashMap::new();
            for c in self.citizens.values() {
                if let (Activity::Waiting { .. }, Some(TransitLeg::Cable { board, .. })) = (&c.activity, c.transit.map(|t| t.leg)) {
                    *waiting.entry(board).or_default() += 1;
                }
            }
            self.cable_waiting = waiting;
        }
        let ids: Vec<CableLineId> = self.cable.lines.keys().collect();
        for id in ids {
            let (events, open) = {
                let l = &mut self.cable.lines[id];
                let open = l.open_at(minute);
                if open && !l.running {
                    l.running = true;
                }
                if !open && l.running && l.aboard.is_empty() {
                    l.running = false;
                }
                if !l.running {
                    continue;
                }
                let adv = l.advance_per_tick(secs);
                let events = cable_events(l, adv, secs);
                match l.kind {
                    CableKind::Gondola => {
                        let len = l.loop_layout().length.max(1.0);
                        l.phase = (l.phase + adv).rem_euclid(len);
                    }
                    CableKind::Tramway => {
                        let period = l.shuttle_ticks(secs).2.max(1.0);
                        l.phase = (l.phase + adv).rem_euclid(period);
                    }
                }
                (events, open)
            };
            for e in events {
                self.cable_cabin_at(id, e, open);
            }
            // Riders ride along.
            let l = &self.cable.lines[id];
            if l.aboard.is_empty() {
                continue;
            }
            let layout = (l.kind == CableKind::Gondola).then(|| l.loop_layout());
            let poses: Vec<(CitizenId, Vec2, f32)> = l
                .aboard
                .iter()
                .map(|a| {
                    let p = match &layout {
                        Some(lay) => l.loop_pose(lay, l.cabin_u(a.cabin, lay)),
                        None => l.shuttle_pose(a.cabin, l.phase, secs),
                    };
                    (a.citizen, p.pos, p.heading)
                })
                .collect();
            for (c, pos, heading) in poses {
                if let Some(c) = self.citizens.get_mut(c) {
                    c.prev_pos = c.pos;
                    c.pos = pos;
                    c.heading = heading;
                }
            }
        }
        // Counts.
        let s = &mut self.cable.stats;
        s.lines = self.cable.lines.len() as u32;
        s.stations = self.cable.stations.len() as u32;
        s.cabins = self.cable.lines.values().map(|l| l.cabins).sum();
        s.pylons = self.cable.lines.values().map(|l| l.pylons().count() as u32).sum();
        s.riding = self.cable.lines.values().map(|l| l.aboard.len() as u32).sum();
        s.waiting = self.cable_waiting.values().sum();
        s.rope_m = self.cable.lines.values().map(|l| l.length()).sum();
    }

    /// A cabin at a station: riders for here get off, people waiting for
    /// this line the way it goes get on (while it is open) and pay.
    fn cable_cabin_at(&mut self, id: CableLineId, e: CabinEvent, open: bool) {
        let Some(l) = self.cable.lines.get(id) else { return };
        let Some(&station) = l.stations.get(e.station as usize) else { return };
        let last = l.stations.len() as u16 - 1;
        let at = self.cable_entrance(station).unwrap_or(self.cable.stations.get(station).map_or(Vec2::ZERO, |s| s.pos));
        // Off: those for here, and everyone at the end of the line.
        let (off, keep): (Vec<Aboard>, Vec<Aboard>) = std::mem::take(&mut self.cable.lines[id].aboard).into_iter().partition(|a| a.cabin == e.cabin && (a.alight == e.station || e.end));
        self.cable.lines[id].aboard = keep;
        for a in off {
            self.cable_rider_off(a.citizen, at);
        }
        if !e.board || !open {
            return;
        }
        let l = &self.cable.lines[id];
        let seats = l.spec().seats as usize;
        let room = seats.saturating_sub(l.in_cabin(e.cabin));
        let forward = e.forward;
        let stations = l.stations.clone();
        let index = |s: CableStationId| stations.iter().position(|x| *x == s);
        let i = e.station as usize;
        let pred = |t: &TransitTrip| match t.leg {
            TransitLeg::Cable { line, board, alight } if line == id && board == station => index(alight).is_some_and(|j| if forward { j > i } else { j < i }),
            _ => false,
        };
        let (name, fare, line_name) = (self.cable.stations[station].name.clone(), l.fare, l.name.clone());
        let boarding = if room > 0 { self.board_waiting(pred, room, fare, Flow::BusFares, Item::Named(format!("{line_name} fares")), &format!("Rode the {line_name} from {name}.")) } else { Vec::new() };
        let left = self.still_waiting(pred) as u32;
        let tick = self.clock.tick;
        let hour = self.clock.minute_of_day() as u32 / 60;
        let n = boarding.len() as u32;
        let waited: u64 = boarding.iter().map(|&p| self.citizens[p].transit.map_or(0, |t| if t.since > 0 { tick.saturating_sub(t.since) } else { 0 })).sum();
        let mut aboard = Vec::with_capacity(boarding.len());
        for p in boarding {
            let alight = self.citizens[p].transit.and_then(|t| match t.leg {
                TransitLeg::Cable { alight, .. } => index(alight),
                _ => None,
            });
            let alight = alight.unwrap_or(if forward { last as usize } else { 0 }) as u16;
            let c = &mut self.citizens[p];
            c.activity = Activity::OnCable { line: id, cabin: e.cabin };
            c.inside = None;
            aboard.push(Aboard { citizen: p, cabin: e.cabin, alight });
        }
        let l = &mut self.cable.lines[id];
        l.aboard.extend(aboard);
        l.seated_today += n;
        // Seats offered and riders left behind, once per departure.
        if e.leaving {
            l.seats_today += seats as u32;
            let full = l.aboard.iter().filter(|a| a.cabin == e.cabin).count() >= seats;
            if left > 0 && full {
                l.left_behind_today += left;
            }
        }
        if n > 0 {
            l.riders_today += n;
            l.timeline.board(hour, n, waited as u32);
            self.cable.stats.riders_today += n;
            self.cable.stats.fares_today += n as f32 * fare;
            if let Some(s) = self.cable.stations.get_mut(station) {
                s.boardings_today += n;
            }
        }
    }

    /// Midnight: stations (their staff), rope and cabins are paid for, and
    /// the day's counts roll over.
    pub(super) fn daily_cable(&mut self) {
        if self.cable.lines.is_empty() && self.cable.stations.is_empty() {
            return;
        }
        let mut upkeep = 0.0;
        for (sid, _) in self.cable.stations.iter() {
            let kind = self.cable.lines.values().find(|l| l.stations.contains(&sid)).map_or(CableKind::Gondola, |l| l.kind);
            upkeep += kind.spec().station_upkeep;
        }
        for l in self.cable.lines.values() {
            let spec = l.spec();
            upkeep += l.length() / 1000.0 * spec.upkeep_per_km + l.cabins as f32 * spec.cabin_upkeep;
        }
        if upkeep > 0.0 {
            self.economy.debit_item(Flow::TransitUpkeep, upkeep, Item::Named("cable cars: stations, staff, rope and cabins".into()));
        }
        self.cable.stats.upkeep_yesterday = upkeep;
        self.cable.roll_day();
    }

    /// After a load: cabins' riders are not saved, so anyone aboard is set
    /// down at the nearest station of their line.
    pub(super) fn cable_after_load(&mut self) {
        let riders: Vec<(CitizenId, Option<CableLineId>)> = self.citizens.iter().filter_map(|(id, c)| if let Activity::OnCable { line, .. } = c.activity { Some((id, Some(line))) } else { None }).collect();
        for (id, line) in riders {
            let pos = self.citizens[id].pos;
            let at = match line.and_then(|l| self.cable.lines.get(l)) {
                Some(l) => self.cable_nearest_entrance(l, pos),
                None => pos,
            };
            self.cable_rider_off(id, at);
        }
        self.cable.version += 1;
        self.rebuild_cable_corridor();
    }

    /// How tall a building may grow under each line: the cabins clear it.
    pub(super) fn rebuild_cable_corridor(&mut self) {
        let mut out: HashMap<LotId, f32> = HashMap::new();
        for l in self.cable.lines.values() {
            let spec = l.spec();
            let pts: Vec<Vec2> = l.supports.iter().filter(|s| s.station.is_some()).map(|s| s.pos).collect();
            let land = CityLand::new(self, &pts, Vec::new());
            let total = l.length();
            let mut s = spec.station_half + 8.0;
            while s < total - spec.station_half - 8.0 {
                for side in [-spec.gauge * 0.5 - 2.0, 0.0, spec.gauge * 0.5 + 2.0] {
                    let (p, _) = l.point_at(s, side);
                    if let Some(&(_, _, lot, _, _, _)) = land.lot_at(p).or_else(|| land.lots.iter().find(|x| self.lots.get(x.2).is_some_and(|lt| point_in_polygon(p, &lt.polygon)))) {
                        let room = l.rope_z(s) - spec.hanger - cable::CLEARANCE - self.cable_ground(p);
                        let e = out.entry(lot).or_insert(f32::INFINITY);
                        *e = e.min(room.max(4.0));
                    }
                }
                s += 8.0;
            }
        }
        self.cable.corridor = out;
    }

    /// [cable] The height limit a line puts on a lot under it (m).
    pub(super) fn cable_height_limit(&self, lot: LotId) -> Option<f32> {
        self.cable.corridor.get(&lot).copied()
    }

    /// Land value: a station lifts its quarter (and hums next door); a
    /// station well above the town has the view.
    pub(super) fn splat_cable(&self, b: &mut LandValueBuilder) {
        if self.cable.stations.is_empty() {
            return;
        }
        let mut hs: Vec<f32> = self.lots.values().map(|l| self.cable_ground(l.center)).collect();
        hs.sort_by(|a, b| a.total_cmp(b));
        let mid = hs.get(hs.len() / 2).copied().unwrap_or(0.0);
        for st in self.cable.stations.values() {
            b.channel(Channel::Transit);
            b.splat(st.pos, cable::LAND_VALUE_RADIUS, cable::LAND_VALUE_LIFT);
            b.channel(Channel::Other);
            b.splat_nuisance(st.pos, cable::NOISE_RADIUS, cable::NOISE);
            let up = ((st.floor - mid - 20.0) / 60.0).clamp(0.0, 1.0);
            if up > 0.0 {
                b.channel(Channel::Landmarks);
                b.splat(st.pos, 160.0, cable::VIEW_LIFT * up);
                b.channel(Channel::Other);
            }
        }
    }

    /// [cable] Scenic lines draw visitors: the top station is the sight.
    pub(super) fn cable_attractions(&self) -> Vec<crate::tourism::Attraction> {
        let mut out = Vec::new();
        for l in self.cable.lines.values() {
            if l.scenic < 0.2 {
                continue;
            }
            let Some(top) = l.stations.iter().filter_map(|s| self.cable.stations.get(*s).map(|x| (*s, x.floor))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0) else { continue };
            let Some(pos) = self.cable_entrance(top) else { continue };
            out.push(crate::tourism::Attraction { kind: crate::tourism::AttractionKind::CableCar, name: l.name.clone(), pos, building: None, score: 2.0 + 10.0 * l.scenic });
        }
        out
    }

    /// [cable] A visitor at a cable car's top station rides it for the
    /// view (the fare there and back).
    pub(super) fn cable_sightseer(&mut self, t: crate::tourism::TouristId) {
        let Some(at) = self.tourism.tourists.get(t).map(|x| x.pos) else { return };
        let best = self
            .cable
            .lines
            .iter()
            .filter(|(_, l)| l.open_at(self.clock.minute_of_day() as u32))
            .filter_map(|(id, l)| l.stations.iter().filter_map(|s| self.cable_entrance(*s)).map(|p| p.distance(at)).fold(None, |m: Option<f32>, d| Some(m.map_or(d, |m| m.min(d)))).map(|d| (id, d)))
            .filter(|(_, d)| *d < 80.0)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((id, _)) = best else { return };
        let fare = 2.0 * self.cable.lines[id].fare;
        if self.tourism.tourists[t].money < fare {
            return;
        }
        self.tourism.tourists[t].spent += fare;
        self.pay_city(Account::Tourist(t), Flow::BusFares, fare, Some(Item::Named(format!("{} fares", self.cable.lines[id].name))));
        let l = &mut self.cable.lines[id];
        l.sightseers_today += 1;
        self.cable.stats.fares_today += fare;
    }

    /// The line and station a rider is waiting at or riding, for the
    /// inspector.
    pub fn cable_station_waiting(&self, s: CableStationId) -> u32 {
        self.cable_waiting.get(&s).copied().unwrap_or(0)
    }
}

/// A cabin reaching the middle of a station this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CabinEvent {
    cabin: u32,
    station: u16,
    /// Riders may board (a tramway lets them on all through its stand).
    board: bool,
    /// Which way it leaves: towards the last station.
    forward: bool,
    /// The end of a run: everyone gets off.
    end: bool,
    /// The cabin leaves the station after this (a tramway stands a while).
    leaving: bool,
}

/// The cabins that reach a station this tick, as the rope moves `adv`.
fn cable_events(l: &CableLine, adv: f32, secs: f32) -> Vec<CabinEvent> {
    let mut out = Vec::new();
    let n = l.stations.len();
    if n < 2 {
        return out;
    }
    match l.kind {
        CableKind::Gondola => {
            let layout = l.loop_layout();
            let len = layout.length.max(1.0);
            let mids: Vec<(f32, u16, bool, bool)> = layout
                .parts
                .iter()
                .filter_map(|&(p, start, plen)| match p {
                    LoopPart::Station { i, forward, turn } => {
                        // A terminal turns the cabin round: it leaves the
                        // first station forward and the last one back.
                        let fwd = if turn { i == 0 } else { forward };
                        Some((start + plen * 0.5, i, fwd, turn))
                    }
                    _ => None,
                })
                .collect();
            for k in 0..l.cabins {
                let u0 = l.cabin_u(k, &layout);
                let u1 = u0 + adv;
                for &(m, i, fwd, turn) in &mids {
                    if (u0 < m && m <= u1) || (u0 < m + len && m + len <= u1) {
                        out.push(CabinEvent { cabin: k, station: i, board: true, forward: fwd, end: turn, leaving: true });
                    }
                }
            }
        }
        CableKind::Tramway => {
            let (dwell, run, period) = l.shuttle_ticks(secs);
            for k in 0..2u32 {
                let t0 = (l.phase + if k == 1 { period * 0.5 } else { 0.0 }).rem_euclid(period);
                let t1 = t0 + adv;
                let (home, away) = (0u16, (n - 1) as u16);
                // Arriving: everyone off (the start of a stand).
                for (start, at) in [(0.0, home), (dwell + run, away)] {
                    for base in [start, start + period] {
                        if t0 < base && base <= t1 {
                            out.push(CabinEvent { cabin: k, station: at, board: false, forward: at == 0, end: true, leaving: false });
                        }
                    }
                }
                // Standing: riders board until it goes.
                let tt = t1.rem_euclid(period);
                if tt < dwell {
                    out.push(CabinEvent { cabin: k, station: home, board: true, forward: home == 0, end: false, leaving: tt + adv >= dwell });
                } else if (dwell + run..2.0 * dwell + run).contains(&tt) {
                    out.push(CabinEvent { cabin: k, station: away, board: true, forward: away == 0, end: false, leaving: tt + adv >= 2.0 * dwell + run });
                }
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "cable_tests.rs"]
mod tests;
